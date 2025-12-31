use std::pin::Pin;
use std::task::{Context, Poll, ready};

use bytes::{Bytes, BytesMut};
use futures::Stream;
use tokio::io::AsyncRead;
use tokio_util::codec::Encoder;
use tokio_util::io::poll_read_buf;

use crate::common::sync::PollMutex;
use crate::net::CHUNK_SIZE;
use crate::net::proto::tunnel::message::TunnelMessage;
use crate::net::proto::turbo::codec::TurboCodec;
use crate::net::proto::turbo::message::TurboMessage;
use crate::net::proto::turbo::state::TurboState;

pub struct TurboStream<R: AsyncRead + Send + Unpin> {
    id: u64,
    state: PollMutex<TurboState>,
    reader: Option<R>,
    buf: BytesMut,
    mode: Mode,
}

#[derive(Debug, PartialEq)]
enum Mode {
    Idle,
    Read,
    Receive(Poll<Option<Bytes>>),
    Closed,
}

impl<R: AsyncRead + Send + Unpin> TurboStream<R> {
    pub fn new(id: u64, state: PollMutex<TurboState>, reader: R) -> Self {
        Self {
            id,
            state,
            reader: Some(reader),
            buf: BytesMut::with_capacity(CHUNK_SIZE),
            mode: Mode::Idle,
        }
    }

    fn poll_state(&mut self, cx: &mut Context) -> Poll<Option<TurboMessage>> {
        loop {
            match &mut self.mode {
                Mode::Idle => {
                    let mut state = ready!(self.state.poll_lock(cx));
                    // Check if the next message can carry a payload.
                    if state.is_payload_next() {
                        self.mode = Mode::Read
                    } else {
                        return state.poll_recv(cx);
                    }
                }
                Mode::Read => self.mode = Mode::Receive(self.poll_reader(cx)),
                Mode::Receive(read_result) => {
                    let mut state = ready!(self.state.poll_lock(cx));
                    let result = state.poll_recv_with_payload(cx, read_result);
                    self.mode = Mode::Idle;
                    return result;
                }
                Mode::Closed => return Poll::Ready(None),
            }
        }
    }

    fn poll_reader(&mut self, cx: &mut Context) -> Poll<Option<Bytes>> {
        if let Some(mut io) = self.reader.take() {
            match poll_read_buf(Pin::new(&mut io), cx, &mut self.buf) {
                Poll::Ready(Ok(0)) => {} // Drop reader on read EOF
                Poll::Ready(Ok(_len)) => self.reader = Some(io),
                Poll::Ready(Err(_e)) => {} // Drop reader on read error
                Poll::Pending => self.reader = Some(io),
            }
        }

        if !self.buf.is_empty() {
            Poll::Ready(Some(self.take_payload()))
        } else if self.reader.is_none() {
            Poll::Ready(None)
        } else {
            Poll::Pending
        }
    }

    fn take_payload(&mut self) -> Bytes {
        let bytes = self.buf.clone().freeze();
        self.buf.clear();
        bytes
    }
}

impl<R: AsyncRead + Send + Unpin> Stream for TurboStream<R> {
    type Item = TunnelMessage;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        match self.as_mut().poll_state(cx) {
            Poll::Ready(Some(msg)) => {
                let msg = TunnelMessage::encapsulated(self.id, encode(msg));
                Poll::Ready(Some(msg))
            }
            Poll::Ready(None) => {
                self.reader = None; // Drop reader when state is shut
                self.mode = Mode::Closed;
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Encode a TurboMessage for tunnel encapsulation.
fn encode(msg: TurboMessage) -> Bytes {
    let mut buf = BytesMut::new();
    TurboCodec.encode(msg, &mut buf).unwrap();
    buf.freeze()
}
