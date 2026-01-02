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
    queue: VecDeque<MessageKind>,
}

#[derive(Default, Debug, Copy, Clone, PartialEq)]
enum State {
    #[default]
    LocalOpenRemoteOpen,
    LocalShuttingRemoteOpen,
    LocalShutRemoteOpen,
    LocalOpenRemoteShut,
    LocalShuttingRemoteShut,
    LocalShutRemoteShut,
}

#[derive(Debug, Clone, PartialEq)]
enum MessageKind {
    Forward(Bytes),
    Shut,
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
                MessageKind::Forward(bytes) => {
                    TurboMessage::forward(self.write_inc(), self.read, bytes)
                }
                MessageKind::Shut => TurboMessage::shut(self.write_inc(), self.read),
                MessageKind::ShutOk => TurboMessage::shut_ok(self.write_inc(), self.read),
                MessageKind::Reset => TurboMessage::reset(self.write_inc(), self.read),
            };
            self.next_ack = false;
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
        // Should not be called unless is_payload_next() is true.
        assert!(self.is_payload_next());

        // We asked the stream to try reading a payload first and give us the result.
        // The message we return next depends on if a payload is ready or not.
        match payload {
            Poll::Ready(opt) => match opt.take() {
                // Forward along the payload.
                Some(bytes) => self.queue(MessageKind::Forward(bytes)),
                // Reader got Error/EOF, so we are done sending payloads now.
                None => {
                    // Initiate a graceful shutdown to indicate we are done writing.
                    let new_state = match self.state {
                        // Should match `is_payload_next()`.
                        State::LocalOpenRemoteOpen => State::LocalShuttingRemoteOpen,
                        State::LocalOpenRemoteShut => State::LocalShuttingRemoteShut,
                        _ => unreachable!(),
                    };
                    self.set_state(new_state);
                    self.write_end.get_or_insert(self.write);
                    self.queue(MessageKind::Shut);
                }
            },
            // Already handled in poll_recv().
            Poll::Pending => {}
        };

        // Construct and return any ready messages, or handle Pending logic.
        self.poll_recv(cx)
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
            self.queue(MessageKind::Reset);
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

    fn queue(&mut self, response: MessageKind) {
        self.queue.push_back(response);
        self.wake();
    }

    fn process_fwd_req(&mut self, payload: &Payload) -> Option<Bytes> {
        // Valid unless the remote side is already shut.
        let is_valid = match self.state {
            State::LocalOpenRemoteOpen => true,
            State::LocalShuttingRemoteOpen => true,
            State::LocalShutRemoteOpen => true,
            State::LocalOpenRemoteShut => false,
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
            State::LocalOpenRemoteOpen => State::LocalOpenRemoteShut,
            State::LocalShuttingRemoteOpen => State::LocalShuttingRemoteShut,
            State::LocalShutRemoteOpen => State::LocalShutRemoteShut,
            _ => return false,
        };

        // Store the final position of the read cursor.
        self.read_end.get_or_insert(cursor);

        // Our sequential protocol means we must have read everything before cursor.
        assert!(self.read >= cursor);

        // The remote moves from open to shut.
        self.queue(MessageKind::ShutOk);
        self.set_state(shut_state);
        true
    }

    fn process_shut_resp(&mut self, result: &message::Result) -> bool {
        // Valid only if the local side is shutting.
        let shut_state = match self.state {
            State::LocalShuttingRemoteOpen => State::LocalShutRemoteOpen,
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
}

#[cfg(test)]
mod tests {
    use std::future::PollFn;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Waker};

    use bytes::Bytes;
    use tokio_test::assert_ready_eq;
    use tokio_test::task::Spawn;

    use crate::net::proto::turbo::message::{DataCursor, TurboMessage};
    use crate::net::proto::turbo::state::{State, TurboState};

    #[test]
    fn new_default() {
        let ts = TurboState::default();
        assert_eq!(ts.state, State::LocalOpenRemoteOpen);
    }

    #[test]
    fn new_wants_payload() {
        let ts = TurboState::default();
        assert!(ts.is_payload_next());
    }

    fn new_pending_task() -> (
        Arc<Mutex<TurboState>>,
        Spawn<PollFn<impl FnMut(&mut Context) -> Poll<Option<TurboMessage>>>>,
    ) {
        let ts = Arc::new(Mutex::new(TurboState::default()));
        let shared = ts.clone();

        let mut task = tokio_test::task::spawn(std::future::poll_fn(move |cx| {
            shared
                .lock()
                .unwrap()
                .poll_recv_with_payload(cx, &mut Poll::Pending)
        }));

        assert_eq!(task.poll(), Poll::Pending);
        assert!(!task.is_woken());
        (ts, task)
    }

    #[test]
    fn wakeup_for_ack() {
        let (ts, task) = new_pending_task();
        let fwd = TurboMessage::forward(0, 0, Bytes::from("test"));
        ts.lock().unwrap().send(&fwd);
        assert!(task.is_woken());
    }

    #[test]
    fn wakeup_for_reset() {
        let (ts, task) = new_pending_task();
        let rst = TurboMessage::reset(0, 0);
        ts.lock().unwrap().send(&rst);
        assert!(task.is_woken());
    }

    #[test]
    fn wakeup_on_close() {
        let (ts, task) = new_pending_task();
        ts.lock().unwrap().close();
        assert!(task.is_woken());
    }

    #[test]
    fn forward_request() {
        let mut ts = TurboState::default();
        let mut cx = Context::from_waker(Waker::noop());
        let payload = Bytes::from("test");

        for w_cursor in 0..10 {
            let res = ts.poll_recv_with_payload(&mut cx, &mut Poll::Ready(Some(payload.clone())));
            assert_ready_eq!(
                res,
                Some(TurboMessage::forward(w_cursor, 0, payload.clone()))
            );
        }
    }

    fn deliver_n_fwd_msgs(ts: &mut TurboState, payload: &Bytes, n: DataCursor) {
        for w_cursor in 0..n {
            let msg = TurboMessage::forward(w_cursor, 0, payload.clone());
            assert_eq!(ts.send(&msg), Some(payload.clone()));
        }
    }

    fn assert_ack(ts: &mut TurboState, w: DataCursor, r: DataCursor) {
        let mut cx = Context::from_waker(Waker::noop());
        let res = ts.poll_recv_with_payload(&mut cx, &mut Poll::Pending);
        assert_ready_eq!(res, Some(TurboMessage::forward_ok(w, r)));
    }

    fn assert_fwd(ts: &mut TurboState, w: DataCursor, r: DataCursor, payload: &Bytes) {
        let mut cx = Context::from_waker(Waker::noop());
        let res = ts.poll_recv_with_payload(&mut cx, &mut Poll::Ready(Some(payload.clone())));
        assert_ready_eq!(res, Some(TurboMessage::forward(w, r, payload.clone())));
    }

    #[test]
    fn forward_response_payload() {
        let mut ts = TurboState::default();
        let payload = Bytes::from("test");

        deliver_n_fwd_msgs(&mut ts, &payload, 10);

        // Ack all 10 packets while including payload of our own.
        assert_fwd(&mut ts, 0, 10, &payload);
    }

    #[test]
    fn forward_response_ack() {
        let mut ts = TurboState::default();
        let payload = Bytes::from("test");

        deliver_n_fwd_msgs(&mut ts, &payload, 10);

        // Ack all 10 packets even with no payload present.
        assert_ack(&mut ts, 0, 10);
    }

    #[test]
    fn local_shuts_first() {
        let mut ts = TurboState::default();
        let mut cx = Context::from_waker(Waker::noop());
        let payload = Bytes::from("test");

        deliver_n_fwd_msgs(&mut ts, &payload, 10);
        assert_ack(&mut ts, 0, 10);
        assert_eq!(ts.state, State::LocalOpenRemoteOpen);

        // Signal EOF should produce a shut message.
        let res = ts.poll_recv_with_payload(&mut cx, &mut Poll::Ready(None));
        assert_ready_eq!(res, Some(TurboMessage::shut(1, 10)));
        assert_eq!(ts.state, State::LocalShuttingRemoteOpen);
        assert!(!ts.is_payload_next());

        // Local is shut after getting the response.
        let msg = TurboMessage::shut_ok(10, 2);
        assert_eq!(ts.send(&msg), None);
        assert_eq!(ts.state, State::LocalShutRemoteOpen);

        // Remote wants to shut.
        let msg = TurboMessage::shut(11, 2);
        assert_eq!(ts.send(&msg), None);
        assert_eq!(ts.state, State::LocalShutRemoteShut);

        // Should return the final shut ack response (last ack).
        assert!(!ts.is_payload_next());
        let res = ts.poll_recv(&mut cx);
        assert_ready_eq!(res, Some(TurboMessage::shut_ok(2, 12)));
        assert_eq!(ts.state, State::LocalShutRemoteShut);

        // Stream is done.
        assert_eq!(ts.poll_recv(&mut cx), Poll::Ready(None));
    }

    #[test]
    fn remote_shuts_first() {
        let mut ts = TurboState::default();
        let mut cx = Context::from_waker(Waker::noop());
        let payload = Bytes::from("test");

        deliver_n_fwd_msgs(&mut ts, &payload, 10);
        assert_ack(&mut ts, 0, 10);
        assert_eq!(ts.state, State::LocalOpenRemoteOpen);

        // Remote wants to shut.
        let msg = TurboMessage::shut(10, 1);
        assert_eq!(ts.send(&msg), None);
        assert_eq!(ts.state, State::LocalOpenRemoteShut);

        // Should return a shut ack response next.
        assert!(!ts.is_payload_next());
        let res = ts.poll_recv(&mut cx);
        assert_ready_eq!(res, Some(TurboMessage::shut_ok(1, 11)));
        assert_eq!(ts.state, State::LocalOpenRemoteShut);

        // Signal EOF should produce a shut message.
        assert!(ts.is_payload_next());
        let res = ts.poll_recv_with_payload(&mut cx, &mut Poll::Ready(None));
        assert_ready_eq!(res, Some(TurboMessage::shut(2, 11)));
        assert_eq!(ts.state, State::LocalShuttingRemoteShut);
        assert!(!ts.is_payload_next());

        // Local is shut after getting the response.
        let msg = TurboMessage::shut_ok(11, 3);
        assert_eq!(ts.send(&msg), None);
        assert_eq!(ts.state, State::LocalShutRemoteShut);

        // Stream is done.
        assert_eq!(ts.poll_recv(&mut cx), Poll::Ready(None));
    }

    #[test]
    fn simultaneous_shut() {
        let mut ts = TurboState::default();
        let mut cx = Context::from_waker(Waker::noop());
        let payload = Bytes::from("test");

        deliver_n_fwd_msgs(&mut ts, &payload, 10);
        assert_ack(&mut ts, 0, 10);
        assert_eq!(ts.state, State::LocalOpenRemoteOpen);

        // Signal EOF should produce a shut message.
        let res = ts.poll_recv_with_payload(&mut cx, &mut Poll::Ready(None));
        assert_ready_eq!(res, Some(TurboMessage::shut(1, 10)));
        assert_eq!(ts.state, State::LocalShuttingRemoteOpen);
        assert!(!ts.is_payload_next());

        // Remote wants to shut.
        let msg = TurboMessage::shut(10, 2);
        assert_eq!(ts.send(&msg), None);
        assert_eq!(ts.state, State::LocalShuttingRemoteShut);

        // Should return a shut ack response.
        assert!(!ts.is_payload_next());
        let res = ts.poll_recv(&mut cx);
        assert_ready_eq!(res, Some(TurboMessage::shut_ok(2, 11)));
        assert_eq!(ts.state, State::LocalShuttingRemoteShut);

        // Local is shut after getting the last ack.
        let msg = TurboMessage::shut_ok(11, 3);
        assert_eq!(ts.send(&msg), None);
        assert_eq!(ts.state, State::LocalShutRemoteShut);

        // Stream is done.
        assert_eq!(ts.poll_recv(&mut cx), Poll::Ready(None));
    }
}
