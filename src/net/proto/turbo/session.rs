use std::io::Cursor;
use std::ops::Range;
use std::sync::{Arc, Mutex};

use anyhow::bail;
use async_trait::async_trait;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use log::warn;
use tokio::sync::Notify;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::time::Duration;

use crate::net::proto::turbo::formatter::Formatter;
use crate::net::proto::turbo::frames::{Command, DataCursor, Message, Payload};
use crate::net::{Deserializer, READ_CAPACITY, Reader, Serialize, Serializer, Writer};

#[derive(Default, Debug, Copy, Clone, PartialEq)]
enum ProtocolState {
    #[default]
    Start,
    Resuming,
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

/// The state needed to implement our turbo session protocol that needs to be
/// shared across the session reader and writer forwarding directions.
#[derive(Default)]
struct TurboState {
    id: Option<u64>,
    proto_state: ProtocolState,
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
    retransmit: Vec<Payload>,
}

pub struct SharedTurboState {
    inner: Arc<Mutex<TurboState>>,
    id_notify: Option<Arc<Notify>>,
    state_notify: Arc<Notify>,
}

impl SharedTurboState {
    pub fn new(id: Option<u64>) -> Self {
        let mut state = TurboState::default();
        state.id = id;

        Self {
            inner: Arc::new(Mutex::new(state)),
            id_notify: id.map_or_else(|| Some(Arc::new(Notify::new())), |_| None),
            state_notify: Arc::new(Notify::new()),
        }
    }

    fn has_id(&self) -> bool {
        self.inner.lock().unwrap().id.is_some()
    }

    async fn wait_id(&mut self) -> u64 {
        if let Some(notify) = self.id_notify.take() {
            log::info!("Waiting for the session id to be set to run the session...");
            notify.notified().await;
        }
        self.id()
    }

    fn notify_id(&mut self, value: u64) {
        if let Some(notify) = self.id_notify.take() {
            self.set_id(value);
            log::info!("Setting the session id so we can run the session");
            notify.notify_one();
        }
    }

    fn id(&self) -> u64 {
        self.inner.lock().unwrap().id.unwrap()
    }

    fn set_id(&self, value: u64) {
        self.inner.lock().unwrap().id = Some(value);
    }

    fn id_equal(&self, other: u64) -> bool {
        self.inner
            .lock()
            .unwrap()
            .id
            .map_or(false, |id| id == other)
    }

    fn protocol_state(&self) -> ProtocolState {
        self.inner.lock().unwrap().proto_state
    }

    fn set_protocol_state(&self, proto_state: ProtocolState) {
        log::debug!("Setting protocol state to {proto_state:?}");
        {
            self.inner.lock().unwrap().proto_state = proto_state;
        }
        self.notify_protocol_state();
    }

    async fn wait_protocol_state(&self) {
        self.state_notify.notified().await
    }

    fn notify_protocol_state(&self) {
        self.state_notify.notify_one();
    }

    fn write(&self) -> DataCursor {
        self.inner.lock().unwrap().write
    }

    fn set_write_unacked(&self, value: DataCursor) {
        let mut state = self.inner.lock().unwrap();
        if value < state.write_ack {
            state.write = state.write_ack;
        } else if value < state.write {
            state.write = value;
        }
    }

    fn read(&self) -> DataCursor {
        self.inner.lock().unwrap().read
    }

    fn increment_read(&self) {
        let mut state = self.inner.lock().unwrap();
        state.read += 1;
    }

    fn set_read_end(&self, end: DataCursor) {
        self.inner.lock().unwrap().read_end = Some(end);
    }

    fn is_read_complete(&self) -> bool {
        let state = self.inner.lock().unwrap();
        state.read_end.map(|end| state.read >= end).unwrap_or(false)
    }

    fn set_ack_max(&self, write_ack: DataCursor) {
        let mut state = self.inner.lock().unwrap();
        state.write_ack = state.write_ack.max(write_ack);
    }

    fn is_ack_complete(&self) -> bool {
        let state = self.inner.lock().unwrap();
        state
            .write_end
            .map(|end| state.write_ack >= end)
            .unwrap_or(false)
    }

    fn build_resume_message(&self) -> Message {
        let (session_id, write, read) = {
            let mut state = self.inner.lock().unwrap();
            state.write += 1;
            (state.id.unwrap(), state.write - 1, state.read)
        };

        Message {
            session_id,
            write,
            read,
            command: Command::Resume,
        }
    }

