use std::collections::HashMap;
use std::marker::PhantomData;
use std::ops::Range;
use std::sync::Arc;

use anyhow::bail;
use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use futures::SinkExt;
use futures::stream::{SelectAll, StreamExt};
use rand::RngCore;
use rand::rngs::ThreadRng;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::Mutex;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio_util::codec::{Decoder, Encoder};

use crate::net::proto::socks::address::{Socks5Address, Socks5Target};
use crate::net::proto::turbo::codec::TurboCodec;
use crate::net::proto::turbo::message::{Command, Request};
use crate::net::proto::turbo::session::{TurboSession, TurboSink, TurboStream};
use crate::net::{AsyncConnect, Deserializer, Reader, Serializer, Writer};

pub struct SessionBroker<R, W, C>
where
    R: AsyncRead + Send + Unpin,
    W: AsyncWrite + Send + Unpin,
    C: AsyncConnect<R, W> + From<Socks5Target> + Send,
{
    shared_read_state: Arc<Mutex<ReadState<R>>>,
    shared_write_state: Arc<Mutex<WriteState<W>>>,
    stream_sender: Sender<TurboStream<R>>,
    sink_sender: Sender<TurboSink<W>>,
    pinned_target: Option<Socks5Target>,
    _phantom: PhantomData<C>,
}

struct ReadState<R: AsyncRead + Send + Unpin> {
    streams: SelectAll<TurboStream<R>>,
    pending: Option<Receiver<TurboStream<R>>>,
    buffer: BytesMut,
    eof_on_empty: bool,
}

struct WriteState<W: AsyncWrite + Send + Unpin> {
    sinks: HashMap<u64, TurboSink<W>>,
    pending: Receiver<TurboSink<W>>,
    buffer: BytesMut,
}

impl<R, W, C> SessionBroker<R, W, C>
where
    R: AsyncRead + Send + Unpin,
    W: AsyncWrite + Send + Unpin,
    C: AsyncConnect<R, W> + From<Socks5Target> + Send,
{
    fn new(pinned_target: Option<Socks5Target>, eof_on_empty: bool) -> Self {
        let (stream_sender, stream_receiver) = mpsc::channel(1_000);
        let (sink_sender, sink_receiver) = mpsc::channel(1_000);

        Self {
            shared_read_state: Arc::new(Mutex::new(ReadState {
                streams: SelectAll::new(),
                pending: Some(stream_receiver),
                buffer: BytesMut::new(),
                eof_on_empty,
            })),
            shared_write_state: Arc::new(Mutex::new(WriteState {
                sinks: HashMap::new(),
                pending: sink_receiver,
                buffer: BytesMut::new(),
            })),
            stream_sender,
            sink_sender,
            pinned_target,
            _phantom: PhantomData,
        }
    }

    pub fn new_pt_client() -> Self {
        Self::new(None, true)
    }

    pub fn new_socks_client() -> Self {
        Self::new(None, false)
    }

    pub fn new_pt_server(pinned_target: Socks5Target) -> Self {
        Self::new(Some(pinned_target), true)
    }

    pub fn new_socks_server() -> Self {
        Self::new(None, false)
    }

    async fn generate_session_id(&self) -> u64 {
        loop {
            let id = ThreadRng::default().next_u64();
            if id > 0 {
                let state = self.shared_write_state.lock().await;
                if !state.sinks.contains_key(&id) {
                    return id;
                }
            }
        }
    }

    async fn store_session(&mut self, session: TurboSession<R, W>) {
        let (stream, sink) = session.into_split();
        // TODO: handle send errors in the following?
        let _ = self.stream_sender.send(stream).await;
        let _ = self.sink_sender.send(sink).await;
    }

    pub async fn add_session_socks_client(&mut self, src: R, dst: W, target: Socks5Target) {
        self.add_session_client(src, dst, target).await;
    }

    pub async fn add_session_pt_client(&mut self, src: R, dst: W) {
        // The server will already be configured with the connect target.
        self.add_session_client(src, dst, Socks5Target::new(Socks5Address::Unknown, 0))
            .await;
    }

    async fn add_session_client(&mut self, src: R, dst: W, target: Socks5Target) {
        let id = self.generate_session_id().await;
        self.add_session_client_inner(src, dst, id, target).await;
    }

    async fn add_session_client_inner(&mut self, src: R, dst: W, id: u64, target: Socks5Target) {
        let mut session = TurboSession::new(id, src, dst);
        session.open(target);
        self.store_session(session).await;
    }

    #[cfg(test)]
    pub async fn add_session_client_test(&mut self, src: R, dst: W, id: u64, target: Socks5Target) {
        self.add_session_client_inner(src, dst, id, target).await;
    }

    async fn add_session_server(&mut self, src: R, dst: W, id: u64) {
        let session = TurboSession::new(id, src, dst);
        self.store_session(session).await;
    }

    #[cfg(test)]
    pub async fn add_session_server_test(&mut self, src: R, dst: W, id: u64) {
        self.add_session_server(src, dst, id).await;
    }

    async fn connect(&mut self, id: u64, target: Socks5Target) {
        let connector = match &self.pinned_target {
            Some(pinned) => C::from(pinned.clone()),
            None => C::from(target.clone()),
        };

        log::debug!("Launching connection to {target} now");

        // TODO: what about returning connect ok/error frames?
        match connector.connect().await {
            Ok((src, dst, _)) => {
                log::debug!("Connection to {target} succeeded");
                self.add_session_server(src, dst, id).await
            }
            Err(e) => {
                log::debug!("Connection to {target} failed with error: {e}");
            }
        }
    }
}

