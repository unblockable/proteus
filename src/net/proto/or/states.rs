use async_trait::async_trait;

use crate::net::{
    proto::or::{self, frames::*, spec::extor::*},
    Connection,
};

impl InitializationState for ExtOrProtocol<Initialization> {
    fn new(conn: Connection) -> ExtOrProtocol<Initialization> {
        Initialization { conn }.into()
    }

    fn start(self) -> ExtOrProtocol<ClientHandshake> {
        let conn = self.state.conn;
        ClientHandshake { conn }.into()
    }
}

#[async_trait]
impl ClientHandshakeState for ExtOrProtocol<ClientHandshake> {
    async fn greeting(mut self) -> ClientHandshakeResult {
        log::debug!("Waiting for greeting");

        match self.state.conn.read_frame::<Greeting>().await {
            Ok(greeting) => {
                let conn = self.state.conn;
                let next = ServerHandshake { conn, greeting };
                ClientHandshakeResult::ServerHandshake(next.into())
            }
            Err(net_err) => {
                let error = or::Error::from(net_err);
                let next = Error { error };
                ClientHandshakeResult::Error(next.into())
            }
        }
    }
}

#[async_trait]
impl ServerHandshakeState for ExtOrProtocol<ServerHandshake> {
    async fn choice(mut self) -> ServerHandshakeResult {
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
                    let next = ClientAuthNonce { conn };
                    ServerHandshakeResult::ClientAuthNonce(next.into())
                }
                Err(net_err) => {
                    let error = or::Error::from(net_err);
                    let next = Error { error };
                    ServerHandshakeResult::Error(next.into())
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
            return ServerHandshakeResult::Error(next.into());
        }
    }
}

#[async_trait]
impl ClientAuthNonceState for ExtOrProtocol<ClientAuthNonce> {
    async fn auth_nonce(mut self) -> ClientAuthNonceResult {
        let client_auth = ClientNonce {
            nonce: [1; 32], // XXX fill with random data
        };

        match self.state.conn.write_frame(&client_auth).await {
            Ok(_) => {
                let conn = self.state.conn;
                let next = ServerAuthNonceHash { conn, client_auth };
                ClientAuthNonceResult::ServerAuthNonceHash(next.into())
            }
            Err(net_err) => {
                let error = or::Error::from(net_err);
                let next = Error { error };
                ClientAuthNonceResult::Error(next.into())
            }
        }
    }
}

#[async_trait]
impl ServerAuthNonceHashState for ExtOrProtocol<ServerAuthNonceHash> {
    async fn auth_nonce_hash(mut self) -> ServerAuthNonceHashResult {
        log::debug!("Waiting for server auth nonce and hash");

        match self.state.conn.read_frame::<ServerHashNonce>().await {
            Ok(server_auth) => {
                let next = ClientAuthHash {
                    conn: self.state.conn,
                    client_auth: self.state.client_auth,
                    server_auth,
                };
                ServerAuthNonceHashResult::ClientAuthHash(next.into())
            }
            Err(net_err) => {
                let error = or::Error::from(net_err);
                let next = Error { error };
                ServerAuthNonceHashResult::Error(next.into())
            }
        }
    }
}

#[async_trait]
impl ClientAuthHashState for ExtOrProtocol<ClientAuthHash> {
    async fn auth_hash(mut self) -> ClientAuthHashResult {
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

        // and send to server, then move to next state (ServerAuthStatus)

        todo!()
    }
}

#[async_trait]
impl ServerAuthStatusState for ExtOrProtocol<ServerAuthStatus> {
    async fn auth_status(mut self) -> ServerAuthStatusResult {
        log::debug!("Waiting for server auth status");

        match self.state.conn.read_frame::<ServerStatus>().await {
            Ok(auth_result) => {
                // Check the auth status.
                match auth_result.status {
                    EXTOR_AUTH_STATUS_SUCCESS => {
                        log::debug!("Server auth status succeeded");
                        let next = ClientCommand {
                            conn: self.state.conn,
                        };
                        ServerAuthStatusResult::ClientCommand(next.into())
                    }
                    EXTOR_AUTH_STATUS_FAILURE => {
                        log::debug!("Server auth status failed");
                        let error = or::Error::AuthStatusFailed;
                        let next = Error { error };
                        ServerAuthStatusResult::Error(next.into())
                    }
                    _ => {
                        log::debug!("Received unknown server auth status");
                        let error = or::Error::AuthStatusUnknown;
                        let next = Error { error };
                        ServerAuthStatusResult::Error(next.into())
                    }
                }
            }
            Err(net_err) => {
                let error = or::Error::from(net_err);
                let next = Error { error };
                ServerAuthStatusResult::Error(next.into())
            }
        }
    }
}

#[async_trait]
impl ClientCommandState for ExtOrProtocol<ClientCommand> {
    async fn command(mut self) -> ClientCommandResult {
        // https://github.com/torproject/torspec/blob/26a2dc7470b1dc41720fd64080ab8386c47df31d/ext-orport-spec.txt#L150
        todo!()
    }
}

#[async_trait]
impl ServerCommandState for ExtOrProtocol<ServerCommand> {
    async fn reply(mut self) -> ServerCommandResult {
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
