use std::future::Future;
use std::io;

use bytes::{BufMut, Bytes, BytesMut};
use rand::distr::{Alphanumeric, Distribution};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
#[cfg(test)]
use {
    crate::net::AsyncConnect,
    crate::net::proto::socks::address::{Socks5Address, Socks5Target},
    std::pin::Pin,
    std::sync::{Arc, Mutex},
    std::task::{Context, Poll},
    std::time::Duration,
    tokio::io::ReadBuf,
    tokio::time::{Sleep, sleep},
};

use crate::lang;
use crate::lang::interpreter::{ErrorHandler, Interpreter};
use crate::lang::ir::bridge::TaskProvider;
use crate::net::CHUNK_SIZE;

#[cfg(test)]
pub fn simplex(max_buf_size: usize) -> (impl AsyncRead, impl AsyncWrite) {
    tokio::io::simplex(max_buf_size)
}

pub fn duplex(max_buf_size: usize) -> (impl AsyncRead + AsyncWrite, impl AsyncRead + AsyncWrite) {
    tokio::io::duplex(max_buf_size)
}

#[cfg(test)]
pub fn payload_len_iter() -> impl Iterator<Item = usize> {
    [
        1, 10, 100, 1000, 1500, 2000, 5000, 10_000, 100_000, 1_000_000,
    ]
    .into_iter()
}

pub fn payload(len: usize) -> Bytes {
    let mut rng = rand::rng();
    let mut buf = BytesMut::with_capacity(len);
    let cutoff = 1_000_000;

    // Slower: every byte is a different random character.
    for _ in 0..len.min(cutoff) {
        buf.put_u8(Alphanumeric.sample(&mut rng) as u8);
    }

    // Faster: all bytes are the same random character.
    if len > cutoff {
        buf.put_bytes(Alphanumeric.sample(&mut rng), len - cutoff);
    }

    assert_eq!(buf.len(), len);

    buf.freeze()
}

pub struct MockIo {
    pub reader: Box<dyn AsyncRead + Send + Unpin>,
    pub writer: Box<dyn AsyncWrite + Send + Unpin>,
}

impl MockIo {
    pub fn new(
        reader: impl AsyncRead + Send + Unpin + 'static,
        writer: impl AsyncWrite + Send + Unpin + 'static,
    ) -> Self {
        Self {
            reader: Box::new(reader),
            writer: Box::new(writer),
        }
    }

    fn new_split(rw: impl AsyncRead + AsyncWrite + Send + Unpin + 'static) -> Self {
        let (r, w) = tokio::io::split(rw);
        Self::new(r, w)
    }

    fn new_pair() -> (Self, Self) {
        let (io_rw_1, io_rw_2) = duplex(CHUNK_SIZE);
        (Self::new_split(io_rw_1), Self::new_split(io_rw_2))
    }
}

struct MockApplication {
    payload: Bytes,
    io: MockIo,
}

impl MockApplication {
    fn new(payload_len: usize, io: MockIo) -> Self {
        let payload = payload(payload_len);
        Self { payload, io }
    }
}

pub struct MockProxy {
    pub app: MockIo,
    pub net: MockIo,
}

impl MockProxy {
    pub fn new(app: MockIo, net: MockIo) -> Self {
        Self { app, net }
    }
}

/// A mock proxy network that represents the following:
/// c_app <--> c_proxy <--> s_proxy <--> s_app
pub struct MockProxyNetwork {
    c_app: MockApplication,
    c_proxy: MockProxy,
    s_proxy: MockProxy,
    s_app: MockApplication,
}

impl MockProxyNetwork {
    pub fn new(payload_len: usize) -> Self {
        let (c_app, c_proxy_to_app) = MockIo::new_pair();
        let (c_proxy_to_net, s_proxy_to_net) = MockIo::new_pair();
        let (s_app, s_proxy_to_app) = MockIo::new_pair();
        Self {
            c_app: MockApplication::new(payload_len, c_app),
            c_proxy: MockProxy::new(c_proxy_to_app, c_proxy_to_net),
            s_proxy: MockProxy::new(s_proxy_to_app, s_proxy_to_net),
            s_app: MockApplication::new(payload_len, s_app),
        }
    }

