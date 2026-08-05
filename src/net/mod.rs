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
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll};

    use anyhow::anyhow;
    use fast_socks5::util::target_addr::TargetAddr;
    use once_cell::sync::Lazy;
    use supertunnel::proto::{Reliability, ResumptionMap};
    use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf};
    use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

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
        flaky: Option<usize>,
    }
    pub struct MockWriteHalf {
        inner: Option<Box<dyn AsyncWrite + Send + Unpin>>,
    }

    impl MockNetStream {
        pub fn new(io: MockIo, name: impl Into<String>, flaky: Option<usize>) -> Self {
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

    impl AsyncRead for MockReadHalf {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context,
            buf: &mut ReadBuf,
        ) -> Poll<std::io::Result<()>> {
            if let Some(limit) = self.flaky
                && self.count >= limit
            {
                return Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::ConnectionReset,
                    "Mocking a network connection error",
                )));
            }

            let result = Pin::new(&mut self.inner).poll_read(cx, buf);
            if matches!(result, Poll::Ready(Ok(_))) {
                self.count += 1;
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
            // Need to call shutdown to propagate EOF signals during tests.
            if let Some(mut writer) = self.inner.take() {
                tokio::spawn(async move { writer.shutdown().await });
            }
        }
    }

    const FLAKY: Option<usize> = Some(25);
    static CONNECTION_REGISTRY: Lazy<Mutex<HashMap<String, UnboundedSender<MockIo>>>> =
        Lazy::new(|| Mutex::new(HashMap::new()));

    struct MockConnector;
    impl MockConnector {
        fn key<A: tokio::net::ToSocketAddrs + std::fmt::Debug>(addr: A) -> String {
            format!("{addr:?}")
        }

        fn register(key: String) -> UnboundedReceiver<MockIo> {
            let (tx, rx) = mpsc::unbounded_channel();
            CONNECTION_REGISTRY.lock().unwrap().insert(key, tx);
            rx
        }

        async fn connect(key: String) -> std::io::Result<MockNetStream> {
            if let Some(tx) = CONNECTION_REGISTRY.lock().unwrap().get(&key) {
                let (c_io, s_io) = MockIo::new_pair();
                tx.send(s_io).unwrap();
                Ok(MockNetStream::new(
                    c_io,
                    "Reconnected MockNetStream client",
                    FLAKY,
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

        fn clear() {
            CONNECTION_REGISTRY.lock().unwrap().clear();
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
            let addr: TargetAddr = TargetAddr::Domain("test".into(), 443);

            let client_fwd = |proto, proxy: MockProxy| {
                let reconn_addr = addr.clone();
                async move {
                    let inbound = MockNetStream::new(proxy.app, "client_app", None);
                    let outbound = MockNetStream::new(proxy.net, "client_net", None);

                    match client::drive_io_resumable(inbound, outbound, proto, reconn_addr).await {
                        Ok((tx, rx)) => (Ok(tx), Ok(rx)),
                        Err(e) => (Err(anyhow!(e).into()), Err(anyhow!("Test Failed").into())),
                    }
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

    #[tokio::test]
    async fn io_resumable_flaky() {
        // let _ = env_logger::try_init();
        for len in util::payload_len_iter() {
            log::info!("Running test with length {len}");

            let addr: TargetAddr = TargetAddr::Domain("io_resumable_flaky".into(), 666);
            let key = MockConnector::key(addr.to_string());
            let acceptor = Arc::new(Mutex::new(MockConnector::register(key)));

            let client_fwd = |proto: LengthPayloadSpec, proxy: MockProxy| {
                let addr = addr.clone();
                async move {
                    let app = MockNetStream::new(proxy.app, "client_app", None);
                    let net = MockNetStream::new(proxy.net, "client_net", FLAKY);

                    let result = client::drive_io_resumable(app, net, proto, addr).await;
                    MockConnector::clear();
                    log::info!("Client exited");
                    match result {
                        Ok((tx, rx)) => (Ok(tx), Ok(rx)),
                        Err(e) => (Err(anyhow!(e).into()), Err(anyhow!("Test Failed").into())),
                    }
                }
            };

            let server_fwd = |proto: LengthPayloadSpec, proxy: MockProxy| {
                let mut map = ResumptionMap::new();
                let acceptor = acceptor.clone();
                async move {
                    let mut net = MockNetStream::new(proxy.net, "server_net", None);
                    let mut app = MockNetStream::new(proxy.app, "server_app", None);
                    let mut keep_alive = Vec::new();

                    loop {
                        let (proto, map) = (proto.clone(), map.clone());
                        let _ = server::drive_io_resumable(net, app, proto, map).await;

                        let mut rx = acceptor.lock().unwrap();
                        match MockConnector::accept(&mut rx).await {
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
                    return (Ok(0), Ok(0));
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

            MockConnector::clear();
        }
    }
}
