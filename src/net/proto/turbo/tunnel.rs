use std::collections::HashMap;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use bytes::BytesMut;
use futures::SinkExt;
use futures::stream::{SelectAll, StreamExt};
use rand::RngCore;
use rand::rngs::ThreadRng;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
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
    C: AsyncConnectExt<ReadHalf = R, WriteHalf = W> + Clone + 'static,
{
    shared_read_state: Arc<Mutex<ReadState<R>>>,
    shared_write_state: Arc<Mutex<WriteState<W>>>,
    connector: C,
}

struct ReadState<R: AsyncRead + Send + Unpin> {
    streams: SelectAll<TurboStream<R>>,
    waker: Option<Waker>,
    buffer: BytesMut,
    eof_on_empty: bool,
}

struct WriteState<W: AsyncWrite + Send + Unpin> {
    sinks: HashMap<u64, TurboSink<W>>,
    waker: Option<Waker>,
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
    C: AsyncConnectExt<ReadHalf = R, WriteHalf = W> + Clone + 'static,
{
    pub fn new(eof_on_empty: bool, connector: C) -> Self {
        Self {
            shared_read_state: Arc::new(Mutex::new(ReadState {
                streams: SelectAll::new(),
                waker: None,
                buffer: BytesMut::new(),
                eof_on_empty,
            })),
            shared_write_state: Arc::new(Mutex::new(WriteState {
                sinks: HashMap::new(),
                waker: None,
                buffer: BytesMut::new(),
            })),
            connector,
        }
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

    async fn store_session(&mut self, id: u64, session: TurboSession<R, W>) {
        let (stream, sink) = session.into_split();
        {
            let mut state = self.shared_read_state.lock().await;
            state.streams.push(stream);
            Self::wake(&state.waker);
        }
        {
            let mut state = self.shared_write_state.lock().await;
            state.sinks.insert(id, sink);
            Self::wake(&state.waker);
        }
    }

    fn wake(maybe_waker: &Option<Waker>) {
        // Wake the waker it one exists, without dropping our handle.
        maybe_waker.as_ref().and_then(|w| Some(w.clone().wake()));
        // Instead, we could wake the waker and drop the handle to prevent multiple wake-ups.
        // maybe_waker.take().map(|w| w.wake());
    }

    pub async fn add_session_socks_client(&mut self, src: R, dst: W, target: Socks5Target) {
        let id = self.generate_session_id().await;
        self.add_session_connected_client(id, src, dst, target)
            .await;
    }

    pub async fn add_session_pt_client(&mut self, src: R, dst: W) {
        // The server will already be configured with a connect target, so we use a dummy one.
        let target = Socks5Target::new(Socks5Address::Unknown, 0);
        let id = self.generate_session_id().await;
        self.add_session_connected_client(id, src, dst, target)
            .await;
    }

    async fn add_session_connected_client(
        &mut self,
        id: u64,
        src: R,
        dst: W,
        target: Socks5Target,
    ) {
        // Clients are always connected to their app.
        let mut session = TurboSession::connected(id, src, dst);
        // Clients start the turbo protocol session, asking the server to open to a new target.
        session.open(target);
        // Track the session bits for future io.
        self.store_session(id, session).await;
    }

    #[cfg(test)]
    async fn add_session_connected_server(&mut self, id: u64, src: R, dst: W) {
        // A server that is already connected to its app.
        let session = TurboSession::connected(id, src, dst);
        // Track the session bits for future io.
        self.store_session(id, session).await;
    }

    #[cfg(test)]
    async fn add_session_disconnected_server(&mut self, id: u64, target: Socks5Target) {
        // A server that first needs to connect to its app.
        let session = TurboSession::<R, W>::disconnected(id, target, self.connector.clone());
        // Track the session bits for future io.
        self.store_session(id, session).await;
    }

    fn add_session_disconnected_server_with_write_lock(
        &self,
        id: u64,
        target: Socks5Target,
        state: &mut MutexGuard<WriteState<W>>,
    ) {
        // A server that first needs to connect to its app.
        let session = TurboSession::<R, W>::disconnected(id, target, self.connector.clone());
        let (stream, sink) = session.into_split();

        // Store the sink using our locked state.
        state.sinks.insert(id, sink);
        Self::wake(&state.waker);

        // Asynchronously store the stream since we do not have a read lock.
        let read_state = self.shared_read_state.clone();
        tokio::spawn(async move {
            let mut state = read_state.lock().await;
            state.streams.push(stream);
            Self::wake(&state.waker);
        });
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
                self.add_session_disconnected_server_with_write_lock(id, target.clone(), state);
            }
        }
    }
}