    #[cfg(test)]
    async fn run_direct_io(self) -> self::Result {
        self.run_with_forwarder(None::<u8>, &io_copy_direct, None::<u8>, &io_copy_direct)
            .await
    }

    pub async fn run_with_forwarder<C, FC, S, FS, FutC, FutS>(
        self,
        client: C,
        client_fwd: FC,
        server: S,
        server_fwd: FS,
    ) -> self::Result
    where
        FC: Fn(C, MockProxy) -> FutC,
        FS: Fn(S, MockProxy) -> FutS,
        FutC: Future<Output = (lang::Result<usize>, lang::Result<usize>)>,
        FutS: Future<Output = (lang::Result<usize>, lang::Result<usize>)>,
    {
        let results = tokio::join!(
            // Run the client-side app tasks.
            stream_then_shutdown(self.c_app.io.writer, self.c_app.payload),
            sink_until_eof(self.c_app.io.reader),
            // Run the client-side proxy tasks.
            client_fwd(client, self.c_proxy),
            // Run the server-side proxy tasks.
            server_fwd(server, self.s_proxy),
            // Run the server-side app tasks.
            stream_then_shutdown(self.s_app.io.writer, self.s_app.payload),
            sink_until_eof(self.s_app.io.reader),
        );

        self::Result {
            c_app_src: results.0,
            c_app_dst: results.1,
            c_app_to_net: results.2.0,
            c_net_to_app: results.2.1,
            s_app_to_net: results.3.0,
            s_net_to_app: results.3.1,
            s_app_src: results.4,
            s_app_dst: results.5,
        }
    }
}

pub struct Result {
    pub c_app_src: io::Result<Bytes>,
    pub c_app_dst: io::Result<Bytes>,
    pub c_app_to_net: lang::Result<usize>,
    pub c_net_to_app: lang::Result<usize>,
    pub s_app_to_net: lang::Result<usize>,
    pub s_net_to_app: lang::Result<usize>,
    pub s_app_src: io::Result<Bytes>,
    pub s_app_dst: io::Result<Bytes>,
}

#[cfg(test)]
impl Result {
    pub fn assert(&self, len: usize) {
        self.assert_success();

        let c_src = self.c_app_src.as_ref().unwrap();
        let s_dst = self.s_app_dst.as_ref().unwrap();
        Self::assert_payload(c_src, s_dst, len);

        let s_src = self.s_app_src.as_ref().unwrap();
        let c_dst = self.c_app_dst.as_ref().unwrap();
        Self::assert_payload(s_src, c_dst, len);
    }

    fn assert_success(&self) {
        use crate::lang::ResultExt;

        assert!(self.c_app_src.is_ok());
        assert!(self.c_app_dst.is_ok());
        assert!(self.c_app_to_net.is_success());
        assert!(self.c_net_to_app.is_success());
        assert!(self.s_app_to_net.is_success());
        assert!(self.s_net_to_app.is_success());
        assert!(self.s_app_src.is_ok());
        assert!(self.s_app_dst.is_ok());
    }

    fn assert_payload(a: &Bytes, b: &Bytes, len: usize) {
        if len > 0 {
            assert!(!a.is_empty());
            assert!(!b.is_empty());
        }
        assert_eq!(a.len(), len);
        assert_eq!(b.len(), len);
        assert_eq!(&a[..], &b[..]);
    }
}

async fn stream_then_shutdown<W>(mut writer: W, payload: Bytes) -> io::Result<Bytes>
where
    W: AsyncWrite + Unpin,
{
    writer.write_all(&payload[..]).await?;
    writer.shutdown().await?; // Signal EOF to the connected read side
    Ok(payload)
}