    fn build_resume_ok_message(&self) -> Message {
        let (session_id, write, read) = {
            let mut state = self.inner.lock().unwrap();
            state.write += 1;
            (state.id.unwrap(), state.write - 1, state.read)
        };

        Message {
            session_id,
            write,
            read,
            command: Command::ResumeOk,
        }
    }

    fn build_forward_message(&self, data: Bytes) -> Message {
        let len = data.len() as u64;

        let (session_id, read, write, ack) = {
            let mut state = self.inner.lock().unwrap();
            state.write += 1;
            (
                state.id.unwrap(),
                state.read,
                state.write - 1,
                state.write_ack,
            )
        };

        log::debug!(
            "Session {session_id} sending {len} bytes; read {read} write {write} ack {ack}",
        );

        Message {
            session_id,
            write,
            read,
            command: Command::Forward(data),
        }
    }

    fn build_forward_ok_message(&self) -> Message {
        let (session_id, write, read) = {
            let mut state = self.inner.lock().unwrap();
            state.write += 1;
            (state.id.unwrap(), state.write - 1, state.read)
        };

        Message {
            session_id,
            write,
            read,
            command: Command::ForwardOk,
        }
    }

    fn build_shut_message(&self) -> Message {
        let (session_id, write, read) = {
            let mut state = self.inner.lock().unwrap();
            if state.write_end.is_none() {
                state.write_end = Some(state.write);
            }
            state.write += 1;
            (state.id.unwrap(), state.write - 1, state.read)
        };

        Message {
            session_id,
            write,
            read,
            command: Command::Shut,
        }
    }

    fn build_shut_ok_message(&self) -> Message {
        assert!(self.is_read_complete());
        let (session_id, write, read) = {
            let mut state = self.inner.lock().unwrap();
            state.write += 1;
            (state.id.unwrap(), state.write - 1, state.read)
        };

        Message {
            session_id,
            write,
            read,
            command: Command::ShutOk,
        }
    }
}

impl Clone for SharedTurboState {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            id_notify: self.id_notify.clone(),
            state_notify: self.state_notify.clone(),
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
enum NextMessageError {
    ChannelClosed,
    ProtocolStateChanged,
    ProtocolShut,
    Timeout,
}

/// A wrapper around the application data source, responsible for returning app
/// bytes to the interpreter. This wraps app data inside of our turbo protocol
/// to enable support for session resumption.
pub struct TurboReader<R: Reader + Send> {
    src: R,
    buffer: BytesMut,
    state: SharedTurboState,
    receiver: UnboundedReceiver<Message>,
}

/// A wrapper around the application data sink, responsible for writing app
/// bytes from the interpreter into the application data sink. This unwraps app
/// data from our turbo protocol frames and then writes it to the app, to enable
/// support for session resumption.
pub struct TurboWriter<W: Writer + Send> {
    dst: W,
    buffer: BytesMut,
    state: SharedTurboState,
    sender: UnboundedSender<Message>,
}

impl<R: Reader + Send> TurboReader<R> {
    pub fn new(src: R, state: SharedTurboState, receiver: UnboundedReceiver<Message>) -> Self {
        let mut reader = Self {
            src,
            buffer: BytesMut::new(),
            state,
            receiver,
        };
        if reader.state.has_id() {
            reader.put_message(reader.state.build_resume_message());
            reader.state.set_protocol_state(ProtocolState::Resuming);
        }
        reader
    }

    fn put_message(&mut self, message: Message) {
        self.buffer.put_slice(&message.serialize());
        log::trace!("Buffered for sending: {message:?}");
    }

    fn get_bytes(&mut self, range: &Range<usize>) -> Option<Bytes> {
        if self.buffer.len() >= range.start {
            let len = self.buffer.len().min(range.end);
            let bytes = self.buffer.split_to(len).freeze();
            log::trace!("Removed {len} message bytes from read buffer");
            Some(bytes)
        } else {
            None
        }
    }

    async fn serialize_messages(&mut self, range: Range<usize>) -> anyhow::Result<Bytes> {
        // We do not start making messages until we have set a session id.
        // XXX: This will deadlock if the server must send first and requires a non-zero payload.
        let _ = self.state.wait_id().await;

        loop {
            while let Some(message) = self.try_next_message() {
                self.put_message(message);
            }

            if let Some(bytes) = self.get_bytes(&range) {
                return Ok(bytes);
            }

            match self.next_message().await {
                Ok(message) => self.put_message(message),
                Err(e) => {
                    log::info!(
                        "Session {} TurboReader returning {e:?} after processing {} bytes",
                        self.state.id(),
                        self.state.write()
                    );
                    bail!(e)
                }
            }
        }
    }

