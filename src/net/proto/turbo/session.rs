use std::io::Cursor;
use std::ops::Range;
use std::sync::{Arc, Mutex};

use anyhow::bail;
use async_trait::async_trait;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use tokio::sync::Notify;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::net::proto::turbo::formatter::Formatter;
use crate::net::proto::turbo::frames::{Command, DataCursor, Message, Payload, ShutWhich, Status};
use crate::net::{Deserializer, Reader, Serialize, Serializer, Writer};

/// The state needed to implement our turbo session protocol that needs to be
/// shared across the session reader and writer forwarding directions.
#[derive(Default)]
struct TurboState {
    id: Option<u64>,
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
}

impl SharedTurboState {
    pub fn new(id: Option<u64>) -> Self {
        let mut state = TurboState::default();
        state.id = id;

        Self {
            inner: Arc::new(Mutex::new(state)),
            id_notify: id.map_or_else(|| Some(Arc::new(Notify::new())), |_| None),
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

    fn set_id(&mut self, value: u64) {
        self.inner.lock().unwrap().id = Some(value);
    }

    fn id_equal(&self, other: u64) -> bool {
        self.inner
            .lock()
            .unwrap()
            .id
            .map_or(false, |id| id == other)
    }

    fn write_ack(&self) -> DataCursor {
        self.inner.lock().unwrap().write_ack
    }

    fn set_write_ack_max(&mut self, value: DataCursor) -> (DataCursor, DataCursor) {
        let mut state = self.inner.lock().unwrap();
        let old = state.write_ack;
        state.write_ack = state.write_ack.max(value);
        (old, state.write_ack)
    }

    fn write(&self) -> DataCursor {
        self.inner.lock().unwrap().write
    }

    fn write_end(&self) -> bool {
        self.inner.lock().unwrap().write_end.is_some()
    }

    fn set_write_end(&mut self) {
        let mut state = self.inner.lock().unwrap();
        state.write_end = Some(state.write);
    }

    fn increment_write(&mut self, value: DataCursor) -> (DataCursor, DataCursor) {
        let mut state = self.inner.lock().unwrap();
        let old = state.write;
        state.write += value;
        (old, state.write)
    }

    fn set_write_if_unacked(&mut self, value: DataCursor) -> bool {
        let mut inner = self.inner.lock().unwrap();
        if value >= inner.write_ack && value <= inner.write {
            inner.write = value;
            true
        } else {
            false
        }
    }

    fn read(&self) -> DataCursor {
        self.inner.lock().unwrap().read
    }

    fn increment_read(&mut self, value: DataCursor) -> (DataCursor, DataCursor) {
        let mut state = self.inner.lock().unwrap();
        let old = state.read;
        state.read += value;
        (old, state.read)
    }

    fn set_read_end(&mut self, end: DataCursor) {
        self.inner.lock().unwrap().read_end = Some(end);
    }

    fn read_complete(&self) -> bool {
        let state = self.inner.lock().unwrap();
        state.read_end.map(|end| state.read >= end).unwrap_or(false)
    }

    fn write_complete(&self) -> bool {
        let state = self.inner.lock().unwrap();
        state
            .write_end
            .map(|end| state.write_ack >= end)
            .unwrap_or(false)
    }
}

impl Clone for SharedTurboState {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            id_notify: self.id_notify.clone(),
        }
    }
}

/// A wrapper around the application data source, responsible for returning app
/// bytes to the interpreter. This wraps app data inside of our turbo protocol
/// to enable support for session resumption.
pub struct TurboReader<R: Reader + Send> {
    src: R,
    buffer: BytesMut,
    state: SharedTurboState,
    receiver: UnboundedReceiver<Message>,
    read_error: Option<anyhow::Error>,
}

/// A wrapper around the application data sink, responsible for writing app
/// bytes from the interpreter into the application data sink. This unwraps app
/// data from our turbo protocol frames and then writes it to the app, to enable
/// support for session resumption.
pub struct TurboWriter<W: Writer + Send> {
    dst: W,
    buffer: BytesMut,
    state: SharedTurboState,
    // This is an option so that we can drop it to close the reader.
    sender: Option<UnboundedSender<Message>>,
}

impl<R: Reader + Send> TurboReader<R> {
    pub fn new(src: R, state: SharedTurboState, receiver: UnboundedReceiver<Message>) -> Self {
        let mut reader = Self {
            src,
            buffer: BytesMut::new(),
            state,
            receiver,
            read_error: None,
        };
        if reader.state.has_id() {
            reader.buffer_message(reader.package_resume_message(reader.state.id()));
        }
        reader
    }

