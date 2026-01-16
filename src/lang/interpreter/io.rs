use std::io;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

use bytes::{BufMut, Bytes, BytesMut};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};

use crate::net::CHUNK_SIZE;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("A normal end of file was reached on an io reader")]
    ReadEof,
    #[error(transparent)]
    Standard(#[from] std::io::Error),
}

impl Clone for Error {
    fn clone(&self) -> Self {
        match self {
            Self::ReadEof => Self::ReadEof,
            Self::Standard(e) => Self::Standard(io::Error::from(e.kind())),
        }
    }
}

pub struct IoStream<R, W>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    src: R,
    n_recv_src: usize,
    src_error: Option<self::Error>,
    dst: W,
    n_sent_dst: usize,
    dst_error: Option<self::Error>,
}

impl<R, W> IoStream<R, W>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    pub fn new(src: R, dst: W) -> Self {
        Self {
            src,
            n_recv_src: 0,
            src_error: None,
            dst,
            n_sent_dst: 0,
            dst_error: None,
        }
    }

    pub fn into_inner(self) -> (R, W) {
        (self.src, self.dst)
    }

    pub async fn send(&mut self, mut bytes: Bytes) -> Result<usize, self::Error> {
        log::trace!("Entering io::send({})", bytes.len());

        if let Some(e) = self.dst_error.as_ref() {
            return Err(e.clone());
        }

        let num_written = bytes.len();

        if let Err(e) = self.dst.write_all_buf(&mut bytes).await {
            return Err(self.dst_err(e));
        }

        if let Err(e) = self.dst.flush().await {
            return Err(self.dst_err(e));
        }

        self.n_sent_dst += num_written;
        log::trace!("Sent {num_written} bytes to dst");

        Ok(num_written)
    }

    pub async fn flush(&mut self) -> Result<(), self::Error> {
        if let Some(e) = self.dst_error.as_ref() {
            return Err(e.clone());
        }

        if let Err(e) = self.dst.flush().await {
            return Err(self.dst_err(e));
        }
        Ok(())
    }

    pub async fn shutdown(&mut self) -> Result<(), self::Error> {
        if let Some(e) = self.dst_error.as_ref() {
            return Err(e.clone());
        }

        if let Err(e) = self.dst.shutdown().await {
            return Err(self.dst_err(e));
        }
        Ok(())
    }

    /// Reads up to len bytes. May read less than len if fewer bytes are available.
    pub async fn read(&mut self, len: usize) -> Result<Bytes, self::Error> {
        log::trace!("Entering io::read({len})");

        if let Some(e) = self.src_error.as_ref() {
            return Err(e.clone());
        }

        // To avoid large pre-allocation, we
        // 1. use read_buf() to async-read the first chunk
        // 2. if we get a full chunk, use try_read() to sync-read remaining available chunks
        let limit = len.min(CHUNK_SIZE);
        let mut limited_buf = BytesMut::with_capacity(limit).limit(limit);

        match self.src.read_buf(&mut limited_buf).await {
            Ok(0) => Err(self.read_eof()),
            Ok(n_bytes) => {
                let mut buf = limited_buf.into_inner();
                if n_bytes == limit && n_bytes < len {
                    // Any err msg from try_read will be set internally.
                    if let Ok(Some(more)) = self.try_read(len - buf.len()) {
                        buf.extend_from_slice(&more);
                    }
                }
                Ok(self.read_ok(buf))
            }
            Err(e) => Err(self.src_err(e)),
        }
    }

    /// Like read, but returns immediately even if no bytes are available.
    pub fn try_read(&mut self, len: usize) -> Result<Option<Bytes>, self::Error> {
        log::trace!("Entering io::try_read({len})");

        if let Some(e) = self.src_error.as_ref() {
            return Err(e.clone());
        }

        // We sync-read in chunks to avoid large allocations.
        let mut buf = BytesMut::new();
        let mut ctx = Context::from_waker(Waker::noop());

        while buf.len() < len {
            let limit = CHUNK_SIZE.min(len - buf.len());
            let mut chunk = BytesMut::with_capacity(limit);
            chunk.resize(limit, 0);

            let mut read_buf = ReadBuf::new(&mut chunk);

            match Pin::new(&mut self.src).poll_read(&mut ctx, &mut read_buf) {
                Poll::Ready(Ok(_)) => {
                    let n_bytes = read_buf.filled().len();
                    if n_bytes > 0 {
                        chunk.truncate(n_bytes);
                        buf.extend_from_slice(&chunk);
                        continue;
                    }
                    self.read_eof();
                }
                Poll::Ready(Err(e)) => {
                    self.src_err(e);
                }
                Poll::Pending => {}
            };
            break;
        }

        if !buf.is_empty() {
            Ok(Some(self.read_ok(buf)))
        } else if let Some(e) = self.src_error.as_ref() {
            Err(e.clone())
        } else {
            Ok(None)
        }
    }

    /// Reads exactly len bytes, waiting until len bytes are available.
    pub async fn read_exact(&mut self, len: usize) -> Result<Bytes, self::Error> {
        log::trace!("Entering io::read_exact({len})");

        if let Some(e) = self.src_error.as_ref() {
            return Err(e.clone());
        }

        // Here we need to pre-allocate with the exactly requested len.
        let mut buf = BytesMut::with_capacity(len);
        buf.resize(len, 0);
        match self.src.read_exact(&mut buf).await {
            Ok(_) => Ok(self.read_ok(buf)),
            Err(e) => match e.kind() {
                // If we are trying to read the len field of the next message,
                // but the other end doesn't want to send any more messages, we
                // consider that an expected eof.
                io::ErrorKind::UnexpectedEof => Err(self.read_eof()),
                _ => Err(self.src_err(e)),
            },
        }
    }

    fn dst_err(&mut self, error: std::io::Error) -> self::Error {
        let err = self::Error::from(error);
        self.dst_error = Some(err.clone());
        err
    }

    fn src_err(&mut self, error: std::io::Error) -> self::Error {
        let err = self::Error::from(error);
        self.src_error = Some(err.clone());
        err
    }

    fn read_ok(&mut self, buf: BytesMut) -> Bytes {
        log::trace!("io read {} bytes", buf.len());
        self.n_recv_src += buf.len();
        buf.freeze()
    }

    fn read_eof(&mut self) -> self::Error {
        self.src_error = Some(self::Error::ReadEof);
        self::Error::ReadEof
    }
}