    fn try_next_message(&mut self) -> Option<Message> {
        // TODO: We could try_recv on our src too if it was AsyncRead.
        match self.state.protocol_state() {
            ProtocolState::LocalShutRemoteShut => {
                if self.receiver.is_empty() {
                    None
                } else {
                    self.receiver.try_recv().ok()
                }
            }
            _ => self.receiver.try_recv().ok(),
        }
    }

    async fn next_message(&mut self) -> anyhow::Result<Message> {
        // Loop to handle the case where the protocol state is changed by the
        // TurboWriter while we are in an async/await block.
        loop {
            let result = match self.state.protocol_state() {
                ProtocolState::Start => self.next_message_channel().await,
                ProtocolState::Resuming => self.next_message_channel().await,
                ProtocolState::LocalOpenRemoteOpen => self.next_message_any().await,
                ProtocolState::LocalShuttingRemoteOpen => self.next_message_channel().await,
                ProtocolState::LocalShutRemoteOpen => self.next_message_channel().await,
                ProtocolState::LocalOpenRemoteShutting => self.next_message_any().await,
                ProtocolState::LocalOpenRemoteShut => self.next_message_any().await,
                ProtocolState::LocalShuttingRemoteShutting => self.next_message_channel().await,
                ProtocolState::LocalShutRemoteShutting => self.next_message_channel().await,
                ProtocolState::LocalShuttingRemoteShut => self.next_message_channel().await,
                ProtocolState::LocalShutRemoteShut => {
                    if self.receiver.is_empty() {
                        Err(NextMessageError::ProtocolShut)
                    } else {
                        self.next_message_channel().await
                    }
                }
            };

            match result {
                Ok(message) => return Ok(message),
                Err(e) => match e {
                    NextMessageError::ProtocolStateChanged => continue,
                    NextMessageError::Timeout => continue,
                    _ => bail!("{e:?}"),
                },
            }
        }
    }

    async fn next_message_channel(&mut self) -> Result<Message, NextMessageError> {
        tokio::select! {
            _ = self.state.wait_protocol_state() => Err(NextMessageError::ProtocolStateChanged),
            _ = tokio::time::sleep(Duration::from_secs(1)) => Err(NextMessageError::Timeout),
            message_maybe = self.receiver.recv() => message_maybe.ok_or(NextMessageError::ChannelClosed),
        }
    }

    async fn next_message_any(&mut self) -> Result<Message, NextMessageError> {
        tokio::select! {
            _ = self.state.wait_protocol_state() => Err(NextMessageError::ProtocolStateChanged),
            _ = tokio::time::sleep(Duration::from_secs(1)) => Err(NextMessageError::Timeout),
            message_maybe = self.receiver.recv() => message_maybe.ok_or(NextMessageError::ChannelClosed),
            result = self.src.read_bytes(1..READ_CAPACITY*2) => {
                match result {
                    Ok(payload) => {
                        // Create a Forward message containing the app payload.
                        log::trace!("Packaging a new Forward message");
                        Ok(self.state.build_forward_message(payload))
                    }
                    Err(e) => {
                        // Error/EOF, reading from the app is now shut.
                        log::debug!("TurboReader: error reading src: {e}");
                        match self.state.protocol_state() {
                            ProtocolState::LocalOpenRemoteOpen => {
                                self.state.set_protocol_state(ProtocolState::LocalShuttingRemoteOpen)
                            },
                            ProtocolState::LocalOpenRemoteShutting => {
                                self.state.set_protocol_state(ProtocolState::LocalShuttingRemoteShutting)
                            },
                            ProtocolState::LocalOpenRemoteShut => {
                                self.state.set_protocol_state(ProtocolState::LocalShuttingRemoteShut)
                            },
                            _ => assert!(false)
                        };
                        Ok(self.state.build_shut_message())
                    }
                }
            }
        }
    }
}

impl<W: Writer + Send> TurboWriter<W> {
    pub fn new(dst: W, state: SharedTurboState, sender: UnboundedSender<Message>) -> Self {
        Self {
            dst,
            buffer: BytesMut::new(),
            state,
            sender,
        }
    }