    async fn serialize_messages(&mut self, len: Range<usize>) -> anyhow::Result<Bytes> {
        // We do not start making messages until we have set a session id.
        //
        // XXX: if the server is supposed to send first and also wants a payload
        // for that first message, but gets here and is blocked on the session
        // id, it will never get a chance to provide the required bytes.
        let session_id = self.state.wait_id().await;

        loop {
            // Buffer control/retransmit messages queued by the writer.
            if self.read_error.is_some() {
                // Source reached EOF, but keep the session open until the writer is done.
                log::trace!("Waiting for a message from the message channel asynchronously...");
                if let Some(message) = self.receiver.recv().await {
                    self.buffer_message(message);
                } else {
                    log::info!(
                        "Session {session_id} TurboReader EOF after processing {} bytes",
                        self.state.write()
                    );
                    bail!("TurboReader EOF: {}", self.read_error.as_ref().unwrap())
                }
            } else {
                // Buffer available control/retransmit messages queued by the writer.
                while let Ok(message) = self.receiver.try_recv() {
                    self.buffer_message(message);
                }
            }

            // If we have enough bytes in our buffer, return those.
            if let Some(b) = self.ready_bytes(session_id, &len) {
                return Ok(b);
            }

            // We need to supply more bytes, try to get some from our source.
            if self.read_error.is_none() {
                let message = self.read_source(session_id).await;
                self.buffer_message(message);
            }
        }
    }

    fn ready_bytes(&mut self, session_id: u64, len: &Range<usize>) -> Option<Bytes> {
        if self.buffer.len() >= len.start {
            let read_len = self.buffer.len().min(len.end);
            log::debug!(
                "Session {session_id} requested to read [{}:{}), returning {read_len}",
                len.start,
                len.end
            );
            Some(self.buffer.split_to(read_len).freeze())
        } else {
            None
        }
    }

    async fn read_source(&mut self, session_id: u64) -> Message {
        log::trace!("Reading application bytes asynchronously...");
        match self.src.read_bytes(1..u16::MAX as usize).await {
            Ok(app_bytes) => {
                // Create a Forward message containing the app data and buffer it.
                log::trace!("Packaging a new Forward message");
                self.package_forward_message(session_id, app_bytes)
            }
            Err(e) => {
                // Error or EOF, we won't be reading any more app bytes.
                log::debug!("TurboReader got error on src.read_bytes(): {e}");
                self.read_error = Some(e);
                self.state.set_write_end();
                self.package_shutdown_message(session_id)
            }
        }
    }

    fn buffer_message(&mut self, message: Message) {
        let before = self.buffer.len();
        self.buffer.put_slice(&message.serialize());
        let n = self.buffer.len() - before;
        log::trace!("Added {n} message bytes to read buffer");
    }

    fn package_resume_message(&self, session_id: u64) -> Message {
        Message {
            session_id,
            command: Command::Resume(self.state.read()),
        }
    }

    fn package_forward_message(&mut self, session_id: u64, data: Bytes) -> Message {
        let len = data.len() as u64;
        let (write, write_new) = self.state.increment_write(len);
        let read = self.state.read();

        log::debug!(
            "Session {session_id} sending {len} bytes; read {read} write {} ack {}",
            format_status(write, write_new),
            self.state.write_ack(),
        );

        Message {
            session_id,
            command: Command::Forward(Payload { write, read, data }),
        }
    }

