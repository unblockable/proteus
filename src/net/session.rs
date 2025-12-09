use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures::{FutureExt, Sink, SinkExt, Stream, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::oneshot::{self, Receiver};

use crate::net::proto::socks::address::Socks5Target;
use crate::net::{AsyncConnect, AsyncConnectExt};

pub trait SessionBuilder {
    type Message;
    type ReadHalf: AsyncRead + Send + Unpin;
    type WriteHalf: AsyncWrite + Send + Unpin;
    type StreamHalf: Stream<Item = Self::Message> + Send + Unpin;
    type SinkHalf: Sink<Self::Message, Error = io::Error> + Send + Unpin;

    fn build(
        id: u64,
        src: Self::ReadHalf,
        dst: Self::WriteHalf,
    ) -> (Self::StreamHalf, Self::SinkHalf);
}

pub struct Session<T: SessionBuilder> {
    stream: SessionHalf<T::StreamHalf>,
    sink: SessionHalf<T::SinkHalf>,
}

pub struct SessionHalf<T> {
    io: SessionIo<T>,
}

enum SessionIo<T> {
    Connected(T),
    Disconnected(Receiver<io::Result<T>>),
}

impl<T: SessionBuilder> Session<T> {
    pub fn from_io(id: u64, src: T::ReadHalf, dst: T::WriteHalf) -> Session<T> {
        let (stream, sink) = T::build(id, src, dst);

        let stream_half = SessionHalf {
            io: SessionIo::Connected(stream),
        };
        let sink_half = SessionHalf {
            io: SessionIo::Connected(sink),
        };

        Session {
            stream: stream_half,
            sink: sink_half,
        }
    }

    pub fn from_connector<C>(id: u64, connector: C, target: Socks5Target) -> Session<T>
    where
        C: AsyncConnect<ReadHalf = T::ReadHalf, WriteHalf = T::WriteHalf>,
        C: AsMut<C> + Send + Unpin + 'static,
        T::StreamHalf: 'static,
        T::SinkHalf: 'static,
    {
        // Communicate the connect result with the session halves.
        let (read_tx, read_rx) = oneshot::channel();
        let (write_tx, write_rx) = oneshot::channel();

        // Spawn a background task to establish and split the connection.
        tokio::spawn(async move {
            let mut connector = connector;
            let result = connector.connect(target).await;

            match result {
                Ok((src, dst, _name)) => {
                    let (stream, sink) = T::build(id, src, dst);
                    let _ = read_tx.send(Ok(stream));
                    let _ = write_tx.send(Ok(sink));
                }
                Err(e) => {
                    let _ = read_tx.send(Err(io::Error::from(e.kind())));
                    let _ = write_tx.send(Err(e));
                }
            }
        });

        let stream_half = SessionHalf {
            io: SessionIo::Disconnected(read_rx),
        };

        let sink_half = SessionHalf {
            io: SessionIo::Disconnected(write_rx),
        };

        Session {
            stream: stream_half,
            sink: sink_half,
        }
    }

    pub fn into_split(self) -> (SessionHalf<T::StreamHalf>, SessionHalf<T::SinkHalf>) {
        (self.stream, self.sink)
    }
}

impl<T: SessionBuilder> Stream for Session<T> {
    type Item = T::Message;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        self.get_mut().stream.poll_next_unpin(cx)
    }
}

impl<T: Stream + Unpin> Stream for SessionHalf<T> {
    type Item = T::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        match &mut this.io {
            SessionIo::Connected(stream) => stream.poll_next_unpin(cx),
            SessionIo::Disconnected(chan_rx) => match chan_rx.poll_unpin(cx) {
                Poll::Ready(Ok(factory_result)) => match factory_result {
                    Ok(mut stream) => {
                        let result = stream.poll_next_unpin(cx);
                        this.io = SessionIo::Connected(stream);
                        result
                    }
                    Err(_) => Poll::Ready(None),
                },
                Poll::Ready(Err(_)) => Poll::Ready(None),
                Poll::Pending => Poll::Pending,
            },
        }
    }
}

impl<Item, T: SessionBuilder<Message = Item>> Sink<Item> for Session<T> {
    type Error = io::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        self.get_mut().sink.poll_ready_unpin(cx)
    }

    fn start_send(self: Pin<&mut Self>, item: Item) -> Result<(), Self::Error> {
        self.get_mut().sink.start_send_unpin(item)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        self.get_mut().sink.poll_flush_unpin(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        self.get_mut().sink.poll_close_unpin(cx)
    }
}

impl<Item, T> Sink<Item> for SessionHalf<T>
where
    T: Sink<Item> + Unpin,
    T::Error: From<io::Error>,
{
    type Error = T::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();

        match &mut this.io {
            SessionIo::Connected(sink) => sink.poll_ready_unpin(cx),
            SessionIo::Disconnected(chan_rx) => match chan_rx.poll_unpin(cx) {
                Poll::Ready(Ok(factory_result)) => match factory_result {
                    Ok(mut sink) => {
                        let result = sink.poll_ready_unpin(cx);
                        this.io = SessionIo::Connected(sink);
                        result
                    }
                    Err(e) => Poll::Ready(Err(T::Error::from(e))),
                },
                Poll::Ready(Err(_)) => {
                    Poll::Ready(Err(io::Error::from(io::ErrorKind::BrokenPipe).into()))
                }
                Poll::Pending => Poll::Pending,
            },
        }
    }

    fn start_send(self: Pin<&mut Self>, item: Item) -> Result<(), Self::Error> {
        let SessionIo::Connected(sink) = &mut self.get_mut().io else {
            return Err(io::Error::from(io::ErrorKind::BrokenPipe).into());
        };

        sink.start_send_unpin(item)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        match &mut self.get_mut().io {
            SessionIo::Connected(sink) => sink.poll_flush_unpin(cx),
            SessionIo::Disconnected(chan_rx) => {
                chan_rx.close();
                Poll::Ready(Ok(()))
            }
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        match &mut self.get_mut().io {
            SessionIo::Connected(sink) => sink.poll_close_unpin(cx),
            SessionIo::Disconnected(chan_rx) => {
                chan_rx.close();
                Poll::Ready(Ok(()))
            }
        }
    }
}
