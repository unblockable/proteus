use std::fmt::Debug;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures::FutureExt;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::oneshot::Receiver;
use tokio::sync::oneshot::error::RecvError;
use tokio::sync::{Mutex, Notify, oneshot};

use crate::net::AsyncConnectExt;
use crate::net::proto::socks::address::Socks5Target;

enum ChannelIo<T> {
    Connected(T),
    Disconnected((Arc<Notify>, Receiver<io::Result<T>>)),
    Error(io::Error),
}

impl<T> Debug for ChannelIo<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ChannelIo::Connected(_) => write!(f, "ChannelIo(State:Connected)"),
            ChannelIo::Disconnected(_) => write!(f, "ChannelIo(State:Disconnected)"),
            ChannelIo::Error(_) => write!(f, "ChannelIo(State:Error)"),
        }
    }
}

pub struct Channel<R, W>
where
    R: AsyncRead + Send + Unpin,
    W: AsyncWrite + Send + Unpin,
{
    reader: Arc<Mutex<ChannelIo<R>>>,
    writer: Arc<Mutex<ChannelIo<W>>>,
    peer: Option<Socks5Target>,
}

impl<R, W> Channel<R, W>
where
    R: AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
{
    fn new(reader: ChannelIo<R>, writer: ChannelIo<W>, peer: Option<Socks5Target>) -> Self {
        Self {
            reader: Arc::new(Mutex::new(reader)),
            writer: Arc::new(Mutex::new(writer)),
            peer,
        }
    }

    /// Create a disconnected channel that will connect to the given `peer` in the background,
    /// and then all reads and writes on this channel will be forwarded to the peer connection.
    pub fn disconnected<C>(peer: Socks5Target) -> Self
    where
        C: AsyncConnectExt<ReadHalf = R, WriteHalf = W> + Default,
    {
        let (read_tx, read_rx) = oneshot::channel();
        let (write_tx, write_rx) = oneshot::channel();

        let ready_to_connect = Arc::new(Notify::new());
        let connect_target = peer.clone();
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

        let r_io = ChannelIo::Disconnected((ready.clone(), read_rx));
        let w_io = ChannelIo::Disconnected((ready, write_rx));

        Self::new(r_io, w_io, Some(peer))
    }

    pub fn connected(net_src: R, net_dst: W) -> Self {
        Self::new(
            ChannelIo::Connected(net_src),
            ChannelIo::Connected(net_dst),
            None,
        )
    }
}

impl<R, W> Clone for Channel<R, W>
where
    R: AsyncRead + Send + Unpin,
    W: AsyncWrite + Send + Unpin,
{
    fn clone(&self) -> Self {
        Self {
            reader: self.reader.clone(),
            writer: self.writer.clone(),
            peer: self.peer.clone(),
        }
    }
}

impl<R, W> AsyncRead for Channel<R, W>
where
    R: AsyncRead + Send + Unpin,
    W: AsyncWrite + Send + Unpin,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut ReadBuf,
    ) -> Poll<io::Result<()>> {
        // Obtain the read lock before proceeding.
        let mut r_future = Box::pin(self.reader.lock());
        let mut r_state = futures::ready!(r_future.as_mut().poll(cx));

        let buf_len_before = buf.filled().len();

        let result = match &mut *r_state {
            ChannelIo::Connected(reader) => Pin::new(reader).poll_read(cx, buf),
            ChannelIo::Disconnected((notify, receiver)) => {
                // Notify the connection task to do the connection now.
                notify.notify_one();

                // Wait for the connection result.
                match receiver.poll_unpin(cx) {
                    Poll::Ready(Ok(conn_result)) => match conn_result {
                        Ok(mut reader) => {
                            let result = Pin::new(&mut reader).poll_read(cx, buf);
                            *r_state = ChannelIo::Connected(reader);
                            result
                        }
                        Err(e) => Poll::Ready(Err(e)),
                    },
                    Poll::Ready(Err(e)) => Poll::Ready(Err(broken_pipe_error(e))),
                    Poll::Pending => Poll::Pending,
                }
            }
            ChannelIo::Error(e) => Poll::Ready(Err(io::Error::from(e.kind()))),
        };

        if let Poll::Ready(Err(e)) = &result {
            if !matches!(&*r_state, ChannelIo::Error(_)) {
                *r_state = ChannelIo::Error(io::Error::from(e.kind()));
            }
        }

        let buf_len_after = buf.filled().len();

        log::trace!(
            "poll_read({:?}) state: {:?}, result: {result:?}, buf_len: {buf_len_before}->{buf_len_after}",
            self.peer,
            &mut *r_state
        );

        result
    }
}

