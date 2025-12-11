use std::fmt::Debug;
use std::io;
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, ready};

use futures::FutureExt;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::oneshot::Receiver;
use tokio::sync::oneshot::error::RecvError;
use tokio::sync::{Notify, oneshot};

use crate::common::sync::PollMutex;
use crate::net::AsyncConnectExt;
use crate::net::proto::socks::address::Socks5Target;

pub struct Channel<R, W>
where
    R: AsyncRead + Send + Unpin,
    W: AsyncWrite + Send + Unpin,
{
    reader: PollMutex<ChannelIo<R>>,
    writer: PollMutex<ChannelIo<W>>,
}

enum ChannelIo<T> {
    Connected((T, String)),
    Disconnected((Arc<Notify>, Receiver<io::Result<(T, String)>>)),
    Error(io::Error),
}

impl<T> ChannelIo<T> {
    fn poll_connected(&mut self, cx: &mut Context) -> Poll<()> {
        match self {
            ChannelIo::Connected(_) => Poll::Ready(()),
            ChannelIo::Disconnected((notify, conn_rx)) => {
                // Notify the background task to connect if it didn't already.
                notify.notify_one();

                // Check for a connection result.
                match conn_rx.poll_unpin(cx) {
                    Poll::Ready(rx_result) => {
                        *self = match rx_result {
                            // Connection was successful.
                            Ok(Ok((io, name))) => ChannelIo::Connected((io, name)),
                            // Connection error.
                            Ok(Err(e)) => ChannelIo::Error(e),
                            // Error getting result from the receiver channel.
                            Err(e) => ChannelIo::Error(broken_pipe_error(e)),
                        };
                        Poll::Ready(())
                    }
                    Poll::Pending => Poll::Pending,
                }
            }
            ChannelIo::Error(_) => Poll::Ready(()),
        }
    }

    fn inspect_err<U>(&mut self, result: Poll<Result<U, io::Error>>) -> Poll<Result<U, io::Error>> {
        if let Poll::Ready(Err(e)) = &result {
            *self = ChannelIo::Error(io::Error::from(e.kind()));
        }
        result
    }
}

impl<T> Debug for ChannelIo<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ChannelIo::Connected((_, name)) => write!(f, "Connected:{name}"),
            ChannelIo::Disconnected(_) => write!(f, "Disconnected"),
            ChannelIo::Error(_) => write!(f, "Error"),
        }
    }
}

impl<R, W> Channel<R, W>
where
    R: AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
{
    fn new(reader: ChannelIo<R>, writer: ChannelIo<W>) -> Self {
        Self {
            reader: PollMutex::new(reader),
            writer: PollMutex::new(writer),
        }
    }

    /// Create a disconnected channel that will connect to the given `peer` in the background,
    /// and then all reads and writes on this channel will be forwarded to the peer connection.
    pub fn disconnected<C>(peer: Socks5Target, mut connector: C) -> Self
    where
        C: AsyncConnectExt<ReadHalf = R, WriteHalf = W> + 'static,
    {
        let (read_tx, read_rx) = oneshot::channel();
        let (write_tx, write_rx) = oneshot::channel();

        let ready_to_connect = Arc::new(Notify::new());
        let connect_target = peer.clone();
        let ready = ready_to_connect.clone();

        // Spawn a background task to establish the connection and split the stream.
        tokio::spawn(async move {
            ready_to_connect.notified().await;

            let result = connector.connect(connect_target).await;

            match result {
                Ok((reader, writer, name)) => {
                    let _ = read_tx.send(Ok((reader, name.clone())));
                    let _ = write_tx.send(Ok((writer, name)));
                }
                Err(e) => {
                    let _ = read_tx.send(Err(io::Error::from(e.kind())));
                    let _ = write_tx.send(Err(e));
                }
            }
        });

        let r_io = ChannelIo::Disconnected((ready.clone(), read_rx));
        let w_io = ChannelIo::Disconnected((ready, write_rx));

        Self::new(r_io, w_io)
    }

    pub fn connected(net_src: R, net_dst: W, name: String) -> Self {
        Self::new(
            ChannelIo::Connected((net_src, name.clone())),
            ChannelIo::Connected((net_dst, name)),
        )
    }
}

impl<R, W> Clone for Channel<R, W>
where
    R: AsyncRead + Send + Unpin,
    W: AsyncWrite + Send + Unpin,
{
    fn clone(&self) -> Self {
        Self {
            reader: self.reader.clone(),
            writer: self.writer.clone(),
        }
    }
}

