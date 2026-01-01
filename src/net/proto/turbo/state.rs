use std::collections::VecDeque;
use std::task::{Context, Poll, Waker};

use crate::net::proto::turbo::message::{
    self, Command, DataCursor, Payload, Request, Response, TurboMessage,
};
use bytes::Bytes;

/// The state needed to implement our turbo session protocol that needs to be
/// shared across the session reader and writer forwarding directions.
#[derive(Default)]
pub struct TurboState {
    /// The stream's waker if it is waiting for us to produce a message.
    stream_waker: Option<Waker>,
    /// The current phase of the protocol.
    state: State,
    /// The cursor of written and acknowledged data. Data before this cursor was
    /// written and acked, data at and after this cursor was not yet acked.
    write_ack: DataCursor,
    /// The write cursor for the next data we send to the peer.
    write: DataCursor,
    /// The final value of the write cursor once we reached EOF on the source.
    write_end: Option<DataCursor>,
    /// The read cursor for data we have successfully received from the peer.
    read: DataCursor,
    /// The final value of the read cursor.
    read_end: Option<DataCursor>,
    /// True if we should return an ack next.
    next_ack: bool,
    /// Holds control messages that should be sent before any forward messages.
    queue: VecDeque<MessageResponse>,
}

#[derive(Default, Debug, Copy, Clone, PartialEq)]
enum State {
    #[default]
    LocalOpenRemoteOpen,
    LocalShuttingRemoteOpen,
    LocalShutRemoteOpen,
    LocalOpenRemoteShutting,
    LocalOpenRemoteShut,
    LocalShuttingRemoteShutting,
    LocalShutRemoteShutting,
    LocalShuttingRemoteShut,
    LocalShutRemoteShut,
}

#[derive(Debug, Copy, Clone, PartialEq)]
enum MessageResponse {
    ShutOk,
    Reset,
}

impl TurboState {
    /// Returns `true` if we want to add payload to the next outgoing message,
    /// `false` otherwise. This informs the caller to read from the source
    /// prior to calling `poll_next()` to provide any available bytes.
    pub fn is_payload_next(&self) -> bool {
        if self.queue.is_empty() {
            match self.state {
                State::LocalOpenRemoteOpen => true,
                State::LocalOpenRemoteShutting => true,
                State::LocalOpenRemoteShut => true,
                _ => false,
            }
        } else {
            false
        }
    }

    pub fn poll_recv(&mut self, cx: &mut Context) -> Poll<Option<TurboMessage>> {
        if let Some(response) = self.queue.pop_front() {
            let msg = match response {
                MessageResponse::Reset => TurboMessage::reset(self.write_inc(), self.read),
                MessageResponse::ShutOk => TurboMessage::shut_ok(self.write_inc(), self.read),
            };
            Poll::Ready(Some(msg))
        } else if self.state == State::LocalShutRemoteShut {
            Poll::Ready(None)
        } else if self.next_ack {
            let ack = TurboMessage::forward_ok(self.write_inc(), self.read);
            self.next_ack = false;
            Poll::Ready(Some(ack))
        } else {
            self.set_waker(cx);
            Poll::Pending
        }
    }

    pub fn poll_recv_with_payload(
        &mut self,
        cx: &mut Context,
        payload: &mut Poll<Option<Bytes>>,
    ) -> Poll<Option<TurboMessage>> {
        assert!(self.is_payload_next());

        // We asked the stream to try reading a payload first and give us the result.
        // The message we return next depends on if a payload is ready or not.
        match payload {
            Poll::Ready(opt) => match opt.take() {
                Some(bytes) => {
                    // Forward the payload. This doubles as an ack too.
                    let fwd = TurboMessage::forward(self.write_inc(), self.read, bytes);
                    self.next_ack = false;
                    Poll::Ready(Some(fwd))
                }
                None => {
                    // Reader got Error/EOF, we get no more payloads, start shuting our side.
                    let new_state = match self.state {
                        State::LocalOpenRemoteOpen => State::LocalShuttingRemoteOpen,
                        State::LocalOpenRemoteShutting => State::LocalShuttingRemoteShutting,
                        State::LocalOpenRemoteShut => State::LocalShuttingRemoteShut,
                        _ => unreachable!(),
                    };
                    self.set_state(new_state);

                    // We are done writing now.
                    self.write_end.get_or_insert(self.write);
                    let shut = TurboMessage::shut(self.write_inc(), self.read);
                    self.next_ack = false;
                    Poll::Ready(Some(shut))
                }
            },
            Poll::Pending => {
                // No payload bytes are ready yet.
                if self.next_ack {
                    // We need to send an ack now anyway.
                    let ack = TurboMessage::forward_ok(self.write_inc(), self.read);
                    self.next_ack = false;
                    Poll::Ready(Some(ack))
                } else {
                    // The stream should wakeup if payload arrives, we should wakeup
                    // if we need to send a control or ack message.
                    self.set_waker(cx);
                    Poll::Pending
                }
            }
        }
    }

