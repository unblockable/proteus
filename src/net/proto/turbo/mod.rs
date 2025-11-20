mod codec;
mod message;
mod session;
mod tunnel;

use session::{TurboSession, TurboSink, TurboStream};
use codec::TurboCodec;
use message::TurboMessage;

pub use tunnel::TurboTunnel;

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    // use async_trait::async_trait;
    use tokio::io::{AsyncRead, DuplexStream};

    use crate::common::mock::tests::NullSpec;
    use crate::common::mock::{self, MockConnection};
    use crate::lang::ir::bridge::TaskProvider;
    use crate::net::proto::socks::address::{Socks5Address, Socks5Target};
    // use crate::net::proto::turbo::broker::SessionBroker;

    // pub struct MockConnector {}

    // impl From<Socks5Target> for MockConnector {
    //     fn from(_: Socks5Target) -> Self {
    //         Self {}
    //     }
    // }

    // impl AsyncRead for MockConnector {
    //     fn poll_read(
    //         self: std::pin::Pin<&mut Self>,
    //         _cx: &mut std::task::Context<'_>,
    //         _buf: &mut tokio::io::ReadBuf<'_>,
    //     ) -> std::task::Poll<std::io::Result<()>> {
    //         todo!()
    //     }
    // }

    // async fn run_session_copier<T: TaskProvider + Send>(
    //     _: T,
    //     net_conn: MockConnection,
    //     app_conn: MockConnection,
    //     is_client: bool,
    // ) -> anyhow::Result<()> {
    //     // Unwrap the Connection and BufReader.
    //     let (mut net_r, mut net_w) = net_conn;

    //     // Make a session from the app connection.
    //     let (src, dst) = app_conn;

    //     let session_id = 12345;

    //     let broker: SessionBroker<DuplexStream, DuplexStream, MockConnector> = if is_client {
    //         let mut broker = SessionBroker::new_pt_client();
    //         broker
    //             .add_session_client_test(
    //                 src,
    //                 dst,
    //                 session_id,
    //                 Socks5Target::new(Socks5Address::Unknown, 0),
    //             )
    //             .await;
    //         broker
    //     } else {
    //         let mock_target = Socks5Target::new(Socks5Address::Unknown, 0);
    //         let mut broker = SessionBroker::new_pt_server(mock_target);
    //         broker.add_session_server_test(src, dst, session_id).await;
    //         broker
    //     };

    //     // We need to move the streams into `forward()` so that the DuplexStreams close
    //     // when the tokio::io::copy function receives EOF and returns. Otherwise the EOF
    //     // does not properly propagate backward.
    //     let mut broker_r = broker.clone();
    //     let mut broker_w = broker;
    //     let (_, _) = tokio::join!(
    //         tokio::io::copy(&mut broker_r, &mut net_w),
    //         tokio::io::copy(&mut net_r, &mut broker_w)
    //     );

    //     Ok(())
    // }

    // #[tokio::test]
    // async fn session() {
    //     let _ = env_logger::try_init();
    //     for len in mock::tests::payload_len_iter() {
    //         let result =
    //             mock::run_proxy_network(NullSpec {}, NullSpec {}, &run_session_copier, len).await;
    //         mock::tests::assert_mock_result(result, len)
    //     }
    // }
}