async fn sink_until_eof<R>(mut reader: R) -> io::Result<Bytes>
where
    R: AsyncRead + Unpin,
{
    let mut buf = Vec::new();
    let _n_bytes = reader.read_to_end(&mut buf).await?;
    Ok(Bytes::from(buf))
}

/// Like `tokio::io::copy()` but takes ownership of the reader and writer,
/// shuts down the writer when the reader gets an EOF to make sure it
/// propagates backward, and drops the reader and writer on return.
#[cfg(test)]
async fn copy_then_shutdown<R, W>(mut src: R, mut dst: W) -> io::Result<usize>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let n_bytes = tokio::io::copy(&mut src, &mut dst).await?;
    dst.shutdown().await?; // Signal EOF to the connected read side
    Ok(n_bytes as usize)
}

#[cfg(test)]
pub async fn io_copy_direct(
    _: Option<u8>,
    proxy: MockProxy,
) -> (lang::Result<usize>, lang::Result<usize>) {
    // Note: we MUST moved the streams so they are dropped when the copy completes.
    // Use the `mock::copy()` function. This ensures that when the copy completes,
    // the underlying streams are dropped, and the EOF correctly propagates backwards.

    let (app_to_net, net_to_app) = tokio::join!(
        copy_then_shutdown(proxy.app.reader, proxy.net.writer),
        copy_then_shutdown(proxy.net.reader, proxy.app.writer),
    );

    (
        app_to_net
            .map_err(|e| lang::Error::Io(lang::interpreter::io::Error::Read(e.into()))),
        net_to_app
            .map_err(|e| lang::Error::Io(lang::interpreter::io::Error::Read(e.into()))),
    )
}

pub async fn io_copy_interpreter<T: TaskProvider + Clone + Send>(
    protospec: T,
    proxy: MockProxy,
) -> (lang::Result<usize>, lang::Result<usize>) {
    let handler = ErrorHandler::builder()
        .shutdown_net_on_app_eof()
        .shutdown_app_on_net_eof();
    let mut interpreter = Interpreter::new(
        proxy.app.reader,
        proxy.app.writer,
        proxy.net.reader,
        proxy.net.writer,
        protospec,
    );
    let (r1, r2) = interpreter.run_join(handler).await;
    (r1.map_err(|e| e.into()), r2.map_err(|e| e.into()))
}

pub async fn check_protocol_interpretability<T: TaskProvider + Clone + Send>(
    client: T,
    server: T,
    payload_len: usize,
) -> self::Result {
    MockProxyNetwork::new(payload_len)
        .run_with_forwarder(client, &io_copy_interpreter, server, &io_copy_interpreter)
        .await
}

#[cfg(test)]
pub async fn test_protocol_interpretability<T>(client: T, server: T)
where
    T: TaskProvider + Clone + Send,
{
    for len in payload_len_iter() {
        check_protocol_interpretability(client.clone(), server.clone(), len)
            .await
            .assert(len);
    }
}

#[cfg(test)]
#[derive(Default)]
pub struct MockConnector {
    /// The io returned during the connect operation.
    local_socket: Arc<Mutex<Option<MockIo>>>,
    /// The socket of the peer that would accept our connection on the remote end.
    remote_socket: Arc<Mutex<Option<MockIo>>>,
    state: MockConnectorState,
}

#[cfg(test)]
enum MockConnectorState {
    Initial(Option<Duration>),
    Sleeping(Pin<Box<Sleep>>),
    Connecting,
    Done,
}

#[cfg(test)]
impl Clone for MockConnectorState {
    fn clone(&self) -> Self {
        if let MockConnectorState::Initial(duration) = self {
            MockConnectorState::Initial(*duration)
        } else {
            MockConnectorState::Initial(None)
        }
    }
}

#[cfg(test)]
impl Default for MockConnectorState {
    fn default() -> Self {
        MockConnectorState::Initial(None)
    }
}

#[cfg(test)]
impl MockConnector {
    pub fn new(connect_delay: Option<Duration>) -> Self {
        let (local, remote) = MockIo::new_pair();
        Self::new_with_io(connect_delay, local, remote)
    }

