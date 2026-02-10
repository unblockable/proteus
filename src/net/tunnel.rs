use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet, VecDeque};
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll, Waker, ready};

use bytes::BytesMut;
use futures::stream::{SelectAll, StreamExt};
use futures::{Sink, SinkExt, Stream};
use rand::RngCore;
use rand::rngs::ThreadRng;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio_util::codec::{Decoder, Encoder};

use crate::common::sync::{AsyncMap, PollMutex, PollTaskChannel, PollTaskReceiver, PollTaskSender};
use crate::net::AsyncConnect;
use crate::net::proto::socks::address::{Socks5Address, Socks5Target};
use crate::net::proto::tunnel::codec::TunnelCodec;
use crate::net::proto::tunnel::message::{TunnelMessage, TunnelMessageKind};
use crate::net::session::{Session, SessionBuilder, SessionHalf};

#[derive(Debug)]
pub enum TunnelEofMethod {
    OnClose,
    OnStreamCount(usize),
}

impl TunnelEofMethod {
    fn is_eof(&self, n_streams: usize) -> bool {
        match self {
            TunnelEofMethod::OnClose => false,
            TunnelEofMethod::OnStreamCount(n_max) => n_streams >= *n_max,
        }
    }
}

pub struct TunnelClient<S>
where
    S: SessionBuilder<Message = TunnelMessage>,
{
    shared_reader: PollMutex<TunnelReader<S::StreamHalf>>,
    shared_writer: PollMutex<TunnelWriter<S::SinkHalf>>,
    tunnel_id: PollMutex<Option<u64>>,
}

impl<S> TunnelClient<S>
where
    S: SessionBuilder<Message = TunnelMessage> + 'static,
{
    pub fn new(eof_method: TunnelEofMethod) -> Self {
        let (task_tx, task_rx) = PollTaskChannel::channel::<TunnelMessage>(1_000);

        let mut reader = TunnelReader::new(eof_method);
        reader.put_buf(TunnelMessage::open(0));

        let client = Self {
            shared_reader: PollMutex::new(reader),
            shared_writer: PollMutex::new(TunnelWriter::new(task_tx)),
            tunnel_id: PollMutex::new(None),
        };

        client.clone().into_background_task_manager(task_rx);

        client
    }

    // Asynchronously handle tasks that require communication across our inner
    // reader and writer, such as messages from the server side.
    fn into_background_task_manager(mut self, mut task_chan: PollTaskReceiver<TunnelMessage>) {
        tokio::spawn(async move {
            while let Some(task) = task_chan.recv().await {
                let msg = &*task;
                match &msg.kind {
                    TunnelMessageKind::Open => {}
                    TunnelMessageKind::Opened => {
                        if msg.id > 0 {
                            *self.tunnel_id.lock().await = Some(msg.id);
                        }
                    }
                    TunnelMessageKind::Close => {
                        self.close_writer().await;
                        self.close_reader(TunnelMessage::closed()).await;
                    }
                    TunnelMessageKind::Closed => {
                        self.close_writer().await;
                    }
                    TunnelMessageKind::Connect(_) => {}
                    TunnelMessageKind::Encapsulate(_) => unreachable!(),
                }
            }
        });
    }

    /// Adds a new tunnel session from a client that is already connected to its
    /// application.
    ///
    /// `target` can be `None` only if we expect the remote server to have a
    /// pre-configured forwarding address, as is the case in Tor's v1 Pluggable
    /// Transport protocol.
    pub async fn add_session(
        &mut self,
        src: S::ReadHalf,
        dst: S::WriteHalf,
        target: Option<Socks5Target>,
    ) {
        self.add_session_inner(src, dst, target, None).await
    }

    // Mostly to support supplying our own id in unit tests.
    async fn add_session_inner(
        &mut self,
        src: S::ReadHalf,
        dst: S::WriteHalf,
        target: Option<Socks5Target>,
        id: Option<u64>,
    ) {
        let target = target.unwrap_or(Socks5Target::new(Socks5Address::Unknown, 0));

        // Create the session with an unused session id.
        let (id, stream) = {
            // Hold the writer lock until we add the sink to avoid race condition on id.
            let mut writer = self.shared_writer.lock().await;
            let id = id.unwrap_or_else(|| writer.generate_session_id());

            let (stream, sink) = Session::<S>::from_io(id, src, dst).into_split();
            writer.add(id, sink);

            (id, stream)
        };

        // Buffer a message for the remote server instructing it to open to a new
        // target destination app.
        {
            let mut reader = self.shared_reader.lock().await;
            reader.add(id, stream);
            reader.put_buf(TunnelMessage::connect(id, target));
            reader.wake();
        }
    }

    async fn id(&mut self) -> Option<u64> {
        if let Some(tunnel_id) = *self.tunnel_id.lock().await
            && tunnel_id > 0
        {
            Some(tunnel_id)
        } else {
            None
        }
    }

    pub async fn can_resume(&mut self) -> Option<u64> {
        // TODO, check and make sure we are not in an error state.
        self.id().await
    }

    /// Attempt to recover from a network error on the channel to the server by
    /// asking the server to resume this tunnel from a previous tunnel state.
    /// This should only be called if using an encapsulated protocol that can
    /// recover from lost messages (e.g., `TurboSession`).
    pub async fn initiate_resume(&mut self, tunnel_id: u64) {
        // Clear the io buffers to guarantee message alignment in case a previous
        // message was only partially sent, then send the previous tunnel id.
        {
            self.shared_writer.lock().await.buffer.clear();
        }
        {
            let mut reader = self.shared_reader.lock().await;
            reader.buffer.clear();
            reader.put_buf(TunnelMessage::open(tunnel_id));
            reader.wake();
        }
    }

    /// Gracefully shut down the tunnel by queuing a close message to the server,
    /// and arrange for an EOF to be raised after the message is sent.
    pub async fn _close(&mut self) {
        self.close_reader(TunnelMessage::close()).await
    }

    async fn close_reader(&mut self, last_msg: TunnelMessage) {
        self.shared_reader.lock().await.close(last_msg);
    }

    async fn close_writer(&mut self) {
        let _ = self.shutdown().await;
    }
}