    pub fn send(&mut self, msg: &TurboMessage) -> Option<Bytes> {
        // Process the message, return any payload bytes that should be written.
        if msg.write < self.read {
            log::debug!("Dup message: expected {}, got {}", self.read, msg.write);
        } else if msg.write > self.read {
            // TODO initiate a rewind?
            log::warn!("Lost message: expected {}, got {}", self.read, msg.write);
        }

        // Process the message if it is the next in sequence.
        let is_valid = if msg.write == self.read {
            self.read += 1;
            self.write_ack = self.write_ack.max(msg.read);

            // Will return false if we're in the wrong state.
            let is_valid = match &msg.command {
                Command::Request(request) => match request {
                    Request::Forward(payload) => match self.process_fwd_req(payload) {
                        Some(bytes) => return Some(bytes),
                        None => false,
                    },
                    Request::Rewind => self.process_rwd_req(),
                    Request::Shut => self.process_shut_req(msg.write),
                },
                Command::Response(response) => match response {
                    Response::Forward(result) => self.process_fwd_resp(result),
                    Response::Rewind(result) => self.process_rwd_resp(result),
                    Response::Shut(result) => self.process_shut_resp(result),
                },
                Command::Reset => self.process_reset(),
            };

            // We incremented our read cursor, so the remote might be shut now.
            self.shut_remote_if_done_reading();
            is_valid
        } else {
            false
        };

        let prefix = if is_valid { "Processed" } else { "Dropped" };
        log::trace!("{prefix} message {msg:?} in state {:?}", self.state);
        None
    }

    pub fn close(&mut self) {
        // The sink will not call send() anymore, so we will never get another message.
        // We require bidirectional communication for acks and a graceful shutdown.
        if self.state != State::LocalShutRemoteShut {
            self.write_end.get_or_insert(self.write);
            self.read_end.get_or_insert(self.read);
            self.queue(MessageResponse::Reset);
            self.set_state(State::LocalShutRemoteShut);
        }
    }
}

impl TurboState {
    fn set_waker(&mut self, cx: &mut Context) {
        match self.stream_waker.as_mut() {
            Some(w) => w.clone_from(cx.waker()),
            None => self.stream_waker = Some(cx.waker().clone()),
        }
    }

    fn wake(&self) {
        if let Some(waker) = self.stream_waker.as_ref() {
            waker.wake_by_ref()
        }
        // self.waker.take().map(|w| w.wake());
    }

    fn set_state(&mut self, new_state: State) {
        self.state = new_state;
        // Wake stream if we need to return Ready(None).
        if self.state == State::LocalShutRemoteShut {
            self.wake();
        }
    }

    fn write_inc(&mut self) -> DataCursor {
        let val = self.write;
        self.write += 1;
        val
    }

    fn queue(&mut self, response: MessageResponse) {
        self.queue.push_back(response);
        self.wake();
    }

    fn process_fwd_req(&mut self, payload: &Payload) -> Option<Bytes> {
        // Valid unless the remote side is already shut.
        let is_valid = match self.state {
            State::LocalOpenRemoteOpen => true,
            State::LocalShuttingRemoteOpen => true,
            State::LocalShutRemoteOpen => true,
            State::LocalOpenRemoteShutting => true,
            State::LocalOpenRemoteShut => false,
            State::LocalShuttingRemoteShutting => true,
            State::LocalShutRemoteShutting => true,
            State::LocalShuttingRemoteShut => false,
            State::LocalShutRemoteShut => false,
        };

        is_valid.then(|| {
            // Arrange for us to reply with an ack.
            self.next_ack = true;
            self.wake();
            // Return the payload bytes so they get written.
            payload.data.clone()
        })
    }

    fn process_fwd_resp(&mut self, result: &message::Result) -> bool {
        // No special processing needed outside of updating the read
        // cursor as we do for all message types.
        true
    }

    fn process_rwd_req(&mut self) -> bool {
        todo!()
    }

    fn process_rwd_resp(&mut self, result: &message::Result) -> bool {
        todo!()
    }

    fn process_shut_req(&mut self, cursor: DataCursor) -> bool {
        // Valid if the remote side is still open.
        let shut_state = match self.state {
            State::LocalOpenRemoteOpen => State::LocalOpenRemoteShutting,
            State::LocalShuttingRemoteOpen => State::LocalShuttingRemoteShutting,
            State::LocalShutRemoteOpen => State::LocalShutRemoteShutting,
            _ => return false,
        };

        // Store the final position of the read cursor.
        self.read_end.get_or_insert(cursor);

        // The remote moves from open to shutting.
        // Note: we might transition to shut every time we increment our read cursor.
        self.set_state(shut_state);
        true
    }

    fn process_shut_resp(&mut self, result: &message::Result) -> bool {
        // Valid only if the local side is shutting.
        let shut_state = match self.state {
            State::LocalShuttingRemoteOpen => State::LocalShutRemoteOpen,
            State::LocalShuttingRemoteShutting => State::LocalShutRemoteShutting,
            State::LocalShuttingRemoteShut => State::LocalShutRemoteShut,
            _ => return false,
        };

        let done_acking_writes = self
            .write_end
            .map(|end| self.write_ack >= end)
            .unwrap_or(false);

        if done_acking_writes {
            self.set_state(shut_state);
        } else {
            log::warn!("Got Shut response with incomplete ack, closing now");
            self.close();
        }

        true
    }

    fn process_reset(&mut self) -> bool {
        if self.state == State::LocalShutRemoteShut {
            return false;
        }

        // Overwrite any previously set end cursors.
        self.write_end = Some(self.write);
        self.read_end = Some(self.read);
        self.set_state(State::LocalShutRemoteShut);
        self.queue.clear();
        // Wake stream so we can return Ready(None).
        self.wake();
        true
    }

    fn shut_remote_if_done_reading(&mut self) {
        let shut_state = match self.state {
            State::LocalOpenRemoteShutting => State::LocalOpenRemoteShut,
            State::LocalShuttingRemoteShutting => State::LocalShuttingRemoteShut,
            State::LocalShutRemoteShutting => State::LocalShutRemoteShut,
            _ => return,
        };

        let done_reading = self.read_end.map(|end| self.read >= end).unwrap_or(false);

        if done_reading {
            self.queue(MessageResponse::ShutOk);
            self.set_state(shut_state);
        }
    }
}

#[cfg(test)]
mod tests {
    // TODO
}
