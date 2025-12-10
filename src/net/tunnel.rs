use std::collections::{HashMap, VecDeque};
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker, ready};

use bytes::BytesMut;
use futures::stream::{SelectAll, StreamExt};
use futures::{FutureExt, Sink, SinkExt, Stream};
use rand::RngCore;
use rand::rngs::ThreadRng;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::Notify;
use tokio::sync::mpsc::error::SendError;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio_util::codec::{Decoder, Encoder};

use crate::common::sync::PollMutex;
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
}

impl<S> TunnelClient<S>
where
    S: SessionBuilder<Message = TunnelMessage>,
{
    pub fn new(eof_method: TunnelEofMethod) -> Self {
        Self {
            shared_reader: PollMutex::new(TunnelReader::new(eof_method)),
            shared_writer: PollMutex::new(TunnelWriter::new(None)),
        }
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
            reader.add(stream);
            reader.put_buf(TunnelMessage::open(id, target));
            reader.wake();
        }
    }

    pub async fn _close(&mut self) {
        let mut reader = self.shared_reader.lock().await;
        reader.put_buf(TunnelMessage::close());
        reader.set_eof();
        reader.wake();
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
    connector: C,
}

impl<S, C> TunnelServer<S, C>
where
    S: SessionBuilder<Message = TunnelMessage> + 'static,
    C: AsyncConnect<ReadHalf = S::ReadHalf, WriteHalf = S::WriteHalf>,
    C: AsMut<C> + Clone + Send + Unpin + 'static,
{
    pub fn new(eof_method: TunnelEofMethod, connector: C) -> Self {
        let (task_tx, task_rx) = mpsc::channel(1_000);

        let server = Self {
            shared_reader: PollMutex::new(TunnelReader::new(eof_method)),
            shared_writer: PollMutex::new(TunnelWriter::new(Some(task_tx))),
            connector,
        };

        server.clone().run_background_task_manager(task_rx);
        server
    }

    // Asynchronously handle tasks that require communication across our inner
    // reader and writer, such as open and close tasks from the client side.
    fn run_background_task_manager(mut self, mut task_chan: Receiver<AsyncTask>) {
        tokio::spawn(async move {
            while let Some(task) = task_chan.recv().await {
                match task {
                    AsyncTask::Open((notify, id, target)) => {
                        let connector = self.connector.clone();
                        let session = Session::<S>::from_connector(id, connector, target);
                        let (stream, sink) = session.into_split();

                        self.shared_reader.lock().await.add(stream);
                        self.shared_writer.lock().await.add(id, sink);

                        notify.notify_one();
                    }
                    AsyncTask::Close(notify) => {
                        self.close().await;
                        notify.notify_one();
                    }
                }
            }
        });
    }

    /// Add a session that is already connected to its apps io channels.
    #[cfg(test)]
    async fn add_session(&mut self, src: S::ReadHalf, dst: S::WriteHalf, id: u64) {
        let (stream, sink) = Session::<S>::from_io(id, src, dst).into_split();
        self.shared_reader.lock().await.add(stream);
        self.shared_writer.lock().await.add(id, sink);
    }

    pub async fn close(&mut self) {
        let mut reader = self.shared_reader.lock().await;
        reader.put_buf(TunnelMessage::close());
        reader.set_eof();
        reader.wake();
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

    fn add(&mut self, stream: SessionHalf<T>) {
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

    fn wake(&self) {
        // Wake the waker it one exists, without dropping our handle.
        self.waker.as_ref().and_then(|w| Some(w.clone().wake()));
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
        match self.waker.as_mut() {
            Some(w) => w.clone_from(cx.waker()),
            None => self.waker = Some(cx.waker().clone()),
        }

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

        if self.buffer.len() > 0 {
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
}

struct TunnelWriter<T>
where
    T: Sink<TunnelMessage> + Send + Unpin,
{
    sinks: HashMap<u64, SessionHalf<T>>,
    waker: Option<Waker>,
    buffer: BytesMut,
    tasks: VecDeque<Task>,
    task_channel: Option<Sender<AsyncTask>>,
}

impl<T> TunnelWriter<T>
where
    T: Sink<TunnelMessage, Error = io::Error> + Send + Unpin,
{
    fn new(task_channel: Option<Sender<AsyncTask>>) -> Self {
        Self {
            sinks: HashMap::new(),
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
        if !self.sinks.contains_key(&id) {
            self.sinks.insert(id, sink);
            self.wake();
        } else {
            log::warn!("Cannot add sink at existing session id {id}");
        }
    }

    fn wake(&self) {
        // Wake the waker it one exists, without dropping our handle.
        self.waker.as_ref().and_then(|w| Some(w.clone().wake()));
        // Instead, we could wake the waker and drop the handle to prevent multiple wake-ups.
        // maybe_waker.take().map(|w| w.wake());
    }

    fn poll_write(&mut self, cx: &mut Context, buf: &[u8]) -> Poll<Result<usize, io::Error>> {
        // Store the waker in case a new sink arrives while we are in a Pending state.
        match self.waker.as_mut() {
            Some(w) => w.clone_from(cx.waker()),
            None => self.waker = Some(cx.waker().clone()),
        }

        // Don't accept new data until our pending task queue is ready.
        match self.poll_tasks(cx) {
            Poll::Ready(Ok(_)) => {}
            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
            Poll::Pending => return Poll::Pending,
        }

        // 'Write' all of the bytes, do not return Poll::Pending after this.
        self.buffer.extend_from_slice(buf);

        // Decode all available messages into our task queue.
        loop {
            let msg = match TunnelCodec.decode(&mut self.buffer) {
                Ok(Some(msg)) => msg,
                Ok(None) => break,
                Err(e) => return Poll::Ready(Err(e)),
            };
            self.tasks.push_back(Task::new(msg));
        }

        // Opportunistically process as many tasks as we can right now. Ignore a
        // Poll::Pending result, we'll handle it on the next call to poll_write.
        if let Poll::Ready(Err(e)) = self.poll_tasks(cx) {
            Poll::Ready(Err(e))
        } else {
            Poll::Ready(Ok(buf.len()))
        }
    }

    fn poll_tasks(&mut self, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        // Note: we currently use a simple queue and process tasks one at a
        // time. This guarantees that any open messages are handled before later
        // encapsulated messages. If we change the queue in the future, we need
        // to uphold this guarantee to make sure we don't drop a session's
        // messages as we wait for its connection to complete.
        while let Some(mut task) = self.tasks.pop_front() {
            match task.poll(cx, self.sinks.get_mut(&task.id), self.task_channel.as_mut()) {
                Poll::Ready(Ok(_)) => {}
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Pending => {
                    self.tasks.push_front(task);
                    return Poll::Pending;
                }
            }
        }
        Poll::Ready(Ok(()))
    }

    fn poll_flush(&mut self, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        match self.poll_tasks(cx) {
            Poll::Ready(Ok(_)) => self.poll_all_sinks(cx, PollOperation::Flush),
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_shutdown(&mut self, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        match self.poll_tasks(cx) {
            Poll::Ready(Ok(_)) => self.poll_all_sinks(cx, PollOperation::Shutdown),
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_all_sinks(
        &mut self,
        cx: &mut Context,
        op: PollOperation,
    ) -> Poll<Result<(), io::Error>> {
        // Run op on all of the inner sinks, tracking if any are pending and which have error.
        let mut error_ids = vec![];
        let mut has_pending = false;

        for (id, sink) in self.sinks.iter_mut() {
            let result = match op {
                PollOperation::Flush => sink.poll_flush_unpin(cx),
                PollOperation::Shutdown => sink.poll_close_unpin(cx),
            };

            match result {
                Poll::Ready(Ok(_)) => {}
                Poll::Ready(Err(e)) => {
                    log::debug!("poll_all_sinks({op:?}): sink error: {e}");
                    error_ids.push(*id);
                }
                Poll::Pending => has_pending = true,
            }
        }

        // Drop the sinks that are in an error state.
        for id in error_ids {
            self.sinks.remove(&id);
        }

        // We are pending if any sink operations are pending.
        if has_pending {
            Poll::Pending
        } else {
            Poll::Ready(Ok(()))
        }
    }
}

#[derive(Debug)]
enum PollOperation {
    Flush,
    Shutdown,
}

struct Task {
    id: u64,
    state: Option<TaskState>,
}

enum TaskState {
    Unprocessed(TunnelMessage),
    ChannelSend(AsyncTaskSender),
    SinkSend(TunnelMessage),
}

impl Task {
    fn new(msg: TunnelMessage) -> Self {
        Self {
            id: msg.session_id,
            state: Some(TaskState::Unprocessed(msg)),
        }
    }

    fn poll<T: Sink<TunnelMessage, Error = io::Error> + Send + Unpin>(
        &mut self,
        cx: &mut Context,
        mut sink: Option<&mut SessionHalf<T>>,
        mut channel: Option<&mut Sender<AsyncTask>>,
    ) -> Poll<Result<(), io::Error>> {
        while let Some(state) = self.state.take() {
            match state {
                TaskState::Unprocessed(msg) => {
                    match &msg.kind {
                        TunnelMessageKind::Open(target) => {
                            if let Some(tx) = channel {
                                let task = AsyncTask::open(msg.session_id, target);
                                let sender = AsyncTaskSender::new(tx.clone(), task);
                                self.state = Some(TaskState::ChannelSend(sender));
                                channel = Some(tx);
                            } else {
                                log::debug!("Missing channel for open task, dropping");
                            }
                        }
                        TunnelMessageKind::Encapsulated(_) => {
                            self.state = Some(TaskState::SinkSend(msg));
                        }
                        TunnelMessageKind::Close => {
                            if let Some(tx) = channel {
                                let task = AsyncTask::close();
                                let sender = AsyncTaskSender::new(tx.clone(), task);
                                self.state = Some(TaskState::ChannelSend(sender));
                                channel = Some(tx);
                            } else {
                                log::debug!("Missing channel for close task, dropping");
                            }
                        }
                    };
                }
                TaskState::ChannelSend(mut sender) => match sender.poll(cx) {
                    Poll::Ready(Ok(_)) => {}
                    Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                    Poll::Pending => {
                        self.state = Some(TaskState::ChannelSend(sender));
                        return Poll::Pending;
                    }
                },
                TaskState::SinkSend(msg) => {
                    if let Some(tx) = sink {
                        match tx.poll_ready_unpin(cx) {
                            Poll::Ready(Ok(_)) => match tx.start_send_unpin(msg) {
                                Ok(_) => {}
                                Err(e) => return Poll::Ready(Err(e)),
                            },
                            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                            Poll::Pending => {
                                self.state = Some(TaskState::SinkSend(msg));
                                return Poll::Pending;
                            }
                        }
                        sink = Some(tx);
                    } else {
                        log::debug!(
                            "Missing sink for message from session {}, dropping",
                            msg.session_id
                        );
                    }
                }
            }
        }
        Poll::Ready(Ok(()))
    }
}

#[derive(Debug)]
enum AsyncTask {
    Open((Arc<Notify>, u64, Socks5Target)),
    Close(Arc<Notify>),
}

impl AsyncTask {
    fn open(id: u64, target: &Socks5Target) -> Self {
        let notify = Arc::new(Notify::new());
        Self::Open((notify, id, target.clone()))
    }

    fn close() -> Self {
        let notify = Arc::new(Notify::new());
        Self::Close(notify)
    }

    fn clone_notify(&self) -> Arc<Notify> {
        match self {
            AsyncTask::Open((notify, _, _)) => notify.clone(),
            AsyncTask::Close(notify) => notify.clone(),
        }
    }
}

struct AsyncTaskSender {
    send_future: Option<Pin<Box<dyn Future<Output = Result<(), SendError<AsyncTask>>> + Send>>>,
    notify_future: Option<Pin<Box<dyn Future<Output = ()> + Send>>>,
}

impl AsyncTaskSender {
    fn new(channel: Sender<AsyncTask>, task: AsyncTask) -> Self {
        let notify = task.clone_notify();
        let notify_future = async move { notify.notified().await };

        let send_future = async move { channel.send(task).await };

        Self {
            send_future: Some(Box::pin(send_future)),
            notify_future: Some(Box::pin(notify_future)),
        }
    }

    fn poll(&mut self, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        // Check that it successfully finished sending.
        if let Some(mut future) = self.send_future.take() {
            match future.poll_unpin(cx) {
                Poll::Ready(Ok(_)) => {}
                Poll::Ready(Err(_e)) => return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into())),
                Poll::Pending => {
                    self.send_future = Some(future);
                    return Poll::Pending;
                }
            }
        }

        // Check that the task was successfully asynchronously executed.
        if let Some(mut future) = self.notify_future.take() {
            match future.poll_unpin(cx) {
                Poll::Ready(_) => {}
                Poll::Pending => {
                    self.notify_future = Some(future);
                    return Poll::Pending;
                }
            }
        }

        // Ready!
        Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use futures::StreamExt;
    use futures::stream::{self, SelectAll};
    use tokio::io::{AsyncRead, AsyncWrite};

    use crate::common::mock::{self, MockConnector, MockIo, MockProxy, MockProxyNetwork};
    use crate::lang::Role;
    use crate::lang::ir::test::basic_enc::EncryptedLengthPayloadSpec;
    use crate::net::proto::BytesSession;
    use crate::net::proto::tunnel::message::TunnelMessage;
    use crate::net::session::SessionBuilder;
    use crate::net::tunnel::TunnelEofMethod;
    use crate::net::{AsyncConnectExt, TunnelClient, TunnelServer};

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

        assert!(matches!(all_streams.next().await, Some(_)));
        assert_eq!(all_streams.len(), 1);
        assert!(matches!(all_streams.next().await, None));
        assert_eq!(all_streams.len(), 0);

        assert!(matches!(all_streams.next().await, None));
        assert!(matches!(all_streams.next().await, None));
        assert!(matches!(all_streams.next().await, None));

        all_streams.push(stream2);
        assert_eq!(all_streams.len(), 1);

        assert!(matches!(all_streams.next().await, Some(_)));
        assert_eq!(all_streams.len(), 1);
        assert!(matches!(all_streams.next().await, None));
        assert_eq!(all_streams.len(), 0);
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
    enum MockIoKind {
        Direct,
        Interpreter,
    }

    enum MockSocketKind {
        Connected,
        Disconnected(MockConnector),
    }

    async fn connected_client(
        io_kind: MockIoKind,
        proxy: MockProxy,
    ) -> (anyhow::Result<()>, anyhow::Result<()>) {
        let (src, dst) = (proxy.app.reader, proxy.app.writer);

        let mut tunnel: TunnelClient<BytesSession<_, _>> =
            TunnelClient::new(TunnelEofMethod::OnStreamCount(1));

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

    async fn server(
        args: (MockIoKind, MockSocketKind),
        proxy: MockProxy,
    ) -> (anyhow::Result<()>, anyhow::Result<()>) {
        let (io_kind, sock_kind) = args;
        let (src, dst) = (proxy.app.reader, proxy.app.writer);

        let tunnel: TunnelServer<BytesSession<_, _>, _> = match sock_kind {
            MockSocketKind::Connected => {
                // We are connected and want to end when this connection is done.
                let mut tunnel =
                    TunnelServer::new(TunnelEofMethod::OnStreamCount(1), MockConnector::default());
                tunnel.add_session(src, dst, TEST_ID).await;
                tunnel
            }
            MockSocketKind::Disconnected(connector) => {
                // We are disconnected, we need to hold the tunnel open until
                // the first stream from the client is complete.
                TunnelServer::new(TunnelEofMethod::OnStreamCount(1), connector)
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

    #[tokio::test]
    async fn proxy_network_connected_tunnel_direct_io() {
        // let _ = env_logger::try_init();
        for len in mock::payload_len_iter() {
            MockProxyNetwork::new(len)
                .run_with_forwarder(
                    MockIoKind::Direct,
                    &connected_client,
                    (MockIoKind::Direct, MockSocketKind::Connected),
                    &server,
                )
                .await
                .assert(len);
        }
    }

    #[tokio::test]
    async fn proxy_network_connected_tunnel_interpreter_io() {
        // let _ = env_logger::try_init();
        for len in mock::payload_len_iter() {
            MockProxyNetwork::new(len)
                .run_with_forwarder(
                    MockIoKind::Interpreter,
                    &connected_client,
                    (MockIoKind::Interpreter, MockSocketKind::Connected),
                    &server,
                )
                .await
                .assert(len);
        }
    }

    async fn proxy_network_disconnected_helper(io_kind: MockIoKind, len: usize) -> mock::Result {
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
            &connected_client,
            (io_kind, MockSocketKind::Disconnected(conn)),
            &server,
        )
        .await
    }

    #[tokio::test]
    async fn proxy_network_disconnected_tunnel_direct_io() {
        // let _ = env_logger::try_init();
        for len in mock::payload_len_iter() {
            proxy_network_disconnected_helper(MockIoKind::Direct, len)
                .await
                .assert(len);
        }
    }

    #[tokio::test]
    async fn proxy_network_disconnected_tunnel_interpreter_io() {
        // let _ = env_logger::try_init();
        for len in mock::payload_len_iter() {
            proxy_network_disconnected_helper(MockIoKind::Interpreter, len)
                .await
                .assert(len);
        }
    }
}