impl<S> Clone for TunnelClient<S>
where
    S: SessionBuilder<Message = TunnelMessage>,
{
    fn clone(&self) -> Self {
        Self {
            shared_reader: self.shared_reader.clone(),
            shared_writer: self.shared_writer.clone(),
            tunnel_id: self.tunnel_id.clone(),
        }
    }
}

impl<S> AsyncRead for TunnelClient<S>
where
    S: SessionBuilder<Message = TunnelMessage>,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut ReadBuf,
    ) -> Poll<io::Result<()>> {
        let mut reader = ready!(self.get_mut().shared_reader.poll_lock(cx));

        let len = buf.remaining();
        let result = reader.poll_read(cx, buf);
        let amt = len - buf.remaining();

        if matches!(result, Poll::Ready(Ok(()))) {
            log::trace!("TunnelClient::poll_read({len}) -> Ready(Ok({amt}))");
        } else {
            log::trace!("TunnelClient::poll_read({len}) -> {result:?}");
        }

        result
    }
}

impl<S> AsyncWrite for TunnelClient<S>
where
    S: SessionBuilder<Message = TunnelMessage>,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        let mut writer = ready!(self.get_mut().shared_writer.poll_lock(cx));
        let result = writer.poll_write(cx, buf);
        log::trace!("TunnelClient::poll_write({}) -> {result:?}", buf.len());
        result
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        let mut writer = ready!(self.get_mut().shared_writer.poll_lock(cx));
        let result = writer.poll_flush(cx);
        log::trace!("TunnelClient::poll_flush() -> {result:?}");
        result
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        let mut writer = ready!(self.get_mut().shared_writer.poll_lock(cx));
        let result = writer.poll_shutdown(cx);
        log::trace!("TunnelClient::poll_shutdown() -> {result:?}");
        result
    }
}

pub struct TunnelServer<S, C>
where
    S: SessionBuilder<Message = TunnelMessage>,
    C: AsyncConnect<ReadHalf = S::ReadHalf, WriteHalf = S::WriteHalf>,
    C: AsMut<C> + Clone + Send + Unpin,
{
    shared_reader: PollMutex<TunnelReader<S::StreamHalf>>,
    shared_writer: PollMutex<TunnelWriter<S::SinkHalf>>,
    tunnel_id: PollMutex<Option<u64>>,
    connector: C,
}

