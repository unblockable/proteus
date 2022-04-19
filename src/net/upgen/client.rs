use async_trait::async_trait;

use crate::net::{
    upgen::{self, upgen_protocol::*},
    Connection,
};

impl InitializationState for UpgenProtocol<Initialization> {
    fn new(client_conn: Connection, bridge_conn: Connection) -> UpgenProtocol<Initialization> {
        Initialization {
            client_conn,
            bridge_conn,
        }
        .into()
    }

    fn start(self) -> UpgenProtocol<ClientHandshake> {
        ClientHandshake {
            client_conn: self.state.client_conn,
            bridge_conn: self.state.bridge_conn,
        }
        .into()
    }
}

#[async_trait]
impl ClientHandshakeState for UpgenProtocol<ClientHandshake> {
    async fn request(mut self) -> ClientHandshakeResult {
        todo!()
    }
}

#[async_trait]
impl ServerHandshakeState for UpgenProtocol<ServerHandshake> {
    async fn response(mut self) -> ServerHandshakeResult {
        todo!()
    }
}

#[async_trait]
impl DataState for UpgenProtocol<Data> {
    async fn data(mut self) -> DataResult {
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
    todo!()
}