    fn push_message(&self, message: Message) {
        // If the channel receiver closed, we don't want to send any more control messages.
        let _ = self.sender.send(message);
        // self.sender.send(message).unwrap()
    }

    async fn deserialize_messages(&mut self, bytes: &Bytes) -> anyhow::Result<usize> {
        // Append the incoming bytes to our write buffer.
        self.buffer.put_slice(bytes);

        // Process all complete turbo messages from our buffer.
        loop {
            let mut read_cursor = Cursor::new(&self.buffer);
            let Some(message) = Formatter::default().deserialize_frame(&mut read_cursor) else {
                break;
            };

            // We got a full message, mark that we consumed the bytes from our buffer.
            self.buffer.advance(read_cursor.position() as usize);

            // Process the message and if we have payload, write it to the destination app.
            if let Some(payload) = self.process_message(message) {
                if let Err(e) = self.dst.write_bytes(&payload).await {
                    // TODO: We need to handle IO error on the write side. Do we
                    // need a new Close command and protocol state to track that
                    // the app disappeared and we cant write to it?
                    warn!("Error writing payload to application: {e}")
                };
            }
        }

        // We always report that we have processed all of the bytes given to us.
        return Ok(bytes.len());
    }

    fn process_message(&mut self, message: Message) -> Option<Bytes> {
        log::trace!("Processing message {message:?}");

        // First we establish our session id if we don't have one yet.
        if let ProtocolState::Start = self.state.protocol_state() {
            if matches!(message.command, Command::Resume) {
                self.state.notify_id(message.session_id)
            }
        }

        // Drop if the message was intended for a different session.
        if !self.state.id_equal(message.session_id) {
            self.drop_message(message);
            return None;
        }

        if message.write != self.state.read() {
            // Could be a dup or a gap: TODO is this where we need to initiate a resume?
            warn!("Dup or gap detected");
            return None;
        }

        self.state.increment_read();
        self.state.set_ack_max(message.read);

        // Conditional processing.
        match self.state.protocol_state() {
            ProtocolState::Start => match message.command {
                Command::Resume => {
                    self.process_resume(message.read, ProtocolState::LocalOpenRemoteOpen)
                }
                _ => self.drop_message(message),
            },
            ProtocolState::Resuming => match message.command {
                Command::ResumeOk => {
                    self.process_resume_ok(message.read, ProtocolState::LocalOpenRemoteOpen);
                }
                _ => self.drop_message(message),
            },
            ProtocolState::LocalOpenRemoteOpen => match message.command {
                Command::Forward(payload) => return Some(payload),
                Command::Shut => self.process_shut(
                    message.write,
                    ProtocolState::LocalOpenRemoteShut,
                    ProtocolState::LocalOpenRemoteShutting,
                ),
                _ => self.drop_message(message),
            },
            ProtocolState::LocalShuttingRemoteOpen => match message.command {
                Command::Forward(payload) => {
                    self.send_ack();
                    return Some(payload);
                }
                Command::Shut => self.process_shut(
                    message.write,
                    ProtocolState::LocalShuttingRemoteShut,
                    ProtocolState::LocalShuttingRemoteShutting,
                ),
                Command::ShutOk => {
                    self.process_shut_ok(message.read, ProtocolState::LocalShutRemoteOpen)
                }
                _ => self.drop_message(message),
            },
            ProtocolState::LocalShutRemoteOpen => match message.command {
                Command::Forward(payload) => {
                    self.send_ack();
                    return Some(payload);
                }
                Command::Shut => self.process_shut(
                    message.write,
                    ProtocolState::LocalShutRemoteShut,
                    ProtocolState::LocalShutRemoteShutting,
                ),
                _ => self.drop_message(message),
            },
            ProtocolState::LocalOpenRemoteShutting => match message.command {
                Command::Forward(payload) => return Some(payload),
                _ => self.drop_message(message),
            },
            ProtocolState::LocalOpenRemoteShut => match message.command {
                _ => self.drop_message(message),
            },
            ProtocolState::LocalShuttingRemoteShutting => match message.command {
                Command::Forward(payload) => {
                    self.send_ack();
                    return Some(payload);
                }
                Command::ShutOk => {
                    self.process_shut_ok(message.read, ProtocolState::LocalShutRemoteShutting)
                }
                _ => self.drop_message(message),
            },
            ProtocolState::LocalShutRemoteShutting => match message.command {
                Command::Forward(payload) => {
                    self.send_ack();
                    return Some(payload);
                }
                _ => self.drop_message(message),
            },
            ProtocolState::LocalShuttingRemoteShut => match message.command {
                Command::ShutOk => {
                    self.process_shut_ok(message.read, ProtocolState::LocalShutRemoteShut)
                }
                _ => self.drop_message(message),
            },
            ProtocolState::LocalShutRemoteShut => self.drop_message(message),
        };

        None
    }