impl<R, W, C> Clone for SessionBroker<R, W, C>
where
    R: AsyncRead + Send + Unpin,
    W: AsyncWrite + Send + Unpin,
    C: AsyncConnect<R, W> + From<Socks5Target> + Send,
{
    fn clone(&self) -> Self {
        Self {
            shared_read_state: self.shared_read_state.clone(),
            shared_write_state: self.shared_write_state.clone(),
            stream_sender: self.stream_sender.clone(),
            sink_sender: self.sink_sender.clone(),
            pinned_target: self.pinned_target.clone(),
            _phantom: PhantomData,
        }
    }
}

#[async_trait]
impl<R, W, C> Reader for SessionBroker<R, W, C>
where
    R: AsyncRead + Send + Unpin,
    W: AsyncWrite + Send + Unpin,
    C: AsyncConnect<R, W> + From<Socks5Target> + Send,
{
    async fn read_bytes(&mut self, len: Range<usize>) -> anyhow::Result<Bytes> {
        log::trace!("read_bytes() is called on TurboSessionBroker");

        loop {
            log::trace!("Waiting for read lock");
            let mut state = self.shared_read_state.lock().await;

            // Hold pending queue in a temporary variable so we can select it in parallel.
            let mut pending = state.pending.take().unwrap();

            // Decide on which event sources will allow us to progress.
            let (recv_event, next_event) = if state.streams.is_empty() {
                log::trace!("Waiting for new streams");
                (Some(pending.recv().await), None)
            } else if len.start == 0 && (len.end <= 1 || state.streams.iter().all(|x| x.is_empty()))
            {
                // Requested 0 and we have 0.
                (None, None)
            } else {
                log::trace!("Waiting for either new streams or i/o on existing streams");
                tokio::select! {
                    recv_result = pending.recv() => (Some(recv_result), None),
                    next_result = state.streams.next() => (None, Some(next_result))
                }
            };

            // Restore pending back in our state before looping or returning.
            state.pending = Some(pending);

            // Handle whichever event became available.
            if let Some(recv_result) = recv_event {
                match recv_result {
                    Some(stream) => state.streams.push(stream),
                    None => todo!(),
                }
            } else if let Some(next_result) = next_event {
                match next_result {
                    Some(msg) => {
                        log::trace!("Buffering next message in read buffer: {msg:?}");
                        if let Err(e) = TurboCodec.encode(msg, &mut state.buffer) {
                            bail!("Turbo message encode error: {e}")
                        }

                        if state.buffer.len() >= len.start {
                            let end = state.buffer.len().min(len.end);
                            let bytes = state.buffer.split_to(end).freeze();
                            log::trace!(
                                "TurboSessionBroker returning {end} message bytes from read buffer"
                            );
                            return Ok(bytes);
                        }
                    }
                    None => {
                        // TODO: do we want to close if we don't see a new stream after a timeout?
                        // For now, we want to return 0 for PT mode.
                        if state.eof_on_empty {
                            bail!("EOF: no streams left")
                        }
                    }
                }
            } else {
                // Read 0 was requested and we have no data.
                log::trace!("Returning Ok(0 bytes)");
                return Ok(Bytes::new());
            }
        }
    }

    async fn read_frame<F, D>(&mut self, _deserializer: &mut D) -> anyhow::Result<F>
    where
        D: Deserializer<F> + Send,
    {
        log::trace!("read_frame() is called on TurboSessionBroker");
        unimplemented!()
    }
}

