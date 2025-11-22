use std::future::Future;
use std::io;

use anyhow::anyhow;
use bytes::Bytes;
use rand::distributions::{Alphanumeric, DistString};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::lang::interpreter::Interpreter;
use crate::lang::ir::bridge::TaskProvider;
use crate::net::READ_CAPACITY;

pub fn simplex(max_buf_size: usize) -> (impl AsyncRead, impl AsyncWrite) {
    tokio::io::simplex(max_buf_size)
}

pub fn duplex(max_buf_size: usize) -> (impl AsyncRead + AsyncWrite, impl AsyncRead + AsyncWrite) {
    tokio::io::duplex(max_buf_size)
}

pub fn payload_len_iter() -> impl Iterator<Item = usize> {
    [
        1, 10, 100, 1000, 1500, 2000, 5000, 10_000, 100_000, 1_000_000,
    ]
    .into_iter()
}

pub fn payload(len: usize) -> Bytes {
    let mut rng = rand::thread_rng();
    let s = Alphanumeric.sample_string(&mut rng, len);
    Bytes::from(s)
}

pub struct Result {
    pub c_app_src: io::Result<Bytes>,
    pub c_app_dst: io::Result<Bytes>,
    pub c_app_to_net: anyhow::Result<()>,
    pub c_net_to_app: anyhow::Result<()>,
    pub s_app_to_net: anyhow::Result<()>,
    pub s_net_to_app: anyhow::Result<()>,
    pub s_app_src: io::Result<Bytes>,
    pub s_app_dst: io::Result<Bytes>,
}

impl Result {
    fn assert(&self, len: usize) {
        self.assert_success();

        let c_src = self.c_app_src.as_ref().unwrap();
        let s_dst = self.s_app_dst.as_ref().unwrap();
        Self::assert_payload(c_src, s_dst, len);

        let s_src = self.s_app_src.as_ref().unwrap();
        let c_dst = self.c_app_dst.as_ref().unwrap();
        Self::assert_payload(s_src, c_dst, len);
    }

    fn assert_success(&self) {
        assert!(self.c_app_src.is_ok());
        assert!(self.c_app_dst.is_ok());
        assert!(self.c_app_to_net.is_ok());
        assert!(self.c_net_to_app.is_ok());
        assert!(self.s_app_to_net.is_ok());
        assert!(self.s_net_to_app.is_ok());
        assert!(self.s_app_src.is_ok());
        assert!(self.s_app_dst.is_ok());
    }

    fn assert_payload(a: &Bytes, b: &Bytes, len: usize) {
        if len > 0 {
            assert!(!a.is_empty());
            assert!(!b.is_empty());
        }
        assert_eq!(a.len(), len);
        assert_eq!(b.len(), len);
        assert_eq!(&a[..], &b[..]);
    }
}

pub struct MockIo {
    reader: Box<dyn AsyncRead + Unpin>,
    writer: Box<dyn AsyncWrite + Unpin>,
}

impl MockIo {
    fn new(
        reader: impl AsyncRead + Unpin + 'static,
        writer: impl AsyncWrite + Unpin + 'static,
    ) -> Self {
        Self {
            reader: Box::new(reader),
            writer: Box::new(writer),
        }
    }

    fn new_split(rw: impl AsyncRead + AsyncWrite + Unpin + 'static) -> Self {
        let (r, w) = tokio::io::split(rw);
        Self::new(r, w)
    }

    fn new_pair() -> (Self, Self) {
        let (io_rw_1, io_rw_2) = duplex(READ_CAPACITY);
        (Self::new_split(io_rw_1), Self::new_split(io_rw_2))
    }
}

struct MockApplication {
    payload: Bytes,
    io: MockIo,
}

impl MockApplication {
    fn new(payload_len: usize, io: MockIo) -> Self {
        let payload = payload(payload_len);
        Self { payload, io }
    }
}

pub struct MockProxy {
    app: MockIo,
    net: MockIo,
}

impl MockProxy {
    fn new(app: MockIo, net: MockIo) -> Self {
        Self { app, net }
    }
}

/// A mock proxy network that represents the following:
/// c_app <--> c_proxy <--> s_proxy <--> s_app
pub struct MockProxyNetwork {
    c_app: MockApplication,
    c_proxy: MockProxy,
    s_proxy: MockProxy,
    s_app: MockApplication,
}