    fn process_resume(&self, cursor: DataCursor, resume: ProtocolState) {
        self.state.set_write_unacked(cursor);
        let message = self.state.build_resume_ok_message();
        self.push_message(message);
        self.state.set_protocol_state(resume);
    }

    fn process_resume_ok(&self, cursor: DataCursor, resume: ProtocolState) {
        self.state.set_write_unacked(cursor);
        self.state.set_protocol_state(resume);
    }

    fn send_ack(&self) {
        let message = self.state.build_forward_ok_message();
        self.push_message(message);
    }

    fn process_shut(&self, cursor: DataCursor, shut: ProtocolState, shutting: ProtocolState) {
        self.state.set_read_end(cursor);
        if self.state.is_read_complete() {
            let message = self.state.build_shut_ok_message();
            self.push_message(message);
            self.state.set_protocol_state(shut);
        } else {
            self.state.set_protocol_state(shutting);
        }
    }

    fn process_shut_ok(&self, cursor: DataCursor, shut: ProtocolState) {
        if self.state.is_ack_complete() {
            self.state.set_protocol_state(shut);
        } else {
            log::warn!(
                "Got ShutOk({cursor}) but our write cursor is {}.",
                self.state.write()
            );
        }
    }

    fn drop_message(&self, message: Message) {
        log::trace!(
            "Session {} dropping message {message:?} from protocol state {:?}",
            self.state.id(),
            self.state.protocol_state()
        );
    }
}

#[async_trait]
impl<R: Reader + Send> Reader for TurboReader<R> {
    async fn read_bytes(&mut self, len: Range<usize>) -> anyhow::Result<Bytes> {
        log::trace!("read_bytes() is called on TurboReader");
        self.serialize_messages(len).await
    }

    async fn read_frame<F, D>(&mut self, _deserializer: &mut D) -> anyhow::Result<F>
    where
        D: Deserializer<F> + Send,
    {
        log::trace!("read_frame() is called on TurboReader");
        unimplemented!()
    }
}

#[async_trait]
impl<W: Writer + Send> Writer for TurboWriter<W> {
    async fn write_bytes(&mut self, bytes: &Bytes) -> anyhow::Result<usize> {
        log::trace!("write_bytes() is called on TurboWriter");
        self.deserialize_messages(bytes).await
    }

    async fn write_frame<F, S>(&mut self, _serializer: &mut S, _frame: F) -> anyhow::Result<usize>
    where
        S: Serializer<F> + Send,
        F: Send,
    {
        log::trace!("write_frame() is called on TurboWriter");
        unimplemented!()
    }

    async fn flush(&mut self) -> anyhow::Result<()> {
        log::trace!("flush() is called on TurboWriter");
        self.dst.flush().await
    }

