use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use crate::net::proto::turbo::message::{Command, DataCursor, Payload, TurboMessage};
use bytes::Bytes;
use tokio::time::Instant;

fn shut_deadline() -> Instant {
    Instant::now() + Duration::from_millis(300)
}

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
    shut_deadline: Option<Instant>,
    /// A queue for outgoing message operations.
    send_q: VecDeque<Command>,
    /// Min-heap holding messages we already sent but have not yet been acknowledged.
    retransmit_q: BinaryHeap<Reverse<TurboMessage>>,
}

#[derive(Default, Debug, Copy, Clone, PartialEq)]
enum State {
    #[default]
    LocalOpenRemoteOpen,
    LocalOpenRemoteRewinding(DataCursor),
    LocalOpenRemoteShut,
    LocalShuttingRemoteOpen,
    LocalShuttingRemoteRewinding(DataCursor),
    LocalShuttingRemoteShut,
    LocalShutRemoteOpen,
    LocalShutRemoteRewinding(DataCursor),
    LocalShutRemoteShut,
}

impl TurboState {
    /// Returns `true` if we want to add payload to the next outgoing message,
    /// `false` otherwise. This informs the caller to read from the source
    /// prior to calling `poll_next()` to provide any available bytes.
    pub fn is_payload_next(&self) -> bool {
        if self.send_q.is_empty() {
            match self.state {
                State::LocalOpenRemoteOpen => true,
                State::LocalOpenRemoteRewinding(_) => true,
                State::LocalOpenRemoteShut => true,
                _ => false,
            }
        } else {
            false
        }
    }

    pub fn poll_recv(&mut self, cx: &mut Context) -> Poll<Option<TurboMessage>> {
        if let Some(command) = self.send_q.pop_front() {
            let msg = TurboMessage::new(self.write_inc(), self.read, command);
            self.retransmit_q.push(Reverse(msg.clone()));
            self.next_ack = false;
            Poll::Ready(Some(msg))
        } else if self.state == State::LocalShutRemoteShut {
            Poll::Ready(None)
        } else if self.next_ack {
            let ack = TurboMessage::forward_ack(self.write_inc(), self.read);
            self.retransmit_q.push(Reverse(ack.clone()));
            self.next_ack = false;
            Poll::Ready(Some(ack))
        } else {
            // Handling the case where the last ack is lost.
            if let State::LocalShuttingRemoteShut = self.state
                && let Some(deadline) = self.shut_deadline
            {
                let now = Instant::now();
                if now < deadline {
                    let waker = cx.waker().clone();
                    tokio::spawn(async move {
                        tokio::time::sleep_until(deadline).await;
                        waker.wake();
                    });
                } else {
                    self.shut_deadline = Some(shut_deadline());
                    self.send_q.push_back(Command::Shut);
                    // self.close(); // to send Reset
                    return self.poll_recv(cx);
                }
            }

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
                Some(bytes) => self.send_q.push_back(Command::Forward(bytes.into())),
                // Reader got Error/EOF, so we are done sending payloads now.
                None => self.handle_eof_signal(),
            },
            // Already handled in poll_recv().
            Poll::Pending => {}
        };

