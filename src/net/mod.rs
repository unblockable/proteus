use std::fmt::Debug;
use std::time::Duration;

use supertunnel::SessionConnector;
use supertunnel::proto::{Payload, TcpPayloadFactory};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream, ToSocketAddrs};

pub mod client;
pub mod server;

pub const CHUNK_SIZE: usize = 2usize.pow(14u32); // 16 KiB

type NetPayload<S> = Payload<<S as NetStream>::ReadHalf, <S as NetStream>::WriteHalf>;

pub trait NetStream: AsyncRead + AsyncWrite + Send + Unpin + 'static {
    type ReadHalf: AsyncRead + Send + Unpin;
    type WriteHalf: AsyncWrite + Send + Unpin;

    fn into_split(self) -> (Self::ReadHalf, Self::WriteHalf);

    fn connect<A>(addr: A) -> impl Future<Output = std::io::Result<Self>> + Send
    where
        A: ToSocketAddrs + std::fmt::Debug + Send,
        Self: Sized;

    fn name(&self) -> String;
}

pub trait NetPayloadFactory:
    SessionConnector<Item = NetPayload<Self::Stream>> + Send + Sync + 'static
{
    type Stream: NetStream;

    fn new(parent_mss: usize) -> Self;
}

impl NetStream for TcpStream {
    type ReadHalf = OwnedReadHalf;
    type WriteHalf = OwnedWriteHalf;

    fn into_split(self) -> (Self::ReadHalf, Self::WriteHalf) {
        self.into_split()
    }

    async fn connect<A: ToSocketAddrs + Send>(addr: A) -> std::io::Result<Self>
    where
        Self: Sized,
    {
        TcpStream::connect(addr).await
    }

    fn name(&self) -> String {
        fmt_stream_name(self)
    }
}

impl NetPayloadFactory for TcpPayloadFactory {
    type Stream = TcpStream;

    fn new(parent_mss: usize) -> Self {
        TcpPayloadFactory::new(parent_mss)
    }
}

pub async fn connect_timeout<A, S>(target: A, timeout: Duration) -> std::io::Result<S>
where
    A: ToSocketAddrs + Send + Clone + Debug,
    S: NetStream + Send,
{
    match tokio::time::timeout(timeout, S::connect(target.clone())).await {
        Ok(result) => match result {
            Ok(outbound) => {
                log::debug!("Successfully connected to {target:?}: {}", outbound.name());
                Ok(outbound)
            }
            Err(e) => {
                log::warn!("Connection to {target:?} failed with error {e}.");
                Err(e)
            }
        },
        Err(_) => {
            log::warn!("Connection to {target:?} timed out.");
            Err(std::io::ErrorKind::TimedOut.into())
        }
    }
}

pub fn fmt_stream_name(stream: &TcpStream) -> String {
    let peer = match stream.peer_addr() {
        Ok(addr) => format!("{addr}"),
        Err(_) => "unknown".to_string(),
    };
    let local = match stream.local_addr() {
        Ok(addr) => format!("{addr}"),
        Err(_) => "unknown".to_string(),
    };
    format!("[{local}]->[{peer}]")
}