    fn package_shutdown_message(&self, session_id: u64) -> Message {
        Message {
            session_id,
            command: Command::Shutdown(ShutWhich::Write(self.state.write())),
        }
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

impl<W: Writer + Send> TurboWriter<W> {
    pub fn new(dst: W, state: SharedTurboState, sender: UnboundedSender<Message>) -> Self {
        Self {
            dst,
            buffer: BytesMut::new(),
            state,
            sender: Some(sender),
        }
    }

    fn push_message(&self, message: Message) {
        if let Some(sender) = self.sender.as_ref() {
            if self.state.read_complete() && self.state.write_complete() {
                // Best effort as the receiver may have closed.
                let _ = sender.send(message);
            } else {
                // Receiver should not have closed yet.
                sender.send(message).unwrap()
            }
        }
    }

    fn push_status_message(&self, session_id: u64, status: Status) {
        let message = Message {
            session_id,
            command: Command::Notify(status),
        };
        self.push_message(message);
    }

    fn push_shutdown_message(&self, session_id: u64) {
        let message = Message {
            session_id,
            command: Command::Shutdown(ShutWhich::Read(self.state.read())),
        };
        self.push_message(message);
    }

    async fn deserialize_messages(&mut self, bytes: &Bytes) -> anyhow::Result<usize> {
        // Append the incoming bytes to our write buffer.
        self.buffer.put_slice(bytes);

        // Process all complete turbo messages from our buffer.
        loop {
            let mut src = Cursor::new(&self.buffer);
            if let Some(msg) = Formatter::default().deserialize_frame(&mut src) {
                // Mark the bytes as consumed.
                let num_consumed = src.position() as usize;
                self.buffer.advance(num_consumed);
                self.process_message(msg).await?;
            } else {
                break;
            }
        }

        // We always report that we have processed all of the bytes.
        Ok(bytes.len())
    }

    async fn process_message(&mut self, msg: Message) -> anyhow::Result<()> {
        let session_id = msg.session_id;

        let command_str = match &msg.command {
            Command::Connect(_) => "Connect",
            Command::Resume(_) => "Resume",
            Command::Forward(_) => "Forward",
            Command::Shutdown(_) => "Shutdown",
            Command::Notify(_) => "Notify",
            Command::Invalid => "Invalid",
        };
        log::trace!("Session {session_id} processing a {command_str} message");

        if self.state.read_complete() && self.state.write_complete() {
            bail!("TurboWriter EOF")
        }

        // Take the necessary action depending on the message type.
        match msg.command {
            Command::Resume(write) => {
                self.state.notify_id(session_id);

                if !self.state.id_equal(session_id) || !self.state.set_write_if_unacked(write) {
                    self.push_status_message(session_id, Status::ResumeError);
                    bail!("Got error in a Resume command")
                }

                self.push_status_message(session_id, Status::ResumeOk(self.state.write()));
            }
            Command::Forward(payload) => {
                if !self.state.id_equal(session_id) || payload.write != self.state.read() {
                    self.push_status_message(session_id, Status::ForwardError);
                    bail!("Got error in a Forward command")
                }

                let len = payload.data.len() as u64;

                log::trace!("Writing {len} application bytes asynchronously...");
                self.dst.write_bytes(&payload.data).await?;

                let (read_old, read_new) = self.state.increment_read(len);
                let (ack_old, ack_new) = self.state.set_write_ack_max(payload.read);
                let write = self.state.write();

                log::debug!(
                    "Session {session_id} received {len} bytes; read {} write {} ack {}",
                    format_status(read_old, read_new),
                    format_status(write, write),
                    format_status(ack_old, ack_new),
                );

                // Send an ACK that we read some data, but only if we are done
                // sending Forward message which would already contain an ack.
                if self.state.write_end() {
                    self.push_status_message(session_id, Status::ForwardOk(read_new));
                }
            }
            Command::Shutdown(which) => match which {
                ShutWhich::Read(ack) => self.process_ack(session_id, ack),
                ShutWhich::Write(end) => {
                    self.state.set_read_end(end);
                    self.push_status_message(session_id, Status::ShutdownOk(self.state.read()))
                }
                ShutWhich::Invalid => self.push_status_message(session_id, Status::ShutdownError),
            },
            Command::Notify(status) => match status {
                Status::ForwardOk(ack) => self.process_ack(session_id, ack),
                Status::ShutdownOk(ack) => self.process_ack(session_id, ack),
                _ => {}
            },
            _ => todo!(),
        }

        if self.state.read_complete() && self.state.write_complete() {
            self.push_shutdown_message(session_id);
            log::info!(
                "Session {session_id} TurboWriter EOF after processing {} bytes",
                self.state.read()
            );
            // Drop the sender to close the message channel.
            let _ = self.sender.take();
            self.sender = None;
            bail!("TurboWriter EOF")
        } else {
            Ok(())
        }
    }

    fn process_ack(&mut self, session_id: u64, ack: u64) {
        let (ack_old, ack_new) = self.state.set_write_ack_max(ack);
        let len = ack_new.saturating_sub(ack_old);
        log::debug!(
            "Session {session_id} acked {len} bytes; read {} write {} ack {}",
            self.state.read(),
            self.state.write(),
            format_status(ack_old, ack_new),
        );
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

fn format_status(old: u64, new: u64) -> String {
    if old == new {
        format!("{new}")
    } else {
        format!("[{old}:{new})")
    }
}

#[cfg(test)]
mod tests {}
