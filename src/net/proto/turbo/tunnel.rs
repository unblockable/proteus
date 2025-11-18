use std::io;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures::FutureExt;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::oneshot::Receiver;
use tokio::sync::{Mutex, Notify, oneshot};

use crate::net::AsyncConnectExt;
use crate::net::proto::socks::address::Socks5Target;

enum TunnelIo<T> {
    Connected(T),
    Disconnected((Arc<Notify>, Receiver<io::Result<T>>)),
}

pub struct Tunnel<R, W, C>
where
    R: AsyncRead + Send + Unpin,
    W: AsyncWrite + Send + Unpin,
    C: AsyncConnectExt<ReadHalf = R, WriteHalf = W> + Default,
{
    reader: Arc<Mutex<TunnelIo<R>>>,
    writer: Arc<Mutex<TunnelIo<W>>>,
    peer: Option<Socks5Target>,
    _phantom: PhantomData<C>,
}

impl<R, W, C> Tunnel<R, W, C>
where
    R: AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
    C: AsyncConnectExt<ReadHalf = R, WriteHalf = W> + Default,
{
    fn new(reader: TunnelIo<R>, writer: TunnelIo<W>, peer: Option<Socks5Target>) -> Self {
        Self {
            reader: Arc::new(Mutex::new(reader)),
            writer: Arc::new(Mutex::new(writer)),
            peer,
            _phantom: PhantomData,
        }
    }

    pub fn new_client(server: Socks5Target) -> Self {
        let (read_tx, read_rx) = oneshot::channel();
        let (write_tx, write_rx) = oneshot::channel();

        let ready_to_connect = Arc::new(Notify::new());
        let connect_target = server.clone();
        let ready = ready_to_connect.clone();

        // Spawn a background task to establish the connection and split the stream.
        tokio::spawn(async move {
            ready_to_connect.notified().await;

            let mut connector = C::default();
            let result = connector.connect(connect_target).await;

            match result {
                Ok((reader, writer)) => {
                    let _ = read_tx.send(Ok(reader));
                    let _ = write_tx.send(Ok(writer));
                }
                Err(e) => {
                    let _ = read_tx.send(Err(io::Error::from(e.kind())));
                    let _ = write_tx.send(Err(e));
                }
            }
        });

        let r_io = TunnelIo::Disconnected((ready.clone(), read_rx));
        let w_io = TunnelIo::Disconnected((ready, write_rx));

        Self::new(r_io, w_io, Some(server))
    }

    pub fn new_server(net_src: R, net_dst: W) -> Self {
        Self::new(
            TunnelIo::Connected(net_src),
            TunnelIo::Connected(net_dst),
            None,
        )
    }
}

impl<R, W, C> Clone for Tunnel<R, W, C>
where
    R: AsyncRead + Send + Unpin,
    W: AsyncWrite + Send + Unpin,
    C: AsyncConnectExt<ReadHalf = R, WriteHalf = W> + Default,
{
    fn clone(&self) -> Self {
        Self {
            reader: self.reader.clone(),
            writer: self.writer.clone(),
            peer: self.peer.clone(),
            _phantom: PhantomData,
        }
    }
}

impl<R, W, C> AsyncRead for Tunnel<R, W, C>
where
    R: AsyncRead + Send + Unpin,
    W: AsyncWrite + Send + Unpin,
    C: AsyncConnectExt<ReadHalf = R, WriteHalf = W> + Default,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut ReadBuf,
    ) -> Poll<io::Result<()>> {
        log::trace!("poll_read() is called on TurboTunnelController");

        let mut r_future = Box::pin(self.reader.lock());
        let mut r_state = futures::ready!(r_future.as_mut().poll(cx));

        match &mut *r_state {
            TunnelIo::Connected(reader) => Pin::new(reader).poll_read(cx, buf),
            TunnelIo::Disconnected((notify, receiver)) => {
                // Notify the connnection task to do the connection now.
                notify.notify_one();

                // Wait for the connection result.
                match receiver.poll_unpin(cx) {
                    Poll::Ready(Ok(conn_result)) => match conn_result {
                        Ok(mut reader) => {
                            let result = Pin::new(&mut reader).poll_read(cx, buf);
                            *r_state = TunnelIo::Connected(reader);
                            result
                        }
                        Err(e) => Poll::Ready(Err(e)),
                    },
                    Poll::Ready(Err(e)) => Poll::Ready(Err(std::io::Error::new(
                        std::io::ErrorKind::BrokenPipe,
                        format!("RecvError from connect pipe: {e}"),
                    ))),
                    Poll::Pending => Poll::Pending,
                }
            }
        }
    }
}

