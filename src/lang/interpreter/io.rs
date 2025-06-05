use std::ops::Range;

use anyhow::bail;
use bytes::Bytes;

use crate::net::{Reader, Writer};

pub struct IoStream<R: Reader, W: Writer> {
    src: R,
    n_recv_src: usize,
    dst: W,
    n_sent_dst: usize,
}

impl<R: Reader, W: Writer> IoStream<R, W> {
    pub fn new(src: R, dst: W) -> Self {
        Self {
            src,
            n_recv_src: 0,
            dst,
            n_sent_dst: 0,
        }
    }

    pub async fn send(&mut self, bytes: Bytes) -> anyhow::Result<usize> {
        log::trace!("trying to send {} bytes to dst", bytes.len());

        let num_written = match self.dst.write_bytes(&bytes).await {
            Ok(num) => num,
            Err(e) => bail!("Error sending to dst: {e}"),
        };

        self.n_sent_dst += num_written;
        log::trace!("sent {num_written} bytes to dst");

        Ok(num_written)
    }

    pub async fn flush(&mut self) -> anyhow::Result<()> {
        self.dst.flush().await
    }

    pub async fn recv(&mut self, len: Range<usize>) -> anyhow::Result<Bytes> {
        log::trace!("Trying to receive {len:?} bytes from src",);

        let data = match self.src.read_bytes(len).await {
            Ok(data) => data,
            Err(e) => bail!(e),
            // If we return an error on EOF, will the entire connection
            // close even if we could still possibly send?
            // Do we need to just go to sleep forever upon EOF, and let
            // an error on the other direction close us down?
            // Err(net_err) => match net_err {
            //     net::Error::Eof => break,
            //     _ => return Err(proteus::Error::from(net_err)),
            // },
        };

        let n_bytes = data.len();
        self.n_recv_src += n_bytes;
        log::trace!("Received {n_bytes} bytes from src");

        Ok(data)
    }
}
