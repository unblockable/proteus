use async_trait::async_trait;

use crate::net::{
    proto::or::{self, frames::*, spec::extor::*},
    Connection,
};

impl InitState for ExtOrProtocol<Init> {
    fn new(conn: Connection) -> ExtOrProtocol<Init> {
        Init { conn }.into()
    }

    fn start_client(self) -> ExtOrProtocol<ClientHandshake1> {
        let conn = self.state.conn;
        ClientHandshake1 { conn }.into()
    }
}

#[async_trait]
impl ClientHandshake1State for ExtOrProtocol<ClientHandshake1> {
    async fn recv_greeting(mut self) -> ClientHandshake1Result {
        log::debug!("Waiting for greeting");

        match self.state.conn.read_frame::<Greeting>().await {
            Ok(greeting) => {
                let conn = self.state.conn;
                let next = ClientHandshake2 { conn, greeting };
                ClientHandshake1Result::ClientHandshake2(next.into())
            }
            Err(net_err) => {
                let error = or::Error::from(net_err);
                let next = Error { error };
                ClientHandshake1Result::Error(next.into())
            }
        }
    }
}

#[async_trait]
impl ClientHandshake2State for ExtOrProtocol<ClientHandshake2> {
    async fn send_choice(mut self) -> ClientHandshake2Result {
        let types = self.state.greeting.auth_types;

        // We only support safe cookie.
        if let Some(_) = types
            .iter()
            .find(|&&val| val == EXTOR_AUTH_TYPE_SAFE_COOKIE)
        {
            log::debug!("Choosing SAFE_COOKIE authentication");

            let choice = Choice {
                auth_type: EXTOR_AUTH_TYPE_SAFE_COOKIE,
            };

            match self.state.conn.write_frame(&choice).await {
                Ok(_) => {
                    let conn = self.state.conn;
                    let next = ClientAuth1 { conn };
                    ClientHandshake2Result::ClientAuth1(next.into())
                }
                Err(net_err) => {
                    let error = or::Error::from(net_err);
                    let next = Error { error };
                    ClientHandshake2Result::Error(next.into())
                }
            }
        } else {
            log::debug!("Authentication methods are unsupported");

            let choice = Choice {
                auth_type: EXTOR_AUTH_TYPE_END,
            };

            // Do not propagate any net error; the or error is more precise.
            match self.state.conn.write_frame(&choice).await {
                Ok(_) => log::debug!("Success writing choice failure message"),
                Err(e) => log::debug!("Error writing choice failure message: {}", e),
            }

            let error = or::Error::AuthMethod;
            let next = Error { error };
            return ClientHandshake2Result::Error(next.into());
        }
    }
}

#[async_trait]
impl ClientAuth1State for ExtOrProtocol<ClientAuth1> {
    async fn send_nonce(mut self) -> ClientAuth1Result {
        let client_auth = ClientNonce {
            nonce: [1; 32], // XXX fill with random data
        };

        match self.state.conn.write_frame(&client_auth).await {
            Ok(_) => {
                let conn = self.state.conn;
                let next = ClientAuth2 { conn, client_auth };
                ClientAuth1Result::ClientAuth2(next.into())
            }
            Err(net_err) => {
                let error = or::Error::from(net_err);
                let next = Error { error };
                ClientAuth1Result::Error(next.into())
            }
        }
    }
}

#[async_trait]
impl ClientAuth2State for ExtOrProtocol<ClientAuth2> {
    async fn recv_nonce_hash(mut self) -> ClientAuth2Result {
        log::debug!("Waiting for server auth nonce and hash");

        match self.state.conn.read_frame::<ServerHashNonce>().await {
            Ok(server_auth) => {
                let next = ClientAuth3 {
                    conn: self.state.conn,
                    client_auth: self.state.client_auth,
                    server_auth,
                };
                ClientAuth2Result::ClientAuth3(next.into())
            }
            Err(net_err) => {
                let error = or::Error::from(net_err);
                let next = Error { error };
                ClientAuth2Result::Error(next.into())
            }
        }
    }
}

#[async_trait]
impl ClientAuth3State for ExtOrProtocol<ClientAuth3> {
    async fn send_hash(mut self) -> ClientAuth3Result {
        // WIP: we have this state:
        // self.state.conn;
        // self.state.server_auth; // server nonce and hash
        // self.state.client_auth; // client nonce

        // now we need to read CookieString from the EXTOR port file from the config
        // we may need to go back and actually pass that path in when calling run_protocol()
        // and then propagate it to here.

        // then the client must compute ServerHash as:
        //   HMAC-SHA256(CookieString,
        //     "ExtORPort authentication server-to-client hash" | ClientNonce | ServerNonce)

        // then validate that the above matches what the server sent
        // in self.state.server_auth

        // if invalid, we must move to error state and terminate the connection

        // if valid, we compute ClientHash as:
        //   HMAC-SHA256(CookieString,
        //     "ExtORPort authentication client-to-server hash" | ClientNonce | ServerNonce)

        // and send to server, then move to next state (ClientAuth4)

        todo!()
    }
}

#[async_trait]
impl ClientAuth4State for ExtOrProtocol<ClientAuth4> {
    async fn recv_status(mut self) -> ClientAuth4Result {
        log::debug!("Waiting for server auth status");

        match self.state.conn.read_frame::<ServerStatus>().await {
            Ok(auth_result) => {
                // Check the auth status.
                match auth_result.status {
                    EXTOR_AUTH_STATUS_SUCCESS => {
                        log::debug!("Server auth status succeeded");
                        let next = ClientCommand1 {
                            conn: self.state.conn,
                        };
                        ClientAuth4Result::ClientCommand1(next.into())
                    }
                    EXTOR_AUTH_STATUS_FAILURE => {
                        log::debug!("Server auth status failed");
                        let error = or::Error::AuthStatusFailed;
                        let next = Error { error };
                        ClientAuth4Result::Error(next.into())
                    }
                    _ => {
                        log::debug!("Received unknown server auth status");
                        let error = or::Error::AuthStatusUnknown;
                        let next = Error { error };
                        ClientAuth4Result::Error(next.into())
                    }
                }
            }
            Err(net_err) => {
                let error = or::Error::from(net_err);
                let next = Error { error };
                ClientAuth4Result::Error(next.into())
            }
        }
    }
}

#[async_trait]
impl ClientCommand1State for ExtOrProtocol<ClientCommand1> {
    async fn send_command(mut self) -> ClientCommand1Result {
        // https://github.com/torproject/torspec/blob/26a2dc7470b1dc41720fd64080ab8386c47df31d/ext-orport-spec.txt#L150
        todo!()
    }
}

#[async_trait]
impl ClientCommand2State for ExtOrProtocol<ClientCommand2> {
    async fn recv_reply(mut self) -> ClientCommand2Result {
        todo!()
    }
}

impl SuccessState for ExtOrProtocol<Success> {
    fn finish(self) -> Connection {
        self.state.conn
    }
}

impl ErrorState for ExtOrProtocol<Error> {
    fn finish(self) -> or::Error {
        self.state.error
    }
}
