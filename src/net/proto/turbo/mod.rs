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
    use crate::common::mock;
    use crate::net::proto::TurboSession;
    use crate::net::tunnel::tests::{
        MockIoKind, proxy_network_connected, proxy_network_disconnected,
    };

    #[tokio::test]
    async fn connected_direct_tunnel() {
        // let _ = env_logger::try_init();
        for len in mock::payload_len_iter() {
            proxy_network_connected::<TurboSession<_, _>>(MockIoKind::Direct, len)
                .await
                .assert(len);
        }
    }

    #[tokio::test]
    async fn connected_interpreter_tunnel() {
        // let _ = env_logger::try_init();
        for len in mock::payload_len_iter() {
            proxy_network_connected::<TurboSession<_, _>>(MockIoKind::Interpreter, len)
                .await
                .assert(len);
        }
    }

    #[tokio::test]
    async fn disconnected_direct_tunnel() {
        // let _ = env_logger::try_init();
        for len in mock::payload_len_iter() {
            proxy_network_disconnected::<TurboSession<_, _>>(MockIoKind::Direct, len)
                .await
                .assert(len);
        }
    }

    #[tokio::test]
    async fn disconnected_interpreter_tunnel() {
        // let _ = env_logger::try_init();
        for len in mock::payload_len_iter() {
            proxy_network_disconnected::<TurboSession<_, _>>(MockIoKind::Interpreter, len)
                .await
                .assert(len);
        }
    }
}