#[async_trait]
impl<R, W, C> Writer for SessionBroker<R, W, C>
where
    R: AsyncRead + Send + Unpin,
    W: AsyncWrite + Send + Unpin,
    C: AsyncConnect<R, W> + From<Socks5Target> + Send,
{
    async fn write_bytes(&mut self, bytes: &Bytes) -> anyhow::Result<usize> {
        log::trace!(
            "write_bytes() is called on TurboSessionBroker with {} bytes",
            bytes.len()
        );

        let mut state = self.shared_write_state.lock().await;

        // Append the incoming bytes to our write buffer.
        state.buffer.extend_from_slice(bytes);

        // Process all complete turbo messages from our buffer.
        loop {
            // Check if we have a full message in our buffer.
            let msg = {
                match TurboCodec.decode(&mut state.buffer) {
                    Ok(Some(msg)) => msg,
                    Ok(None) => break,
                    Err(e) => bail!("Turbo message decode error: {e}")
                }
            };

            log::trace!("Got new message to send to the sink: {msg:?}");

            while let Ok(sink) = state.pending.try_recv() {
                state.sinks.insert(sink.id(), sink);
            }

            // Process the message.
            if let Command::Request(Request::Open(target)) = &msg.command {
                log::debug!("Intercepted incoming connect message for {target}");
                if !state.sinks.contains_key(&msg.session_id) {
                    drop(state);
                    self.connect(msg.session_id, target.clone()).await;
                    state = self.shared_write_state.lock().await;
                }
                // TODO: pass the Connect frame to the TurboSink in case it wants to reply?
                continue;
            }

            if let Some(sink) = state.sinks.get_mut(&msg.session_id) {
                log::trace!("Sending message to sink");
                // TODO: what if send failed?
                let _ = sink.send(msg).await;
            } else {
                log::debug!("No sink available for session {}, dropping message", msg.session_id);
            }
        }

        // We always report that we have processed all of the bytes given to us.
        log::trace!("Returning from write_bytes()");
        return Ok(bytes.len());
    }

    async fn write_frame<F, S>(&mut self, _serializer: &mut S, _frame: F) -> anyhow::Result<usize>
    where
        S: Serializer<F> + Send,
        F: Send,
    {
        log::trace!("write_frame() is called on TurboSessionBroker");
        unimplemented!()
    }

    async fn flush(&mut self) -> anyhow::Result<()> {
        log::trace!("flush() is called on TurboSessionBroker");
        let mut state = self.shared_write_state.lock().await;
        for sink in state.sinks.values_mut() {
            sink.flush().await?;
        }
        Ok(())
    }

    async fn shutdown(&mut self) -> anyhow::Result<()> {
        log::trace!("shutdown() is called on TurboSessionBroker");
        let mut state = self.shared_write_state.lock().await;
        for sink in state.sinks.values_mut() {
            sink.close().await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    // TODO
}