        // Construct and return any ready messages, or handle Pending logic.
        self.poll_recv(cx)
    }

    pub fn send(&mut self, msg: &TurboMessage) -> Option<Bytes> {
        // We always utilize the ack info since they monotonically increase.
        self.process_ack(msg.read);

        // We only process messages in sequence order.
        // Return any payload bytes that should be written next.
        if msg.write < self.read {
            log::debug!("Ignoring duplicate message {msg:?}");
        } else if msg.write == self.read {
            log::debug!("Processing message {msg:?}");

            // Advance our read cursor.
            self.read += 1;

            match &msg.command {
                Command::Forward(payload) => {
                    if let Some(bytes) = self.process_fwd(payload) {
                        return Some(bytes);
                    }
                }
                Command::ForwardAck => {} // Handled in `self.process_ack()`.
                Command::Rewind => self.process_rwd(),
                Command::RewindAck => self.process_rwd_ack(),
                Command::Shut => self.process_shut(msg.write),
                Command::ShutAck => self.process_shut_ack(),
                Command::Reset => self.process_reset(),
            }
        } else if msg.write > self.read {
            if let Command::Rewind = &msg.command {
                // This queues: RewindAck -> Rewind -> Retransmissions...
                self.process_rwd();
                self.handle_lost_message_signal(1, msg.write);
            } else {
                // This queues a Rewind in the front.
                self.handle_lost_message_signal(0, msg.write);
            }
        }

        // Wake the stream side if messages are ready.
        if !self.send_q.is_empty() {
            self.wake();
        }

        None
    }

    pub fn close(&mut self) {
        // The sink will not call send() anymore, so we will never get another message.
        // We require bidirectional communication for acks and a graceful shutdown.
        if self.state != State::LocalShutRemoteShut {
            self.write_end.get_or_insert(self.write);
            self.read_end.get_or_insert(self.read);
            self.send_q.push_back(Command::Reset);
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
        log::trace!("Setting state {:?} -> {:?}", self.state, new_state);
        self.state = new_state;
        // Wake stream if we need to return Ready(None).
        if self.state == State::LocalShutRemoteShut {
            self.wake();
        } else if self.state == State::LocalShuttingRemoteShut {
            self.shut_deadline = Some(shut_deadline());
        }
    }

    fn write_inc(&mut self) -> DataCursor {
        let val = self.write;
        self.write += 1;
        val
    }

    fn handle_eof_signal(&mut self) {
        // Initiate a graceful shutdown to indicate we are done writing.
        let new_state = match self.state {
            // Should match `is_payload_next()`.
            State::LocalOpenRemoteOpen => State::LocalShuttingRemoteOpen,
            State::LocalOpenRemoteRewinding(c) => State::LocalShuttingRemoteRewinding(c),
            State::LocalOpenRemoteShut => State::LocalShuttingRemoteShut,
            _ => unreachable!(),
        };
        self.set_state(new_state);
        self.write_end.get_or_insert(self.write);
        self.send_q.push_back(Command::Shut);
    }

    fn handle_lost_message_signal(&mut self, index: usize, new: DataCursor) {
        // Initiate the remote to rewind it's write cursor.
        // Avoid sending multiple rewind messages for the same lost message.
        // But do send another rewind if we detect a different lost message.
        let (new_state, do_rewind) = match self.state {
            State::LocalOpenRemoteOpen => (State::LocalOpenRemoteRewinding(new), true),
            State::LocalShuttingRemoteOpen => (State::LocalShuttingRemoteRewinding(new), true),
            State::LocalShutRemoteOpen => (State::LocalShutRemoteRewinding(new), true),
            State::LocalOpenRemoteRewinding(c) => {
                (State::LocalOpenRemoteRewinding(c.max(new)), new <= c)
            }
            State::LocalShuttingRemoteRewinding(c) => {
                (State::LocalShuttingRemoteRewinding(c.max(new)), new <= c)
            }
            State::LocalShutRemoteRewinding(c) => {
                (State::LocalShutRemoteRewinding(c.max(new)), new <= c)
            }
            State::LocalOpenRemoteShut => return,
            State::LocalShuttingRemoteShut => return,
            State::LocalShutRemoteShut => return,
        };

        self.set_state(new_state);
        if do_rewind {
            self.send_q.insert(index, Command::Rewind);
        }
    }

    fn process_ack(&mut self, ack: DataCursor) {
        // Ack sequence numbers are monotonically increasing.
        self.write_ack = self.write_ack.max(ack);

        // Clear retransmit queue of any messages already acked.
        while let Some(Reverse(msg)) = self.retransmit_q.peek() {
            if msg.write < self.write_ack {
                self.retransmit_q.pop();
            } else {
                break;
            }
        }
    }

    fn process_fwd(&mut self, payload: &Payload) -> Option<Bytes> {
        // Valid unless the remote side is already shut.
        let is_valid = match self.state {
            State::LocalOpenRemoteOpen => true,
            State::LocalShuttingRemoteOpen => true,
            State::LocalShutRemoteOpen => true,
            State::LocalOpenRemoteRewinding(_) => true,
            State::LocalShuttingRemoteRewinding(_) => true,
            State::LocalShutRemoteRewinding(_) => true,
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

    fn process_rwd(&mut self) {
        if let Some(Reverse(msg)) = self.retransmit_q.peek()
            && msg.write == self.write_ack
        {
            // Rewind our write cursor.
            self.write = self.write_ack;

            // Restart the stream at the new cursor, keeping messages in order
            // of the lowest original sequence number first.
            let mut sorted = Vec::new();
            while let Some(Reverse(msg)) = self.retransmit_q.pop() {
                sorted.push(msg.command);
            }

            // Reverse the order so the lowest original sequence number is last.
            // The original ordering will be preserved with `push_front` below.
            sorted.reverse();

            for command in sorted.into_iter() {
                // XXX: Do we avoid retransmitting rewind commands if we are
                // no longer in a rewinding state? Maybe we already recovered?
                self.send_q.push_front(command);
            }

            // The very next message is the Ack, followed by the retransmissions.
            self.send_q.push_front(Command::RewindAck);
        } else {
            // Error, reset and close.
            log::warn!("Cannot complete rewind request, closing now");
            self.send_q.clear();
            self.close();
        }
    }

    fn process_rwd_ack(&mut self) {
        let new_state = match self.state {
            State::LocalOpenRemoteRewinding(_) => State::LocalOpenRemoteOpen,
            State::LocalShuttingRemoteRewinding(_) => State::LocalShuttingRemoteOpen,
            State::LocalShutRemoteRewinding(_) => State::LocalShutRemoteOpen,
            _ => return,
        };
        self.set_state(new_state);
    }

    fn process_shut(&mut self, cursor: DataCursor) {
        // Valid if the remote side is still open.
        let shut_state = match self.state {
            State::LocalOpenRemoteOpen => State::LocalOpenRemoteShut,
            State::LocalOpenRemoteRewinding(_) => State::LocalOpenRemoteShut,
            State::LocalShuttingRemoteOpen => State::LocalShuttingRemoteShut,
            State::LocalShuttingRemoteRewinding(_) => State::LocalShuttingRemoteShut,
            State::LocalShutRemoteOpen => State::LocalShutRemoteShut,
            State::LocalShutRemoteRewinding(_) => State::LocalShutRemoteShut,
            _ => return,
        };

        // Store the final position of the read cursor.
        self.read_end.get_or_insert(cursor);

        // Our sequential protocol means we must have read everything before cursor.
        assert!(self.read >= cursor);

        // The remote moves from open to shut.
        self.set_state(shut_state);
        self.send_q.push_back(Command::ShutAck);
    }

    fn process_shut_ack(&mut self) {
        // Valid only if the local side is shutting.
        let shut_state = match self.state {
            State::LocalShuttingRemoteOpen => State::LocalShutRemoteOpen,
            State::LocalShuttingRemoteRewinding(c) => State::LocalShutRemoteRewinding(c),
            State::LocalShuttingRemoteShut => State::LocalShutRemoteShut,
            _ => return,
        };

        let done_acking_writes = self
            .write_end
            .map(|end| self.write_ack >= end)
            .unwrap_or(false);

        if done_acking_writes {
            self.set_state(shut_state);
        } else {
            log::warn!("Got Shut response with incomplete ack, closing now");
            self.send_q.clear();
            self.close();
        }
    }

    fn process_reset(&mut self) {
        if self.state == State::LocalShutRemoteShut {
            return;
        }

        // Overwrite any previously set end cursors.
        self.write_end = Some(self.write);
        self.read_end = Some(self.read);
        self.send_q.clear();
        // Will wake stream so we can return Ready(None).
        self.set_state(State::LocalShutRemoteShut);
    }
}

#[cfg(test)]
mod tests {
    use std::future::PollFn;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Waker};
    use std::time::Duration;

    use bytes::Bytes;
    use tokio_test::assert_ready_eq;
    use tokio_test::task::Spawn;

    use crate::common::mock;
    use crate::net::proto::turbo::message::{Command, DataCursor, TurboMessage};
    use crate::net::proto::turbo::state::{State, TurboState};

    struct Node<'a> {
        turbo: TurboState,
        cx: Context<'a>,
    }

    impl<'a> Node<'a> {
        fn new() -> Self {
            Self {
                turbo: TurboState::default(),
                cx: Context::from_waker(Waker::noop()),
            }
        }

        fn initialized(n_puts: DataCursor, n_gets: DataCursor) -> Self {
            let mut node = Self::new();
            node.state(State::LocalOpenRemoteOpen);
            for cursor in 0..n_puts {
                node.put_fwd_some(cursor, 0);
            }
            for cursor in 0..n_gets {
                node.get_payload_ready_fwd(cursor, n_puts);
            }
            node.state(State::LocalOpenRemoteOpen);
            node
        }

        fn put_fwd_some(&mut self, w: DataCursor, r: DataCursor) {
            let payload = self.payload();
            let msg = TurboMessage::forward(w, r, payload.clone());
            assert_eq!(self.turbo.send(&msg), Some(payload.clone()));
        }

        fn put_fwd_none(&mut self, w: DataCursor, r: DataCursor) {
            let payload = self.payload();
            let msg = TurboMessage::forward(w, r, payload.clone());
            assert_eq!(self.turbo.send(&msg), None);
        }

        fn put_fwd_ack(&mut self, w: DataCursor, r: DataCursor) {
            let msg = TurboMessage::forward_ack(w, r);
            assert_eq!(self.turbo.send(&msg), None);
        }

        fn put_rwd(&mut self, w: DataCursor, r: DataCursor) {
            let msg = TurboMessage::rewind(w, r);
            assert_eq!(self.turbo.send(&msg), None);
        }

        fn put_rwd_ack(&mut self, w: DataCursor, r: DataCursor) {
            let msg = TurboMessage::rewind_ack(w, r);
            assert_eq!(self.turbo.send(&msg), None);
        }

        fn put_shut(&mut self, w: DataCursor, r: DataCursor) {
            let msg = TurboMessage::shut(w, r);
            assert_eq!(self.turbo.send(&msg), None);
        }

        fn put_shut_ack(&mut self, w: DataCursor, r: DataCursor) {
            let msg = TurboMessage::shut_ack(w, r);
            assert_eq!(self.turbo.send(&msg), None);
        }

        fn get_payload(&mut self, mut poll: Poll<Option<Bytes>>) -> Poll<Option<TurboMessage>> {
            assert!(self.turbo.is_payload_next());
            self.turbo.poll_recv_with_payload(&mut self.cx, &mut poll)
        }

        fn get_payload_ready_fwd(&mut self, w: DataCursor, r: DataCursor) {
            let payload = self.payload();
            assert_ready_eq!(
                self.get_payload(Poll::Ready(Some(payload.clone()))),
                Some(TurboMessage::forward(w, r, payload))
            );
        }

        fn get_payload_pending_fwd_ack(&mut self, w: DataCursor, r: DataCursor) {
            assert_ready_eq!(
                self.get_payload(Poll::Pending),
                Some(TurboMessage::forward_ack(w, r))
            );
        }

        fn get_payload_eof_shut(&mut self, w: DataCursor, r: DataCursor) {
            assert_ready_eq!(
                self.get_payload(Poll::Ready(None)),
                Some(TurboMessage::shut(w, r))
            );
        }

        fn get_fwd(&mut self, w: DataCursor, r: DataCursor) {
            assert!(matches!(
                self.turbo.poll_recv(&mut self.cx),
                Poll::Ready(Some(TurboMessage {
                    write: w,
                    read: r,
                    command: Command::Forward(_)
                }))
            ));
        }

        fn get_rwd(&mut self, w: DataCursor, r: DataCursor) {
            assert_ready_eq!(
                self.turbo.poll_recv(&mut self.cx),
                Some(TurboMessage::rewind(w, r))
            );
        }

        fn get_rwd_ack(&mut self, w: DataCursor, r: DataCursor) {
            assert_ready_eq!(
                self.turbo.poll_recv(&mut self.cx),
                Some(TurboMessage::rewind_ack(w, r))
            );
        }

        fn get_shut(&mut self, w: DataCursor, r: DataCursor) {
            assert_ready_eq!(
                self.turbo.poll_recv(&mut self.cx),
                Some(TurboMessage::shut(w, r))
            );
        }

        fn get_shut_ack(&mut self, w: DataCursor, r: DataCursor) {
            assert_ready_eq!(
                self.turbo.poll_recv(&mut self.cx),
                Some(TurboMessage::shut_ack(w, r))
            );
        }

        fn get_eof(&mut self) {
            assert_eq!(self.turbo.poll_recv(&mut self.cx), Poll::Ready(None));
        }

        fn payload(&self) -> Bytes {
            mock::payload(32)
        }

        fn state(&mut self, expected: State) {
            assert_eq!(self.turbo.state, expected);
        }
    }

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

    #[test]
    fn forward_response_payload() {
        let mut node = Node::new();
        for w_cursor in 0..10 {
            node.put_fwd_some(w_cursor, 0);
        }
        // Ack all 10 packets while including payload of our own.
        node.get_payload_ready_fwd(0, 10);
    }

    #[test]
    fn forward_response_ack() {
        let mut node = Node::new();
        for w_cursor in 0..10 {
            node.put_fwd_some(w_cursor, 0);
        }
        // Ack all 10 packets even with no payload present.
        node.get_payload_pending_fwd_ack(0, 10);
    }

    #[test]
    fn node_quick_init() {
        // Test to make sure the test node helper doesn't fail.
        Node::initialized(10, 10);
    }

    #[test]
    fn local_shuts_first() {
        let mut node = Node::initialized(10, 5);

        // Signal EOF should produce a shut message.
        node.get_payload_eof_shut(5, 10);
        node.state(State::LocalShuttingRemoteOpen);
        assert!(!node.turbo.is_payload_next());

        // Local is shut after getting the response.
        node.put_shut_ack(10, 6);
        node.state(State::LocalShutRemoteOpen);

        // Remote wants to shut, should respond with a shut ack.
        node.put_shut(11, 6);
        node.get_shut_ack(6, 12);
        node.state(State::LocalShutRemoteShut);

        // Stream is done.
        node.get_eof();
    }

    #[test]
    fn remote_shuts_first() {
        let mut node = Node::initialized(10, 5);

        // Remote wants to shut, should respond with a shut ack.
        node.put_shut(10, 5);
        node.get_shut_ack(5, 11);
        node.state(State::LocalOpenRemoteShut);

        // Signal EOF should produce a shut message.
        node.get_payload_eof_shut(6, 11);
        node.state(State::LocalShuttingRemoteShut);
        assert!(!node.turbo.is_payload_next());

        // Local is shut after getting the response.
        node.put_shut_ack(11, 7);
        node.state(State::LocalShutRemoteShut);

        // Stream is done.
        node.get_eof();
    }

    #[test]
    fn simultaneous_shut() {
        let mut node = Node::initialized(10, 5);

        // Signal EOF should produce a shut message.
        node.get_payload_eof_shut(5, 10);
        node.state(State::LocalShuttingRemoteOpen);
        assert!(!node.turbo.is_payload_next());

        // Remote wants to shut, should respond with a shut ack.
        node.put_shut(10, 6);
        node.get_shut_ack(6, 11);
        node.state(State::LocalShuttingRemoteShut);

        // Local is shut after getting the response.
        node.put_shut_ack(11, 7);
        node.state(State::LocalShutRemoteShut);

        // Stream is done.
        node.get_eof();
    }

    #[test]
    fn local_rewind_first() {
        let mut node = Node::initialized(10, 5);

        // Simulate a lost message. Payload is None because it's out of order.
        node.put_fwd_none(11, 5);

        // We should get a rewind expecting 10 next.
        node.get_rwd(5, 10);
        node.state(State::LocalOpenRemoteRewinding(11));

        // Deliver the rewind ack and the retransmissions.
        node.put_rwd_ack(10, 6);
        node.put_fwd_some(11, 6);
        node.put_fwd_some(12, 6);

        // Should ack the payloads and be back to open now.
        node.get_payload_pending_fwd_ack(6, 13);
        node.state(State::LocalOpenRemoteOpen);
    }

    #[test]
    fn remote_rewind_first() {
        let mut node = Node::initialized(10, 5);

        // Simulate the remote claiming to have lost a message.
        node.put_rwd(10, 3);

        // Should rewind the cursor.
        node.get_rwd_ack(3, 11);
        node.get_fwd(4, 11);
        node.get_fwd(5, 11);
        node.put_fwd_ack(11, 6);
        assert!(node.turbo.is_payload_next());
        node.state(State::LocalOpenRemoteOpen);
    }

    #[test]
    fn simultaneous_rewind() {
        let mut node = Node::initialized(10, 5);

        // Simulate the remote claiming to have lost a message.
        // And this message is out of order for the local.
        node.put_rwd(11, 3);

        // Should get a rewind ack, a rewind, and retransmissions.
        node.get_rwd_ack(3, 10);
        node.get_rwd(4, 10);
        node.get_fwd(5, 10);
        node.get_fwd(6, 10);

        node.put_rwd_ack(10, 7);

        assert!(node.turbo.is_payload_next());
        node.state(State::LocalOpenRemoteOpen);
    }

    #[test]
    fn local_rewind_request_dropped() {
        let mut node = Node::initialized(10, 5);

        // Simulate a lost message. Payload is None because it's out of order.
        node.put_fwd_none(11, 4);

        // We should get a rewind expecting 10 next.
        node.get_rwd(5, 10); // This is 'dropped'
        node.state(State::LocalOpenRemoteRewinding(11));

        // Local side keeps sending payload.
        node.get_payload_ready_fwd(6, 10);

        // Remote side detects gap and wants to rewind to 4.
        // Remote side still thinks it sent 11 last.
        node.put_rwd(12, 4);

        // Local side acks...
        node.get_rwd_ack(4, 10);
        // ...and retransmits.
        node.get_fwd(5, 10); // Orig 4 (payload)
        node.get_rwd(6, 10); // Orig 5 ('dropped' rewind)
        node.get_fwd(7, 10); // Orig 6 (payload)

        node.put_rwd_ack(10, 8);
        assert!(node.turbo.is_payload_next());
        node.state(State::LocalOpenRemoteOpen);
    }

    #[test]
    fn remote_rewind_response_dropped() {
        let mut node = Node::initialized(10, 5);

        // Simulate a lost message. Payload is None because it's out of order.
        node.put_fwd_none(11, 4);

        // We should get a rewind expecting 10 next.
        node.get_rwd(5, 10);
        node.state(State::LocalOpenRemoteRewinding(11));

        // Local side keeps sending payload.
        node.get_payload_ready_fwd(6, 10);

        // Remote side would ack and retransmit.
        // node.put_rwd_ack(10, 7); // This is 'dropped'
        // Payload is None because it's (still) out of order.
        node.put_fwd_none(11, 7);

        // We are (still) expecting 10 next.
        node.get_rwd(7, 10);

        // Succeeds this time.
        node.put_rwd_ack(10, 8);
        node.put_fwd_some(11, 8);
        node.put_fwd_some(12, 8);

        assert!(node.turbo.is_payload_next());
        node.state(State::LocalOpenRemoteOpen);
    }

    #[test]
    fn remote_rewind_request_dropped() {
        let mut node = Node::initialized(10, 5);

        // Simulate the remote claiming to have lost a message.
        // node.put_rwd(10, 3); // 'dropped'

        // Remote keeps sending. Payload is None because it's out of order.
        node.put_fwd_none(11, 3);

        // We should get a rewind expecting 10 next.
        node.get_rwd(5, 10);
        node.state(State::LocalOpenRemoteRewinding(11));

        // Local side keeps sending payload.
        node.get_payload_ready_fwd(6, 10);

        // Remote side acks...
        node.put_rwd_ack(10, 3);
        // ...and retransmits.
        node.put_rwd(11, 3); // Orig 10 ('dropped' rewind)

        // Local now realizes it needs to rewind too.
        node.get_rwd_ack(3, 12);
        node.get_fwd(4, 12); // Orig 3 (payload)
        node.get_fwd(5, 12); // Orig 4 (payload)
        node.get_rwd(6, 12); // Orig 5 (rewind)
        node.get_fwd(7, 12); // Orig 6 (payload)

        node.put_fwd_some(12, 3); // Orig 11 (payload)

        assert!(node.turbo.is_payload_next());
        node.state(State::LocalOpenRemoteOpen);
    }

    #[test]
    fn shut_request_dropped() {
        let mut node = Node::initialized(10, 5);

        // Signal EOF should produce a shut message.
        node.get_payload_eof_shut(5, 10);
        node.state(State::LocalShuttingRemoteOpen);
        assert!(!node.turbo.is_payload_next());

        // The local shut was 'dropped', but now remote wants to shut.
        node.put_shut(10, 5);

        node.get_shut_ack(6, 11); // Remote will drop
        node.put_rwd(11, 5);

        node.get_rwd_ack(5, 12);
        node.get_shut(6, 12); // Orig 5
        node.get_shut_ack(7, 12); // Orig 6

        node.put_shut_ack(12, 8);

        node.state(State::LocalShutRemoteShut);

        // Stream is done.
        node.get_eof();
    }

    #[test]
    fn shut_response_dropped() {
        let mut node = Node::initialized(10, 5);

        // Signal EOF should produce a shut message.
        node.get_payload_eof_shut(5, 10);
        node.state(State::LocalShuttingRemoteOpen);
        assert!(!node.turbo.is_payload_next());

        // node.put_shut_ack(10, 6); // 'dropped'
        node.put_shut(11, 6); // Local ignores

        node.get_rwd(6, 10);

        node.put_rwd_ack(10, 7);
        node.put_shut_ack(11, 7);
        node.put_shut(12, 7);

        node.get_shut_ack(7, 13);

        // Stream is done.
        node.state(State::LocalShutRemoteShut);
        node.get_eof();
    }

    #[tokio::test]
    async fn shut_last_ack_dropped() {
        let mut node = Node::initialized(10, 5);

        node.put_shut(10, 5);
        node.get_shut_ack(5, 11);
        node.state(State::LocalOpenRemoteShut);

        node.get_payload_eof_shut(6, 11);
        node.state(State::LocalShuttingRemoteShut);

        // remote shut_ack dropped.

        assert_eq!(node.turbo.poll_recv(&mut node.cx), Poll::Pending);
        node.state(State::LocalShuttingRemoteShut);

        match node.turbo.shut_deadline {
            Some(deadline) => {
                tokio::time::sleep_until(deadline + Duration::from_millis(1)).await;
            }
            None => unreachable!(),
        }

        node.get_shut(7, 11);
        node.put_shut_ack(11, 8);
        node.state(State::LocalShutRemoteShut);
        node.get_eof();
    }
}