    pub fn new_with_io(connect_delay: Option<Duration>, local: MockIo, remote: MockIo) -> Self {
        Self {
            local_socket: Arc::new(Mutex::new(Some(local))),
            remote_socket: Arc::new(Mutex::new(Some(remote))),
            state: MockConnectorState::Initial(connect_delay),
        }
    }

    fn local_socket(&mut self) -> Option<MockIo> {
        self.local_socket.lock().unwrap().take()
    }

    pub fn remote_socket(&mut self) -> Option<MockIo> {
        self.remote_socket.lock().unwrap().take()
    }

    pub fn default_target() -> Socks5Target {
        Socks5Target::new(Socks5Address::Unknown, 0)
    }
}

#[cfg(test)]
impl AsMut<MockConnector> for MockConnector {
    fn as_mut(&mut self) -> &mut Self {
        self
    }
}

#[cfg(test)]
impl Clone for MockConnector {
    fn clone(&self) -> Self {
        Self {
            local_socket: self.local_socket.clone(),
            remote_socket: self.remote_socket.clone(),
            state: self.state.clone(),
        }
    }
}

#[cfg(test)]
impl AsyncConnect for MockConnector {
    type ReadHalf = Box<dyn AsyncRead + Send + Unpin>;
    type WriteHalf = Box<dyn AsyncWrite + Send + Unpin>;

    fn poll_connect(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        _target: Socks5Target,
    ) -> Poll<io::Result<(Self::ReadHalf, Self::WriteHalf, String)>> {
        loop {
            match &mut self.state {
                MockConnectorState::Initial(delay) => match delay {
                    Some(dur) => self.state = MockConnectorState::Sleeping(Box::pin(sleep(*dur))),
                    None => self.state = MockConnectorState::Connecting,
                },
                MockConnectorState::Sleeping(fut) => match fut.as_mut().poll(cx) {
                    Poll::Ready(_) => self.state = MockConnectorState::Connecting,
                    Poll::Pending => return Poll::Pending,
                },
                MockConnectorState::Connecting => {
                    self.state = MockConnectorState::Done;
                    if let Some(io) = self.local_socket() {
                        return Poll::Ready(Ok((io.reader, io.writer, String::from("Mock->Mock"))));
                    }
                }
                MockConnectorState::Done => {
                    return Poll::Ready(Err(io::ErrorKind::NetworkDown.into()));
                }
            }
        }
    }
}

#[cfg(test)]
pub use flaky::{MockFlakyConnector, MockFlakyReader, MockIoFlaky};

#[cfg(test)]
pub mod flaky {
    use std::collections::VecDeque;
    use std::task::{Waker, ready};

    use supertunnel::sync::PollMutex;

    use super::*;

    pub struct MockIoFlaky {
        pub reader: Box<dyn AsyncRead + Send + Unpin>,
        pub writer: Box<dyn AsyncWrite + Send + Unpin>,
    }

    impl MockIoFlaky {
        fn new(
            reader: impl AsyncRead + Send + Unpin + 'static,
            writer: impl AsyncWrite + Send + Unpin + 'static,
        ) -> Self {
            Self {
                reader: Box::new(MockFlakyReader::new(reader)),
                writer: Box::new(writer),
            }
        }

        pub fn new_pair() -> (Self, Self) {
            let (rw1, rw2) = duplex(CHUNK_SIZE);

            let (r1, w1) = tokio::io::split(rw1);
            let (r2, w2) = tokio::io::split(rw2);

            (Self::new(r1, w1), Self::new(r2, w2))
        }
    }

    impl From<MockIoFlaky> for MockIo {
        fn from(io: MockIoFlaky) -> Self {
            Self {
                reader: io.reader,
                writer: io.writer,
            }
        }
    }

    pub struct MockFlakyReader {
        pub inner: Box<dyn AsyncRead + Send + Unpin>,
        count: usize,
    }

