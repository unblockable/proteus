use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use bytes::BytesMut;
use futures::{Sink, Stream};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::io::{poll_read_buf, poll_write_buf};

use crate::net::READ_CAPACITY;
use crate::net::proto::socks::address::Socks5Target;
use crate::net::proto::turbo::message::{Command, Message, Payload, Request};

#[derive(Debug)]
pub enum TurboError {
    WouldBlock,
    Unknown,
}

pub struct TurboSession<R, W>
where
    R: AsyncRead + Send + Unpin,
    W: AsyncWrite + Send + Unpin,
{
    id: u64,
    stream: TurboStream<R>,
    sink: TurboSink<W>,
}

impl<R, W> TurboSession<R, W>
where
    R: AsyncRead + Send + Unpin,
    W: AsyncWrite + Send + Unpin,
{
    pub fn new(id: u64, src: R, dst: W) -> Self {
        let state = SharedSessionState::new(id);
        Self {
            id,
            stream: TurboStream {
                src,
                state: state.clone(),
                init: None,
            },
            sink: TurboSink {
                dst,
                pending: None,
                state,
            },
        }
    }

    pub fn open(&mut self, target: Socks5Target) {
        let msg = Message {
            session_id: self.id,
            write: 0,
            read: 0,
            command: Command::Request(Request::Open(target)),
        };
        self.stream.init = Some(msg);
    }

    pub fn into_split(self) -> (TurboStream<R>, TurboSink<W>) {
        (self.stream, self.sink)
    }
}

pub struct TurboStream<R: AsyncRead + Unpin> {
    src: R,
    state: SharedSessionState,
    init: Option<Message>,
}

impl<R: AsyncRead + Unpin> TurboStream<R> {
    pub fn _id(&self) -> u64 {
        self.state.id()
    }

    pub fn is_empty(&self) -> bool {
        matches!(self.init, None)
    }
}

impl<R: AsyncRead + Unpin> Stream for TurboStream<R> {
    type Item = Message;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        // TODO
        // do we need to remove any previously stored shared waker first?

        // check the current protocol state, if we are blocked then store the waker

        // then let each of our potential sources of Messages clone the waker so it can wake if needed.
        // mspc.receiver.poll_recv(cx)
        // if that is pending, then also get more bytes from the src using something like:
        // self.src.poll_recv(cx) or self.get_mut().src.poll_read(self, cx)

        if let Some(msg) = self.init.take() {
            return Poll::Ready(Some(msg));
        }

        let mut buf = BytesMut::with_capacity(READ_CAPACITY);
        match poll_read_buf(Pin::new(&mut self.src), cx, &mut buf) {
            Poll::Ready(Ok(0)) => Poll::Ready(None),
            Poll::Ready(Ok(_len)) => Poll::Ready(Some(Message {
                session_id: self.state.id,
                write: 0,
                read: 0,
                command: Command::Request(Request::Forward(Payload { data: buf.freeze() })),
            })),
            Poll::Ready(Err(_e)) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

pub struct TurboSink<W: AsyncWrite + Unpin> {
    dst: W,
    pending: Option<BytesMut>,
    state: SharedSessionState,
}

impl<W: AsyncWrite + Unpin> TurboSink<W> {
    fn process_message(&mut self, msg: Message) -> Result<(), TurboError> {
        // Make sure we store the payload bytes in the pending option if we have payload to write.

        if let Command::Request(Request::Forward(payload)) = msg.command {
            if self.pending.is_none() {
                let _ = self.pending.insert(BytesMut::from(payload.data));
                Ok(())
            } else {
                Err(TurboError::WouldBlock)
            }
        } else {
            Err(TurboError::Unknown)
        }
    }

    pub fn id(&self) -> u64 {
        self.state.id()
    }
}

impl<W: AsyncWrite + Unpin> Sink<Message> for TurboSink<W> {
    type Error = std::io::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        // A pending write must finish before we are ready.
        if self.pending.is_some() {
            self.poll_flush(cx)
        } else {
            // TODO: return an error if the turbo protocol completed and we want to be dropped.
            Poll::Ready(Ok(()))
        }
    }

    fn start_send(mut self: Pin<&mut Self>, msg: Message) -> Result<(), Self::Error> {
        if self.pending.is_none() {
            match self.process_message(msg) {
                Ok(_) => Ok(()),
                Err(e) => Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("{:?}", e),
                )),
            }
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "Cannot send, write is pending",
            ))
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        // If we have pending data, write that before flushing dst.
        if let Some(mut buf) = self.pending.take() {
            match poll_write_buf(Pin::new(&mut self.dst), cx, &mut buf) {
                Poll::Ready(Ok(_)) => Pin::new(&mut self.dst).poll_flush(cx),
                Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                Poll::Pending => {
                    let _ = self.pending.insert(buf);
                    Poll::Pending
                }
            }
        } else {
            Pin::new(&mut self.dst).poll_flush(cx)
        }
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        // Flush our own buffer, then shutdown the dst.
        match self.as_mut().poll_flush(cx) {
            Poll::Ready(Ok(_)) => Pin::new(&mut self.dst).poll_shutdown(cx),
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    }
}

struct SessionState {}

struct SharedSessionState {
    id: u64,
    inner: Arc<Mutex<SessionState>>,
    stream_waker: Arc<Mutex<Option<Waker>>>,
}

impl SharedSessionState {
    fn new(id: u64) -> Self {
        Self {
            id,
            inner: Arc::new(Mutex::new(SessionState {})),
            stream_waker: Arc::new(Mutex::new(None)),
        }
    }

    fn id(&self) -> u64 {
        self.id
    }
}

impl Clone for SharedSessionState {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            inner: self.inner.clone(),
            stream_waker: self.stream_waker.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    // TODO
}