impl<R, W, C> Clone for TurboTunnel<R, W, C>
where
    R: AsyncRead + Send + Unpin,
    W: AsyncWrite + Send + Unpin,
    C: AsyncConnectExt<ReadHalf = R, WriteHalf = W> + Clone + 'static,
{
    fn clone(&self) -> Self {
        Self {
            shared_read_state: self.shared_read_state.clone(),
            shared_write_state: self.shared_write_state.clone(),
            connector: self.connector.clone(),
        }
    }
}

impl<R, W, C> AsyncRead for TurboTunnel<R, W, C>
where
    R: AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
    C: AsyncConnectExt<ReadHalf = R, WriteHalf = W> + Clone + 'static,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut ReadBuf,
    ) -> Poll<io::Result<()>> {
        log::trace!("TurboTunnel::poll_read({})", buf.remaining());

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

        // Store the waker in case a new stream arrives while we are in a Pending state.
        match state.waker.as_mut() {
            Some(w) => w.clone_from(cx.waker()),
            None => state.waker = Some(cx.waker().clone()),
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
        } else if state.streams.is_empty() && state.eof_on_empty {
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
    C: AsyncConnectExt<ReadHalf = R, WriteHalf = W> + Clone + 'static,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        log::trace!("TurboTunnel::poll_write({})", buf.len());

        // Acquire the mutex lock asynchronously.
        let this = self.get_mut();
        let mut future = Box::pin(this.shared_write_state.lock());
        let mut state = futures::ready!(future.as_mut().poll(cx));

        // Store the waker in case a new sink arrives while we are in a Pending state.
        match state.waker.as_mut() {
            Some(w) => w.clone_from(cx.waker()),
            None => state.waker = Some(cx.waker().clone()),
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
        log::trace!("TurboTunnel::poll_flush()");
        self.get_mut().poll_helper(cx, PollHelperOp::Flush)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        log::trace!("TurboTunnel::poll_shutdown()");
        self.get_mut().poll_helper(cx, PollHelperOp::Shutdown)
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
    use crate::net::AsyncConnectExt;
    use crate::net::proto::turbo::TurboTunnel;

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

    #[tokio::test]
    async fn new_sink_immediately_available() {
        let tunnel = TurboTunnel::new(true, MockConnector::default());

        let mut state = tunnel.shared_write_state.lock().await;
        tunnel.add_session_disconnected_server_with_write_lock(
            TEST_ID,
            MockConnector::default_target(),
            &mut state,
        );

        assert!(state.sinks.get_mut(&TEST_ID).is_some());
    }

    fn wrapped_proxy<R, W, C>(app_io: TurboTunnel<R, W, C>, net_io: MockIo) -> MockProxy
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
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

        let mut tunnel = TurboTunnel::new(true, MockConnector::default());
        let target = MockConnector::default_target();
        tunnel
            .add_session_connected_client(TEST_ID, src, dst, target)
            .await;

        match io_kind {
            MockIoKind::Direct => {
                mock::io_copy_direct(None::<u8>, wrapped_proxy(tunnel, proxy.net)).await
            }
            MockIoKind::Interpreter => {
                let protospec = EncryptedLengthPayloadSpec::new(Role::Client);
                mock::io_copy_interpreter(protospec, wrapped_proxy(tunnel, proxy.net)).await
            }
        }
    }

    async fn server(
        args: (MockIoKind, MockSocketKind),
        proxy: MockProxy,
    ) -> (anyhow::Result<()>, anyhow::Result<()>) {
        let (io_kind, sock_kind) = args;
        let (src, dst) = (proxy.app.reader, proxy.app.writer);

        let tunnel = match sock_kind {
            MockSocketKind::Connected => {
                let mut tunnel = TurboTunnel::new(true, MockConnector::default());
                tunnel.add_session_connected_server(TEST_ID, src, dst).await;
                tunnel
            }
            MockSocketKind::Disconnected(connector) => {
                let mut tunnel = TurboTunnel::new(true, connector);
                let target = MockConnector::default_target();
                tunnel
                    .add_session_disconnected_server(TEST_ID, target)
                    .await;
                tunnel
            }
        };

        match io_kind {
            MockIoKind::Direct => {
                mock::io_copy_direct(None::<u8>, wrapped_proxy(tunnel, proxy.net)).await
            }
            MockIoKind::Interpreter => {
                let protospec = EncryptedLengthPayloadSpec::new(Role::Server);
                mock::io_copy_interpreter(protospec, wrapped_proxy(tunnel, proxy.net)).await
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
