use std::future::{Future, poll_fn};
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

use crate::net::proto::socks::address::{Socks5Address, Socks5Target};

pub mod proto;

pub const READ_CAPACITY: usize = 2usize.pow(14u32); // 16 KiB

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
    ) -> Poll<io::Result<(Self::ReadHalf, Self::WriteHalf)>>;
}

/// An extension trait for `AsyncConnect` that provides an `async` method.
pub trait AsyncConnectExt: AsyncConnect + Unpin + AsMut<Self> + Send {
    fn connect(
        &mut self,
        target: Socks5Target,
    ) -> impl Future<Output = io::Result<(Self::ReadHalf, Self::WriteHalf)>> + Send {
        poll_fn(move |cx| Pin::new(self.as_mut()).poll_connect(cx, target.clone()))
    }
}

/// Blanket implementation for all types that satisfy the bounds.
impl<T: AsyncConnect + Unpin + AsMut<Self> + Send> AsyncConnectExt for T {}

/// A connector for TCP sockets.
#[derive(Default)]
pub struct TcpConnector {
    pinned_target: Option<Socks5Target>,
    future: Option<Pin<Box<dyn Future<Output = io::Result<TcpStream>> + Send + Sync>>>,
}

impl AsMut<TcpConnector> for TcpConnector {
    fn as_mut(&mut self) -> &mut TcpConnector {
        self
    }
}

impl AsyncConnect for TcpConnector {
    type ReadHalf = OwnedReadHalf;
    type WriteHalf = OwnedWriteHalf;

    fn poll_connect(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        target: Socks5Target,
    ) -> Poll<io::Result<(Self::ReadHalf, Self::WriteHalf)>> {
        // Initiate a connection if we don't have one pending.
        if self.future.is_none() {
            // If we pinned a target, use it, otherwise use the provided.
            let target = self.pinned_target.as_ref().unwrap_or(&target);

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
                        "Unable to connect: connector has unknown address",
                    )));
                }
            };
        }

        // Now poll the connection. The future should be Some at this point.
        let future = self.future.as_mut().unwrap();
        match future.as_mut().poll(cx) {
            Poll::Ready(Ok(stream)) => {
                // Dropping the future allows us to do another connect.
                self.future = None;
                Poll::Ready(Ok(stream.into_split()))
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Clone for TcpConnector {
    fn clone(&self) -> Self {
        Self {
            pinned_target: self.pinned_target.clone(),
            future: None,
        }
    }
}
