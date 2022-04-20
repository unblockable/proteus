use async_trait::async_trait;

use crate::net::{
    proto::upgen::{self, generator::OvertProtocolSpec, upgen_protocol::*},
    Connection,
};

impl InitState for UpgenProtocol<Init> {
    fn new(client_conn: Connection, server_conn: Connection) -> UpgenProtocol<Init> {
        Init {
            client_conn,
            server_conn,
        }
        .into()
    }

    fn start_client(self) -> UpgenProtocol<ClientHandshake1> {
        ClientHandshake1 {
            client_conn: self.state.client_conn,
            server_conn: self.state.server_conn,
        }
        .into()
    }

    fn start_server(self) -> UpgenProtocol<ServerHandshake1> {
        ServerHandshake1 {
            client_conn: self.state.client_conn,
            server_conn: self.state.server_conn,
        }
        .into()
    }
}

#[async_trait]
impl ClientHandshake1State for UpgenProtocol<ClientHandshake1> {
    async fn send_handshake1(mut self) -> ClientHandshake1Result {
        todo!()
    }
}

#[async_trait]
impl ClientHandshake2State for UpgenProtocol<ClientHandshake2> {
    async fn recv_handshake2(mut self) -> ClientHandshake2Result {
        todo!()
    }
}

#[async_trait]
impl ClientDataState for UpgenProtocol<ClientData> {
    async fn forward_data(mut self) -> ClientDataResult {
        todo!()
    }
}

#[async_trait]
impl ServerHandshake1State for UpgenProtocol<ServerHandshake1> {
    async fn recv_handshake1(mut self) -> ServerHandshake1Result {
        todo!()
    }
}

#[async_trait]
impl ServerHandshake2State for UpgenProtocol<ServerHandshake2> {
    async fn send_handshake2(mut self) -> ServerHandshake2Result {
        todo!()
    }
}

#[async_trait]
impl ServerDataState for UpgenProtocol<ServerData> {
    async fn forward_data(mut self) -> ServerDataResult {
        todo!()
    }
}

impl SuccessState for UpgenProtocol<Success> {
    fn finish(self) {
        // close connections
    }
}

impl ErrorState for UpgenProtocol<Error> {
    fn finish(self) -> upgen::Error {
        self.state.error
    }
}

pub async fn run_protocol(
    client_conn: Connection,
    bridge_conn: Connection,
) -> Result<(), upgen::Error> {
    let spec = OvertProtocolSpec::new(123456);
    todo!()
}
