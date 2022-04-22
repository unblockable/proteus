use async_trait::async_trait;

use crate::net::{
    proto::upgen::{
        self,
        generator::{OvertFrameType, OvertProtocolSpec},
        spec::{self, upgen::*},
    },
    Connection,
};

impl InitState for UpgenProtocol<Init> {
    fn new(
        client_conn: Connection,
        server_conn: Connection,
        seed: u64,
    ) -> UpgenProtocol<Init> {
        Init {
            client_conn,
            server_conn,
            spec: OvertProtocolSpec::new(seed),
        }
        .into()
    }

    fn start_client(self) -> UpgenProtocol<ClientHandshake1> {
        ClientHandshake1 {
            client_conn: self.state.client_conn,
            server_conn: self.state.server_conn,
            spec: self.state.spec,
        }
        .into()
    }

    fn start_server(self) -> UpgenProtocol<ServerHandshake1> {
        ServerHandshake1 {
            client_conn: self.state.client_conn,
            server_conn: self.state.server_conn,
            spec: self.state.spec,
        }
        .into()
    }
}

#[async_trait]
impl ClientHandshake1State for UpgenProtocol<ClientHandshake1> {
    async fn send_handshake1(mut self) -> ClientHandshake1Result {
        let frame_fmt = self.state.spec.get_frame_fmt(OvertFrameType::Handshake1);

        match self.state.server_conn.write_frame_fmt(frame_fmt).await {
            Ok(_) => {
                let next = ClientHandshake2 {
                    client_conn: self.state.client_conn,
                    server_conn: self.state.server_conn,
                    spec: self.state.spec,
                };
                ClientHandshake1Result::ClientHandshake2(next.into())
            }
            Err(net_err) => {
                let error = upgen::Error::from(net_err);
                let next = spec::upgen::Error { error };
                ClientHandshake1Result::Error(next.into())
            }
        }
    }
}

#[async_trait]
impl ClientHandshake2State for UpgenProtocol<ClientHandshake2> {
    async fn recv_handshake2(mut self) -> ClientHandshake2Result {
        let frame_fmt = self.state.spec.get_frame_fmt(OvertFrameType::Handshake2);

        match self.state.server_conn.read_frame_fmt(frame_fmt).await {
            Ok(bytes) => match bytes.len() {
                0 => {
                    // tor_conn here is the browser
                    let next = Data {
                        upgen_conn: self.state.server_conn,
                        tor_conn: self.state.client_conn,
                        spec: self.state.spec,
                    };
                    ClientHandshake2Result::Data(next.into())
                }
                _ => {
                    let error = upgen::Error::ClientHandshake(String::from(
                        "Unexpectedly received non-zero payload",
                    ));
                    let next = spec::upgen::Error { error };
                    ClientHandshake2Result::Error(next.into())
                }
            },
            Err(net_err) => {
                let error = upgen::Error::from(net_err);
                let next = spec::upgen::Error { error };
                ClientHandshake2Result::Error(next.into())
            }
        }
    }
}

#[async_trait]
impl ServerHandshake1State for UpgenProtocol<ServerHandshake1> {
    async fn recv_handshake1(mut self) -> ServerHandshake1Result {
        let frame_fmt = self.state.spec.get_frame_fmt(OvertFrameType::Handshake1);

        match self.state.client_conn.read_frame_fmt(frame_fmt).await {
            Ok(bytes) => match bytes.len() {
                0 => {
                    let next = ServerHandshake2 {
                        client_conn: self.state.client_conn,
                        server_conn: self.state.server_conn,
                        spec: self.state.spec,
                    };
                    ServerHandshake1Result::ServerHandshake2(next.into())
                }
                _ => {
                    let error = upgen::Error::ServerHandshake(String::from(
                        "Unexpectedly received non-zero payload",
                    ));
                    let next = spec::upgen::Error { error };
                    ServerHandshake1Result::Error(next.into())
                }
            },
            Err(net_err) => {
                let error = upgen::Error::from(net_err);
                let next = spec::upgen::Error { error };
                ServerHandshake1Result::Error(next.into())
            }
        }
    }
}

#[async_trait]
impl ServerHandshake2State for UpgenProtocol<ServerHandshake2> {
    async fn send_handshake2(mut self) -> ServerHandshake2Result {
        let frame_fmt = self.state.spec.get_frame_fmt(OvertFrameType::Handshake2);

        match self.state.client_conn.write_frame_fmt(frame_fmt).await {
            Ok(_) => {
                // tor_conn here is the bridge
                let next = Data {
                    upgen_conn: self.state.client_conn,
                    tor_conn: self.state.server_conn,
                    spec: self.state.spec,
                };
                ServerHandshake2Result::Data(next.into())
            }
            Err(net_err) => {
                let error = upgen::Error::from(net_err);
                let next = spec::upgen::Error { error };
                ServerHandshake2Result::Error(next.into())
            }
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