impl<R, W> AsyncWrite for Channel<R, W>
where
    R: AsyncRead + Send + Unpin,
    W: AsyncWrite + Send + Unpin,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        // Obtain the write lock before proceeding.
        let mut w_future = Box::pin(self.writer.lock());
        let mut w_state = futures::ready!(w_future.as_mut().poll(cx));

        let result = match &mut *w_state {
            ChannelIo::Connected(writer) => Pin::new(writer).poll_write(cx, buf),
            ChannelIo::Disconnected((notify, receiver)) => {
                // Notify the connection task to do the connection now.
                notify.notify_one();

                // Wait for the connection result.
                match receiver.poll_unpin(cx) {
                    Poll::Ready(Ok(conn_result)) => match conn_result {
                        Ok(mut writer) => {
                            let result = Pin::new(&mut writer).poll_write(cx, buf);
                            *w_state = ChannelIo::Connected(writer);
                            result
                        }
                        Err(e) => Poll::Ready(Err(e)),
                    },
                    Poll::Ready(Err(e)) => Poll::Ready(Err(broken_pipe_error(e))),
                    Poll::Pending => Poll::Pending,
                }
            }
            ChannelIo::Error(e) => Poll::Ready(Err(io::Error::from(e.kind()))),
        };

        if let Poll::Ready(Err(e)) = &result {
            if !matches!(&*w_state, ChannelIo::Error(_)) {
                *w_state = ChannelIo::Error(io::Error::from(e.kind()));
            }
        }

        log::trace!(
            "poll_write({:?}) state: {:?}, result: {result:?}",
            self.peer,
            &mut *w_state
        );

        result
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        // Obtain the write lock before proceeding.
        let mut w_future = Box::pin(self.writer.lock());
        let mut w_state = futures::ready!(w_future.as_mut().poll(cx));

        let result = match &mut *w_state {
            ChannelIo::Connected(writer) => Pin::new(writer).poll_flush(cx),
            ChannelIo::Disconnected((notify, receiver)) => {
                // Notify the connection task to do the connection now.
                notify.notify_one();

                // Wait for the connection result.
                match receiver.poll_unpin(cx) {
                    Poll::Ready(Ok(conn_result)) => match conn_result {
                        Ok(mut writer) => {
                            let result = Pin::new(&mut writer).poll_flush(cx);
                            *w_state = ChannelIo::Connected(writer);
                            result
                        }
                        Err(e) => Poll::Ready(Err(e)),
                    },
                    Poll::Ready(Err(e)) => Poll::Ready(Err(broken_pipe_error(e))),
                    Poll::Pending => Poll::Pending,
                }
            }
            ChannelIo::Error(e) => Poll::Ready(Err(io::Error::from(e.kind()))),
        };

        if let Poll::Ready(Err(e)) = &result {
            if !matches!(&*w_state, ChannelIo::Error(_)) {
                *w_state = ChannelIo::Error(io::Error::from(e.kind()));
            }
        }

        log::trace!(
            "poll_flush({:?}) state: {:?}, result: {result:?}",
            self.peer,
            &mut *w_state
        );

        result
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        // Obtain the write lock before proceeding.
        let mut w_future = Box::pin(self.writer.lock());
        let mut w_state = futures::ready!(w_future.as_mut().poll(cx));

        let result = match &mut *w_state {
            ChannelIo::Connected(writer) => Pin::new(writer).poll_shutdown(cx),
            ChannelIo::Disconnected((notify, receiver)) => {
                // Notify the connection task to do the connection now.
                notify.notify_one();

                // Wait for the connection result.
                match receiver.poll_unpin(cx) {
                    Poll::Ready(Ok(conn_result)) => match conn_result {
                        Ok(mut writer) => {
                            let result = Pin::new(&mut writer).poll_shutdown(cx);
                            *w_state = ChannelIo::Connected(writer);
                            result
                        }
                        Err(e) => Poll::Ready(Err(e)),
                    },
                    Poll::Ready(Err(e)) => Poll::Ready(Err(broken_pipe_error(e))),
                    Poll::Pending => Poll::Pending,
                }
            }
            ChannelIo::Error(e) => Poll::Ready(Err(io::Error::from(e.kind()))),
        };

        if let Poll::Ready(Err(e)) = &result {
            if !matches!(&*w_state, ChannelIo::Error(_)) {
                *w_state = ChannelIo::Error(io::Error::from(e.kind()));
            }
        }

        log::trace!(
            "poll_shutdown({:?}) state: {:?}, result: {result:?}",
            self.peer,
            &mut *w_state
        );

        result
    }
}

fn broken_pipe_error(e: RecvError) -> io::Error {
    io::Error::new(
        std::io::ErrorKind::BrokenPipe,
        format!("RecvError from connect pipe: {e}"),
    )
}

#[cfg(test)]
mod tests {
    // TODO
}