    async fn shutdown(&mut self) -> anyhow::Result<()> {
        log::trace!("shutdown() is called on TurboWriter");
        self.dst.shutdown().await
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use bytes::{Buf, BufMut, Bytes, BytesMut};
    use rand::Rng;
    use tokio::io::{AsyncWriteExt, DuplexStream};

    use crate::common::mock;
    use crate::net::proto::turbo::TurboSession;
    use crate::net::proto::turbo::formatter::Formatter;
    use crate::net::proto::turbo::frames::{Command, Message};
    use crate::net::proto::turbo::session::{ProtocolState, TurboReader, TurboWriter};
    use crate::net::{BufReader, Connection, Deserializer, Reader, Serialize, Writer};

    struct TurboNode {
        payload: Bytes,
        app: Connection<BufReader<DuplexStream>, DuplexStream>,
        tr: TurboReader<BufReader<DuplexStream>>,
        tw: TurboWriter<DuplexStream>,
    }

    #[derive(Debug)]
    enum TurboTransferError {
        #[allow(dead_code)]
        Get(TurboGetError),
        Put(TurboPutError),
    }

    #[derive(Debug)]
    enum TurboGetError {
        EndOfFile,
        IncompleteMessage,
    }

    #[derive(Debug)]
    enum TurboPutError {
        WriteFailed,
    }

    #[derive(Debug)]
    enum TurboShutError {
        Failed,
    }

    impl TurboNode {
        async fn new(payload_len: usize, is_client: bool) -> TurboNode {
            let (mut app, proxy) = mock::connection_pair(usize::MAX);

            // Load the app with payload that can be read from the proxy.
            let payload = mock::payload(payload_len);
            app.dst.write_all(&payload).await.unwrap();

            // When this proxy reads, the above payload is returned.
            let (tr, tw) = if is_client {
                TurboSession::new_connected_client(proxy)
            } else {
                TurboSession::new_connected_server(proxy)
            };

            TurboNode {
                payload,
                app,
                tr,
                tw,
            }
        }

        async fn shut_app(&mut self) -> Result<(), TurboShutError> {
            Writer::shutdown(&mut self.app.dst)
                .await
                .map_err(|_| TurboShutError::Failed)
        }

        async fn get(&mut self) -> Result<Message, TurboGetError> {
            let bytes = self
                .tr
                .read_bytes(1..usize::MAX)
                .await
                .map_err(|_| TurboGetError::EndOfFile)?;
            deserialize(&bytes).ok_or(TurboGetError::IncompleteMessage)
        }

        async fn put(&mut self, message: &Message) -> Result<usize, TurboPutError> {
            let bytes = serialize(message);
            self.tw
                .write_bytes(&bytes)
                .await
                .map_err(|_| TurboPutError::WriteFailed)
        }

        fn state(&self) -> ProtocolState {
            self.tr.state.protocol_state()
        }

        async fn assert_delivered_payload(&mut self, expected: &Bytes) {
            let mut delivered = BytesMut::with_capacity(expected.len());

            while delivered.len() < expected.len() {
                let chunk = self.app.src.read_bytes(1..usize::MAX).await.unwrap();
                delivered.put(chunk);
            }

            assert_eq!(expected.len(), delivered.len());
            assert_eq!(&expected[..], &delivered[..]);
        }
    }

    async fn open_turbo_pair(client_len: usize, server_len: usize) -> (TurboNode, TurboNode) {
        let mut client = TurboNode::new(client_len, true).await;
        let mut server = TurboNode::new(server_len, false).await;

        // Perform the 1 round handshake to get into the open state.
        transfer_message(&mut client, &mut server).await.unwrap();
        transfer_message(&mut server, &mut client).await.unwrap();

        (client, server)
    }

    fn deserialize(bytes: &Bytes) -> Option<Message> {
        let mut buf = BytesMut::from(bytes.clone());
        let mut cursor = Cursor::new(&buf);

        if let Some(message) = Formatter::default().deserialize_frame(&mut cursor) {
            buf.advance(cursor.position() as usize);
            // We might need to deserialize multiple messages at once.
            assert!(buf.is_empty());
            Some(message)
        } else {
            None
        }
    }

    fn serialize(message: &Message) -> Bytes {
        Message::serialize(message)
    }

    async fn transfer_message(
        src: &mut TurboNode,
        dst: &mut TurboNode,
    ) -> Result<usize, TurboTransferError> {
        let message = src.get().await.map_err(|e| TurboTransferError::Get(e))?;
        dst.put(&message)
            .await
            .map_err(|e| TurboTransferError::Put(e))
    }

    async fn transfer_payload(
        src: &mut TurboNode,
        dst: &mut TurboNode,
    ) -> Result<usize, TurboTransferError> {
        let mut total = 0;
        let mut remaining = src.payload.len();

        while remaining > 0 {
            let message = src.get().await.map_err(|e| TurboTransferError::Get(e))?;

            total += dst
                .put(&message)
                .await
                .map_err(|e| TurboTransferError::Put(e))?;

            if let Command::Forward(payload) = message.command {
                remaining = remaining.saturating_sub(payload.len());
            }
        }

        Ok(total)
    }

    #[tokio::test]
    async fn check() {
        for role in [true, false] {
            let _ = TurboNode::new(0, role).await;
            for len in mock::tests::payload_len_iter() {
                let _ = TurboNode::new(len, role).await;
            }
        }
    }

    #[tokio::test]
    async fn open_with_resume() {
        for (c_len, s_len) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
            let (client, server) = open_turbo_pair(c_len, s_len).await;

            assert_eq!(client.state(), ProtocolState::LocalOpenRemoteOpen);
            assert_eq!(client.state(), client.tw.state.protocol_state());
            assert_eq!(server.state(), ProtocolState::LocalOpenRemoteOpen);
            assert_eq!(server.state(), server.tw.state.protocol_state());
        }
    }

