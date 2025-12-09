use std::io::{self, Cursor};
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::{Buf, Bytes, BytesMut};
use futures::{Sink, Stream};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::io::{poll_read_buf, poll_write_buf};

use crate::net::READ_CAPACITY;
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
        let mut buf = BytesMut::with_capacity(READ_CAPACITY);
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

impl<W: AsyncWrite + Send + Unpin> Sink<TunnelMessage> for BytesSink<W> {
    type Error = io::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        self.poll_flush(cx)
    }

    fn start_send(mut self: Pin<&mut Self>, item: TunnelMessage) -> Result<(), Self::Error> {
        if let Some(_) = self.buf {
            Err(io::ErrorKind::WouldBlock.into())
        } else {
            if let TunnelMessageKind::Encapsulated(bytes) = item.kind {
                self.buf = Some(Cursor::new(bytes));
            }
            Ok(())
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
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
        Pin::new(&mut self.io).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        let result = self.as_mut().poll_flush(cx);
        if let Poll::Ready(Ok(_)) = result {
            Pin::new(&mut self.io).poll_shutdown(cx)
        } else {
            result
        }
    }
}
