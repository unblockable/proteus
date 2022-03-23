use std::io::{self, BufRead, BufReader, BufWriter, Cursor, Write};
use std::net::TcpStream;

use bytes::{Bytes, Buf};

use crate::net;
pub mod socks;

pub trait Frame<T> {
    /// Returns a parsed frame or `None` if it was incomplete.
    fn parse(src: &mut Cursor<&[u8]>) -> Option<T>;
    fn write<W: Write>(&self, dst: &W) -> Result<(), net::Error>;
}

pub enum Error {
    Eof,
    IoError(io::Error),
}

pub struct Connection {
    reader: BufReader<TcpStream>,
    writer: BufWriter<TcpStream>,
}

impl Connection {
    pub fn new(stream: TcpStream) -> io::Result<Connection> {
        Ok(Connection {
            reader: BufReader::new(stream.try_clone()?),
            writer: BufWriter::new(stream),
        })
    }

    /// Read a frame from the connection, blocking until enough data has arrived
    /// to fill the frame.
    ///
    /// Returns `None` if EOF is reached.
    pub fn read_frame<T: Frame<T>>(&mut self) -> Result<T, net::Error> {
        loop {
            // Get a cursor to seek over the buffered bytes.
            // let b = Bytes::from(self.reader.buffer());
            let mut read_cursor = Cursor::new(self.reader.buffer());

            // Try to parse the frame from the buffer.
            if let Some(frame) = T::parse(&mut read_cursor) {
                let num_parsed = read_cursor.position() as usize;

                // Mark the bytes as consumed.
                self.reader.consume(num_parsed);

                return Ok(frame);
            }

            // Pull more bytes in from the source if possible.
            match self.reader.fill_buf() {
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
    pub fn write_frame<T: Frame<T>>(&mut self, frame: &T) -> Result<(), net::Error> {
        // implementation here
        let write_cursor = Cursor::new(&self.writer);

        // let mut w = self.writer.by_ref();
        // w.w

        Ok(())
    }
}