impl MockProxyNetwork {
    fn new(payload_len: usize) -> Self {
        let (c_app, c_proxy_to_app) = MockIo::new_pair();
        let (c_proxy_to_net, s_proxy_to_net) = MockIo::new_pair();
        let (s_app, s_proxy_to_app) = MockIo::new_pair();
        Self {
            c_app: MockApplication::new(payload_len, c_app),
            c_proxy: MockProxy::new(c_proxy_to_app, c_proxy_to_net),
            s_proxy: MockProxy::new(s_proxy_to_app, s_proxy_to_net),
            s_app: MockApplication::new(payload_len, s_app),
        }
    }

    async fn run_direct_io(self) -> self::Result {
        self.run_with_forwarder(None::<u8>, &io_copy_direct, None::<u8>, &io_copy_direct)
            .await
    }

    async fn run_with_forwarder<C, FC, S, FS, Fut>(
        self,
        client: C,
        client_fwd: FC,
        server: S,
        server_fwd: FS,
    ) -> self::Result
    where
        FC: Fn(C, MockProxy) -> Fut,
        FS: Fn(S, MockProxy) -> Fut,
        Fut: Future<Output = (anyhow::Result<()>, anyhow::Result<()>)>,
    {
        let results = tokio::join!(
            // Run the client-side app tasks.
            stream_then_shutdown(self.c_app.io.writer, self.c_app.payload),
            sink_until_eof(self.c_app.io.reader),
            // Run the client-side proxy tasks.
            client_fwd(client, self.c_proxy),
            // Run the server-side proxy tasks.
            server_fwd(server, self.s_proxy),
            // Run the server-side app tasks.
            stream_then_shutdown(self.s_app.io.writer, self.s_app.payload),
            sink_until_eof(self.s_app.io.reader),
        );

        self::Result {
            c_app_src: results.0,
            c_app_dst: results.1,
            c_app_to_net: results.2.0,
            c_net_to_app: results.2.1,
            s_app_to_net: results.3.0,
            s_net_to_app: results.3.1,
            s_app_src: results.4,
            s_app_dst: results.5,
        }
    }
}

async fn stream_then_shutdown<W>(mut writer: W, payload: Bytes) -> io::Result<Bytes>
where
    W: AsyncWrite + Unpin,
{
    writer.write_all(&payload[..]).await?;
    writer.shutdown().await?; // Signal EOF to the connected read side
    Ok(payload)
}

async fn sink_until_eof<R>(mut reader: R) -> io::Result<Bytes>
where
    R: AsyncRead + Unpin,
{
    let mut buf = Vec::new();
    let _n_bytes = reader.read_to_end(&mut buf).await?;
    Ok(Bytes::from(buf))
}

/// Like `tokio::io::copy()` but takes ownership of the reader and writer,
/// shuts down the writer when the reader gets an EOF to make sure it
/// propagates backward, and drops the reader and writer on return.
async fn copy_then_shutdown<R, W>(mut src: R, mut dst: W) -> io::Result<usize>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let n_bytes = tokio::io::copy(&mut src, &mut dst).await?;
    dst.shutdown().await?; // Signal EOF to the connected read side
    Ok(n_bytes as usize)
}

async fn io_copy_direct(
    _: Option<u8>,
    proxy: MockProxy,
) -> (anyhow::Result<()>, anyhow::Result<()>) {
    // Note: we MUST moved the streams so they are dropped when the copy completes.
    // Use the `mock::copy()` function. This ensures that when the copy completes,
    // the underlying streams are dropped, and the EOF correctly propagates backwards.
    let (app_to_net, net_to_app) = tokio::join!(
        copy_then_shutdown(proxy.app.reader, proxy.net.writer),
        copy_then_shutdown(proxy.net.reader, proxy.app.writer),
    );
    // Discard the count of bytes copied on Ok.
    (
        app_to_net.map(|_| ()).map_err(|e| anyhow!(e)),
        net_to_app.map(|_| ()).map_err(|e| anyhow!(e)),
    )
}

async fn io_copy_interpreter<T: TaskProvider + Clone + Send>(
    protospec: T,
    proxy: MockProxy,
) -> (anyhow::Result<()>, anyhow::Result<()>) {
    Interpreter::run(
        proxy.net.reader,
        proxy.net.writer,
        proxy.app.reader,
        proxy.app.writer,
        protospec,
    )
    .await
}

