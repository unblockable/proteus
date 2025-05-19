use std::fmt;
use std::io::Cursor;
use std::net::SocketAddr;
use std::ops::Range;

use anyhow::bail;
use async_trait::async_trait;
use bytes::{Buf, Bytes, BytesMut};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
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
pub trait Serializer<F> {
    fn serialize_frame(&mut self, src: F) -> Bytes;
}

/// Trait for a formatter that can deserialize one or more protocol frames.
pub trait Deserializer<F> {
    fn deserialize_frame(&mut self, src: &mut Cursor<&BytesMut>) -> Option<F>;
}

#[async_trait]
pub trait Connector<R: Reader, W: Writer> {
    async fn connect(&self, addr: SocketAddr) -> anyhow::Result<(Connection<R, W>, SocketAddr)>;
}

#[async_trait]
pub trait Reader {
    async fn read_bytes(&mut self, len: Range<usize>) -> anyhow::Result<Bytes>;
    async fn read_frame<F, D>(&mut self, deserializer: &mut D) -> anyhow::Result<F>
    where
        D: Deserializer<F> + Send;
}

#[async_trait]
pub trait Writer {
    async fn write_bytes(&mut self, bytes: &Bytes) -> anyhow::Result<usize>;
    async fn write_frame<F, S>(&mut self, serializer: &mut S, frame: F) -> anyhow::Result<usize>
    where
        S: Serializer<F> + Send,
        F: Send;
    async fn flush(&mut self) -> anyhow::Result<()>;
}

pub struct BufReader<R: AsyncRead + Send + Unpin> {
    source: R,
    buffer: BytesMut,
}

impl<R: AsyncRead + Send + Unpin> BufReader<R> {
    pub fn new(source: R) -> Self {
        let cap = 2usize.pow(14u32); // 16 KiB
        BufReader::with_capacity(source, cap)
    }

    fn with_capacity(source: R, capacity: usize) -> Self {
        Self {
            source,
            buffer: BytesMut::with_capacity(capacity),
        }
    }

    #[cfg(test)]
    /// Note that this will lose buffered bytes if there are any.
    pub fn into_inner(self) -> R {
        self.source
    }
}

#[async_trait]
impl<R: AsyncRead + Send + Unpin> Reader for BufReader<R> {
    async fn read_bytes(&mut self, len: Range<usize>) -> anyhow::Result<Bytes> {
        let mut fmt = RawFormatter::new(len);
        let data = self.read_frame(&mut fmt).await?;
        Ok(data.into())
    }

    async fn read_frame<F, D>(&mut self, deserializer: &mut D) -> anyhow::Result<F>
    where
        D: Deserializer<F> + Send,
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
            // self.read_inner().await?;
            let _ = match self.source.read_buf(&mut self.buffer).await {
                Ok(n_bytes) => match n_bytes {
                    0 => bail!(net::Error::Eof),
                    _ => n_bytes,
                },
                Err(e) => bail!(net::Error::IoError(e)),
            };
        }
    }
}

#[async_trait]
impl<W: AsyncWrite + Send + Unpin> Writer for W {
    async fn write_bytes(&mut self, bytes: &Bytes) -> anyhow::Result<usize> {
        let num_bytes = bytes.len();
        match self.write_all(bytes).await {
            Ok(_) => Ok(num_bytes),
            Err(e) => bail!(net::Error::IoError(e)),
        }
    }

    async fn write_frame<F, S>(&mut self, serializer: &mut S, frame: F) -> anyhow::Result<usize>
    where
        S: Serializer<F> + Send,
        F: Send,
    {
        let bytes = serializer.serialize_frame(frame);
        Ok(self.write_bytes(&bytes).await?)
    }

    async fn flush(&mut self) -> anyhow::Result<()> {
        Ok(AsyncWriteExt::flush(&mut self).await?)
    }
}

pub struct Connection<R: Reader, W: Writer> {
    pub src: R,
    pub dst: W,
}

impl<R: Reader, W: Writer> Connection<R, W> {
    pub fn new(src: R, dst: W) -> Self {
        Self { src, dst }
    }

    pub fn into_split(self) -> (R, W) {
        (self.src, self.dst)
    }
}

pub struct TcpConnector {}

