use std::fmt;
use std::io::Cursor;
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::io::{AsyncWriteExt, AsyncReadExt};
use bytes::{BytesMut, Buf};

use crate::net;

// pub mod or; // TODO uncomment when ExtOr protocol is implemented
pub mod socks;

pub trait Frame<T> {
    /// Returns a parsed frame or `None` if it was incomplete.
    fn deserialize(src: &mut Cursor<&BytesMut>) -> Option<T>;
    /// Returns the bytes representation of the frame.
    fn serialize(&self) -> BytesMut;
}

fn get_bytes_vec(buf: &mut Cursor<&BytesMut>, num_bytes: usize) -> Option<Vec<u8>> {
    let mut bytes_vec = Vec::new();
    for _ in 0..num_bytes {
        let b = buf.has_remaining().then(|| buf.get_u8())?;
        bytes_vec.push(b);
    }
    Some(bytes_vec)
}

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
            read_half, write_half, buffer
        }
    }

    /// Read a frame from the connection, waiting until enough data has arrived
    /// to fill the frame.
    pub async fn read_frame<T: Frame<T>>(&mut self) -> Result<T, net::Error> {
        loop {
            // Get a cursor to seek over the buffered bytes.
            let mut read_cursor = Cursor::new(&self.buffer);

            // Try to parse the frame from the buffer.
            if let Some(frame) = T::deserialize(&mut read_cursor) {
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

    /// Write a frame to the connection.
    pub async fn write_frame<T: Frame<T>>(&mut self, frame: &T) -> Result<(), net::Error> {
        let bytes = frame.serialize();

        if let Err(e) = self.write_half.write_all(&bytes).await {
            return Err(net::Error::IoError(e));
        }

        Ok(())
    }
}
