pub mod broker;
mod codec;
mod message;
mod session;
pub mod tunnel;

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::ops::Range;

    use anyhow::bail;
    use async_trait::async_trait;
    use bytes::{Bytes, BytesMut};
    use tokio::io::{AsyncRead, AsyncReadExt, DuplexStream};

    use crate::common::mock::tests::NullSpec;
    use crate::common::mock::{self, MockConnection};
    use crate::lang::ir::bridge::TaskProvider;
    use crate::net::proto::socks::address::{Socks5Address, Socks5Target};
    use crate::net::proto::turbo::broker::SessionBroker;
    use crate::net::{AsyncConnect, Deserializer, Reader, Writer};

    #[async_trait]
    impl Reader for DuplexStream {
        async fn read_bytes(&mut self, _len: Range<usize>) -> anyhow::Result<Bytes> {
            let mut buf = BytesMut::new();
            self.read_buf(&mut buf).await?;
            Ok(buf.freeze())
        }

        async fn read_frame<F, D>(&mut self, _deserializer: &mut D) -> anyhow::Result<F>
        where
            D: Deserializer<F> + Send,
        {
            unimplemented!()
        }
    }

    pub struct MockConnector {}

    #[async_trait]
    impl AsyncConnect<DuplexStream, DuplexStream> for MockConnector {
        async fn connect(&self) -> anyhow::Result<(DuplexStream, DuplexStream, SocketAddr)> {
            unimplemented!()
        }
    }

    impl From<Socks5Target> for MockConnector {
        fn from(_: Socks5Target) -> Self {
            Self {}
        }
    }

    impl AsyncRead for MockConnector {
        fn poll_read(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
            _buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            todo!()
        }
    }

    async fn forward_app_to_net(mut src: impl Reader, mut dst: impl Writer) -> anyhow::Result<u64> {
        loop {
            // This will propagate EOF for us.
            let mut buf = src.read_bytes(1..usize::MAX).await?;
            dst.write_bytes(&mut buf).await?;
        }
    }

    async fn forward_net_to_app(mut src: impl Reader, mut dst: impl Writer) -> anyhow::Result<u64> {
        let mut total = 0;
        loop {
            let buf = src.read_bytes(1..usize::MAX).await?;

            if buf.len() > 0 {
                // Successfully read some bytes.
                total += buf.len();
                dst.write_bytes(&buf).await?;
            } else if buf.len() == 0 {
                // Read EOF, let's return to propagate it.
                return Ok(total as u64);
            } else {
                // Some other IO error.
                bail!("IO Error")
            }
        }
    }

    async fn run_session_copier<T: TaskProvider + Send>(
        _: T,
        net_conn: MockConnection,
        app_conn: MockConnection,
        is_client: bool,
    ) -> anyhow::Result<()> {
        // Unwrap the Connection and BufReader.
        let (net_r, net_w) = net_conn.into_split();
        let net_r = net_r.into_inner();

        // Make a session from the app connection.
        let (src, dst) = app_conn.into_split();
        let src = src.into_inner();

        let session_id = 12345;

        let broker: SessionBroker<DuplexStream, DuplexStream, MockConnector> = if is_client {
            let mut broker = SessionBroker::new_pt_client();
            broker
                .add_session_client_test(
                    src,
                    dst,
                    session_id,
                    Socks5Target::new(Socks5Address::Unknown, 0),
                )
                .await;
            broker
        } else {
            let mock_target = Socks5Target::new(Socks5Address::Unknown, 0);
            let mut broker = SessionBroker::new_pt_server(mock_target);
            broker.add_session_server_test(src, dst, session_id).await;
            broker
        };

        // We need to move the streams into `forward()` so that the DuplexStreams close
        // when the tokio::io::copy function receives EOF and returns. Otherwise the EOF
        // does not properly propagate backward.
        let (_, _) = tokio::join!(
            forward_app_to_net(broker.clone(), net_w),
            forward_net_to_app(net_r, broker)
        );

        Ok(())
    }

    #[tokio::test]
    async fn session() {
        let _ = env_logger::try_init();
        for len in mock::tests::payload_len_iter() {
            let result =
                mock::run_proxy_network(NullSpec {}, NullSpec {}, &run_session_copier, len).await;
            mock::tests::assert_mock_result(result, len)
        }
    }
}
