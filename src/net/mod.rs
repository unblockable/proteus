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
    type ReadHalf: AsyncRead + Send + Unpin + 'static;
    type WriteHalf: AsyncWrite + Send + Unpin + 'static;

    fn into_split(self) -> (Self::ReadHalf, Self::WriteHalf);

    fn connect<A: ToSocketAddrs + Send + 'static>(
        addr: A,
    ) -> impl Future<Output = std::io::Result<Self>> + Send + 'static
    where
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
    A: ToSocketAddrs + Send + Clone + Debug + 'static,
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