#[cfg(test)]
pub mod tests {
    use std::time::Duration;

    use bytes::{BufMut, Bytes, BytesMut};
    use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
    use tokio::time::timeout;
    use tokio_test::io::Builder;

    use crate::common::mock;
    use crate::lang::interpreter::io::IoStream;

    async fn bytes_mut_read_buf_limit(payload_len: usize, buf_limit: usize) {
        let (mut reader, mut writer) = mock::simplex(payload_len);
        let payload = mock::payload(payload_len);

        assert!(writer.write_all(&payload).await.is_ok());

        // If you use BytesMut::new() here, you'll only get a 64 byte allocated buffer.
        // We need with_capacity() if we want read_buf to give us more than 64 bytes.
        // Because we are using limit(), we don't need to call resize().
        let mut read_buf = BytesMut::with_capacity(payload_len).limit(buf_limit);
        let read_result = reader.read_buf(&mut read_buf).await;
        assert!(read_result.is_ok());

        let read_len = read_result.unwrap();

        if buf_limit < payload_len {
            assert_eq!(read_len, buf_limit);
        } else {
            assert_eq!(read_len, payload_len);
        }

        let bytes = read_buf.into_inner().freeze();
        assert_eq!(&payload[0..read_len], &bytes[..]);
    }

    #[tokio::test]
    async fn bytes_mut_read_buf_low_limit() {
        bytes_mut_read_buf_limit(16, 10).await;
        bytes_mut_read_buf_limit(16, 15).await;
    }

    #[tokio::test]
    async fn bytes_mut_read_buf_same_limit() {
        bytes_mut_read_buf_limit(16, 16).await;
    }

    #[tokio::test]
    async fn bytes_mut_read_buf_high_limit() {
        bytes_mut_read_buf_limit(16, 17).await;
        bytes_mut_read_buf_limit(16, 1024).await;
    }

    async fn bytes_mut_read_exact(payload_len: usize, buf_limit: usize) {
        assert!(buf_limit <= payload_len);

        let (mut reader, mut writer) = mock::simplex(payload_len);
        let payload = mock::payload(payload_len);

        assert!(writer.write_all(&payload).await.is_ok());

        let mut read_buf = BytesMut::with_capacity(buf_limit);
        read_buf.resize(buf_limit, 0);
        let read_result = reader.read_exact(&mut read_buf).await;

        let read_len = read_result.unwrap();
        let bytes = read_buf.freeze();
        assert_eq!(read_len, buf_limit);
        assert_eq!(&payload[0..read_len], &bytes[..]);
    }

    #[tokio::test]
    async fn bytes_mut_read_exact_low_limit() {
        bytes_mut_read_exact(16, 10).await;
        bytes_mut_read_exact(16, 15).await;
    }

    #[tokio::test]
    async fn bytes_mut_read_exact_same_limit() {
        bytes_mut_read_exact(16, 16).await;
    }

    #[tokio::test]
    async fn bytes_mut_large_reads() {
        bytes_mut_read_buf_limit(1_000_000, 100_000).await;
        bytes_mut_read_buf_limit(1_000_000, 1_000_000).await;
        bytes_mut_read_buf_limit(1_000_000, 2_000_000).await;
    }

    #[tokio::test]
    async fn try_read_empty() {
        let (reader, writer) = mock::simplex(64);
        let mut io = IoStream::new(reader, writer);
        let result = io.try_read(16).unwrap();
        assert_eq!(result, None);
    }

    async fn new_readable_io(
        payload_len: usize,
    ) -> (IoStream<impl AsyncRead, impl AsyncWrite>, Bytes) {
        let (reader, mut writer) = mock::simplex(payload_len);

        let payload = mock::payload(payload_len);
        if payload_len > 0 {
            assert!(writer.write_all(&payload).await.is_ok());
        }

        (IoStream::new(reader, writer), payload)
    }

