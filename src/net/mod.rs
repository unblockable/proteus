use std::future::{Future, poll_fn};
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

use crate::net::proto::TunnelMessage;
use crate::net::proto::socks::address::{Socks5Address, Socks5Target};
use crate::net::session::SessionBuilder;

mod channel;
pub mod proto;
mod session;
mod tunnel;

// Re-export to make these available in the net namespace.
pub use channel::Channel;
pub use tunnel::{TunnelClient, TunnelEofMethod, TunnelServer};

pub const CHUNK_SIZE: usize = 2usize.pow(14u32); // 16 KiB

type ConnectResult<R, W> = io::Result<(R, W, String)>;

/// A trait for types that can asynchronously establish a connection.
pub trait AsyncConnect {
    /// The reader associated with the established connection.
    type ReadHalf: AsyncRead + Send + Unpin;
    /// The writer associated with the established connection.
    type WriteHalf: AsyncWrite + Send + Unpin;

    /// Attempts to establish a connection to a remote server.
    ///
    /// This method will return `Poll::Pending` if the connection could not be
    /// established immediately. In that case, the current task will be
    /// scheduled to be woken up when the connection is ready to be polled again.
    fn poll_connect(
        self: Pin<&mut Self>,
        cx: &mut Context,
        target: Socks5Target,
    ) -> Poll<ConnectResult<Self::ReadHalf, Self::WriteHalf>>;
}

/// An extension trait for `AsyncConnect` that provides an `async` method.
pub trait AsyncConnectExt: AsyncConnect + AsMut<Self> + Send + Unpin {
    fn connect(
        &mut self,
        target: Socks5Target,
    ) -> impl Future<Output = ConnectResult<Self::ReadHalf, Self::WriteHalf>> + Send {
        #[allow(clippy::useless_asref)]
        poll_fn(move |cx| Pin::new(self.as_mut()).poll_connect(cx, target.clone()))
    }
}

/// Blanket implementation for all types that satisfy the bounds.
impl<T: AsyncConnect + AsMut<Self> + Send + Unpin> AsyncConnectExt for T {}

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

/// A connector for TCP sockets.
#[derive(Default)]
pub struct TcpConnector {
    future: Option<Pin<Box<dyn Future<Output = io::Result<TcpStream>> + Send + Sync>>>,
}

impl AsyncConnect for TcpConnector {
    type ReadHalf = OwnedReadHalf;
    type WriteHalf = OwnedWriteHalf;

    fn poll_connect(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        target: Socks5Target,
    ) -> Poll<io::Result<(Self::ReadHalf, Self::WriteHalf, String)>> {
        // Initiate a connection if we don't have one pending.
        if self.future.is_none() {
            log::debug!("Initiating TCP connection to target {target}");
            match target.addr() {
                Socks5Address::IpAddr(addr) => {
                    self.future = Some(Box::pin(TcpStream::connect((addr, target.port()))))
                }
                Socks5Address::Name(name) => {
                    self.future = Some(Box::pin(TcpStream::connect(format!(
                        "{name}:{}",
                        target.port()
                    ))))
                }
                Socks5Address::Unknown => {
                    return Poll::Ready(Err(std::io::Error::new(
                        io::ErrorKind::AddrNotAvailable,
                        "Unable to connect to unknown address",
                    )));
                }
            };
        }

        // Now poll the connection. The future should be Some at this point.
        let future = self.future.as_mut().unwrap();
        match future.as_mut().poll(cx) {
            Poll::Ready(Ok(stream)) => {
                let name = fmt_stream_name(&stream);
                log::debug!("TcpStream connected: {name}",);
                // Dropping the future allows us to do another connect.
                self.future = None;
                let (rx, tx) = stream.into_split();
                Poll::Ready(Ok((rx, tx, name)))
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl AsMut<TcpConnector> for TcpConnector {
    fn as_mut(&mut self) -> &mut Self {
        self
    }
}

impl Clone for TcpConnector {
    fn clone(&self) -> Self {
        Self { future: None }
    }
}

pub struct FixedTargetTcpConnector {
    fixed_target: Socks5Target,
    connector: TcpConnector,
}

impl FixedTargetTcpConnector {
    pub fn new(fixed_target: Socks5Target) -> Self {
        Self {
            fixed_target,
            connector: TcpConnector::default(),
        }
    }
}

impl AsyncConnect for FixedTargetTcpConnector {
    type ReadHalf = <TcpConnector as AsyncConnect>::ReadHalf;
    type WriteHalf = <TcpConnector as AsyncConnect>::WriteHalf;

    fn poll_connect(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        _: Socks5Target,
    ) -> Poll<io::Result<(Self::ReadHalf, Self::WriteHalf, String)>> {
        let target = self.fixed_target.clone();
        Pin::new(self.connector.as_mut()).poll_connect(cx, target)
    }
}

impl AsMut<FixedTargetTcpConnector> for FixedTargetTcpConnector {
    fn as_mut(&mut self) -> &mut Self {
        self
    }
}

impl Clone for FixedTargetTcpConnector {
    fn clone(&self) -> Self {
        Self::new(self.fixed_target.clone())
    }
}

/// A SessionBuilder backed by TCP connection halves.
pub trait TcpSessionBuilder:
    SessionBuilder<Message = TunnelMessage, ReadHalf = OwnedReadHalf, WriteHalf = OwnedWriteHalf>
    + 'static
{
}
/// Blanket implementation for all types that satisfy the bounds.
impl<T> TcpSessionBuilder for T where
    T: SessionBuilder<
            Message = TunnelMessage,
            ReadHalf = OwnedReadHalf,
            WriteHalf = OwnedWriteHalf,
        > + 'static
{
}
