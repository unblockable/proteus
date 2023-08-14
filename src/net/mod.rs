use bytes::{Buf, Bytes, BytesMut};
use std::fmt;
use std::io::Cursor;
use std::ops::Range;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;

use crate::net;

pub mod proto;

#[derive(std::fmt::Debug)]
pub enum Error {
    Eof,
    IoError(std::io::Error),
    _Reunite,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Eof => write!(f, "Reached EOF during network I/O operation"),
            Error::IoError(e) => write!(f, "I/O error during network operation: {}", e),
            Error::_Reunite => write!(f, "Error reuniting read and write stream halves"),
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
    fn serialize_frame(&mut self, src: F) -> Bytes;
}

/// Trait for a formatter that can deserialize one or more protocol frames.
pub trait Deserializer<F> {
    fn deserialize_frame(&mut self, src: &mut Cursor<&BytesMut>) -> Option<F>;
}

pub struct Connection {
    source: NetSource,
    sink: NetSink,
}

impl Connection {
    pub fn new(stream: TcpStream) -> Connection {
        let (read_half, write_half) = stream.into_split();
        Connection {
            source: NetSource::new(read_half),
            sink: NetSink::new(write_half),
        }
    }

    /// Reconstructs the TCP Stream, returning any unhandled bytes in our read buffer.
    fn _into_stream(mut self) -> Result<(TcpStream, Bytes), net::Error> {
        match self.source.read_half.reunite(self.sink.write_half) {
            Ok(s) => Ok((s, self.source.buffer.split().freeze())),
            Err(_) => Err(net::Error::_Reunite),
        }
    }

    pub fn into_split(self) -> (NetSource, NetSink) {
        (self.source, self.sink)
    }

    async fn read_frame<F, D>(&mut self, deserializer: &mut D) -> Result<F, net::Error>
    where
        D: Deserializer<F>,
    {
        self.source.read_frame(deserializer).await
    }

    async fn write_frame<F, S>(&mut self, serializer: &mut S, frame: F) -> Result<usize, net::Error>
    where
        S: Serializer<F>,
    {
        self.sink.write_frame(serializer, frame).await
    }

    async fn _read_bytes(&mut self, len: Range<usize>) -> Result<Bytes, net::Error> {
        self.source.read_bytes(len).await
    }

    async fn _write_bytes(&mut self, bytes: &Bytes) -> Result<usize, net::Error> {
        self.sink.write_bytes(bytes).await
    }
}

pub struct NetSource {
    read_half: OwnedReadHalf,
    buffer: BytesMut,
}

impl NetSource {
    fn new(source: OwnedReadHalf) -> NetSource {
        let cap = 2usize.pow(22u32); // 4 MiB
        NetSource {
            read_half: source,
            buffer: BytesMut::with_capacity(cap),
        }
    }

    /// Read raw bytes from a network source, returning only after we have accumulated a
    /// number of bytes in the given range.
    pub async fn read_bytes(&mut self, len: Range<usize>) -> Result<Bytes, net::Error> {
        let mut fmt = RawFormatter::new(len);
        let data = self.read_frame(&mut fmt).await?;
        Ok(data.into())
    }

    /// Read a frame of type `F` from a network source using deserializer `D`,
    /// waiting until enough data has arrived to fill the frame.
    pub async fn read_frame<F, D>(&mut self, deserializer: &mut D) -> Result<F, net::Error>
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

            // Pull more bytes in from the source.
            self.read_inner().await?;
        }
    }

    /// Pull more bytes in from the source into our internal buffer.
    async fn read_inner(&mut self) -> Result<usize, net::Error> {
        match self.read_half.read_buf(&mut self.buffer).await {
            Ok(n_bytes) => match n_bytes {
                0 => Err(net::Error::Eof),
                _ => Ok(n_bytes),
            },
            Err(e) => Err(net::Error::IoError(e)),
        }
    }
}

pub struct NetSink {
    write_half: OwnedWriteHalf,
}

impl NetSink {
    fn new(sink: OwnedWriteHalf) -> NetSink {
        NetSink { write_half: sink }
    }

    /// Writes the given raw bytes to the network sink, returning after all
    /// bytes have been written.
    pub async fn write_bytes(&mut self, bytes: &Bytes) -> Result<usize, net::Error> {
        self.write_inner(bytes).await
    }

    /// Write a frame `F` to the network sink using serializer `S`.
    /// Returns the number of bytes written to the network.
    async fn write_frame<F, S>(&mut self, serializer: &mut S, frame: F) -> Result<usize, net::Error>
    where
        S: Serializer<F>,
    {
        let bytes = serializer.serialize_frame(frame);
        self.write_inner(&bytes).await
    }

    async fn write_inner(&mut self, bytes: &Bytes) -> Result<usize, net::Error> {
        let num_bytes = bytes.len();
        match self.write_half.write_all(bytes).await {
            Ok(_) => Ok(num_bytes),
            Err(e) => Err(net::Error::IoError(e)),
        }
    }
}

/// A default frame for supporting an API for raw bytes.
struct RawData {
    bytes: Bytes,
}

impl From<Bytes> for RawData {
    fn from(bytes: Bytes) -> Self {
        RawData { bytes }
    }
}

impl From<RawData> for Bytes {
    fn from(data: RawData) -> Self {
        data.bytes
    }
}

// We can serialize a `NetData` directly, but we can't deserialize in isolation
// because we need to know how many bytes to read; leave that to a formatter.
impl Serialize<RawData> for RawData {
    fn serialize(&self) -> Bytes {
        self.bytes.clone()
    }
}

/// A default formatter for supporting an API for raw bytes.
struct RawFormatter {
    valid_read_range: Range<usize>,
}

impl RawFormatter {
    fn new(valid_read_range: Range<usize>) -> RawFormatter {
        RawFormatter { valid_read_range }
    }
}

impl Serializer<RawData> for RawFormatter {
    fn serialize_frame(&mut self, src: RawData) -> Bytes {
        src.serialize()
    }
}

impl Deserializer<RawData> for RawFormatter {
    fn deserialize_frame(&mut self, src: &mut std::io::Cursor<&BytesMut>) -> Option<RawData> {
        match src.remaining() >= self.valid_read_range.start {
            true => {
                let num = std::cmp::min(src.remaining(), self.valid_read_range.end - 1);
                Some(RawData::from(src.copy_to_bytes(num)))
            }
            false => None,
        }
    }
}