pub async fn check_protocol_interpretability<T: TaskProvider + Clone + Send>(
    client: T,
    server: T,
    payload_len: usize,
) -> self::Result {
    MockProxyNetwork::new(payload_len)
        .run_with_forwarder(client, &io_copy_interpreter, server, &io_copy_interpreter)
        .await
}

pub async fn test_protocol_interpretability<T>(client: T, server: T)
where
    T: TaskProvider + Clone + Send,
{
    for len in payload_len_iter() {
        check_protocol_interpretability(client.clone(), server.clone(), len)
            .await
            .assert(len);
    }
}

#[cfg(test)]
pub mod tests {
    use crate::common::mock;

    use super::*;

    #[tokio::test]
    async fn duplex_split() {
        for len in payload_len_iter() {
            let (client, server) = MockIo::new_pair();
            let client = MockApplication::new(len, client);
            let server = MockApplication::new(len, server);

            let results = tokio::join!(
                // Run the client-side app tasks.
                stream_then_shutdown(client.io.writer, client.payload),
                sink_until_eof(client.io.reader),
                stream_then_shutdown(server.io.writer, server.payload),
                sink_until_eof(server.io.reader),
            );

            mock::Result::assert_payload(&results.0.unwrap(), &results.3.unwrap(), len);
            mock::Result::assert_payload(&results.1.unwrap(), &results.2.unwrap(), len);
        }
    }

    #[tokio::test]
    async fn duplex_split_copy() {
        for len in payload_len_iter() {
            let (client, proxy_to_client) = MockIo::new_pair();
            let (server, proxy_to_server) = MockIo::new_pair();

            let client = MockApplication::new(len, client);
            let server = MockApplication::new(len, server);
            let proxy = MockProxy::new(proxy_to_client, proxy_to_server);

            let results = tokio::join!(
                // Run the client-side app tasks.
                stream_then_shutdown(client.io.writer, client.payload),
                sink_until_eof(client.io.reader),
                stream_then_shutdown(server.io.writer, server.payload),
                sink_until_eof(server.io.reader),
                copy_then_shutdown(proxy.app.reader, proxy.net.writer),
                copy_then_shutdown(proxy.net.reader, proxy.app.writer),
            );

            mock::Result::assert_payload(&results.0.unwrap(), &results.3.unwrap(), len);
            mock::Result::assert_payload(&results.1.unwrap(), &results.2.unwrap(), len);
        }
    }

    #[tokio::test]
    async fn duplex_split_copy_copy() {
        // let _ = env_logger::try_init();
        for len in payload_len_iter() {
            let (c_app, c_proxy_to_app) = MockIo::new_pair();
            let (c_proxy_to_net, s_proxy_to_net) = MockIo::new_pair();
            let (s_app, s_proxy_to_app) = MockIo::new_pair();

            let c_app = MockApplication::new(len, c_app);
            let c_proxy = MockProxy::new(c_proxy_to_app, c_proxy_to_net);
            let s_proxy = MockProxy::new(s_proxy_to_app, s_proxy_to_net);
            let s_app = MockApplication::new(len, s_app);

            let results = tokio::join!(
                // Run the client-side app tasks.
                stream_then_shutdown(c_app.io.writer, c_app.payload),
                sink_until_eof(c_app.io.reader),
                stream_then_shutdown(s_app.io.writer, s_app.payload),
                sink_until_eof(s_app.io.reader),
                copy_then_shutdown(c_proxy.app.reader, c_proxy.net.writer),
                copy_then_shutdown(c_proxy.net.reader, c_proxy.app.writer),
                copy_then_shutdown(s_proxy.app.reader, s_proxy.net.writer),
                copy_then_shutdown(s_proxy.net.reader, s_proxy.app.writer),
            );

            mock::Result::assert_payload(&results.0.unwrap(), &results.3.unwrap(), len);
            mock::Result::assert_payload(&results.1.unwrap(), &results.2.unwrap(), len);
        }
    }

    #[tokio::test]
    async fn proxy_network_direct_io() {
        for len in payload_len_iter() {
            MockProxyNetwork::new(len).run_direct_io().await.assert(len);
        }
    }
}
