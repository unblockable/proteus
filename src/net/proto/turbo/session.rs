use std::io::Cursor;
use std::ops::Range;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use rand::RngCore;
use rand::rngs::ThreadRng;

use crate::net::proto::turbo::formatter::Formatter;
use crate::net::proto::turbo::frames::{Command, DataCursor, Message, Payload};
use crate::net::{Connection, Deserializer, Reader, Serialize, Serializer, Writer};

/// A session is a singular connection to an application, after the preliminary
/// handshake protocol (e.g., SOCKS) is completed. The app connection should be
/// in a state where it expects us to forward raw data to a target network peer.
pub struct Session<R: Reader + Send, W: Writer + Send> {
    src: R,
    dst: W,
    state: SessionState,
}

impl<R: Reader + Send, W: Writer + Send> Session<R, W> {
    pub fn new(app_conn: Connection<R, W>) -> Self {
        let (app_src, app_dst) = app_conn.into_split();
        Self::from_split(app_src, app_dst)
    }

    pub fn from_split(app_src: R, app_dst: W) -> Self {
        Self {
            src: app_src,
            dst: app_dst,
            state: SessionState::new(),
        }
    }

    pub fn into_split(self) -> (SessionReader<R>, SessionWriter<W>) {
        let shared = SharedSessionState::new(self.state);
        let reader = SessionReader::new(self.src, shared.clone());
        let writer = SessionWriter::new(self.dst, shared);
        (reader, writer)
    }
}

/// The state needed to implement our turbo session protocol.
struct SessionState {
    id: u64,
    seq: DataCursor,
    ack: DataCursor,
}

impl SessionState {
    fn new() -> Self {
        Self::from_id(ThreadRng::default().next_u64())
    }

    fn from_id(id: u64) -> Self {
        Self { id, seq: 0, ack: 0 }
    }
}

/// The state that needs to be shared across the session reader and writer
/// forwarding directions.
#[derive(Clone)]
struct SharedSessionState {
    inner: Arc<Mutex<SessionState>>,
}

impl SharedSessionState {
    fn new(state: SessionState) -> Self {
        Self {
            inner: Arc::new(Mutex::new(state)),
        }
    }
}

/// A wrapper around the application data source, responsible for returning app
/// bytes to the interpreter. This wraps app data inside of our turbo protocol
/// to enable support for session resumption.
pub struct SessionReader<R: Reader + Send> {
    src: R,
    buffer: BytesMut,
    shared_state: SharedSessionState,
}

impl<R: Reader + Send> SessionReader<R> {
    fn new(src: R, shared_state: SharedSessionState) -> Self {
        Self {
            src,
            buffer: BytesMut::new(),
            shared_state,
        }
    }

    async fn next_message(&mut self) -> anyhow::Result<Message> {
        // Get some more app data. Turbo supports payloads up to u16::MAX.
        let app_bytes = self.src.read_bytes(1..u16::MAX as usize).await?;
        let payload_len = app_bytes.len() as u64;

        let (id, seq, ack) = {
            let mut state = self.shared_state.inner.lock().unwrap();
            state.seq += payload_len;
            (state.id, state.seq - payload_len, state.ack)
        };

        // Package it into a turbo frame.
        let msg = Message {
            session_id: id,
            command: Command::Forward(Payload {
                write: seq,
                read: ack,
                data: app_bytes,
            }),
        };

        log::info!(
            "Sending {} bytes in session range [{}:{})",
            payload_len,
            seq,
            seq + payload_len
        );

        Ok(msg)
    }
}

#[async_trait]
impl<R: Reader + Send> Reader for SessionReader<R> {
    async fn read_bytes(&mut self, len: Range<usize>) -> anyhow::Result<Bytes> {
        loop {
            // If we have enough already-formed frame bytes, return those.
            if self.buffer.len() >= len.start {
                let split_pos = self.buffer.len().min(len.end);
                return Ok(self.buffer.split_to(split_pos).freeze());
            }

            // Package the next message into a turbo frame of bytes for the caller.
            let msg = self.next_message().await?;
            self.buffer.put_slice(&msg.serialize());
        }
    }

    async fn read_frame<F, D>(&mut self, _deserializer: &mut D) -> anyhow::Result<F>
    where
        D: Deserializer<F> + Send,
    {
        // Do we ever want to read a frame from an application? Probably not.
        // I think that means our traits are not quite as precise as they could be.
        unimplemented!()
    }
}

