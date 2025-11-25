use std::collections::HashMap;
use std::io;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use bytes::BytesMut;
use futures::SinkExt;
use futures::stream::{SelectAll, StreamExt};
use rand::RngCore;
use rand::rngs::ThreadRng;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::sync::{Mutex, MutexGuard};
use tokio_util::codec::{Decoder, Encoder};

use crate::net::AsyncConnectExt;
use crate::net::proto::socks::address::{Socks5Address, Socks5Target};
use crate::net::proto::turbo::message::{Command, Request};
use crate::net::proto::turbo::{TurboCodec, TurboMessage, TurboSession, TurboSink, TurboStream};

pub struct TurboTunnel<R, W, C>
where
    R: AsyncRead + Send + Unpin,
    W: AsyncWrite + Send + Unpin,
    C: AsyncConnectExt<ReadHalf = R, WriteHalf = W> + Default + 'static,
{
    shared_read_state: Arc<Mutex<ReadState<R>>>,
    shared_write_state: Arc<Mutex<WriteState<W>>>,
    stream_sender: Sender<TurboStream<R>>,
    sink_sender: Sender<TurboSink<W>>,
    pinned_target: Option<Socks5Target>,
    _phantom: PhantomData<C>,
}

struct ReadState<R: AsyncRead + Send + Unpin> {
    streams: SelectAll<TurboStream<R>>,
    pending: Receiver<TurboStream<R>>,
    buffer: BytesMut,
    eof_on_empty: bool,
}

struct WriteState<W: AsyncWrite + Send + Unpin> {
    sinks: HashMap<u64, TurboSink<W>>,
    pending: Receiver<TurboSink<W>>,
    buffer: BytesMut,
}

#[derive(Debug)]
enum PollHelperOp {
    Ready,
    Flush,
    Shutdown,
}