impl<S, C> TunnelServer<S, C>
where
    S: SessionBuilder<Message = TunnelMessage> + 'static,
    C: AsyncConnect<ReadHalf = S::ReadHalf, WriteHalf = S::WriteHalf>,
    C: AsMut<C> + Clone + Send + Sync + Unpin + 'static,
{
    pub fn new(
        eof_method: TunnelEofMethod,
        connector: C,
        resume_map: Option<AsyncMap<Self>>,
    ) -> Self {
        let (task_tx, task_rx) = PollTaskChannel::channel::<TunnelMessage>(1_000);

        let server = Self {
            shared_reader: PollMutex::new(TunnelReader::new(eof_method)),
            shared_writer: PollMutex::new(TunnelWriter::new(task_tx)),
            tunnel_id: PollMutex::new(None),
            connector,
        };

        server
            .clone()
            .into_background_task_manager(task_rx, resume_map);

        server
    }

    pub async fn id(&mut self) -> Option<u64> {
        *self.tunnel_id.lock().await
    }

    async fn set_id(&mut self, id: Option<u64>) {
        *self.tunnel_id.lock().await = id;
    }

    // Asynchronously handle tasks that require communication across our inner
    // reader and writer, such as open and close tasks from the client side.
    fn into_background_task_manager(
        mut self,
        mut task_chan: PollTaskReceiver<TunnelMessage>,
        mut map: Option<AsyncMap<Self>>,
    ) {
        tokio::spawn(async move {
            while let Some(task) = task_chan.recv().await {
                let msg = &*task;
                match &msg.kind {
                    TunnelMessageKind::Open => self.open(msg.id, map.as_mut()).await,
                    TunnelMessageKind::Opened => {}
                    TunnelMessageKind::Close => {
                        self.close_writer().await;
                        self.close_reader(TunnelMessage::closed()).await;
                    }
                    TunnelMessageKind::Closed => {
                        self.close_writer().await;
                    }
                    TunnelMessageKind::Connect(target) => {
                        self.connect(msg.id, target.clone()).await
                    }
                    TunnelMessageKind::Encapsulate(_) => unreachable!(),
                }
            }
        });
    }

    async fn open(&mut self, tunnel_id: u64, map: Option<&mut AsyncMap<Self>>) {
        if tunnel_id == 0 {
            match map {
                Some(map) => self.open_resumable(map).await,
                None => self.open_oneshot().await,
            }
        } else {
            match map {
                Some(map) => self.resume(map, tunnel_id).await,
                None => self.close().await,
            }
        }
    }

    async fn open_resumable(&mut self, map: &mut AsyncMap<Self>) {
        match self.id().await {
            Some(current_id) => self.open_inner(current_id).await,
            None => {
                let new_id = map.insert(self.clone()).await;
                self.set_id(Some(new_id)).await;
                self.open_inner(new_id).await
            }
        }
    }

    async fn open_oneshot(&mut self) {
        self.open_inner(0).await;
    }

    async fn open_inner(&mut self, tunnel_id: u64) {
        let mut reader = self.shared_reader.lock().await;
        reader.put_buf(TunnelMessage::opened(tunnel_id));
        reader.wake();
    }

    async fn resume(&mut self, map: &mut AsyncMap<Self>, prev_id: u64) {
        match self.id().await {
            Some(_current_id) => self.close().await,
            None => match map.remove(prev_id).await {
                Some(other) => {
                    self.replace_io(other).await;
                    self.open_resumable(map).await;
                }
                None => self.close().await,
            },
        }
    }

    /// Steal the inner io state from the other instance we want to resume.
    async fn replace_io(&mut self, mut other: Self) {
        // Replace the other instance with empty inner io elements.
        let other_reader = {
            std::mem::replace(
                &mut *other.shared_reader.lock().await,
                TunnelReader::new(TunnelEofMethod::OnStreamCount(0)),
            )
        };
        let other_writer = {
            let (task_tx, mut task_rx) = PollTaskChannel::channel::<TunnelMessage>(1);
            task_rx.close();
            std::mem::replace(
                &mut *other.shared_writer.lock().await,
                TunnelWriter::new(task_tx),
            )
        };

        // Wake to propagate an EOF to close the old interpreter.
        other_reader.wake();

        // Replace only the session-related io elements. Don't replace the
        // buffers because previous messages may have been only partially
        // sent/received before connection failure and we want to prevent
        // possible misalignment of message boundaries.
        {
            let mut reader = self.shared_reader.lock().await;
            reader.streams = other_reader.streams;
            reader.n_streams = other_reader.n_streams;
        }
        {
            let mut writer = self.shared_writer.lock().await;
            writer.sinks = other_writer.sinks;
            writer.active = other_writer.active;
            writer.tasks = other_writer.tasks;
        }
    }

    pub async fn close(&mut self) {
        self.close_reader(TunnelMessage::close()).await;
    }

    async fn close_reader(&mut self, last_msg: TunnelMessage) {
        self.shared_reader.lock().await.close(last_msg);
    }

    async fn close_writer(&mut self) {
        let _ = self.shutdown().await;
    }

    async fn connect(&mut self, session_id: u64, target: Socks5Target) {
        let connector = self.connector.clone();
        let session = Session::<S>::from_connector(session_id, connector, target);
        let (stream, sink) = session.into_split();

        // TODO: What if the connection fails? How do we propagate to the client?
        // We should add a unit test.
        //
        // We would need to detect that the connection failed. I think that
        // works on the sink side, because when we try to write we would get an
        // error back (see TunnelWriter::poll_tasks()). But right now we just
        // remove the sink on error. Doesn't the client need to know to stop
        // writing to us?
        // But on the stream side, the error just turns into a None, and then
        // the SelectAll instance will drop it. We don't know when that happened,
        // so we can't tell the client we cannot send anymore even if we wanted.
        //
        // I fear we will end up re-implementing a turbo-like protocol, which already
        // handles shutdowns and half-open connections, etc. We could just rely on
        // that session protocol to handle connection failures too, but our design
        // right now only creates that protocol after the connection is already done.
        //
        // I think the simplest solution is to make the client wait:
        // - client sends CONNECT to server, holds session in a prelim connecting map.
        // - server tries to CONNECT
        //   - if success, sends CONNECTED
        //   - if failure, sends DISCONNECTED
        // - client cannot send or receive app data until it gets the server reply.
        //
        // Other notes:
        // SelectAll is efficient but it does not tell us when streams end. If we want
        // to know that a stream was dropped, we need something like:
        //   https://docs.rs/tokio-stream/latest/tokio_stream/struct.StreamNotifyClose.html
        // We could work a similar concept as a wrapper into our SessionHalf object?
        // The following seem like they could work too, but are inefficient or mor complex.
        //   https://docs.rs/tokio-stream/latest/tokio_stream/struct.StreamMap.html
        //   https://crates.io/crates/mapped_futures

        self.shared_reader.lock().await.add(session_id, stream);
        self.shared_writer.lock().await.add(session_id, sink);
    }

    /// Add a session that is already connected to its apps io channels.
    #[cfg(test)]
    async fn add_session(&mut self, src: S::ReadHalf, dst: S::WriteHalf, id: u64) {
        // `id` here is the session id, not the tunnel id.
        let (stream, sink) = Session::<S>::from_io(id, src, dst).into_split();
        self.shared_reader.lock().await.add(id, stream);
        self.shared_writer.lock().await.add(id, sink);
    }
}