impl<R, W> AsyncRead for Channel<R, W>
where
    R: AsyncRead + Send + Unpin,
    W: AsyncWrite + Send + Unpin,
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &mut ReadBuf,
    ) -> Poll<io::Result<()>> {
        // Obtain the read lock before proceeding.
        let mut io = ready!(self.get_mut().reader.poll_lock(cx));

        // Wait until we finish connecting.
        ready!(io.poll_connected(cx));

        // Requested read length.
        let len = buf.remaining();

        let result = match io.deref_mut() {
            ChannelIo::Connected((reader, _name)) => {
                let result = Pin::new(reader).poll_read(cx, buf);
                io.inspect_err(result)
            }
            ChannelIo::Disconnected(_) => Poll::Pending,
            ChannelIo::Error(e) => Poll::Ready(Err(io::Error::from(e.kind()))),
        };

        // Actual amount read.
        let amt = len - buf.remaining();

        // Print nicely for logs.
        if matches!(result, Poll::Ready(Ok(()))) {
            log::trace!(
                "Channel({:?})::poll_read({len}) -> Ready(Ok({amt}))",
                io.deref()
            );
        } else {
            log::trace!("Channel({:?})::poll_read({len}) -> {result:?}", io.deref());
        }

        result
    }
}

impl<R, W> AsyncWrite for Channel<R, W>
where
    R: AsyncRead + Send + Unpin,
    W: AsyncWrite + Send + Unpin,
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        // Obtain the write lock before proceeding.
        let mut io = ready!(self.get_mut().writer.poll_lock(cx));

        // Wait until we finish connecting.
        ready!(io.poll_connected(cx));

        let result = match io.deref_mut() {
            ChannelIo::Connected((writer, _name)) => {
                let result = Pin::new(writer).poll_write(cx, buf);
                io.inspect_err(result)
            }
            ChannelIo::Disconnected(_) => Poll::Pending,
            ChannelIo::Error(e) => Poll::Ready(Err(io::Error::from(e.kind()))),
        };

        log::trace!(
            "Channel({:?})::poll_write({}) -> {result:?}",
            io.deref(),
            buf.len()
        );

        result
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        // Obtain the write lock before proceeding.
        let mut io = ready!(self.get_mut().writer.poll_lock(cx));

        // Wait until we finish connecting.
        ready!(io.poll_connected(cx));

        let result = match io.deref_mut() {
            ChannelIo::Connected((writer, _name)) => {
                let result = Pin::new(writer).poll_flush(cx);
                io.inspect_err(result)
            }
            ChannelIo::Disconnected(_) => Poll::Pending,
            ChannelIo::Error(e) => Poll::Ready(Err(io::Error::from(e.kind()))),
        };

        log::trace!("Channel({:?})::poll_flush() -> {result:?}", io.deref());

        result
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Result<(), io::Error>> {
        // Obtain the write lock before proceeding.
        let mut io = ready!(self.get_mut().writer.poll_lock(cx));

        // Wait until we finish connecting.
        ready!(io.poll_connected(cx));

        let result = match io.deref_mut() {
            ChannelIo::Connected((writer, _name)) => {
                let result = Pin::new(writer).poll_shutdown(cx);
                io.inspect_err(result)
            }
            ChannelIo::Disconnected(_) => Poll::Pending,
            ChannelIo::Error(e) => Poll::Ready(Err(io::Error::from(e.kind()))),
        };

        log::trace!("Channel({:?})::poll_shutdown() -> {result:?}", io.deref());

        result
    }
}