    #[tokio::test]
    async fn forward_client() {
        for len in mock::tests::payload_len_iter() {
            let (mut client, mut server) = open_turbo_pair(len, 0).await;
            transfer_payload(&mut client, &mut server).await.unwrap();
            server.assert_delivered_payload(&client.payload).await;
        }
    }

    #[tokio::test]
    async fn forward_server() {
        for len in mock::tests::payload_len_iter() {
            let (mut client, mut server) = open_turbo_pair(0, len).await;
            transfer_payload(&mut server, &mut client).await.unwrap();
            client.assert_delivered_payload(&server.payload).await;
        }
    }

    #[tokio::test]
    async fn forward_client_and_server() {
        for len in mock::tests::payload_len_iter() {
            let (mut client, mut server) = open_turbo_pair(len, len).await;
            transfer_payload(&mut client, &mut server).await.unwrap();
            transfer_payload(&mut server, &mut client).await.unwrap();
            server.assert_delivered_payload(&client.payload).await;
            client.assert_delivered_payload(&server.payload).await;
        }
    }

    async fn forward_until_error(
        src: &mut TurboReader<BufReader<DuplexStream>>,
        dst: &mut TurboWriter<DuplexStream>,
        reliability: u8,
    ) -> Result<(), TurboTransferError> {
        let mut rng = rand::thread_rng();
        loop {
            let bytes = src
                .read_bytes(1..usize::MAX)
                .await
                .map_err(|_| TurboTransferError::Get(TurboGetError::EndOfFile))?;
            if rng.gen_range(0..100) < reliability {
                dst.write_bytes(&bytes)
                    .await
                    .map_err(|_| TurboTransferError::Put(TurboPutError::WriteFailed))?;
            } else {
                let message = deserialize(&bytes);
                log::warn!("DROPPING: {message:?}");
            }
        }
    }

    async fn forward_bidirectional(reliability: u8) {
        for len in mock::tests::payload_len_iter() {
            let (mut client, mut server) = open_turbo_pair(len, len).await;
            client.shut_app().await.unwrap();
            server.shut_app().await.unwrap();

            let (_, _) = tokio::join!(
                forward_until_error(&mut client.tr, &mut server.tw, reliability),
                forward_until_error(&mut server.tr, &mut client.tw, reliability)
            );

            assert_eq!(client.state(), ProtocolState::LocalShutRemoteShut);
            assert_eq!(server.state(), ProtocolState::LocalShutRemoteShut);
            client.assert_delivered_payload(&server.payload).await;
            server.assert_delivered_payload(&client.payload).await;
        }
    }

    #[tokio::test]
    async fn forward_bidirectional_reliable() {
        forward_bidirectional(100).await;
    }

    async fn shutdown_node_half(
        node1: &mut TurboNode,
        node2: &mut TurboNode,
        is_payload_first: bool,
    ) {
        if is_payload_first {
            transfer_payload(node1, node2).await.unwrap();
            node1.shut_app().await.unwrap();
        } else {
            node1.shut_app().await.unwrap();
            transfer_payload(node1, node2).await.unwrap();
        }

        // This should cause EOF to propagate from app to turbo layer.
        transfer_message(node1, node2).await.unwrap();

        assert_eq!(node1.state(), ProtocolState::LocalShuttingRemoteOpen);
        assert_eq!(node2.state(), ProtocolState::LocalOpenRemoteShut);
    }

    #[tokio::test]
    async fn shutdown_client_after_payload() {
        for len in mock::tests::payload_len_iter() {
            let (mut client, mut server) = open_turbo_pair(len, 0).await;
            shutdown_node_half(&mut client, &mut server, true).await;
        }
    }

    #[tokio::test]
    async fn shutdown_client_before_payload() {
        for len in mock::tests::payload_len_iter() {
            let (mut client, mut server) = open_turbo_pair(len, 0).await;
            shutdown_node_half(&mut client, &mut server, false).await;
        }
    }