impl<S, C> Clone for TunnelServer<S, C>
where
    S: SessionBuilder<Message = TunnelMessage>,
    C: AsyncConnect<ReadHalf = S::ReadHalf, WriteHalf = S::WriteHalf>,
    C: AsMut<C> + Clone + Send + Unpin,
{
    fn clone(&self) -> Self {
        Self {
            shared_reader: self.shared_reader.clone(),
            shared_writer: self.shared_writer.clone(),
            tunnel_id: self.tunnel_id.clone(),
            connector: self.connector.clone(),
        }
    }
}

impl<S, C> AsyncRead for TunnelServer<S, C>
where
    S: SessionBuilder<Message = TunnelMessage>,
    C: AsyncConnect<ReadHalf = S::ReadHalf, WriteHalf = S::WriteHalf>,
    C: AsMut<C> + Clone + Send + Unpin,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut ReadBuf,
    ) -> Poll<io::Result<()>> {
        let mut reader = ready!(self.get_mut().shared_reader.poll_lock(cx));

        let len = buf.remaining();
        let result = reader.poll_read(cx, buf);
        let amt = len - buf.remaining();

        if matches!(result, Poll::Ready(Ok(()))) {
            log::trace!("TunnelServer::poll_read({len}) -> Ready(Ok({amt}))");
        } else {
            log::trace!("TunnelServer::poll_read({len}) -> {result:?}");
        }

        result
    }
}

impl<S, C> AsyncWrite for TunnelServer<S, C>
where
    S: SessionBuilder<Message = TunnelMessage>,
    C: AsyncConnect<ReadHalf = S::ReadHalf, WriteHalf = S::WriteHalf>,
    C: AsMut<C> + Clone + Send + Unpin,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        let mut writer = ready!(self.get_mut().shared_writer.poll_lock(cx));
        let result = writer.poll_write(cx, buf);
        log::trace!("TunnelServer::poll_write({}) -> {result:?}", buf.len());
        result
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        let mut writer = ready!(self.get_mut().shared_writer.poll_lock(cx));
        let result = writer.poll_flush(cx);
        log::trace!("TunnelServer::poll_flush() -> {result:?}");
        result
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        let mut writer = ready!(self.get_mut().shared_writer.poll_lock(cx));
        let result = writer.poll_shutdown(cx);
        log::trace!("TunnelServer::poll_shutdown() -> {result:?}");
        result
    }
}

struct TunnelReader<T>
where
    T: Stream<Item = TunnelMessage> + Send + Unpin,
{
    streams: SelectAll<SessionHalf<T>>,
    waker: Option<Waker>,
    buffer: BytesMut,
    n_streams: usize,
    eof_method: TunnelEofMethod,
}

impl<T> TunnelReader<T>
where
    T: Stream<Item = TunnelMessage> + Send + Unpin,
{
    fn new(eof_method: TunnelEofMethod) -> Self {
        Self {
            streams: SelectAll::new(),
            waker: None,
            buffer: BytesMut::new(),
            n_streams: 0,
            eof_method,
        }
    }

    fn add(&mut self, _id: u64, stream: SessionHalf<T>) {
        self.streams.push(stream);
        self.n_streams += 1;
        self.wake();
    }

    /// Buffer the message for sending to the remote side of the tunnel.
    ///
    /// # Panics
    ///
    /// Panics if the payload inside the message is larger than supported by the
    /// codec, which can be considered a programming error.
    fn put_buf(&mut self, msg: TunnelMessage) {
        TunnelCodec.encode(msg, &mut self.buffer).unwrap();
    }

    fn set_eof(&mut self) {
        self.eof_method = TunnelEofMethod::OnStreamCount(0);
    }

    /// Store the latest waker so we can wake any pending tasks when async changes occur.
    fn set_waker(&mut self, cx: &mut Context) {
        match self.waker.as_mut() {
            Some(w) => w.clone_from(cx.waker()),
            None => self.waker = Some(cx.waker().clone()),
        }
    }

    fn wake(&self) {
        // Wake the waker it one exists, without dropping our handle.
        if let Some(w) = self.waker.as_ref() {
            w.wake_by_ref()
        }
        // Instead, we could wake the waker and drop the handle to prevent multiple wake-ups.
        // self.waker.take().map(|w| w.wake());
    }

    fn poll_read(&mut self, cx: &mut Context, buf: &mut ReadBuf) -> Poll<io::Result<()>> {
        // If we already have enough cached bytes to fill the ReadBuf, return quickly.
        if self.buffer.len() >= buf.remaining() {
            let at = buf.remaining();
            let bytes = self.buffer.split_to(at).freeze();
            buf.put_slice(&bytes);
            log::trace!("poll_read(): provided {at} cached bytes");
            return Poll::Ready(Ok(()));
        }

        // Store the waker in case a new stream arrives while we are in a Pending state.
        self.set_waker(cx);

        // Read as many messages as needed to fill the ReadBuf if we can.
        while self.buffer.len() < buf.remaining() {
            match self.streams.poll_next_unpin(cx) {
                Poll::Ready(Some(msg)) => self.put_buf(msg),
                // All streams are closed; a wakeup will occur when a pending one arrives.
                Poll::Ready(None) => break,
                // All streams are pending, a wakeup will occur when a message is ready.
                Poll::Pending => break,
            }
        }

        if !self.buffer.is_empty() {
            // We may or may not be able to fill the ReadBuf, but we provide what we have.
            let at = self.buffer.len().min(buf.remaining());
            let bytes = self.buffer.split_to(at).freeze();
            buf.put_slice(&bytes);
            Poll::Ready(Ok(()))
        } else if self.streams.is_empty() && self.eof_method.is_eof(self.n_streams) {
            // This is an EOF since we did not add to the ReadBuf.
            log::debug!("poll_read(): EOF: empty read buf and no streams remaining");
            Poll::Ready(Ok(()))
        } else {
            // New streams, or new message on existing streams, may arrive in the future.
            Poll::Pending
        }
    }

    fn close(&mut self, last_msg: TunnelMessage) {
        self.streams.clear();
        self.put_buf(last_msg);
        self.set_eof();
        self.wake();
    }
}