impl TcpConnector {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl Connector<BufReader<OwnedReadHalf>, OwnedWriteHalf> for TcpConnector {
    async fn connect(
        &self,
        addr: SocketAddr,
    ) -> anyhow::Result<(
        Connection<BufReader<OwnedReadHalf>, OwnedWriteHalf>,
        SocketAddr,
    )> {
        let stream = TcpStream::connect(addr).await?;
        let local_addr = stream.local_addr()?;
        let conn = Connection::from(stream);
        Ok((conn, local_addr))
    }
}

impl From<TcpStream> for Connection<BufReader<OwnedReadHalf>, OwnedWriteHalf> {
    fn from(stream: TcpStream) -> Self {
        let (read_half, write_half) = stream.into_split();
        Connection {
            src: BufReader::new(read_half),
            dst: write_half,
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

// We can serialize a `RawData` directly, but we can't deserialize in isolation
// because we need to know how many bytes to read; leave that to `RawFormatter`.
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
    fn new(valid_read_range: Range<usize>) -> Self {
        Self { valid_read_range }
    }
}

impl Serializer<RawData> for RawFormatter {
    fn serialize_frame(&mut self, src: RawData) -> Bytes {
        src.serialize()
    }
}

impl Deserializer<RawData> for RawFormatter {
    fn deserialize_frame(&mut self, src: &mut std::io::Cursor<&BytesMut>) -> Option<RawData> {
        if self.valid_read_range.start >= self.valid_read_range.end
            || self.valid_read_range.end <= 1
        {
            Some(RawData::from(Bytes::new()))
        } else if src.remaining() >= self.valid_read_range.start {
            let num = std::cmp::min(src.remaining(), self.valid_read_range.end - 1);
            Some(RawData::from(src.copy_to_bytes(num)))
        } else {
            None
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::common::mock;

    async fn transfer_bytes_helper<W: Writer, R: Reader>(dst: &mut W, src: &mut R, len: usize) {
        let payload = mock::payload(len);

        let num_written = dst.write_bytes(&payload).await.unwrap();
        assert_eq!(num_written, payload.len());

        let transferred = src.read_bytes(num_written..num_written + 1).await.unwrap();
        assert_eq!(num_written, transferred.len());

        assert_eq!(&payload[..], &transferred[..]);
    }

    #[tokio::test]
    async fn transfer_bytes() {
        let max_len = mock::tests::payload_len_iter().max().unwrap();
        let (mut client, mut server) = mock::connection_pair(max_len);
        for len in mock::tests::payload_len_iter() {
            transfer_bytes_helper(&mut client.dst, &mut server.src, len).await;
            transfer_bytes_helper(&mut server.dst, &mut client.src, len).await;
        }
    }

    async fn transfer_frame_helper<W: Writer, R: Reader>(dst: &mut W, src: &mut R, len: usize) {
        let payload = mock::payload(len);

        let frame_out = RawData::from(payload.clone());
        let mut fmt = RawFormatter::new(payload.len()..payload.len() + 1);

        let num_written = dst.write_frame(&mut fmt, frame_out).await.unwrap();
        assert_eq!(num_written, payload.len());

        let frame_in = src.read_frame(&mut fmt).await.unwrap();
        let transferred = Bytes::from(frame_in);
        assert_eq!(num_written, transferred.len());

        assert_eq!(&payload[..], &transferred[..]);
    }

    #[tokio::test]
    async fn transfer_frame() {
        let max_len = mock::tests::payload_len_iter().max().unwrap();
        let (mut client, mut server) = mock::connection_pair(max_len);
        for len in mock::tests::payload_len_iter() {
            transfer_frame_helper(&mut client.dst, &mut server.src, len).await;
            transfer_frame_helper(&mut server.dst, &mut client.src, len).await;
        }
    }

    #[tokio::test]
    async fn reader() {
        let max_len = mock::tests::payload_len_iter().max().unwrap();
        let payload = mock::payload(max_len);

        let mem_stream = Cursor::new(payload.clone());
        let mut src = BufReader::new(mem_stream);

        let bytes = src.read_bytes(0..0).await.unwrap();
        assert_eq!(bytes.len(), 0);
        let bytes = src.read_bytes(0..1).await.unwrap();
        assert_eq!(bytes.len(), 0);
        let bytes = src.read_bytes(5..1).await.unwrap();
        assert_eq!(bytes.len(), 0);

        let bytes = src
            .read_bytes(payload.len()..payload.len() + 1)
            .await
            .unwrap();
        assert_eq!(&payload[..], &bytes[..]);
    }

    #[tokio::test]
    async fn writer() {
        let max_len = mock::tests::payload_len_iter().max().unwrap();
        let payload = mock::payload(max_len);

        let buf = Vec::<u8>::new();
        let mut dst = Cursor::new(buf);
        dst.write_bytes(&payload).await.unwrap();
        let bytes = dst.into_inner();

        assert_eq!(&payload[..], &bytes[..]);
    }
}
