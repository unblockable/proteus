use rand::RngCore;
use rand::rngs::ThreadRng;
use tokio::sync::mpsc;

use crate::net::proto::turbo::session::{SharedTurboState, TurboReader, TurboWriter};
use crate::net::{Connection, Reader, Writer};

mod formatter;
mod frames;
mod session;

fn generate_session_id() -> u64 {
    loop {
        let id = ThreadRng::default().next_u64();
        if id > 0 {
            return id;
        }
    }
}

pub struct TurboSession {}

impl TurboSession {
    fn new_split<R: Reader + Send, W: Writer + Send>(
        app_conn: Connection<R, W>,
        id: Option<u64>,
    ) -> (TurboReader<R>, TurboWriter<W>) {
        let (app_src, app_dst) = app_conn.into_split();
        let state = SharedTurboState::new(id);
        let (sender, receiver) = mpsc::unbounded_channel();

        let reader = TurboReader::new(app_src, state.clone(), receiver);
        let writer = TurboWriter::new(app_dst, state, sender);

        (reader, writer)
    }

    /// A client session is a singular connection to an application, after the preliminary
    /// handshake protocol (e.g., SOCKS) is completed. The app connection should be
    /// in a state where it expects us to forward raw data to a target network peer.
    pub fn new_connected_client<R: Reader + Send, W: Writer + Send>(
        app_conn: Connection<R, W>,
    ) -> (TurboReader<R>, TurboWriter<W>) {
        let id = generate_session_id();
        TurboSession::new_split(app_conn, Some(id))
    }

    pub fn new_connected_server<R: Reader + Send, W: Writer + Send>(
        app_conn: Connection<R, W>,
    ) -> (TurboReader<R>, TurboWriter<W>) {
        TurboSession::new_split(app_conn, None)
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Range;

    use anyhow::bail;
    use async_trait::async_trait;
    use bytes::{Bytes, BytesMut};
    use tokio::io::{AsyncReadExt, DuplexStream};

    use crate::common::mock::tests::NullSpec;
    use crate::common::mock::{self, MockConnection};
    use crate::lang::ir::bridge::TaskProvider;
    use crate::net::proto::turbo::TurboSession;
    use crate::net::{Deserializer, Reader, Writer};

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
        let (app_r, app_w) = if is_client {
            TurboSession::new_connected_client(app_conn)
        } else {
            TurboSession::new_connected_server(app_conn)
        };

        // We need to move the streams into `forward()` so that the DuplexStreams close
        // when the tokio::io::copy function receives EOF and returns. Otherwise the EOF
        // does not properly propagate backward.
        let (_, _) = tokio::join!(
            forward_app_to_net(app_r, net_w),
            forward_net_to_app(net_r, app_w)
        );

        Ok(())
    }

    #[tokio::test]
    async fn session() {
        for len in mock::tests::payload_len_iter() {
            let result =
                mock::run_proxy_network(NullSpec {}, NullSpec {}, &run_session_copier, len).await;
            mock::tests::assert_mock_result(result, len)
        }
    }
}