    impl MockFlakyReader {
        fn new(reader: impl AsyncRead + Send + Unpin + 'static) -> Self {
            Self {
                inner: Box::new(reader),
                count: 0,
            }
        }
    }

    impl AsyncRead for MockFlakyReader {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context,
            buf: &mut ReadBuf,
        ) -> Poll<io::Result<()>> {
            self.count += 1;
            if self.count == 100 {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::ConnectionReset,
                    "Flaky reader returned error",
                )));
            } else {
                Pin::new(&mut self.inner).poll_read(cx, buf)
            }
        }
    }

    pub struct MockFlakyConnector {
        inner: PollMutex<MockFlakyConnectorInner>,
    }

    struct MockFlakyConnectorInner {
        connect_delay: Option<Duration>,
        connector: MockConnector,
        accept_queue: VecDeque<MockIo>,
        accept_waker: Option<Waker>,
    }

    impl MockFlakyConnectorInner {
        fn new(connect_delay: Option<Duration>) -> Self {
            let (local, remote) = MockIoFlaky::new_pair();
            Self {
                connect_delay: connect_delay.clone(),
                connector: MockConnector::new_with_io(connect_delay, local.into(), remote.into()),
                accept_queue: VecDeque::new(),
                accept_waker: None,
            }
        }

        fn reset(&mut self) {
            let (local, remote) = MockIoFlaky::new_pair();
            self.connector =
                MockConnector::new_with_io(self.connect_delay, local.into(), remote.into());
        }
    }

    impl MockFlakyConnector {
        pub fn new(connect_delay: Option<Duration>) -> Self {
            Self {
                inner: PollMutex::new(MockFlakyConnectorInner::new(connect_delay)),
            }
        }

        pub fn accept(&mut self) -> impl Future<Output = MockIo> {
            std::future::poll_fn(move |cx| self.poll_accept(cx))
        }

        pub fn poll_accept(&mut self, cx: &mut Context) -> Poll<MockIo> {
            let mut inner = ready!(self.inner.poll_lock(cx));
            if let Some(io) = inner.accept_queue.pop_front() {
                inner.accept_waker = None;
                Poll::Ready(io)
            } else {
                match inner.accept_waker.as_mut() {
                    Some(w) => w.clone_from(cx.waker()),
                    None => inner.accept_waker = Some(cx.waker().clone()),
                }
                Poll::Pending
            }
        }
    }

    impl AsyncConnect for MockFlakyConnector {
        type ReadHalf = Box<dyn AsyncRead + Send + Unpin>;
        type WriteHalf = Box<dyn AsyncWrite + Send + Unpin>;

        fn poll_connect(
            mut self: Pin<&mut Self>,
            cx: &mut Context,
            _target: Socks5Target,
        ) -> Poll<io::Result<(Self::ReadHalf, Self::WriteHalf, String)>> {
            let mut inner = ready!(self.as_mut().inner.poll_lock(cx));

            let result = Pin::new(inner.connector.as_mut()).poll_connect(cx, _target);

            if let MockConnectorState::Done = &inner.connector.state {
                let remote = inner.connector.remote_socket().unwrap();
                inner.accept_queue.push_back(remote);
                inner.accept_waker.take().map(|w| w.wake());
                inner.reset();
            }

            result
        }
    }

    impl AsMut<MockFlakyConnector> for MockFlakyConnector {
        fn as_mut(&mut self) -> &mut Self {
            self
        }
    }

    impl Clone for MockFlakyConnector {
        fn clone(&self) -> Self {
            Self {
                inner: self.inner.clone(),
            }
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::common::mock;
    use crate::net::AsyncConnectExt;

    async fn test_simple_pair(client: MockIo, server: MockIo, len: usize) {
        let client = MockApplication::new(len, client);
        let server = MockApplication::new(len, server);

        let results = tokio::join!(
            // Run the client-side app tasks.
            stream_then_shutdown(client.io.writer, client.payload),
            sink_until_eof(client.io.reader),
            stream_then_shutdown(server.io.writer, server.payload),
            sink_until_eof(server.io.reader),
        );

        mock::Result::assert_payload(&results.0.unwrap(), &results.3.unwrap(), len);
        mock::Result::assert_payload(&results.1.unwrap(), &results.2.unwrap(), len);
    }

    #[tokio::test]
    async fn duplex_split() {
        for len in payload_len_iter() {
            let (client, server) = MockIo::new_pair();
            test_simple_pair(client, server, len).await;
        }
    }

    #[tokio::test]
    async fn duplex_split_copy() {
        for len in payload_len_iter() {
            let (client, proxy_to_client) = MockIo::new_pair();
            let (server, proxy_to_server) = MockIo::new_pair();

            let client = MockApplication::new(len, client);
            let server = MockApplication::new(len, server);
            let proxy = MockProxy::new(proxy_to_client, proxy_to_server);

            let results = tokio::join!(
                // Run the client-side app tasks.
                stream_then_shutdown(client.io.writer, client.payload),
                sink_until_eof(client.io.reader),
                stream_then_shutdown(server.io.writer, server.payload),
                sink_until_eof(server.io.reader),
                copy_then_shutdown(proxy.app.reader, proxy.net.writer),
                copy_then_shutdown(proxy.net.reader, proxy.app.writer),
            );

            mock::Result::assert_payload(&results.0.unwrap(), &results.3.unwrap(), len);
            mock::Result::assert_payload(&results.1.unwrap(), &results.2.unwrap(), len);
        }
    }

    #[tokio::test]
    async fn duplex_split_copy_copy() {
        // let _ = env_logger::try_init();
        for len in payload_len_iter() {
            let (c_app, c_proxy_to_app) = MockIo::new_pair();
            let (c_proxy_to_net, s_proxy_to_net) = MockIo::new_pair();
            let (s_app, s_proxy_to_app) = MockIo::new_pair();

            let c_app = MockApplication::new(len, c_app);
            let c_proxy = MockProxy::new(c_proxy_to_app, c_proxy_to_net);
            let s_proxy = MockProxy::new(s_proxy_to_app, s_proxy_to_net);
            let s_app = MockApplication::new(len, s_app);

            let results = tokio::join!(
                // Run the client-side app tasks.
                stream_then_shutdown(c_app.io.writer, c_app.payload),
                sink_until_eof(c_app.io.reader),
                stream_then_shutdown(s_app.io.writer, s_app.payload),
                sink_until_eof(s_app.io.reader),
                copy_then_shutdown(c_proxy.app.reader, c_proxy.net.writer),
                copy_then_shutdown(c_proxy.net.reader, c_proxy.app.writer),
                copy_then_shutdown(s_proxy.app.reader, s_proxy.net.writer),
                copy_then_shutdown(s_proxy.net.reader, s_proxy.app.writer),
            );

            mock::Result::assert_payload(&results.0.unwrap(), &results.3.unwrap(), len);
            mock::Result::assert_payload(&results.1.unwrap(), &results.2.unwrap(), len);
        }
    }

    #[tokio::test]
    async fn proxy_network_direct_io() {
        for len in payload_len_iter() {
            MockProxyNetwork::new(len).run_direct_io().await.assert(len);
        }
    }

    async fn test_connector(delay: Option<Duration>) {
        for len in payload_len_iter() {
            let mut connector = MockConnector::new(delay);

            let result = connector.connect(MockConnector::default_target()).await;
            let (r, w, _name) = result.unwrap();

            let client = MockIo::new(r, w);
            let server = connector.remote_socket().unwrap();

            test_simple_pair(client, server, len).await;
        }
    }

    #[tokio::test]
    async fn connector_no_delay() {
        test_connector(None).await;
    }

    #[tokio::test]
    async fn connector_short_delay() {
        test_connector(Some(Duration::from_millis(1))).await;
    }

    #[tokio::test]
    async fn connector_long_delay() {
        test_connector(Some(Duration::from_millis(100))).await;
    }
}
