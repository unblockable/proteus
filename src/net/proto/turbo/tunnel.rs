use std::ops::Range;
use std::sync::Arc;

use anyhow::bail;
use async_trait::async_trait;
use bytes::Bytes;
use tokio::sync::Mutex;

use crate::net::{Connector, Deserializer, Reader, Serializer, Writer};

pub struct Tunnel<R, W, C>
where
    R: Reader + Send,
    W: Writer + Send,
    C: Connector<R, W> + Send,
{
    reader: Arc<Mutex<Option<R>>>,
    writer: Arc<Mutex<Option<W>>>,
    connector: Option<Arc<Mutex<Option<C>>>>,
}

impl<R, W, C> Tunnel<R, W, C>
where
    R: Reader + Send,
    W: Writer + Send,
    C: Connector<R, W> + Send,
{
    fn new(reader: Option<R>, writer: Option<W>, connector: Option<C>) -> Self {
        Self {
            reader: Arc::new(Mutex::new(reader)),
            writer: Arc::new(Mutex::new(writer)),
            connector: Some(Arc::new(Mutex::new(connector))),
        }
    }

    pub fn new_client(connector: C) -> Self {
        Self::new(None, None, Some(connector))
    }

    pub fn new_server(net_src: R, net_dst: W) -> Self {
        Self::new(Some(net_src), Some(net_dst), None)
    }

    async fn check_connection(&mut self) -> anyhow::Result<()> {
        // We want to stop checking the mutex when the connection is established.
        if let Some(connector_lock) = &self.connector {
            log::trace!("Waiting for connector lock");
            let mut opt = connector_lock.lock().await;

            // Only one instance holding a ref will execute the connect.
            if let Some(connector) = opt.take() {
                log::trace!("Found valid connector, trying to connect to peer now");
                // Block the other instances on the connect lock until ready.
                let (conn, addr) = connector.connect().await?;

                log::debug!("Successfully connected to peer {}", addr);

                let (src, dst) = conn.into_split();
                {
                    let mut r = self.reader.lock().await;
                    *r = Some(src);
                }
                {
                    let mut w = self.writer.lock().await;
                    *w = Some(dst);
                }
            }
        }

        // No need to check the mutex again.
        self.connector.take();

        log::trace!("Returning OK from check_connection()");
        Ok(())
    }
}

impl<R, W, C> Clone for Tunnel<R, W, C>
where
    R: Reader + Send,
    W: Writer + Send,
    C: Connector<R, W> + Send,
{
    fn clone(&self) -> Self {
        Self {
            reader: self.reader.clone(),
            writer: self.writer.clone(),
            connector: self.connector.clone(),
        }
    }
}

#[async_trait]
impl<R, W, C> Reader for Tunnel<R, W, C>
where
    R: Reader + Send,
    W: Writer + Send,
    C: Connector<R, W> + Send,
{
    async fn read_bytes(&mut self, len: Range<usize>) -> anyhow::Result<Bytes> {
        log::trace!("read_bytes() is called on TurboTunnelController");

        self.check_connection().await?;

        if let Some(reader) = self.reader.lock().await.as_mut() {
            log::debug!("Waiting for bytes from reader");
            reader.read_bytes(len).await
        } else {
            bail!("No owned reader")
        }
    }

    async fn read_frame<F, D>(&mut self, deserializer: &mut D) -> anyhow::Result<F>
    where
        D: Deserializer<F> + Send,
    {
        log::trace!("read_frame() is called on TurboTunnelController");

        self.check_connection().await?;

        if let Some(reader) = self.reader.lock().await.as_mut() {
            reader.read_frame(deserializer).await
        } else {
            bail!("No owned reader")
        }
    }
}

#[async_trait]
impl<R, W, C> Writer for Tunnel<R, W, C>
where
    R: Reader + Send,
    W: Writer + Send,
    C: Connector<R, W> + Send,
{
    async fn write_bytes(&mut self, bytes: &Bytes) -> anyhow::Result<usize> {
        log::trace!("write_bytes() is called on TurboTunnelController");

        self.check_connection().await?;

        if let Some(writer) = self.writer.lock().await.as_mut() {
            writer.write_bytes(bytes).await
        } else {
            bail!("No owned writer")
        }
    }

    async fn write_frame<F, S>(&mut self, serializer: &mut S, frame: F) -> anyhow::Result<usize>
    where
        S: Serializer<F> + Send,
        F: Send,
    {
        log::trace!("write_frame() is called on TurboTunnelController");

        self.check_connection().await?;

        if let Some(writer) = self.writer.lock().await.as_mut() {
            writer.write_frame(serializer, frame).await
        } else {
            bail!("No owned writer")
        }
    }

    async fn flush(&mut self) -> anyhow::Result<()> {
        log::trace!("flush() is called on TurboTunnelController");

        self.check_connection().await?;

        if let Some(writer) = self.writer.lock().await.as_mut() {
            writer.flush().await
        } else {
            Ok(())
        }
    }

    async fn shutdown(&mut self) -> anyhow::Result<()> {
        log::trace!("shutdown() is called on TurboTunnelController");

        self.check_connection().await?;

        if let Some(writer) = self.writer.lock().await.as_mut() {
            writer.shutdown().await
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    // TODO
}