struct TunnelWriter<T>
where
    T: Sink<TunnelMessage> + Send + Unpin,
{
    sinks: HashMap<u64, SessionHalf<T>>,
    active: HashSet<u64>,
    waker: Option<Waker>,
    buffer: BytesMut,
    tasks: VecDeque<Task>,
    task_channel: PollTaskSender<TunnelMessage>,
}

impl<T> TunnelWriter<T>
where
    T: Sink<TunnelMessage, Error = io::Error> + Send + Unpin,
{
    fn new(task_channel: PollTaskSender<TunnelMessage>) -> Self {
        Self {
            sinks: HashMap::new(),
            active: HashSet::new(),
            waker: None,
            buffer: BytesMut::new(),
            tasks: VecDeque::new(),
            task_channel,
        }
    }

    fn generate_session_id(&self) -> u64 {
        let mut rng = ThreadRng::default();
        loop {
            let id = rng.next_u64();
            if id > 0 && !self.sinks.contains_key(&id) {
                return id;
            }
        }
    }

    fn add(&mut self, id: u64, sink: SessionHalf<T>) {
        if let Entry::Vacant(e) = self.sinks.entry(id) {
            e.insert(sink);
            self.wake();
        } else {
            log::warn!("Cannot add sink at existing session id {id}");
        }
    }

    /// Store the latest waker so we can wake any pending tasks when async changes occur.
    fn set_waker(&mut self, cx: &mut Context) {
        match self.waker.as_mut() {
            Some(w) => w.clone_from(cx.waker()),
            None => self.waker = Some(cx.waker().clone()),
        }
    }

    fn wake(&self) {
        // Wake the waker it one exists, without dropping our handle.
        if let Some(w) = self.waker.as_ref() {
            w.wake_by_ref()
        }
        // Instead, we could wake the waker and drop the handle to prevent multiple wake-ups.
        // maybe_waker.take().map(|w| w.wake());
    }

    fn poll_write(&mut self, cx: &mut Context, buf: &[u8]) -> Poll<Result<usize, io::Error>> {
        self.set_waker(cx);

        // Don't accept new data until our pending task queue is ready.
        ready!(self.poll_tasks(cx));

        // 'Write' all of the bytes, do not return Poll::Pending after this.
        self.buffer.extend_from_slice(buf);

        // Decode all available messages into our task queue.
        while !self.buffer.is_empty() {
            let msg = match TunnelCodec.decode(&mut self.buffer) {
                Ok(Some(msg)) => msg,
                Ok(None) => break,
                Err(e) => return Poll::Ready(Err(e)),
            };
            self.tasks.push_back(Task::new(msg));
        }

        // Opportunistically process as many tasks as we can right now. Ignore a
        // Poll::Pending result, we'll handle it on the next write/flush/shutdown.
        let _ = self.poll_tasks(cx);
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(&mut self, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        self.set_waker(cx);
        ready!(self.poll_tasks(cx));

        for id in self.active.drain().collect::<Vec<u64>>() {
            if let Some(sink) = self.sinks.get_mut(&id) {
                match sink.poll_flush_unpin(cx) {
                    Poll::Ready(Ok(_)) => {}
                    Poll::Ready(Err(_)) => {
                        self.sinks.remove(&id);
                    }
                    Poll::Pending => {
                        self.active.insert(id);
                    }
                };
            };
        }

        if self.active.is_empty() {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }

    fn poll_shutdown(&mut self, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        self.set_waker(cx);
        ready!(self.poll_tasks(cx));

        for id in self.sinks.keys().copied().collect::<Vec<u64>>() {
            if let Some(sink) = self.sinks.get_mut(&id)
                && sink.poll_close_unpin(cx).is_pending()
            {
                continue;
            }
            self.sinks.remove(&id);
        }

        if self.sinks.is_empty() {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }

    fn poll_tasks(&mut self, cx: &mut Context) -> Poll<()> {
        // Note: we currently use a simple queue and process tasks one at a
        // time. This guarantees that any open messages are handled before later
        // encapsulated messages. If we change the queue in the future, we need
        // to uphold this guarantee to make sure we don't drop a session's
        // messages as we wait for its connection to complete.
        while let Some(mut task) = self.tasks.pop_front() {
            match task.poll(cx, &mut self.task_channel, self.sinks.get_mut(&task.id)) {
                Poll::Ready(Ok(_)) => {
                    self.active.insert(task.id);
                }
                Poll::Ready(Err(_)) => {
                    self.sinks.remove(&task.id);
                }
                Poll::Pending => {
                    self.tasks.push_front(task);
                    return Poll::Pending;
                }
            }
        }
        Poll::Ready(())
    }
}

struct Task {
    id: u64,
    state: Option<TaskState>,
}

enum TaskState {
    Unprocessed(TunnelMessage),
    ChannelReserve(TunnelMessage),
    ChannelSend(TunnelMessage),
    ChannelWait,
    SinkReady(TunnelMessage),
    SinkSend(TunnelMessage),
}

impl Task {
    fn new(msg: TunnelMessage) -> Self {
        Self {
            id: msg.id,
            state: Some(TaskState::Unprocessed(msg)),
        }
    }

    fn poll<T: Sink<TunnelMessage, Error = io::Error> + Send + Unpin>(
        &mut self,
        cx: &mut Context,
        channel: &mut PollTaskSender<TunnelMessage>,
        mut sink: Option<&mut SessionHalf<T>>,
    ) -> Poll<Result<(), io::Error>> {
        while let Some(state) = self.state.take() {
            match state {
                TaskState::Unprocessed(msg) => {
                    match &msg.kind {
                        TunnelMessageKind::Encapsulate(_) => {
                            self.state = Some(TaskState::SinkReady(msg))
                        }
                        _ => self.state = Some(TaskState::ChannelReserve(msg)),
                    };
                }
                TaskState::ChannelReserve(msg) => match channel.poll_reserve(cx) {
                    Poll::Pending => {
                        self.state = Some(TaskState::ChannelReserve(msg));
                        return Poll::Pending;
                    }
                    Poll::Ready(Ok(_)) => self.state = Some(TaskState::ChannelSend(msg)),
                    Poll::Ready(Err(_)) => {
                        return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
                    }
                },
                TaskState::ChannelSend(msg) => match channel.send(msg) {
                    Ok(_) => self.state = Some(TaskState::ChannelWait),
                    Err(_) => {
                        return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
                    }
                },
                TaskState::ChannelWait => match channel.poll_wait(cx) {
                    Poll::Pending => {
                        self.state = Some(TaskState::ChannelWait);
                        return Poll::Pending;
                    }
                    Poll::Ready(_) => {}
                },
                TaskState::SinkReady(msg) => match sink.as_mut() {
                    Some(tx) => match tx.poll_ready_unpin(cx) {
                        Poll::Pending => {
                            self.state = Some(TaskState::SinkReady(msg));
                            return Poll::Pending;
                        }
                        Poll::Ready(Ok(_)) => self.state = Some(TaskState::SinkSend(msg)),
                        Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                    },
                    None => {
                        log::debug!("Missing sink for message from session {}, dropping", msg.id)
                    }
                },
                TaskState::SinkSend(msg) => match sink.as_mut() {
                    Some(tx) => match tx.start_send_unpin(msg) {
                        Ok(_) => {}
                        Err(e) => return Poll::Ready(Err(e)),
                    },
                    None => {
                        log::debug!("Missing sink for message from session {}, dropping", msg.id)
                    }
                },
            }
        }
        Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
pub mod tests {
    use std::time::Duration;

    use bytes::Bytes;
    use futures::stream::{self, SelectAll};
    use futures::{SinkExt, StreamExt};
    use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
    use tokio_util::codec::Framed;

    use crate::common::mock::{self, MockConnector, MockIo, MockProxy, MockProxyNetwork};
    use crate::common::sync::AsyncMap;
    use crate::lang::ir::test::basic_enc::EncryptedLengthPayloadSpec;
    use crate::lang::{self, Role};
    use crate::net::proto::BytesSession;
    use crate::net::proto::tunnel::codec::TunnelCodec;
    use crate::net::proto::tunnel::message::{TunnelMessage, TunnelMessageKind};
    use crate::net::session::SessionBuilder;
    use crate::net::tunnel::TunnelEofMethod;
    use crate::net::{AsyncConnect, AsyncConnectExt, TunnelClient, TunnelServer};

    const TEST_ID: u64 = 1234567890;

    #[tokio::test]
    async fn select_all_is_reusable() {
        let stream1 = Box::new(stream::iter(vec![10]));
        let stream2 = Box::new(stream::iter(vec![1]));

        let mut all_streams = SelectAll::new();

        assert!(all_streams.is_empty());
        assert_eq!(all_streams.len(), 0);

        all_streams.push(stream1);
        assert_eq!(all_streams.len(), 1);

        assert!(all_streams.next().await.is_some());
        assert_eq!(all_streams.len(), 1);
        assert!(all_streams.next().await.is_none());
        assert_eq!(all_streams.len(), 0);

        assert!(all_streams.next().await.is_none());
        assert!(all_streams.next().await.is_none());
        assert!(all_streams.next().await.is_none());

        all_streams.push(stream2);
        assert_eq!(all_streams.len(), 1);

        assert!(all_streams.next().await.is_some());
        assert_eq!(all_streams.len(), 1);
        assert!(all_streams.next().await.is_none());
        assert_eq!(all_streams.len(), 0);
    }

    #[tokio::test]
    async fn valid_server_resume() {
        // Provides the remote, destination side socket.
        let mut connector = MockConnector::new(None);
        let mut remote = connector.remote_socket().unwrap();

        // Test tunnel resumption, sub-protocol is not important.
        let map = AsyncMap::new();
        let mut tunnel1 = TunnelServer::<BytesSession<_, _>, _>::new(
            TunnelEofMethod::OnStreamCount(1),
            connector,
            Some(map.clone()),
        );

        // Get a nicer interface for exchanging messages with the tunnel.
        let mut framed1 = Framed::new(tunnel1.clone(), TunnelCodec);

        // Mock deliver some message from client side.
        framed1.send(TunnelMessage::open(0)).await.unwrap();
        let reply = framed1.next().await.unwrap().unwrap();

        // We should get a positive tunnel id assigned by the server.
        assert_eq!(reply.kind, TunnelMessageKind::Opened);
        assert!(reply.id > 0);
        let tunnel1_id = reply.id;

        // Add a new stream to the tunnel, with some payload.
        let target = MockConnector::default_target();
        framed1
            .send(TunnelMessage::connect(TEST_ID, target))
            .await
            .unwrap();
        framed1
            .send(TunnelMessage::encapsulate(TEST_ID, Bytes::from("hello1")))
            .await
            .unwrap();
        assert_eq!(tunnel1.shared_reader.lock().await.streams.len(), 1);

        // Now lets say channel broke, and we want to resume over a new channel.
        // On the server, this is a new connection so it creates a new tunnel.
        let mut tunnel2 = TunnelServer::<BytesSession<_, _>, _>::new(
            TunnelEofMethod::OnStreamCount(1),
            MockConnector::default(),
            Some(map.clone()),
        );
        assert_eq!(tunnel2.shared_reader.lock().await.streams.len(), 0);

        // Now we can resume by opening with the previous tunnel id.
        let mut framed2 = Framed::new(tunnel2.clone(), TunnelCodec);
        framed2.send(TunnelMessage::open(tunnel1_id)).await.unwrap();
        let reply = framed2.next().await.unwrap().unwrap();

        // As before, the reply should be a new id assigned to tunnel2.
        assert_eq!(reply.kind, TunnelMessageKind::Opened);
        assert_ne!(reply.id, 0);
        let _tunnel2_id = reply.id;

        // The stream from tunnel1 should now belong to tunnel2.
        assert_eq!(tunnel1.shared_reader.lock().await.streams.len(), 0);
        assert_eq!(tunnel2.shared_reader.lock().await.streams.len(), 1);

        // Tunnel1 is basically dead now. Another read emits an EOF.
        // Normally, this would be the signal to stop its interpreter.
        assert_eq!(tunnel1.read(&mut vec![0u8; 1]).await.unwrap(), 0);

        // Now when we send, it should go to the original stream endpoint.
        framed2
            .send(TunnelMessage::encapsulate(TEST_ID, Bytes::from("hello2")))
            .await
            .unwrap();

        let mut buf = vec![0u8; 64];
        let len = remote.reader.read(&mut buf).await.unwrap();
        assert_eq!(len, 12);
        assert_eq!(&buf[0..12], b"hello1hello2");
    }

    #[tokio::test]
    async fn invalid_server_resume() {
        for maybe_map in [Some(AsyncMap::new()), None] {
            let tunnel = TunnelServer::<BytesSession<_, _>, _>::new(
                TunnelEofMethod::OnStreamCount(1),
                MockConnector::default(),
                maybe_map.clone(),
            );

            // If we request to open a non-zero tunnel id that does not exist,
            // the tunnel should try to close.
            let mut framed = Framed::new(tunnel.clone(), TunnelCodec);
            let resume_id = TEST_ID;
            framed.send(TunnelMessage::open(resume_id)).await.unwrap();
            let reply = framed.next().await.unwrap().unwrap();

            assert_eq!(reply.kind, TunnelMessageKind::Close);
            assert_eq!(reply.id, 0);
        }
    }

    fn wrapped_client_proxy<R, W, S>(app_io: TunnelClient<S>, net_io: MockIo) -> MockProxy
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
        S: SessionBuilder<ReadHalf = R, WriteHalf = W, Message = TunnelMessage> + 'static,
    {
        let (tunnel_r, tunnel_w) = (app_io.clone(), app_io);
        let wrapped_app = MockIo::new(tunnel_r, tunnel_w);
        MockProxy::new(wrapped_app, net_io)
    }

    fn wrapped_server_proxy<R, W, S, C>(app_io: TunnelServer<S, C>, net_io: MockIo) -> MockProxy
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
        S: SessionBuilder<ReadHalf = R, WriteHalf = W, Message = TunnelMessage> + 'static,
        C: AsyncConnectExt<ReadHalf = R, WriteHalf = W> + Clone + 'static,
    {
        let (tunnel_r, tunnel_w) = (app_io.clone(), app_io);
        let wrapped_app = MockIo::new(tunnel_r, tunnel_w);
        MockProxy::new(wrapped_app, net_io)
    }

    #[derive(Clone, Copy)]
    pub enum MockIoKind {
        Direct,
        Interpreter,
    }

    enum MockSocketKind {
        Connected,
        Disconnected(MockConnector),
    }

    async fn connected_client<S>(
        io_kind: MockIoKind,
        proxy: MockProxy,
    ) -> (lang::Result<()>, lang::Result<()>)
    where
        S: SessionBuilder<
                Message = TunnelMessage,
                ReadHalf = <MockConnector as AsyncConnect>::ReadHalf,
                WriteHalf = <MockConnector as AsyncConnect>::WriteHalf,
            > + 'static,
    {
        let (src, dst) = (proxy.app.reader, proxy.app.writer);

        let mut tunnel: TunnelClient<S> = TunnelClient::new(TunnelEofMethod::OnStreamCount(1));

        let target = MockConnector::default_target();
        tunnel
            .add_session_inner(src, dst, Some(target), Some(TEST_ID))
            .await;

        match io_kind {
            MockIoKind::Direct => {
                mock::io_copy_direct(None::<u8>, wrapped_client_proxy(tunnel, proxy.net)).await
            }
            MockIoKind::Interpreter => {
                let protospec = EncryptedLengthPayloadSpec::new(Role::Client);
                mock::io_copy_interpreter(protospec, wrapped_client_proxy(tunnel, proxy.net)).await
            }
        }
    }

    async fn server<S>(
        args: (MockIoKind, MockSocketKind),
        proxy: MockProxy,
    ) -> (lang::Result<()>, lang::Result<()>)
    where
        S: SessionBuilder<
                Message = TunnelMessage,
                ReadHalf = <MockConnector as AsyncConnect>::ReadHalf,
                WriteHalf = <MockConnector as AsyncConnect>::WriteHalf,
            > + 'static,
    {
        let (io_kind, sock_kind) = args;
        let (src, dst) = (proxy.app.reader, proxy.app.writer);

        let tunnel: TunnelServer<S, MockConnector> = match sock_kind {
            MockSocketKind::Connected => {
                // We are connected and want to end when this connection is done.
                let mut tunnel = TunnelServer::new(
                    TunnelEofMethod::OnStreamCount(1),
                    MockConnector::default(),
                    None,
                );
                tunnel.add_session(src, dst, TEST_ID).await;
                tunnel
            }
            MockSocketKind::Disconnected(connector) => {
                // We are disconnected, we need to hold the tunnel open until
                // the first stream from the client is complete.
                TunnelServer::new(TunnelEofMethod::OnStreamCount(1), connector, None)
            }
        };

        match io_kind {
            MockIoKind::Direct => {
                mock::io_copy_direct(None::<u8>, wrapped_server_proxy(tunnel, proxy.net)).await
            }
            MockIoKind::Interpreter => {
                let protospec = EncryptedLengthPayloadSpec::new(Role::Server);
                mock::io_copy_interpreter(protospec, wrapped_server_proxy(tunnel, proxy.net)).await
            }
        }
    }

    pub async fn proxy_network_connected<S>(io_kind: MockIoKind, len: usize) -> mock::Result
    where
        S: SessionBuilder<
                Message = TunnelMessage,
                ReadHalf = <MockConnector as AsyncConnect>::ReadHalf,
                WriteHalf = <MockConnector as AsyncConnect>::WriteHalf,
            > + 'static,
    {
        MockProxyNetwork::new(len)
            .run_with_forwarder(
                io_kind,
                &connected_client::<S>,
                (io_kind, MockSocketKind::Connected),
                &server::<S>,
            )
            .await
    }

    pub async fn proxy_network_disconnected<S>(io_kind: MockIoKind, len: usize) -> mock::Result
    where
        S: SessionBuilder<
                Message = TunnelMessage,
                ReadHalf = <MockConnector as AsyncConnect>::ReadHalf,
                WriteHalf = <MockConnector as AsyncConnect>::WriteHalf,
            > + 'static,
    {
        let mut mpn = MockProxyNetwork::new(len);

        // The connector will hook up the server side proxy to a new remote socket.
        let mut conn = MockConnector::new(Some(Duration::from_millis(10)));
        let new_remote = conn.remote_socket().unwrap();

        // We need to copy io between that new remote socket and the original
        // socket that the proxy used to connect to its app payload.
        mpn.replace_server_app_io(new_remote);

        // Now we can run the test.
        mpn.run_with_forwarder(
            io_kind,
            &connected_client::<S>,
            (io_kind, MockSocketKind::Disconnected(conn)),
            &server::<S>,
        )
        .await
    }
}
