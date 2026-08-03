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
        A: ToSocketAddrs + Send,
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
    use std::pin::Pin;
    use std::task::{Context, Poll};

    use anyhow::anyhow;
    use fast_socks5::util::target_addr::TargetAddr;
    use supertunnel::proto::{Reliability, ResumptionMap};
    use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

    use crate::lang::Role;
    use crate::lang::ir::test::basic::LengthPayloadSpec;
    use crate::net::{NetPayload, NetStream, client, server};
    use crate::util::{self, MockIo, MockProxy, MockProxyNetwork};

    pub struct MockReadHalf(pub Box<dyn AsyncRead + Send + Unpin>);
    pub struct MockWriteHalf(pub Option<Box<dyn AsyncWrite + Send + Unpin>>);

    impl AsyncRead for MockReadHalf {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context,
            buf: &mut ReadBuf,
        ) -> Poll<std::io::Result<()>> {
            Pin::new(&mut *self.0).poll_read(cx, buf)
        }
    }

    impl AsyncWrite for MockWriteHalf {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut Context,
            buf: &[u8],
        ) -> Poll<Result<usize, std::io::Error>> {
            Pin::new(&mut *self.0.as_mut().unwrap()).poll_write(cx, buf)
        }
        fn poll_flush(
            mut self: Pin<&mut Self>,
            cx: &mut Context,
        ) -> Poll<Result<(), std::io::Error>> {
            Pin::new(&mut *self.0.as_mut().unwrap()).poll_flush(cx)
        }
        fn poll_shutdown(
            mut self: Pin<&mut Self>,
            cx: &mut Context,
        ) -> Poll<Result<(), std::io::Error>> {
            Pin::new(&mut *self.0.as_mut().unwrap()).poll_shutdown(cx)
        }
    }

    pub struct MockNetStream {
        pub reader: Option<MockReadHalf>,
        pub writer: Option<MockWriteHalf>,
        pub name: String,
    }

    impl MockNetStream {
        pub fn new(io: MockIo, name: impl Into<String>) -> Self {
            Self {
                reader: Some(MockReadHalf(io.reader)),
                writer: Some(MockWriteHalf(Some(io.writer))),
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

        async fn connect<A: tokio::net::ToSocketAddrs + Send>(_addr: A) -> std::io::Result<Self> {
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Use mock injection for connections",
            ))
        }

        fn name(&self) -> String {
            self.name.clone()
        }
    }

    impl Drop for MockWriteHalf {
        fn drop(&mut self) {
            log::trace!("Dropping MockWriteHalf");
        }
    }

    impl Drop for MockReadHalf {
        fn drop(&mut self) {
            log::trace!("Dropping MockReadHalf");
        }
    }

    impl Drop for MockNetStream {
        fn drop(&mut self) {
            log::trace!("Dropping MockNetStream");
        }
    }

    #[tokio::test]
    async fn io_direct() {
        for len in util::payload_len_iter() {
            let client_fwd = |proto, proxy: MockProxy| async move {
                let inbound = MockNetStream::new(proxy.app, "client_app");
                let outbound = MockNetStream::new(proxy.net, "client_net");

                match client::drive_io_direct(inbound, outbound, proto).await {
                    Ok((tx, rx)) => (Ok(tx), Ok(rx)),
                    Err(e) => (Err(anyhow!(e).into()), Err(anyhow!("Test Failed").into())),
                }
            };

            let server_fwd = |proto, proxy: MockProxy| async move {
                let inbound = MockNetStream::new(proxy.net, "server_net");
                let outbound = MockNetStream::new(proxy.app, "server_app");

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
        let _ = env_logger::try_init();
        for len in util::payload_len_iter() {
            let map: ResumptionMap<Reliability<NetPayload<MockNetStream>>> = ResumptionMap::new();
            let addr: TargetAddr = TargetAddr::Domain("test".into(), 443);

            let client_fwd = |proto, proxy: MockProxy| {
                let reconn_addr = addr.clone();
                async move {
                    let inbound = MockNetStream::new(proxy.app, "client_app");
                    let outbound = MockNetStream::new(proxy.net, "client_net");

                    match client::drive_io_resumable(inbound, outbound, proto, reconn_addr).await {
                        Ok((tx, rx)) => (Ok(tx), Ok(rx)),
                        Err(e) => (Err(anyhow!(e).into()), Err(anyhow!("Test Failed").into())),
                    }
                }
            };

            let server_fwd = |proto, proxy: MockProxy| {
                let resume_map = map.clone();
                async move {
                    let inbound = MockNetStream::new(proxy.net, "server_net");
                    let outbound = MockNetStream::new(proxy.app, "server_app");

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
}