impl<R, W, C> TurboTunnel<R, W, C>
where
    R: AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
    C: AsyncConnectExt<ReadHalf = R, WriteHalf = W> + Default + 'static,
{
    fn new(pinned_target: Option<Socks5Target>, eof_on_empty: bool) -> Self {
        let (stream_sender, stream_receiver) = mpsc::channel(1_000);
        let (sink_sender, sink_receiver) = mpsc::channel(1_000);

        Self {
            shared_read_state: Arc::new(Mutex::new(ReadState {
                streams: SelectAll::new(),
                pending: stream_receiver,
                buffer: BytesMut::new(),
                eof_on_empty,
            })),
            shared_write_state: Arc::new(Mutex::new(WriteState {
                sinks: HashMap::new(),
                pending: sink_receiver,
                buffer: BytesMut::new(),
            })),
            stream_sender,
            sink_sender,
            pinned_target,
            _phantom: PhantomData,
        }
    }

    pub fn new_pt_client() -> Self {
        Self::new(None, true)
    }

    pub fn new_socks_client() -> Self {
        Self::new(None, false)
    }

    pub fn new_pt_server(pinned_target: Socks5Target) -> Self {
        Self::new(Some(pinned_target), true)
    }

    pub fn new_socks_server() -> Self {
        Self::new(None, false)
    }

    async fn generate_session_id(&self) -> u64 {
        loop {
            let id = ThreadRng::default().next_u64();
            if id > 0 {
                let state = self.shared_write_state.lock().await;
                if !state.sinks.contains_key(&id) {
                    return id;
                }
            }
        }
    }

    async fn store_session(&mut self, session: TurboSession<R, W>) {
        let (stream, sink) = session.into_split();
        // TODO: handle send errors in the following?
        let _ = self.stream_sender.send(stream).await;
        let _ = self.sink_sender.send(sink).await;
    }

    pub async fn add_session_socks_client(&mut self, src: R, dst: W, target: Socks5Target) {
        self.add_session_client(src, dst, target).await;
    }

    pub async fn add_session_pt_client(&mut self, src: R, dst: W) {
        // The server will already be configured with the connect target.
        let target = Socks5Target::new(Socks5Address::Unknown, 0);
        self.add_session_client(src, dst, target).await;
    }

    async fn add_session_client(&mut self, src: R, dst: W, target: Socks5Target) {
        let id = self.generate_session_id().await;
        self.add_session_client_inner(src, dst, id, target).await;
    }

    async fn add_session_client_inner(&mut self, src: R, dst: W, id: u64, target: Socks5Target) {
        let mut session = TurboSession::connected(id, src, dst);
        session.open(target);
        self.store_session(session).await;
    }

    fn add_session_server(&self, id: u64, target: &Socks5Target) {
        let target = match &self.pinned_target {
            Some(pinned) => pinned.clone(),
            None => target.clone(),
        };

        let session = TurboSession::<R, W>::disconnected(id, target, C::default());

        let (stream, sink) = session.into_split();

        // Send the stream/sink through the channels to issue wakeups as needed.
        let sink_sender = self.sink_sender.clone();
        let stream_sender = self.stream_sender.clone();
        tokio::spawn(async move {
            let _ = sink_sender.send(sink).await;
            let _ = stream_sender.send(stream).await;
        });
    }

    #[cfg(test)]
    pub async fn add_session_client_test(&mut self, src: R, dst: W, id: u64) {
        let target = Socks5Target::new(Socks5Address::Unknown, 0);
        self.add_session_client_inner(src, dst, id, target).await;
    }

    #[cfg(test)]
    pub async fn add_session_server_test(&mut self, src: R, dst: W, id: u64) {
        let session = TurboSession::connected(id, src, dst);
        self.store_session(session).await;
    }

    fn poll_helper(&mut self, cx: &mut Context, op: PollHelperOp) -> Poll<Result<(), io::Error>> {
        // Acquire the mutex lock asynchronously.
        let mut future = Box::pin(self.shared_write_state.lock());
        let mut state = futures::ready!(future.as_mut().poll(cx));
        // Run the operation.
        Self::poll_helper_locked(&mut state, cx, op)
    }

    fn poll_helper_locked(
        state: &mut MutexGuard<WriteState<W>>,
        cx: &mut Context,
        op: PollHelperOp,
    ) -> Poll<Result<(), io::Error>> {
        // Run op on all of the inner sinks, tracking if any are pending and which have error.
        let mut error_ids = vec![];
        let mut has_pending = false;

        for sink in state.sinks.values_mut() {
            let result = match op {
                PollHelperOp::Ready => sink.poll_ready_unpin(cx),
                PollHelperOp::Flush => sink.poll_flush_unpin(cx),
                PollHelperOp::Shutdown => sink.poll_close_unpin(cx),
            };

            match result {
                Poll::Ready(Ok(_)) => {}
                Poll::Ready(Err(e)) => {
                    log::debug!("poll_helper(): sink error on {op:?}: {e}");
                    error_ids.push(sink.id());
                }
                Poll::Pending => has_pending = true,
            }
        }

        // Drop the sinks that are in an error state.
        for id in error_ids {
            state.sinks.remove(&id);
        }

        // We are pending if any sink operations are pending.
        if has_pending {
            Poll::Pending
        } else {
            Poll::Ready(Ok(()))
        }
    }

    fn send_message_locked(&self, state: &mut MutexGuard<WriteState<W>>, msg: TurboMessage) {
        let id = msg.session_id;

        // If we already have a sink for the message id, just pass the message along.
        if let Some(sink) = state.sinks.get_mut(&id) {
            if let Err(e) = sink.start_send_unpin(msg) {
                log::debug!("poll_write(): error sending message to sink: {e}");
                state.sinks.remove(&id);
            }
        } else {
            // We don't have a sink for this id, we might need to create one.
            if let Command::Request(Request::Open(target)) = &msg.command {
                self.add_session_server(id, target);
            }
        }
    }
}

impl<R, W, C> Clone for TurboTunnel<R, W, C>
where
    R: AsyncRead + Send + Unpin,
    W: AsyncWrite + Send + Unpin,
    C: AsyncConnectExt<ReadHalf = R, WriteHalf = W> + Default + 'static,
{
    fn clone(&self) -> Self {
        Self {
            shared_read_state: self.shared_read_state.clone(),
            shared_write_state: self.shared_write_state.clone(),
            stream_sender: self.stream_sender.clone(),
            sink_sender: self.sink_sender.clone(),
            pinned_target: self.pinned_target.clone(),
            _phantom: PhantomData,
        }
    }
}