    #[tokio::test]
    async fn try_read_low() {
        let (mut io, payload) = new_readable_io(64).await;

        for i in 0..3 {
            let bytes = io.try_read(16).unwrap().unwrap();
            assert_eq!(bytes.len(), 16);
            let (start, end) = (i * 16, (i + 1) * 16);
            assert_eq!(&bytes, &payload[start..end])
        }
    }

    #[tokio::test]
    async fn try_read_same() {
        let (mut io, payload) = new_readable_io(64).await;

        let bytes = io.try_read(64).unwrap().unwrap();

        assert_eq!(bytes.len(), 64);
        assert_eq!(&bytes, &payload);
    }

    #[tokio::test]
    async fn try_read_high() {
        let (mut io, payload) = new_readable_io(64).await;

        let bytes = io.try_read(128).unwrap().unwrap();
        assert_eq!(bytes.len(), 64);
        assert_eq!(&bytes, &payload);
    }

    #[tokio::test]
    async fn try_read_eof() {
        // Mock readers return EOF when empty.
        let mut io = IoStream::new(Builder::new().build(), Builder::new().build());
        assert!(io.try_read(64).is_err());
    }

    #[tokio::test]
    async fn read_empty() {
        async fn this_should_block() {
            let (reader, writer) = mock::simplex(64);
            let mut io = IoStream::new(reader, writer);
            let _ = io.read(16).await;
        }

        let result = timeout(Duration::from_millis(500), this_should_block()).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn read_low() {
        let (mut io, payload) = new_readable_io(64).await;

        for i in 0..3 {
            let bytes = io.read(16).await.unwrap();
            assert_eq!(bytes.len(), 16);
            let (start, end) = (i * 16, (i + 1) * 16);
            assert_eq!(&bytes, &payload[start..end])
        }
    }

    #[tokio::test]
    async fn read_same() {
        let (mut io, payload) = new_readable_io(64).await;

        let bytes = io.read(64).await.unwrap();

        assert_eq!(bytes.len(), 64);
        assert_eq!(&bytes, &payload);
    }

    #[tokio::test]
    async fn read_high() {
        let (mut io, payload) = new_readable_io(64).await;

        let bytes = io.read(128).await.unwrap();
        assert_eq!(bytes.len(), 64);
        assert_eq!(&bytes, &payload);
    }

    #[tokio::test]
    async fn read_eof() {
        // Mock readers return EOF when empty.
        let mut io = IoStream::new(Builder::new().build(), Builder::new().build());
        assert!(io.read(64).await.is_err());
    }

    #[tokio::test]
    async fn read_exact_empty() {
        async fn this_should_block() {
            let (reader, writer) = mock::simplex(64);
            let mut io = IoStream::new(reader, writer);
            let _ = io.read_exact(16).await;
        }

        let result = timeout(Duration::from_millis(500), this_should_block()).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn read_exact_low() {
        let (mut io, payload) = new_readable_io(64).await;

        for i in 0..3 {
            let bytes = io.read_exact(16).await.unwrap();
            assert_eq!(bytes.len(), 16);
            let (start, end) = (i * 16, (i + 1) * 16);
            assert_eq!(&bytes, &payload[start..end])
        }
    }

    #[tokio::test]
    async fn read_exact_same() {
        let (mut io, payload) = new_readable_io(64).await;

        let bytes = io.read_exact(64).await.unwrap();

        assert_eq!(bytes.len(), 64);
        assert_eq!(&bytes, &payload);
    }

    #[tokio::test]
    async fn read_exact_high() {
        async fn this_should_block() {
            let (mut io, _payload) = new_readable_io(64).await;
            let _ = io.read_exact(128).await;
        }

        let result = timeout(Duration::from_millis(500), this_should_block()).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn read_exact_eof() {
        // Mock readers return EOF when empty.
        let mut io = IoStream::new(Builder::new().build(), Builder::new().build());
        assert!(io.read_exact(64).await.is_err());
    }

    #[tokio::test]
    async fn large_reads() {
        // If source has full payload available, we expect to receive it all.
        for len in mock::payload_len_iter() {
            let (mut io, payload) = new_readable_io(len).await;

            let bytes = io.read(len).await.unwrap();
            assert_eq!(bytes.len(), len);
            assert_eq!(&bytes, &payload);
        }
    }

    #[tokio::test]
    async fn read_all_available() {
        // If source has full payload available, we expect to receive it all.
        let (mut io, _payload) = new_readable_io(64).await;

        let bytes = io.read(9).await.unwrap();
        assert_eq!(bytes.len(), 9);

        let bytes = io.read(9).await.unwrap();
        assert_eq!(bytes.len(), 9);

        let bytes = io.read_exact(5).await.unwrap();
        assert_eq!(bytes.len(), 5);

        let bytes = io.read(100).await.unwrap();
        assert_eq!(bytes.len(), 64 - 9 - 9 - 5);
    }
}