pub fn fmt_listener_name(listener: &TcpListener) -> String {
    let local = match listener.local_addr() {
        Ok(addr) => format!("{addr}"),
        Err(_) => "unknown".to_string(),
    };
    format!("[{local}]")
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::net::SocketAddr;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::task::{Context, Poll};

    use anyhow::anyhow;
    use once_cell::sync::Lazy;
    use supertunnel::proto::{Reliability, ResumptionMap};
    use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
    use tokio::sync::Mutex;
    use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
    use tokio_util::sync::CancellationToken;

    use crate::lang::Role;
    use crate::lang::ir::test::basic::LengthPayloadSpec;
    use crate::net::{NetPayload, NetStream, client, server};
    use crate::util::{self, MockIo, MockProxy, MockProxyNetwork};

    pub struct MockNetStream {
        reader: Option<MockReadHalf>,
        writer: Option<MockWriteHalf>,
        name: String,
    }

    pub struct MockReadHalf {
        inner: Box<dyn AsyncRead + Send + Unpin>,
        count: usize,
        flaky: Option<MockErrSpec>,
    }
    pub struct MockWriteHalf {
        inner: Option<Box<dyn AsyncWrite + Send + Unpin>>,
    }

    impl MockNetStream {
        pub fn new(io: MockIo, name: impl Into<String>, flaky: Option<MockErrSpec>) -> Self {
            Self {
                reader: Some(MockReadHalf {
                    inner: io.reader,
                    count: 0,
                    flaky,
                }),
                writer: Some(MockWriteHalf {
                    inner: Some(io.writer),
                }),
                name: name.into(),
            }
        }
    }

    impl AsyncRead for MockNetStream {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            Pin::new(self.reader.as_mut().unwrap()).poll_read(cx, buf)
        }
    }

    impl AsyncWrite for MockNetStream {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut Context,
            buf: &[u8],
        ) -> Poll<Result<usize, std::io::Error>> {
            Pin::new(self.writer.as_mut().unwrap()).poll_write(cx, buf)
        }
        fn poll_flush(
            mut self: Pin<&mut Self>,
            cx: &mut Context,
        ) -> Poll<Result<(), std::io::Error>> {
            Pin::new(self.writer.as_mut().unwrap()).poll_flush(cx)
        }
        fn poll_shutdown(
            mut self: Pin<&mut Self>,
            cx: &mut Context,
        ) -> Poll<Result<(), std::io::Error>> {
            Pin::new(self.writer.as_mut().unwrap()).poll_shutdown(cx)
        }
    }

    impl NetStream for MockNetStream {
        type ReadHalf = MockReadHalf;
        type WriteHalf = MockWriteHalf;

        fn into_split(mut self) -> (Self::ReadHalf, Self::WriteHalf) {
            (self.reader.take().unwrap(), self.writer.take().unwrap())
        }

        async fn connect<A: tokio::net::ToSocketAddrs + std::fmt::Debug + Send>(
            addr: A,
        ) -> std::io::Result<Self> {
            let key = MockConnector::key(addr);
            MockConnector::connect(key).await
        }

        fn name(&self) -> String {
            self.name.clone()
        }
    }

    #[derive(Copy, Clone, Debug, PartialEq, PartialOrd)]
    pub enum MockErrKind {
        Io(std::io::ErrorKind),
        Eof,
    }

    impl From<std::io::ErrorKind> for MockErrKind {
        fn from(value: std::io::ErrorKind) -> Self {
            MockErrKind::Io(value)
        }
    }

    #[derive(Copy, Clone, Debug, PartialEq, PartialOrd)]
    pub enum MockErrSpec {
        ErrAfterNumReads(usize, MockErrKind),
        ErrEveryNumReads(usize, MockErrKind),
        ErrAfterNumBytes(usize, MockErrKind),
        ErrEveryNumBytes(usize, MockErrKind),
    }

    impl MockErrSpec {
        fn increment(&mut self, num_bytes: usize, counter: usize) -> usize {
            let inc = match self {
                MockErrSpec::ErrAfterNumReads(_, _) | MockErrSpec::ErrEveryNumReads(_, _) => 1,
                MockErrSpec::ErrAfterNumBytes(_, _) | MockErrSpec::ErrEveryNumBytes(_, _) => {
                    num_bytes
                }
            };
            counter + inc
        }

        fn try_error(&self, counter: usize) -> Option<MockErrKind> {
            match self {
                MockErrSpec::ErrAfterNumReads(n, e) => counter.ge(n).then_some(*e),
                MockErrSpec::ErrEveryNumReads(n, e) => counter.ge(n).then_some(*e),
                MockErrSpec::ErrAfterNumBytes(n, e) => counter.ge(n).then_some(*e),
                MockErrSpec::ErrEveryNumBytes(n, e) => counter.ge(n).then_some(*e),
            }
        }
    }

    impl AsyncRead for MockReadHalf {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context,
            buf: &mut ReadBuf,
        ) -> Poll<std::io::Result<()>> {
            let count = self.count;

            if let Some(spec) = self.flaky.as_mut()
                && let Some(err) = spec.try_error(count)
            {
                let result = match err {
                    MockErrKind::Io(kind) => Err(kind.into()),
                    MockErrKind::Eof => Ok(()),
                };
                // return Poll::Ready(Err(std::io::ErrorKind::ConnectionReset.into()));
                // return Poll::Ready(Ok(()));
                log::warn!("Injecting mock error now: {result:?}");
                return Poll::Ready(result);
            }

            let len_before = buf.filled().len();
            let result = Pin::new(&mut self.inner).poll_read(cx, buf);
            let len_after = buf.filled().len();

            if matches!(result, Poll::Ready(Ok(_)))
                && let Some(spec) = self.flaky.as_mut()
            {
                let num_bytes = len_after.saturating_sub(len_before);
                let new_count = spec.increment(num_bytes, count);
                self.count = new_count;
            }

            result
        }
    }

    impl AsyncWrite for MockWriteHalf {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut Context,
            buf: &[u8],
        ) -> Poll<Result<usize, std::io::Error>> {
            Pin::new(&mut *self.inner.as_mut().unwrap()).poll_write(cx, buf)
        }
        fn poll_flush(
            mut self: Pin<&mut Self>,
            cx: &mut Context,
        ) -> Poll<Result<(), std::io::Error>> {
            Pin::new(&mut *self.inner.as_mut().unwrap()).poll_flush(cx)
        }
        fn poll_shutdown(
            mut self: Pin<&mut Self>,
            cx: &mut Context,
        ) -> Poll<Result<(), std::io::Error>> {
            Pin::new(&mut *self.inner.as_mut().unwrap()).poll_shutdown(cx)
        }
    }

    impl Drop for MockNetStream {
        fn drop(&mut self) {
            log::trace!("Dropping MockNetStream");
        }
    }

    impl Drop for MockReadHalf {
        fn drop(&mut self) {
            log::trace!("Dropping MockReadHalf");
        }
    }

    impl Drop for MockWriteHalf {
        fn drop(&mut self) {
            log::trace!("Dropping MockWriteHalf");
        }
    }

    static CONNECTION_REGISTRY: Lazy<
        Mutex<HashMap<String, (UnboundedSender<MockIo>, MockErrSpec)>>,
    > = Lazy::new(|| Mutex::new(HashMap::new()));

    struct MockConnector;
    impl MockConnector {
        fn key<A: tokio::net::ToSocketAddrs + std::fmt::Debug>(addr: A) -> String {
            format!("{addr:?}")
        }

        async fn register(key: String, spec: MockErrSpec) -> UnboundedReceiver<MockIo> {
            let (tx, rx) = mpsc::unbounded_channel();
            CONNECTION_REGISTRY.lock().await.insert(key, (tx, spec));
            rx
        }

        async fn connect(key: String) -> std::io::Result<MockNetStream> {
            if let Some((tx, spec)) = CONNECTION_REGISTRY.lock().await.get(&key) {
                let (c_io, s_io) = MockIo::new_pair();
                tx.send(s_io).unwrap();

                let next_err =
                    match spec {
                        MockErrSpec::ErrAfterNumReads(_, _)
                        | MockErrSpec::ErrAfterNumBytes(_, _) => None,
                        MockErrSpec::ErrEveryNumReads(_, _)
                        | MockErrSpec::ErrEveryNumBytes(_, _) => Some(spec.clone()),
                    };

                Ok(MockNetStream::new(
                    c_io,
                    "Reconnected MockNetStream client",
                    next_err,
                ))
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::ConnectionRefused,
                    format!("No MockConnector registered for {key}"),
                ))
            }
        }

        async fn accept(rx: &mut UnboundedReceiver<MockIo>) -> Option<MockNetStream> {
            rx.recv()
                .await
                .map(|io| MockNetStream::new(io, "Reconnected MockNetStream", None))
        }

        async fn deregister(key: &String) {
            CONNECTION_REGISTRY.lock().await.remove(key);
        }
    }

    #[tokio::test]
    async fn io_direct() {
        // let _ = env_logger::try_init();
        for len in util::payload_len_iter() {
            let client_fwd = |proto, proxy: MockProxy| async move {
                let inbound = MockNetStream::new(proxy.app, "client_app", None);
                let outbound = MockNetStream::new(proxy.net, "client_net", None);

                match client::drive_io_direct(inbound, outbound, proto).await {
                    Ok((tx, rx)) => (Ok(tx), Ok(rx)),
                    Err(e) => (Err(anyhow!(e).into()), Err(anyhow!("Test Failed").into())),
                }
            };

            let server_fwd = |proto, proxy: MockProxy| async move {
                let inbound = MockNetStream::new(proxy.net, "server_net", None);
                let outbound = MockNetStream::new(proxy.app, "server_app", None);

                match server::drive_io_direct(inbound, outbound, proto).await {
                    Ok((tx, rx)) => (Ok(tx), Ok(rx)),
                    Err(e) => (Err(anyhow!(e).into()), Err(anyhow!("Test Failed").into())),
                }
            };

            MockProxyNetwork::new(len)
                .run_with_forwarder(
                    LengthPayloadSpec::new(Role::Client),
                    client_fwd,
                    LengthPayloadSpec::new(Role::Server),
                    server_fwd,
                )
                .await
                .assert(len);
        }
    }

    #[tokio::test]
    async fn io_resumable() {
        // let _ = env_logger::try_init();
        for len in util::payload_len_iter() {
            let map: ResumptionMap<Reliability<NetPayload<MockNetStream>>> = ResumptionMap::new();
            let addr = SocketAddr::from(([127, 0, 0, 1], 0));

            let client_fwd = |proto, proxy: MockProxy| async move {
                let inbound = MockNetStream::new(proxy.app, "client_app", None);
                let outbound = MockNetStream::new(proxy.net, "client_net", None);

                match client::drive_io_resumable(inbound, outbound, proto, addr).await {
                    Ok((tx, rx)) => (Ok(tx), Ok(rx)),
                    Err(e) => (Err(anyhow!(e).into()), Err(anyhow!("Test Failed").into())),
                }
            };

            let server_fwd = |proto, proxy: MockProxy| {
                let resume_map = map.clone();
                async move {
                    let inbound = MockNetStream::new(proxy.net, "server_net", None);
                    let outbound = MockNetStream::new(proxy.app, "server_app", None);

                    match server::drive_io_resumable(inbound, outbound, proto, resume_map).await {
                        Ok((tx, rx)) => (Ok(tx), Ok(rx)),
                        Err(e) => (Err(anyhow!(e).into()), Err(anyhow!("Test Failed").into())),
                    }
                }
            };

            MockProxyNetwork::new(len)
                .run_with_forwarder(
                    LengthPayloadSpec::new(Role::Client),
                    client_fwd,
                    LengthPayloadSpec::new(Role::Server),
                    server_fwd,
                )
                .await
                .assert(len);
        }
    }

    /// WARNING:
    /// Addr must be unique from other tests since we are using a global connector.
    async fn io_resumable_flaky(len: usize, addr: SocketAddr, net_err: MockErrSpec) {
        let key = MockConnector::key(addr);
        let acceptor = MockConnector::register(key.clone(), net_err).await;
        let acceptor = Arc::new(Mutex::new(acceptor));
        let token = CancellationToken::new();

        let client_fwd = |proto: LengthPayloadSpec, proxy: MockProxy| {
            let server_listen = token.clone();

            async move {
                let app = MockNetStream::new(proxy.app, "client_app", None);
                let net = MockNetStream::new(proxy.net, "client_net", Some(net_err));

                let result = client::drive_io_resumable(app, net, proto, addr).await;

                log::debug!("Test client done, cancelling server");
                server_listen.cancel();

                match result {
                    Ok((tx, rx)) => (Ok(tx), Ok(rx)),
                    Err(e) => (Err(anyhow!(e).into()), Err(anyhow!("Test Failed").into())),
                }
            }
        };

        let server_fwd = |proto: LengthPayloadSpec, proxy: MockProxy| {
            let mut map = ResumptionMap::new();
            let acceptor = acceptor.clone();
            let client_reconnect = token.clone();

            async move {
                let mut net = MockNetStream::new(proxy.net, "server_net", None);
                let mut app = MockNetStream::new(proxy.app, "server_app", None);
                let mut keep_alive = Vec::new();

                loop {
                    let (proto, map) = (proto.clone(), map.clone());

                    let _ = server::drive_io_resumable(net, app, proto, map).await;
                    // select! {
                    //     _ = client_reconnect.cancelled() => break,
                    //     _ = server::drive_io_resumable(net, app, proto, map) => {}
                    // };

                    let mut rx = acceptor.lock().await;

                    let result = tokio::select! {
                        _ = client_reconnect.cancelled() => break,
                        result = MockConnector::accept(&mut rx) => result
                    };

                    match result {
                        Some(accepted_stream) => {
                            net = accepted_stream;
                            let (tmp_a, tmp_b) = MockIo::new_pair();
                            app = MockNetStream::new(tmp_a, "server_app_tmp", None);
                            keep_alive.push(tmp_b);
                        }
                        None => break,
                    }
                }

                // Need to clear out the stale NetStreams to propagate EOF signals.
                // This prevents the MockProxyNetwork from stalling.
                map.clear().await;
                log::debug!("Test server done, returning now");
                (Ok(0), Ok(0))
            }
        };

        MockProxyNetwork::new(len)
            .run_with_forwarder(
                LengthPayloadSpec::new(Role::Client),
                client_fwd,
                LengthPayloadSpec::new(Role::Server),
                server_fwd,
            )
            .await
            .assert(len);

        MockConnector::deregister(&key).await;
    }

    #[tokio::test]
    async fn io_resumable_flaky_eof_after_5_reads() {
        // let _ = env_logger::try_init();
        // for (i, len) in util::payload_len_iter().enumerate() {
        for (i, len) in [(0, 1_000_000)] {
            log::info!("Running test {i} with length {len}");
            io_resumable_flaky(
                len,
                SocketAddr::from(([127, 1, 0, 1], i as u16)),
                MockErrSpec::ErrAfterNumReads(5, MockErrKind::Eof),
            )
            .await;
        }
    }

    #[tokio::test]
    async fn io_resumable_flaky_eof_every_25_reads() {
        // let _ = env_logger::try_init();
        for (i, len) in [(0, 1_000_000)] {
            log::info!("Running test {i} with length {len}");
            io_resumable_flaky(
                len,
                SocketAddr::from(([127, 1, 0, 2], i as u16)),
                MockErrSpec::ErrEveryNumReads(25, MockErrKind::Eof),
            )
            .await;
        }
    }

    #[tokio::test]
    async fn io_resumable_flaky_eof_after_10_000_bytes() {
        // let _ = env_logger::try_init();
        for (i, len) in [(0, 1_000_000)] {
            log::info!("Running test {i} with length {len}");
            io_resumable_flaky(
                len,
                SocketAddr::from(([127, 1, 0, 3], i as u16)),
                MockErrSpec::ErrAfterNumBytes(10_000, MockErrKind::Eof),
            )
            .await;
        }
    }

    #[tokio::test]
    async fn io_resumable_flaky_eof_every_100_000_bytes() {
        // let _ = env_logger::try_init();
        for (i, len) in [(0, 1_000_000)] {
            log::info!("Running test {i} with length {len}");
            io_resumable_flaky(
                len,
                SocketAddr::from(([127, 1, 0, 4], i as u16)),
                MockErrSpec::ErrEveryNumBytes(100_000, MockErrKind::Eof),
            )
            .await;
        }
    }

    #[tokio::test]
    async fn io_resumable_flaky_reset_after_5_reads() {
        // let _ = env_logger::try_init();
        for (i, len) in [(0, 1_000_000)] {
            log::info!("Running test {i} with length {len}");
            io_resumable_flaky(
                len,
                SocketAddr::from(([127, 0, 0, 5], i as u16)),
                MockErrSpec::ErrAfterNumReads(5, std::io::ErrorKind::ConnectionReset.into()),
            )
            .await;
        }
    }

    #[tokio::test]
    async fn io_resumable_flaky_reset_every_25_reads() {
        // let _ = env_logger::try_init();
        for (i, len) in [(0, 1_000_000)] {
            log::info!("Running test {i} with length {len}");
            io_resumable_flaky(
                len,
                SocketAddr::from(([127, 0, 0, 6], i as u16)),
                MockErrSpec::ErrEveryNumReads(25, std::io::ErrorKind::ConnectionReset.into()),
            )
            .await;
        }
    }

    #[tokio::test]
    async fn io_resumable_flaky_reset_after_10_000_bytes() {
        // let _ = env_logger::try_init();
        for (i, len) in [(0, 1_000_000)] {
            log::info!("Running test {i} with length {len}");
            io_resumable_flaky(
                len,
                SocketAddr::from(([127, 0, 0, 7], i as u16)),
                MockErrSpec::ErrAfterNumBytes(10_000, std::io::ErrorKind::ConnectionReset.into()),
            )
            .await;
        }
    }

    #[tokio::test]
    async fn io_resumable_flaky_reset_every_100_000_bytes() {
        // let _ = env_logger::try_init();
        for (i, len) in [(0, 1_000_000)] {
            log::info!("Running test {i} with length {len}");
            io_resumable_flaky(
                len,
                SocketAddr::from(([127, 0, 0, 8], i as u16)),
                MockErrSpec::ErrEveryNumBytes(100_000, std::io::ErrorKind::ConnectionReset.into()),
            )
            .await;
        }
    }
}