fn broken_pipe_error(e: RecvError) -> io::Error {
    io::Error::new(
        std::io::ErrorKind::BrokenPipe,
        format!("RecvError from connect pipe: {e}"),
    )
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::io::{AsyncRead, AsyncWrite};

    use crate::common::mock::{self, MockConnector, MockIo, MockProxy, MockProxyNetwork};
    use crate::lang::Role;
    use crate::lang::ir::bridge::TaskProvider;
    use crate::lang::ir::test::basic_enc::EncryptedLengthPayloadSpec;
    use crate::net::Channel;

    fn wrapped_proxy<R, W>(app_io: MockIo, net_io: Channel<R, W>) -> MockProxy
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        let (chan_r, chan_w) = (net_io.clone(), net_io);
        let wrapped_net = MockIo::new(chan_r, chan_w);
        MockProxy::new(app_io, wrapped_net)
    }

    /// Use direct io without an interpreter.
    async fn connected_channel_direct_io(
        _: Option<u8>,
        proxy: MockProxy,
    ) -> (anyhow::Result<()>, anyhow::Result<()>) {
        // Channel is already connected.
        let channel = Channel::connected(proxy.net.reader, proxy.net.writer, String::from("Test"));
        mock::io_copy_direct(None::<u8>, wrapped_proxy(proxy.app, channel)).await
    }

    #[tokio::test]
    async fn proxy_network_connected_channel_direct_io() {
        for len in mock::payload_len_iter() {
            MockProxyNetwork::new(len)
                .run_with_forwarder(
                    None::<u8>,
                    &connected_channel_direct_io,
                    None::<u8>,
                    &connected_channel_direct_io,
                )
                .await
                .assert(len);
        }
    }

    async fn disconnected_channel_direct_io_client(
        connector: MockConnector,
        proxy: MockProxy,
    ) -> (anyhow::Result<()>, anyhow::Result<()>) {
        // Client starts in a disconnected state.
        let channel = Channel::disconnected(MockConnector::default_target(), connector);
        // The channel should handle the connection transparently, so we can start io already.
        mock::io_copy_direct(None::<u8>, wrapped_proxy(proxy.app, channel)).await
    }

    async fn disconnected_channel_direct_io_server(
        replaced_net: MockIo,
        proxy: MockProxy,
    ) -> (anyhow::Result<()>, anyhow::Result<()>) {
        // The server is already connected using the remote socket from the client connection.
        let channel = Channel::connected(
            replaced_net.reader,
            replaced_net.writer,
            String::from("Test"),
        );
        mock::io_copy_direct(None::<u8>, wrapped_proxy(proxy.app, channel)).await
    }

    #[tokio::test]
    async fn proxy_network_disconnected_channel_direct_io() {
        for len in mock::payload_len_iter() {
            // Channel is not connected yet, we need to simulate a connection.
            let mut connector = MockConnector::new(Some(Duration::from_millis(10)));
            let new_net_server = connector.remote_socket().unwrap();

            MockProxyNetwork::new(len)
                .run_with_forwarder(
                    connector,
                    &disconnected_channel_direct_io_client,
                    new_net_server,
                    &disconnected_channel_direct_io_server,
                )
                .await
                .assert(len);
        }
    }

    /// Use io via an interpreter.
    async fn connected_channel_interpreter_io<T: TaskProvider + Clone + Send>(
        protospec: T,
        proxy: MockProxy,
    ) -> (anyhow::Result<()>, anyhow::Result<()>) {
        // Channel is already connected.
        let channel = Channel::connected(proxy.net.reader, proxy.net.writer, String::from("Test"));
        mock::io_copy_interpreter(protospec, wrapped_proxy(proxy.app, channel)).await
    }

    #[tokio::test]
    async fn proxy_network_connected_channel_interpreter_io() {
        for len in mock::payload_len_iter() {
            MockProxyNetwork::new(len)
                .run_with_forwarder(
                    EncryptedLengthPayloadSpec::new(Role::Client),
                    &connected_channel_interpreter_io,
                    EncryptedLengthPayloadSpec::new(Role::Server),
                    &connected_channel_interpreter_io,
                )
                .await
                .assert(len);
        }
    }

    async fn disconnected_channel_interpreter_io_client(
        connector: MockConnector,
        proxy: MockProxy,
    ) -> (anyhow::Result<()>, anyhow::Result<()>) {
        // Client starts in a disconnected state.
        let channel = Channel::disconnected(MockConnector::default_target(), connector);
        // The channel should handle the connection transparently, so we can start io already.
        mock::io_copy_interpreter(
            EncryptedLengthPayloadSpec::new(Role::Client),
            wrapped_proxy(proxy.app, channel),
        )
        .await
    }

    async fn disconnected_channel_interpreter_io_server(
        replaced_net: MockIo,
        proxy: MockProxy,
    ) -> (anyhow::Result<()>, anyhow::Result<()>) {
        // The server is already connected using the remote socket from the client connection.
        let channel = Channel::connected(
            replaced_net.reader,
            replaced_net.writer,
            String::from("Test"),
        );
        mock::io_copy_interpreter(
            EncryptedLengthPayloadSpec::new(Role::Server),
            wrapped_proxy(proxy.app, channel),
        )
        .await
    }

    #[tokio::test]
    async fn proxy_network_disconnected_channel_interpreter_io() {
        for len in mock::payload_len_iter() {
            // Channel is not connected yet, we need to simulate a connection.
            let mut connector = MockConnector::new(Some(Duration::from_millis(10)));
            let new_net_server = connector.remote_socket().unwrap();

            MockProxyNetwork::new(len)
                .run_with_forwarder(
                    connector,
                    &disconnected_channel_interpreter_io_client,
                    new_net_server,
                    &disconnected_channel_interpreter_io_server,
                )
                .await
                .assert(len);
        }
    }
}
