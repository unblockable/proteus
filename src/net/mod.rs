use bytes::{Buf, Bytes, BytesMut};
use std::fmt;
use std::io::Cursor;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;

use crate::net;

pub mod proto;

pub enum Error {
    Eof,
    IoError(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Eof => write!(f, "Reached EOF during network I/O operation"),
            Error::IoError(e) => write!(f, "I/O error during network operation: {}", e),
        }
    }
}

/// Trait for formatting a static frame so it can be sent to the network.
trait Serialize<F> {
    fn serialize(&self) -> Bytes;
}

/// Trait for formatting a static frame so it can be received from the network.
trait Deserialize<F> {
    fn deserialize(src: &mut Cursor<&BytesMut>) -> Option<F>;
}

/// Trait for a formatter that can serialize one or more protocol frames.
trait Serializer<F> {
    fn serialize_frame(&self, src: F) -> Bytes;
}

/// Trait for a formatter that can deserialize one or more protocol frames.
trait Deserializer<F> {
    fn deserialize_frame(&self, src: &mut Cursor<&BytesMut>) -> Option<F>;
}

pub struct Connection {
    read_half: OwnedReadHalf,
    write_half: OwnedWriteHalf,
    buffer: BytesMut,
}

impl Connection {
    pub fn new(stream: TcpStream) -> Connection {
        let (read_half, write_half) = stream.into_split();
        let buffer = BytesMut::new();
        Connection {
            read_half,
            write_half,
            buffer,
        }
    }

    /// Read a frame of type `F` from the connection using deserializer `D`,
    /// waiting until enough data has arrived to fill the frame.
    async fn read_frame<F, D>(&mut self, deserializer: &D) -> Result<F, net::Error>
    where
        D: Deserializer<F>,
    {
        loop {
            // Get a cursor to seek over the buffered bytes.
            let mut read_cursor = Cursor::new(&self.buffer);

            // Try to parse the frame from the buffer.
            if let Some(frame) = deserializer.deserialize_frame(&mut read_cursor) {
                // Mark the bytes as consumed.
                let num_consumed = read_cursor.position() as usize;
                self.buffer.advance(num_consumed);
                return Ok(frame);
            }

            // Pull more bytes in from the source if possible.
            match self.read_half.read_buf(&mut self.buffer).await {
                Ok(n_bytes) => {
                    if n_bytes == 0 {
                        return Err(net::Error::Eof);
                    }
                }
                Err(e) => return Err(net::Error::IoError(e)),
            };
        }
    }

    /// Write a frame `F` to the connection using serializer `S`.
    async fn write_frame<F, S>(&mut self, serializer: &S, frame: F) -> Result<(), net::Error>
    where
        S: Serializer<F>,
    {
        let bytes = serializer.serialize_frame(frame);

        if let Err(e) = self.write_half.write_all(&bytes).await {
            return Err(net::Error::IoError(e));
        }

        Ok(())
    }

    async fn splice_until_eof(&mut self, other: &mut Connection) -> Result<(), net::Error> {
        // TODO for some reason this does not detect when the curl transfer
        // finishes, so this never returns until killing either the pt client or
        // server.
        match tokio::try_join!(
            tokio::io::copy(&mut self.read_half, &mut other.write_half),
            tokio::io::copy(&mut other.read_half, &mut self.write_half),
        ) {
            Ok(_) => Ok(()),
            Err(e) => Err(net::Error::IoError(e)),
        }
    }
}
