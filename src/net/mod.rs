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

// #[async_trait]
// trait NetReader {
//     /// Read a frame of type `F` from a network source using deserializer `D`,
//     /// waiting until enough data has arrived to fill the frame.
//     async fn read_frame<F, D>(&mut self, deserializer: &D) -> Result<F, net::Error>
//     where
//         D: Deserializer<F> + Sync;
// }

// #[async_trait]
// trait NetWriter {
//     /// Write a frame `F` to the network sink using serializer `S`.
//     async fn write_frame<F, S>(&mut self, serializer: &S, frame: F) -> Result<(), net::Error>
//     where
//         S: Serializer<F>;
// }

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
    
    async fn splice_until_eof(&mut self, other: &mut Connection) -> Result<(), net::Error> {
        // TODO for some reason this does not detect when the curl transfer
        // finishes, so this never returns until killing either the pt client or
        // server.
        match tokio::try_join!(
            tokio::io::copy(&mut self.source.read_half, &mut other.sink.write_half),
            tokio::io::copy(&mut other.source.read_half, &mut self.sink.write_half),
        ) {
            Ok(_) => Ok(()),
            Err(e) => Err(net::Error::IoError(e)),
        }
    }

    fn into_split(self) -> (NetSource, NetSink) {
        (self.source, self.sink)
    }

    async fn read_frame<F, D>(&mut self, deserializer: &D) -> Result<F, net::Error>
    where
        D: Deserializer<F>,
    {
        self.source.read_frame(deserializer).await
    }

    async fn write_frame<F, S>(&mut self, serializer: &S, frame: F) -> Result<usize, net::Error>
    where
        S: Serializer<F>,
    {
        self.sink.write_frame(serializer, frame).await
    }
}

struct NetSource {
    read_half: OwnedReadHalf,
    buffer: BytesMut,
}

impl NetSource {
    fn new(source: OwnedReadHalf) -> NetSource {
        NetSource { read_half: source, buffer: BytesMut::with_capacity(32768) }
    }

    /// Read a frame of type `F` from a network source using deserializer `D`,
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
    
            // Pull more bytes in from the source.
            self.read_inner().await?;
        }
    }

    async fn read_bytes(&mut self) -> Result<Bytes, net::Error> {
        self.read_inner().await?;
        Ok(self.buffer.split().freeze())
    }

    /// Pull more bytes in from the source into our internal buffer.
    async fn read_inner(&mut self) -> Result<usize, net::Error> {
        match self.read_half.read_buf(&mut self.buffer).await {
            Ok(n_bytes) => {
                match n_bytes {
                    0 => Err(net::Error::Eof),
                    _ => Ok(n_bytes)
                }
            }
            Err(e) => return Err(net::Error::IoError(e)),
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

    /// Write a frame `F` to the network sink using serializer `S`.
    /// Returns the number of bytes written to the network.
    async fn write_frame<F, S>(&mut self, serializer: &S, frame: F) -> Result<usize, net::Error>
    where
        S: Serializer<F>,
    {
        let bytes = serializer.serialize_frame(frame);
        self.write_bytes(&bytes).await
    }

    async fn write_bytes(&mut self, bytes: &Bytes) -> Result<usize, net::Error> {
        let num_bytes = bytes.len();
        match self.write_half.write_all(&bytes).await {
            Ok(_) => Ok(num_bytes),
            Err(e) => Err(net::Error::IoError(e))
        }
    }
}