/// A wrapper around the application data sink, responsible for writing app
/// bytes from the interpreter into the application data sink. This unwraps app
/// data from our turbo protocol frames and then writes it to the app, to enable
/// support for session resumption.
pub struct SessionWriter<W: Writer + Send> {
    dst: W,
    buffer: BytesMut,
    shared_state: SharedSessionState,
}

impl<W: Writer + Send> SessionWriter<W> {
    fn new(dst: W, shared_state: SharedSessionState) -> Self {
        Self {
            dst,
            buffer: BytesMut::new(),
            shared_state,
        }
    }

    async fn process_message(&mut self, msg: Message) -> anyhow::Result<()> {
        // Take the necessary action depending on the message type.
        match msg.command {
            Command::Forward(payload) => {
                let len = payload.data.len() as u64;
                self.dst.write_bytes(&payload.data).await?;

                let (id, _seq, ack) = {
                    let mut state = self.shared_state.inner.lock().unwrap();
                    state.ack += len;
                    (state.id, state.seq, state.ack - len)
                };

                log::info!(
                    "Receiving {len} bytes in session {id} range [{ack}:{})",
                    ack + len
                );
            }
            _ => todo!(),
        }

        Ok(())
    }
}

#[async_trait]
impl<W: Writer + Send> Writer for SessionWriter<W> {
    async fn write_bytes(&mut self, bytes: &Bytes) -> anyhow::Result<usize> {
        // Append the incoming bytes to our write buffer.
        self.buffer.put_slice(bytes);

        // Process all messages buffered as turbo frames.
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

    async fn write_frame<F, S>(&mut self, _serializer: &mut S, _frame: F) -> anyhow::Result<usize>
    where
        S: Serializer<F> + Send,
        F: Send,
    {
        // Do we ever want to write a frame to an application? Probably not.
        // I think that means our traits are not quite as precise as they could be.
        unimplemented!()
    }

    async fn flush(&mut self) -> anyhow::Result<()> {
        self.dst.flush().await
    }
}

#[cfg(test)]
mod tests {
    use anyhow::bail;
    use bytes::BytesMut;
    use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};

    use crate::common::mock::tests::NullSpec;
    use crate::common::mock::{self, MockConnection};
    use crate::lang::ir::bridge::TaskProvider;
    use crate::net::proto::turbo::session::{Session, SessionReader, SessionWriter};
    use crate::net::{BufReader, Reader, Writer};

    async fn forward_app_to_net(
        mut src: SessionReader<BufReader<DuplexStream>>,
        mut dst: DuplexStream,
    ) -> anyhow::Result<u64> {
        loop {
            // This will propagate EOF for us.
            let mut buf = src.read_bytes(1..usize::MAX).await?;
            dst.write_all_buf(&mut buf).await?;
        }
    }

    async fn forward_net_to_app(
        mut src: DuplexStream,
        mut dst: SessionWriter<DuplexStream>,
    ) -> anyhow::Result<u64> {
        let mut total = 0;
        loop {
            let mut buf = BytesMut::new();

            let n = src.read_buf(&mut buf).await?;

            if n > 0 {
                // Successfully read some bytes.
                total += n;
                dst.write_bytes(&buf.freeze()).await?;
            } else if n == 0 {
                // Read EOF, let's return to propagate it.
                return Ok(total as u64);
            } else {
                // Some other IO error.
                bail!("IO Error")
            }
        }
    }

    async fn run_session_copier<T: TaskProvider + Send>(
        _: T,
        net_conn: MockConnection,
        app_conn: MockConnection,
    ) -> anyhow::Result<()> {
        // Unwrap the Connection and BufReader.
        let (net_r, net_w) = net_conn.into_split();
        let net_r = net_r.into_inner();

        // Make a session from the app connection.
        let (app_r, app_w) = Session::new(app_conn).into_split();

        // We need to move the streams into `forward()` so that the DuplexStreams close
        // when the tokio::io::copy function receives EOF and returns. Otherwise the EOF
        // does not properly propagate backward.
        let (_, _) = tokio::join!(
            forward_app_to_net(app_r, net_w),
            forward_net_to_app(net_r, app_w)
        );

        Ok(())
    }

    #[tokio::test]
    async fn session() {
        for len in mock::tests::payload_len_iter() {
            let result =
                mock::run_proxy_network(NullSpec {}, NullSpec {}, &run_session_copier, len).await;
            mock::tests::assert_mock_result(result, len)
        }
    }
}
