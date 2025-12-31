mod codec;
mod message;
mod sink;
mod state;
mod stream;

use std::marker::PhantomData;

use tokio::io::{AsyncRead, AsyncWrite};

use crate::common::sync::PollMutex;
use crate::net::proto::tunnel::message::TunnelMessage;
use crate::net::proto::turbo::sink::TurboSink;
use crate::net::proto::turbo::state::TurboState;
use crate::net::proto::turbo::stream::TurboStream;
use crate::net::session::SessionBuilder;

pub struct TurboSession<R: AsyncRead + Send + Unpin, W: AsyncWrite + Send + Unpin> {
    _r: PhantomData<R>,
    _w: PhantomData<W>,
}

impl<R, W> SessionBuilder for TurboSession<R, W>
where
    R: AsyncRead + Send + Unpin,
    W: AsyncWrite + Send + Unpin,
{
    type Message = TunnelMessage;
    type ReadHalf = R;
    type WriteHalf = W;
    type StreamHalf = TurboStream<R>;
    type SinkHalf = TurboSink<W>;

    fn build(
        id: u64,
        src: Self::ReadHalf,
        dst: Self::WriteHalf,
    ) -> (Self::StreamHalf, Self::SinkHalf) {
        let shared_state = PollMutex::new(TurboState::default());

        let stream = TurboStream::new(id, shared_state.clone(), src);
        let sink = TurboSink::new(shared_state, dst);

        (stream, sink)
    }
}

#[cfg(test)]
mod tests {
    // use std::net::SocketAddr;

    // use async_trait::async_trait;
    // use tokio::io::{AsyncRead, DuplexStream};

    // use crate::lang::ir::bridge::TaskProvider;
    // use crate::net::proto::socks::address::{Socks5Address, Socks5Target};
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