impl<R, W, C> AsyncRead for TurboTunnel<R, W, C>
where
    R: AsyncRead + Send + Unpin,
    W: AsyncWrite + Send + Unpin,
    C: AsyncConnectExt<ReadHalf = R, WriteHalf = W> + Default + 'static,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut ReadBuf,
    ) -> Poll<io::Result<()>> {
        log::trace!(
            "poll_read({}) is called on TurboSessionBroker",
            buf.remaining()
        );

        // Get a mutable reference to self.
        let this = self.get_mut();

        // Acquire the mutex lock asynchronously.
        let mut future = Box::pin(this.shared_read_state.lock());
        let mut state = futures::ready!(future.as_mut().poll(cx));

        // If we already have enough cached bytes to fill the ReadBuf, return quickly.
        if state.buffer.len() >= buf.remaining() {
            let at = buf.remaining();
            let bytes = state.buffer.split_to(at).freeze();
            buf.put_slice(&bytes);
            log::trace!("poll_read(): provided {at} bytes without i/o");
            return Poll::Ready(Ok(()));
        }

        // Start tracking pending streams from new sessions. The loop ends on a `Poll::Pending`,
        // which ensures that a wakeup will always occur when new streams arrive.
        while let Poll::Ready(maybe_stream) = state.pending.poll_recv(cx) {
            match maybe_stream {
                Some(stream) => state.streams.push(stream),
                None => todo!(),
            }
        }

        // Read as many messages as needed to fill the ReadBuf if we can.
        while state.buffer.len() < buf.remaining() {
            match state.streams.poll_next_unpin(cx) {
                Poll::Ready(Some(msg)) => {
                    log::trace!("Buffering next message in read buffer: {msg:?}");
                    // Encode errors only happen if a payload is larger than supported by the
                    // codec and would be a bug, so let's panic in that case.
                    TurboCodec.encode(msg, &mut state.buffer).unwrap();
                }
                // All streams are closed; a wakeup will occur when a pending one arrives.
                Poll::Ready(None) => break,
                // All streams are pending, a wakeup will occur when a message is ready.
                Poll::Pending => break,
            }
        }

        if state.buffer.len() > 0 {
            // We may or may not be able to fill the ReadBuf, but we provide what we have.
            let at = state.buffer.len().min(buf.remaining());
            let bytes = state.buffer.split_to(at).freeze();
            buf.put_slice(&bytes);
            log::trace!("poll_read(): provided {at} bytes");
            Poll::Ready(Ok(()))
        } else if state.pending.is_empty() && state.streams.is_empty() && state.eof_on_empty {
            // This is an EOF since we did not add to the ReadBuf.
            log::trace!("poll_read(): EOF: empty read buf and no streams remaining");
            Poll::Ready(Ok(()))
        } else {
            // New streams, or new message on existing streams, may arrive in the future.
            log::trace!("poll_read(): pending new streams and messages");
            Poll::Pending
        }
    }
}

