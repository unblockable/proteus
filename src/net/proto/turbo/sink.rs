use std::io::{self, Cursor};
use std::pin::Pin;
use std::task::{Context, Poll, Waker, ready};

use bytes::{Buf, Bytes, BytesMut};
use futures::Sink;
use tokio::io::AsyncWrite;
use tokio_util::codec::Decoder;
use tokio_util::io::poll_write_buf;

use crate::common::sync::PollMutex;
use crate::net::proto::tunnel::message::{TunnelMessage, TunnelMessageKind};
use crate::net::proto::turbo::codec::TurboCodec;
use crate::net::proto::turbo::message::TurboMessage;
use crate::net::proto::turbo::state::TurboState;

pub struct TurboSink<W: AsyncWrite> {
    state: PollMutex<TurboState>,
    writer: Option<W>,
    phase: Phase,
    mode: Mode,
}

#[derive(Debug, PartialEq)]
enum Phase {
    Open,
    Closing,
    Shutting,
    Closed,
}

#[derive(Debug, PartialEq)]
enum Mode {
    Idle,
    Send(TurboMessage),
    Write(Cursor<Bytes>),
    Closed,
}

impl<W: AsyncWrite + Unpin> TurboSink<W> {
    pub fn new(state: PollMutex<TurboState>, writer: W) -> Self {
        Self {
            writer: Some(writer),
            state,
            phase: Phase::Open,
            mode: Mode::Idle,
        }
    }

    fn poll_send_inner(&mut self, cx: &mut Context) -> Poll<io::Result<()>> {
        loop {
            match &mut self.mode {
                Mode::Idle => return Poll::Ready(Ok(())),
                Mode::Send(msg) => {
                    let mut state = ready!(self.state.poll_lock(cx));
                    self.mode = match state.send(msg) {
                        Some(bytes) => Mode::Write(Cursor::new(bytes)),
                        None => Mode::Idle,
                    };
                }
                Mode::Write(cursor) => {
                    if cursor.has_remaining() {
                        let writer = self.writer.as_mut().ok_or(broken_pipe_error())?;
                        match ready!(poll_write_buf(Pin::new(writer), cx, cursor)) {
                            Ok(_n) => continue, // Made progress, try again.
                            Err(e) => return Poll::Ready(self.error(e)),
                        }
                    } else {
                        self.mode = Mode::Idle;
                    }
                }
                Mode::Closed => return Poll::Ready(self.error(broken_pipe_error())),
            }
        }
    }

    fn poll_close_inner(&mut self, cx: &mut Context) -> Poll<io::Result<()>> {
        loop {
            match &mut self.phase {
                Phase::Open => self.phase = Phase::Closing,
                Phase::Closing => {
                    let mut state = ready!(self.state.poll_lock(cx));
                    state.close();
                    self.phase = Phase::Shutting;
                }
                Phase::Shutting => {
                    let writer = self.writer.as_mut().ok_or(broken_pipe_error())?;
                    let result = ready!(Pin::new(writer).poll_shutdown(cx));
                    self.set_closed();
                    return Poll::Ready(result);
                }
                Phase::Closed => return Poll::Ready(self.error(broken_pipe_error())),
            }
        }
    }

    fn set_closed(&mut self) {
        self.phase = Phase::Closed;
        self.mode = Mode::Closed;
        self.writer = None;
    }

    fn idle(&mut self) -> io::Result<()> {
        match self.mode {
            Mode::Idle => Ok(()),
            Mode::Send(_) => Err(would_block_error()), // Recoverable
            Mode::Write(_) => Err(would_block_error()), // Recoverable
            Mode::Closed => self.error(broken_pipe_error()),
        }
    }

    fn error(&mut self, e: io::Error) -> io::Result<()> {
        self.set_closed();
        Err(e)
    }
}

impl<W: AsyncWrite + Send + Unpin> Sink<TunnelMessage> for TurboSink<W> {
    type Error = io::Error;

    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        self.as_mut().poll_send_inner(cx)
    }

    fn start_send(mut self: Pin<&mut Self>, item: TunnelMessage) -> Result<(), Self::Error> {
        // Return error if we're not ready.
        self.idle()?;

        // We handle encapsulated messages and drop the others.
        match item.kind {
            TunnelMessageKind::Open(_) => {} // Drop.
            TunnelMessageKind::Encapsulated(bytes) => {
                let mut buf = BytesMut::with_capacity(bytes.len());
                buf.extend_from_slice(&bytes);

                match TurboCodec.decode(&mut buf) {
                    Ok(Some(msg)) => {
                        self.mode = Mode::Send(msg);

                        // Do a best-effort write while ignoring pending signals. If
                        // the write cannot complete now, we'll poll again in the next
                        // call to `poll_ready()` or `poll_flush()`.
                        let mut cx = Context::from_waker(Waker::noop());
                        if let Poll::Ready(result) = self.as_mut().poll_send_inner(&mut cx) {
                            return result;
                        }
                    }
                    Ok(None) => unreachable!(), // `TunnelMessage` is complete.
                    Err(_) => todo!(),          // Encapsulated message is corrupt.
                }
            }
            TunnelMessageKind::Close => {} // Drop.
        }

        Ok(())
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        // Flush our own layer, and then flush the wrapped writer.
        ready!(self.as_mut().poll_send_inner(cx))?;
        let writer = self.writer.as_mut().ok_or(broken_pipe_error())?;
        Pin::new(writer).poll_flush(cx)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), Self::Error>> {
        // Flush our own layer, and then close the inner objects.
        ready!(self.as_mut().poll_send_inner(cx))?;
        self.poll_close_inner(cx)
    }
}

fn broken_pipe_error() -> io::Error {
    io::Error::new(
        std::io::ErrorKind::BrokenPipe,
        format!("No writer for write operation"),
    )
}

fn would_block_error() -> io::Error {
    io::Error::new(
        std::io::ErrorKind::WouldBlock,
        format!("Busy sending previous message"),
    )
}
