use async_trait::async_trait;

use crate::net::{
    proto::upgen::{
        self,
        formatter::Formatter,
        protocols::*,
        spec::{self, upgen::*},
    },
    Connection,
};

use super::message::OvertMessage;

impl InitState for UpgenProtocol<Init> {
    fn new(
        upgen_conn: Connection,
        other_conn: Connection,
        spec: OvertProtocol,
    ) -> UpgenProtocol<Init> {
        UpgenProtocol::<Init> {
            state: Init {
                upgen_conn,
                other_conn,
                fmt: Formatter::new(),
                spec,
            },
        }
    }

    fn start_client(self) -> UpgenProtocol<ClientHandshake1> {
        self.into()
    }

    fn start_server(self) -> UpgenProtocol<ServerHandshake1> {
        self.into()
    }
}

#[async_trait]
impl ClientHandshake1State for UpgenProtocol<ClientHandshake1> {
    async fn send_handshake1(mut self) -> ClientHandshake1Result {
        // Get frame spec we want to send at this phase.
        let frame_spec = match &self.state.spec {
            OvertProtocol::OneRtt(p) => p.get_frame_spec(onertt::ProtocolPhase::Handshake1),
        };

        // Tell the serializer the frame spec it should follow.
        self.state.fmt.set_frame_spec(frame_spec);

        // We don't send any payload yet.
        let msg = OvertMessage { payload: None };

        match self
            .state
            .upgen_conn
            .write_frame(&self.state.fmt, msg)
            .await
        {
            Ok(_) => ClientHandshake1Result::ClientHandshake2(self.into()),
            Err(net_err) => ClientHandshake1Result::Error(upgen::Error::from(net_err).into()),
        }
    }
}

#[async_trait]
impl ClientHandshake2State for UpgenProtocol<ClientHandshake2> {
    async fn recv_handshake2(mut self) -> ClientHandshake2Result {
        // Get frame spec we expect to receive at this phase.
        let frame_spec = match &self.state.spec {
            OvertProtocol::OneRtt(p) => p.get_frame_spec(onertt::ProtocolPhase::Handshake2),
        };

        // Tell the deserializer the frame spec it should expect.
        self.state.fmt.set_frame_spec(frame_spec);

        match self.state.upgen_conn.read_frame(&self.state.fmt).await {
            Ok(_) => ClientHandshake2Result::Data(self.into()),
            Err(net_err) => ClientHandshake2Result::Error(upgen::Error::from(net_err).into()),
        }
    }
}

#[async_trait]
impl ServerHandshake1State for UpgenProtocol<ServerHandshake1> {
    async fn recv_handshake1(mut self) -> ServerHandshake1Result {
        // Get frame spec we expect to receive at this phase.
        let frame_spec = match &self.state.spec {
            OvertProtocol::OneRtt(p) => p.get_frame_spec(onertt::ProtocolPhase::Handshake1),
        };

        // Tell the deserializer the frame spec it should expect.
        self.state.fmt.set_frame_spec(frame_spec);

        match self.state.upgen_conn.read_frame(&self.state.fmt).await {
            Ok(_) => ServerHandshake1Result::ServerHandshake2(self.into()),
            Err(net_err) => ServerHandshake1Result::Error(upgen::Error::from(net_err).into()),
        }
    }
}

#[async_trait]
impl ServerHandshake2State for UpgenProtocol<ServerHandshake2> {
    async fn send_handshake2(mut self) -> ServerHandshake2Result {
        // Get frame spec we expect to receive at this phase.
        let frame_spec = match &self.state.spec {
            OvertProtocol::OneRtt(p) => p.get_frame_spec(onertt::ProtocolPhase::Handshake2),
        };

        // Tell the deserializer the frame spec it should expect.
        self.state.fmt.set_frame_spec(frame_spec);

        // We don't send any payload yet.
        let msg = OvertMessage { payload: None };

        match self
            .state
            .upgen_conn
            .write_frame(&self.state.fmt, msg)
            .await
        {
            Ok(_) => ServerHandshake2Result::Data(self.into()),
            Err(net_err) => ServerHandshake2Result::Error(upgen::Error::from(net_err).into()),
        }
    }
}

impl UpgenProtocol<Data> {
    async fn helper(&self) {}
}

#[async_trait]
impl DataState for UpgenProtocol<Data> {
    async fn forward_data(mut self) -> DataResult {
        // read from tor, encrypt, send to upgen
        // read from upgen, decrypt, send to tor

        // Get frame spec we expect to receive at this phase.
        let frame_spec = match &self.state.spec {
            OvertProtocol::OneRtt(p) => p.get_frame_spec(onertt::ProtocolPhase::Data),
        };

        // Tell the deserializer the frame spec it should expect.
        self.state.fmt.set_frame_spec(frame_spec);

        self.helper();
        todo!()
    }
}

impl SuccessState for UpgenProtocol<Success> {
    fn finish(self) {
        log::debug!("UPGen protocol completed successfully");
    }
}

impl ErrorState for UpgenProtocol<spec::upgen::Error> {
    fn finish(self) -> upgen::Error {
        self.state.error
    }
}