impl<R, W, C> AsyncWrite for Tunnel<R, W, C>
where
    R: AsyncRead + Send + Unpin,
    W: AsyncWrite + Send + Unpin,
    C: AsyncConnectExt<ReadHalf = R, WriteHalf = W> + Default,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        log::trace!("poll_write() is called on TurboTunnelController");

        let mut w_future = Box::pin(self.writer.lock());
        let mut w_state = futures::ready!(w_future.as_mut().poll(cx));

        match &mut *w_state {
            TunnelIo::Connected(writer) => Pin::new(writer).poll_write(cx, buf),
            TunnelIo::Disconnected((notify, receiver)) => {
                // Notify the connnection task to do the connection now.
                notify.notify_one();

                // Wait for the connection result.
                match receiver.poll_unpin(cx) {
                    Poll::Ready(Ok(conn_result)) => match conn_result {
                        Ok(mut writer) => {
                            let result = Pin::new(&mut writer).poll_write(cx, buf);
                            *w_state = TunnelIo::Connected(writer);
                            result
                        }
                        Err(e) => Poll::Ready(Err(e)),
                    },
                    Poll::Ready(Err(e)) => Poll::Ready(Err(std::io::Error::new(
                        std::io::ErrorKind::BrokenPipe,
                        format!("RecvError from connect pipe: {e}"),
                    ))),
                    Poll::Pending => Poll::Pending,
                }
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        log::trace!("poll_flush() is called on TurboTunnelController");

        let mut w_future = Box::pin(self.writer.lock());
        let mut w_state = futures::ready!(w_future.as_mut().poll(cx));

        match &mut *w_state {
            TunnelIo::Connected(writer) => Pin::new(writer).poll_flush(cx),
            TunnelIo::Disconnected((notify, receiver)) => {
                // Notify the connnection task to do the connection now.
                notify.notify_one();

                // Wait for the connection result.
                match receiver.poll_unpin(cx) {
                    Poll::Ready(Ok(conn_result)) => match conn_result {
                        Ok(mut writer) => {
                            let result = Pin::new(&mut writer).poll_flush(cx);
                            *w_state = TunnelIo::Connected(writer);
                            result
                        }
                        Err(e) => Poll::Ready(Err(e)),
                    },
                    Poll::Ready(Err(e)) => Poll::Ready(Err(std::io::Error::new(
                        std::io::ErrorKind::BrokenPipe,
                        format!("RecvError from connect pipe: {e}"),
                    ))),
                    Poll::Pending => Poll::Pending,
                }
            }
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        log::trace!("poll_shutdown() is called on TurboTunnelController");

        let mut w_future = Box::pin(self.writer.lock());
        let mut w_state = futures::ready!(w_future.as_mut().poll(cx));

        match &mut *w_state {
            TunnelIo::Connected(writer) => Pin::new(writer).poll_shutdown(cx),
            TunnelIo::Disconnected((notify, receiver)) => {
                // Notify the connnection task to do the connection now.
                notify.notify_one();

                // Wait for the connection result.
                match receiver.poll_unpin(cx) {
                    Poll::Ready(Ok(conn_result)) => match conn_result {
                        Ok(mut writer) => {
                            let result = Pin::new(&mut writer).poll_shutdown(cx);
                            *w_state = TunnelIo::Connected(writer);
                            result
                        }
                        Err(e) => Poll::Ready(Err(e)),
                    },
                    Poll::Ready(Err(e)) => Poll::Ready(Err(std::io::Error::new(
                        std::io::ErrorKind::BrokenPipe,
                        format!("RecvError from connect pipe: {e}"),
                    ))),
                    Poll::Pending => Poll::Pending,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    // TODO
}
