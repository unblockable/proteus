use std::fmt::Debug;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::BytesMut;
use futures::{Sink, Stream, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_util::codec::{Decoder, Encoder};

// Uses a Codec to encode the messages taken from the inner Stream into bytes
// that are given to the caller through the AsyncRead interface.
pub struct FramedStreamReader<T, S, E>
where
    S: Stream<Item = T>,
    E: Encoder<T>,
    E::Error: Into<io::Error> + Debug,
{
    stream: Option<S>,
    encoder: E,
    buffer: BytesMut,
}

impl<T, S, E> FramedStreamReader<T, S, E>
where
    S: Stream<Item = T>,
    E: Encoder<T>,
    E::Error: Into<io::Error> + Debug,
{
    pub fn new(stream: S, encoder: E) -> Self {
        Self {
            stream: Some(stream),
            encoder,
            buffer: BytesMut::new(),
        }
    }

    /// Buffer the message for sending.
    ///
    /// # Panics
    ///
    /// Panics if the payload inside the message is larger than supported by the
    /// codec, which can be considered a programming error.
    fn put_buf(&mut self, msg: T) {
        self.encoder.encode(msg, &mut self.buffer).unwrap();
    }
}

impl<T, S, E> AsyncRead for FramedStreamReader<T, S, E>
where
    S: Stream<Item = T> + Unpin,
    E: Encoder<T> + Unpin,
    E::Error: Into<io::Error> + Debug,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut ReadBuf,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();

        // If we already have enough cached bytes to fill the ReadBuf, return quickly.
        if this.buffer.len() >= buf.remaining() {
            let at = buf.remaining();
            let bytes = this.buffer.split_to(at).freeze();
            buf.put_slice(&bytes);
            return Poll::Ready(Ok(()));
        }

        // Read as many messages as needed to fill the ReadBuf if we can.
        while let Some(stream) = this.stream.as_mut()
            && this.buffer.len() < buf.remaining()
        {
            match stream.poll_next_unpin(cx) {
                Poll::Ready(Some(msg)) => this.put_buf(msg),
                Poll::Ready(None) => this.stream = None, // Stream is done, drop it.
                Poll::Pending => break,                  // Stream will reactivate later.
            }
        }

        if !this.buffer.is_empty() {
            // We may or may not be able to fill the ReadBuf, but we provide what we have.
            let at = this.buffer.len().min(buf.remaining());
            let bytes = this.buffer.split_to(at).freeze();
            buf.put_slice(&bytes);
            Poll::Ready(Ok(()))
        } else if this.stream.is_none() {
            // Stream is done, this is an EOF since we did not add to the ReadBuf.
            Poll::Ready(Ok(()))
        } else {
            // Stream will reactivate later.
            Poll::Pending
        }
    }
}

// Uses a Codec to decode the bytes taken from the caller through the AsyncWrite interface
// into messages that are passed to an inner Sink for processing.
pub struct FramedSinkWriter<T, S, D>
where
    S: Sink<T>,
    S::Error: Into<io::Error>,
    D: Decoder<Item = T>,
    D::Error: Into<io::Error>,
{
    sink: S,
    decoder: D,
    buffer: BytesMut,
}

impl<T, S, D> FramedSinkWriter<T, S, D>
where
    S: Sink<T> + Unpin,
    S::Error: Into<io::Error>,
    D: Decoder<Item = T>,
    D::Error: Into<io::Error>,
{
    pub fn new(sink: S, decoder: D) -> Self {
        Self {
            sink,
            decoder,
            buffer: BytesMut::new(),
        }
    }

    fn send_all_from_buffer(&mut self, cx: &mut Context) -> Poll<io::Result<()>> {
        loop {
            // Check if the sink is ready to accept a new message.
            match Pin::new(&mut self.sink).poll_ready(cx) {
                Poll::Ready(Ok(())) => {
                    // Try to decode a message from our buffer.
                    match self.decoder.decode(&mut self.buffer) {
                        // A message was decoded, send it to the sink.
                        Ok(Some(msg)) => match Pin::new(&mut self.sink).start_send(msg) {
                            Ok(()) => continue, // Success, try again.
                            Err(e) => return Poll::Ready(Err(e.into())),
                        },
                        // Need more bytes to decode a full message.
                        Ok(None) => return Poll::Ready(Ok(())),
                        // An error occurred during decoding.
                        Err(e) => return Poll::Ready(Err(e.into())),
                    }
                }
                // The sink has an error.
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e.into())),
                // Sink not ready yet, try again later.
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

impl<T, S, D> AsyncWrite for FramedSinkWriter<T, S, D>
where
    S: Sink<T> + Unpin,
    S::Error: Into<io::Error>,
    D: Decoder<Item = T> + Unpin,
    D::Error: Into<io::Error>,
{
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<io::Result<usize>> {
        let this = self.get_mut();

        // Don't accept any new bytes until all buffered messages are sent.
        match this.send_all_from_buffer(cx) {
            Poll::Ready(Ok(())) => {
                // Now we processed all buffered messages, and we need more
                // bytes to decode the next full message. So 'write' all of the
                // bytes from buf.
                this.buffer.extend_from_slice(buf);

                // Repeat the send loop, but don't return Poll::Pending this
                // time since the bytes are already considered written.
                match this.send_all_from_buffer(cx) {
                    Poll::Ready(Ok(())) => Poll::Ready(Ok(buf.len())),
                    Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                    Poll::Pending => Poll::Ready(Ok(buf.len())),
                }
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().sink)
            .poll_flush(cx)
            .map_err(Into::into)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().sink)
            .poll_close(cx)
            .map_err(Into::into)
    }
}
