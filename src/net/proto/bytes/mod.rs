use std::io::{self, Cursor};
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll, Waker, ready};

use bytes::{Buf, Bytes, BytesMut};
use futures::{Sink, Stream};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::io::{poll_read_buf, poll_write_buf};

use crate::net::proto::tunnel::codec::TunnelCodec;
use crate::net::proto::tunnel::message::{TunnelMessage, TunnelMessageKind};
use crate::net::session::SessionBuilder;

pub struct BytesSession<R: AsyncRead + Send + Unpin, W: AsyncWrite + Send + Unpin> {
    _r: PhantomData<R>,
    _w: PhantomData<W>,
}

impl<R, W> SessionBuilder for BytesSession<R, W>
where
    R: AsyncRead + Send + Unpin,
    W: AsyncWrite + Send + Unpin,
{
    type Message = TunnelMessage;
    type ReadHalf = R;
    type WriteHalf = W;
    type StreamHalf = BytesStream<R>;
    type SinkHalf = BytesSink<W>;

    fn build(
        id: u64,
        src: Self::ReadHalf,
        dst: Self::WriteHalf,
    ) -> (Self::StreamHalf, Self::SinkHalf) {
        (
            BytesStream { io: src, id },
            BytesSink { io: dst, buf: None },
        )
    }
}

pub struct BytesStream<R: AsyncRead + Send + Unpin> {
    io: R,
    id: u64,
}

impl<R: AsyncRead + Send + Unpin> Stream for BytesStream<R> {
    type Item = TunnelMessage;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let mut buf = BytesMut::with_capacity(TunnelCodec::encapsulated_bytes_max_len());
        match poll_read_buf(Pin::new(&mut self.io), cx, &mut buf) {
            Poll::Ready(Ok(0)) => Poll::Ready(None),
            Poll::Ready(Ok(_len)) => {
                Poll::Ready(Some(TunnelMessage::encapsulated(self.id, buf.freeze())))
            }
            Poll::Ready(Err(_e)) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

pub struct BytesSink<W: AsyncWrite> {
    io: W,
    buf: Option<Cursor<Bytes>>,
}

impl<W: AsyncWrite + Send + Unpin> BytesSink<W> {
    fn poll_write_inner(&mut self, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        if let Some(mut cursor) = self.buf.take() {
            while cursor.has_remaining() {
                match poll_write_buf(Pin::new(&mut self.io), cx, &mut cursor) {
                    Poll::Ready(Ok(_n)) => continue,
                    Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                    Poll::Pending => {
                        self.buf = Some(cursor);
                        return Poll::Pending;
                    }
                };
            }
        }
        Poll::Ready(Ok(()))
    }
}

impl<W: AsyncWrite + Send + Unpin> Sink<TunnelMessage> for BytesSink<W> {
    type Error = io::Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        self.as_mut().poll_write_inner(cx)
    }

    fn start_send(mut self: Pin<&mut Self>, item: TunnelMessage) -> Result<(), Self::Error> {
        if self.buf.is_some() {
            Err(io::ErrorKind::WouldBlock.into())
        } else {
            if let TunnelMessageKind::Encapsulated(bytes) = item.kind {
                self.buf = Some(Cursor::new(bytes));
                // This is best-effort, so it's safe to ignore pending signals.
                let mut cx = Context::from_waker(Waker::noop());
                if let Poll::Ready(result) = self.as_mut().poll_ready(&mut cx) {
                    return result;
                }
            }
            Ok(())
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        ready!(self.as_mut().poll_ready(cx))?;
        Pin::new(&mut self.io).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        ready!(self.as_mut().poll_flush(cx))?;
        Pin::new(&mut self.io).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use crate::common::mock;
    use crate::net::proto::BytesSession;
    use crate::net::tunnel::tests::{
        MockIoKind, proxy_network_connected, proxy_network_disconnected,
    };

    #[tokio::test]
    async fn connected_direct_tunnel() {
        // let _ = env_logger::try_init();
        for len in mock::payload_len_iter() {
            proxy_network_connected::<BytesSession<_, _>>(MockIoKind::Direct, len)
                .await
                .assert(len);
        }
    }

    #[tokio::test]
    async fn connected_interpreter_tunnel() {
        // let _ = env_logger::try_init();
        for len in mock::payload_len_iter() {
            proxy_network_connected::<BytesSession<_, _>>(MockIoKind::Interpreter, len)
                .await
                .assert(len);
        }
    }

    #[tokio::test]
    async fn disconnected_direct_tunnel() {
        // let _ = env_logger::try_init();
        for len in mock::payload_len_iter() {
            proxy_network_disconnected::<BytesSession<_, _>>(MockIoKind::Direct, len)
                .await
                .assert(len);
        }
    }

    #[tokio::test]
    async fn disconnected_interpreter_tunnel() {
        // let _ = env_logger::try_init();
        for len in mock::payload_len_iter() {
            proxy_network_disconnected::<BytesSession<_, _>>(MockIoKind::Interpreter, len)
                .await
                .assert(len);
        }
    }
}