    #[tokio::test]
    async fn shutdown_server_after_payload() {
        for len in mock::tests::payload_len_iter() {
            let (mut client, mut server) = open_turbo_pair(0, len).await;
            shutdown_node_half(&mut server, &mut client, true).await;
        }
    }

    #[tokio::test]
    async fn shutdown_server_before_payload() {
        for len in mock::tests::payload_len_iter() {
            let (mut client, mut server) = open_turbo_pair(0, len).await;
            shutdown_node_half(&mut server, &mut client, false).await;
        }
    }

    async fn shutdown_node_full(
        node1: &mut TurboNode,
        node2: &mut TurboNode,
        is_payload_first: bool,
    ) {
        if is_payload_first {
            transfer_payload(node1, node2).await.unwrap();
            transfer_payload(node2, node1).await.unwrap();
            node1.shut_app().await.unwrap();
            node2.shut_app().await.unwrap();
        } else {
            node1.shut_app().await.unwrap();
            node2.shut_app().await.unwrap();
            transfer_payload(node1, node2).await.unwrap();
            transfer_payload(node2, node1).await.unwrap();
        }

        assert_eq!(node1.state(), ProtocolState::LocalOpenRemoteOpen);
        assert_eq!(node2.state(), ProtocolState::LocalOpenRemoteOpen);

        // This should cause EOF to propagate from app to turbo layer.
        transfer_message(node1, node2).await.unwrap();

        assert_eq!(node1.state(), ProtocolState::LocalShuttingRemoteOpen);
        assert_eq!(node2.state(), ProtocolState::LocalOpenRemoteShut);

        transfer_message(node2, node1).await.unwrap();

        assert_eq!(node1.state(), ProtocolState::LocalShutRemoteOpen);
        assert_eq!(node2.state(), ProtocolState::LocalOpenRemoteShut);

        transfer_message(node2, node1).await.unwrap();

        assert_eq!(node1.state(), ProtocolState::LocalShutRemoteShut);
        assert_eq!(node2.state(), ProtocolState::LocalShuttingRemoteShut);

        transfer_message(node1, node2).await.unwrap();

        assert_eq!(node1.state(), ProtocolState::LocalShutRemoteShut);
        assert_eq!(node2.state(), ProtocolState::LocalShutRemoteShut);
    }

    #[tokio::test]
    async fn shutdown_both_after_payload() {
        for len in mock::tests::payload_len_iter() {
            let (mut client, mut server) = open_turbo_pair(len, len).await;
            shutdown_node_full(&mut server, &mut client, true).await;
        }
    }

    #[tokio::test]
    async fn shutdown_both_before_payload() {
        for len in mock::tests::payload_len_iter() {
            let (mut client, mut server) = open_turbo_pair(len, len).await;
            shutdown_node_full(&mut server, &mut client, false).await;
        }
    }

    async fn get_message(node: &mut TurboNode) -> Message {
        node.get()
            .await
            .map_err(|e| TurboTransferError::Get(e))
            .unwrap()
    }

    async fn put_message(node: &mut TurboNode, message: &Message) {
        node.put(&message)
            .await
            .map_err(|e| TurboTransferError::Put(e))
            .unwrap();
    }

    #[tokio::test]
    async fn shutdown_simultaneous() {
        let (mut client, mut server) = open_turbo_pair(0, 0).await;

        client.shut_app().await.unwrap();
        server.shut_app().await.unwrap();

        let c_shut = get_message(&mut client).await;
        assert_eq!(client.state(), ProtocolState::LocalShuttingRemoteOpen);

        let s_shut = get_message(&mut server).await;
        assert_eq!(server.state(), ProtocolState::LocalShuttingRemoteOpen);

        put_message(&mut client, &s_shut).await;
        assert_eq!(client.state(), ProtocolState::LocalShuttingRemoteShut);

        put_message(&mut server, &c_shut).await;
        assert_eq!(server.state(), ProtocolState::LocalShuttingRemoteShut);

        let c_shut_ok = get_message(&mut client).await;
        assert_eq!(client.state(), ProtocolState::LocalShuttingRemoteShut);

        let s_shut_ok = get_message(&mut server).await;
        assert_eq!(server.state(), ProtocolState::LocalShuttingRemoteShut);

        put_message(&mut client, &s_shut_ok).await;
        assert_eq!(client.state(), ProtocolState::LocalShutRemoteShut);

        put_message(&mut server, &c_shut_ok).await;
        assert_eq!(server.state(), ProtocolState::LocalShutRemoteShut);
    }
}