impl<R, W, C> AsyncWrite for TurboTunnel<R, W, C>
where
    R: AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
    C: AsyncConnectExt<ReadHalf = R, WriteHalf = W> + Default + 'static,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        log::trace!("poll_write({}) is called on TurboSessionBroker", buf.len());

        // Acquire the mutex lock asynchronously.
        let this = self.get_mut();
        let mut future = Box::pin(this.shared_write_state.lock());
        let mut state = futures::ready!(future.as_mut().poll(cx));

        // Start tracking pending sinks from new sessions. The loop ends on a `Poll::Pending`,
        // which ensures that a wakeup will occur when new sinks arrive.
        while let Poll::Ready(maybe_sink) = state.pending.poll_recv(cx) {
            match maybe_sink {
                Some(sink) => {
                    state.sinks.insert(sink.id(), sink);
                }
                None => todo!(),
            }
        }

        // TODO this gets tricky, we cannot take any of the incoming bytes if we return pending,
        // because we would end up taking them again. There is probably a more efficient way,
        // but for now we first guarantee that every sink is ready before taking any bytes.
        if let Poll::Pending = Self::poll_helper_locked(&mut state, cx, PollHelperOp::Ready) {
            return Poll::Pending;
        }

        // If we have any lingering messages in the buffer, handle those first.
        while !state.buffer.is_empty() {
            let msg = match TurboCodec.decode(&mut state.buffer) {
                Ok(Some(msg)) => msg,
                Ok(None) => break,
                Err(e) => return Poll::Ready(Err(e)),
            };

            // Send the message to the sink.
            Self::send_message_locked(this, &mut state, msg);

            if let Poll::Pending = Self::poll_helper_locked(&mut state, cx, PollHelperOp::Ready) {
                return Poll::Pending;
            }
        }

        // Our buffer does not contain a full message, so we need more bytes.
        // After this, we should not return pending anymore.
        state.buffer.extend_from_slice(buf);

        // TODO: can I turn this into a loop? as long as each sink is ready to receive,
        // i could process all messages?
        match TurboCodec.decode(&mut state.buffer) {
            Ok(Some(msg)) => {
                // Send the message to the sink.
                Self::send_message_locked(this, &mut state, msg);

                // We only count the bytes needed to get the full message as written.
                let len_written = buf.len() - state.buffer.len();
                // The other bytes will come back on the next call to poll_write.
                state.buffer.truncate(0);
                Poll::Ready(Ok(len_written))
            }
            Ok(None) => Poll::Ready(Ok(buf.len())),
            Err(e) => Poll::Ready(Err(e)),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        log::trace!("poll_flush() is called on TurboSessionBroker");
        self.get_mut().poll_helper(cx, PollHelperOp::Flush)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        log::trace!("poll_shutdown() is called on TurboSessionBroker");
        self.get_mut().poll_helper(cx, PollHelperOp::Shutdown)
    }
}

#[cfg(test)]
mod tests {
    use futures::StreamExt;
    use futures::stream::{self, SelectAll};
    use tokio::io::{AsyncRead, AsyncWrite};

    use crate::common::mock::{self, MockConnector, MockIo, MockProxy, MockProxyNetwork};
    use crate::net::AsyncConnectExt;
    use crate::net::proto::turbo::TurboTunnel;

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

    fn wrapped_proxy<R, W, C>(app_io: TurboTunnel<R, W, C>, net_io: MockIo) -> MockProxy
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
        C: AsyncConnectExt<ReadHalf = R, WriteHalf = W> + Default + 'static,
    {
        let (tunnel_r, tunnel_w) = (app_io.clone(), app_io);
        let wrapped_app = MockIo::new(tunnel_r, tunnel_w);
        MockProxy::new(wrapped_app, net_io)
    }

    /// Use direct io without an interpreter.
    async fn connected_tunnel_direct_io(
        is_client: bool,
        proxy: MockProxy,
    ) -> (anyhow::Result<()>, anyhow::Result<()>) {
        let mut tunnel = TurboTunnel::<
            Box<dyn AsyncRead + Send + Unpin>,
            Box<dyn AsyncWrite + Send + Unpin>,
            MockConnector,
        >::new(None, true);
        let id = 1234567890;
        if is_client {
            tunnel
                .add_session_client_test(proxy.app.reader, proxy.app.writer, id)
                .await;
        } else {
            tunnel
                .add_session_server_test(proxy.app.reader, proxy.app.writer, id)
                .await;
        }
        mock::io_copy_direct(None::<u8>, wrapped_proxy(tunnel, proxy.net)).await
    }

    #[tokio::test]
    async fn proxy_network_connected_tunnel_direct_io() {
        let _ = env_logger::try_init();
        for len in mock::payload_len_iter() {
            MockProxyNetwork::new(len)
                .run_with_forwarder(
                    true,
                    &connected_tunnel_direct_io,
                    false,
                    &connected_tunnel_direct_io,
                )
                .await
                .assert(len);
        }
    }
}
