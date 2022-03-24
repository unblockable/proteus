// use std::io::{self, BufRead, Cursor, Write};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::io::{BufReader, BufWriter, AsyncBufReadExt, AsyncWriteExt};

use bytes::BytesMut;

use crate::net;
pub mod socks;

pub trait Frame<T> {
    /// Returns a parsed frame or `None` if it was incomplete.
    fn deserialize(src: &mut BytesMut) -> Option<T>;
    fn serialize(&self) -> BytesMut;
}

pub enum Error {
    Eof,
    IoError(std::io::Error),
}

pub struct Connection {
    reader: BufReader<OwnedReadHalf>,
    writer: BufWriter<OwnedWriteHalf>,
}

impl Connection {
    pub fn new(stream: TcpStream) -> Connection {
        let (read_half, write_half) = stream.into_split();
        Connection {
            reader: BufReader::new(read_half),
            writer: BufWriter::new(write_half),
        }
    }

    /// Read a frame from the connection, waiting until enough data has arrived
    /// to fill the frame.
    pub async fn read_frame<T: Frame<T>>(&mut self) -> Result<T, net::Error> {
        loop {
            // Get a cursor to seek over the buffered bytes.
            let mut bytes = BytesMut::from(self.reader.buffer());

            // Try to parse the frame from the buffer.
            if let Some(frame) = T::deserialize(&mut bytes) {
                // Mark the bytes as consumed.
                let amt = bytes.len();
                self.reader.consume(amt);

                return Ok(frame);
            }

            // Pull more bytes in from the source if possible.
            match self.reader.fill_buf().await {
                Ok(buf) => {
                    if buf.is_empty() {
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

        if let Err(e) = self.writer.write_all(&bytes).await {
            return Err(net::Error::IoError(e));
        }

        if let Err(e) = self.writer.flush().await {
            return Err(net::Error::IoError(e));
        }

        Ok(())
    }
}
