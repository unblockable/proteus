use async_trait::async_trait;
use std::net::SocketAddr;
use std::io;
use tokio::net::TcpStream;

use crate::net::{
    self,
    socks::{self, socks5_protocol::*, Socks5Address},
    Connection,
};

impl InitializationState for Socks5Protocol<Initialization> {
    fn new(conn: Connection) -> Socks5Protocol<Initialization> {
        Initialization { conn }.into()
    }

    fn start(self) -> Socks5Protocol<ClientHandshake> {
        let conn = self.state.conn;
        ClientHandshake { conn }.into()
    }
}

#[async_trait]
impl ClientHandshakeState for Socks5Protocol<ClientHandshake> {
    async fn greeting(mut self) -> ClientHandshakeResult {
        log::debug!("Waiting for greeting");

        match self.state.conn.read_frame::<socks::Greeting>().await {
            Ok(greeting) => {
                log::debug!("Read greeting {:?}", greeting);
                let conn = self.state.conn;
                let next = ServerHandshake { conn, greeting };
                ClientHandshakeResult::ServerHandshake(next.into())
            }
            Err(net_err) => {
                let error = socks::Error::from(net_err);
                let next = Error { error };
                ClientHandshakeResult::Error(next.into())
            }
        }
    }
}

#[async_trait]
impl ServerHandshakeState for Socks5Protocol<ServerHandshake> {
    async fn choice(mut self) -> ServerHandshakeResult {
        // Must be socks version 5, or we close the connection without a response.
        if self.state.greeting.version != SOCKS_VERSION_5 {
            let error = socks::Error::Version;
            let next = Error { error };
            return ServerHandshakeResult::Error(next.into());
        }

        // Check the auth methods supported by the client.
        let methods = self.state.greeting.supported_auth_methods;

        // We support user/pass or none; prefer user/pass.
        if let Some(_) = methods.iter().find(|&&val| val == SOCKS_AUTH_USERPASS) {
            log::debug!("Choosing username/password authentication");

            let choice = socks::Choice {
                version: SOCKS_VERSION_5,
                auth_method: SOCKS_AUTH_USERPASS,
            };

            match self.state.conn.write_frame(&choice).await {
                Ok(_) => {
                    let conn = self.state.conn;
                    let next = ClientAuthentication { conn };
                    ServerHandshakeResult::ClientAuthentication(next.into())
                }
                Err(net_err) => {
                    let error = socks::Error::from(net_err);
                    let next = Error { error };
                    ServerHandshakeResult::Error(next.into())
                }
            }
        } else if let Some(_) = methods.iter().find(|&&val| val == SOCKS_AUTH_NONE) {
            log::debug!("Choosing no authentication");

            let choice = socks::Choice {
                version: SOCKS_VERSION_5,
                auth_method: SOCKS_AUTH_NONE,
            };

            match self.state.conn.write_frame(&choice).await {
                Ok(_) => {
                    log::debug!("Wrote choice");
                    let conn = self.state.conn;
                    let next = ClientCommand { conn };
                    ServerHandshakeResult::ClientCommand(next.into())
                }
                Err(net_err) => {
                    log::debug!("Error writing choice");
                    let error = socks::Error::from(net_err);
                    let next = Error { error };
                    ServerHandshakeResult::Error(next.into())
                }
            }
        } else {
            log::debug!("Authentication methods are unsupported");

            let choice = socks::Choice {
                version: SOCKS_VERSION_5,
                auth_method: SOCKS_AUTH_UNSUPPORTED,
            };

            // Do not propagate any net error; the socks error is more precise.
            match self.state.conn.write_frame(&choice).await {
                Ok(_) => log::debug!("Success writing choice failure message"),
                Err(e) => log::debug!("Error writing choice failure message: {}", e),
            }

            let error = socks::Error::AuthMethod;
            let next = Error { error };
            ServerHandshakeResult::Error(next.into())
        }
    }
}

#[async_trait]
impl ClientAuthenticationState for Socks5Protocol<ClientAuthentication> {
    async fn auth_request(mut self) -> ClientAuthenticationResult {
        log::debug!("Waiting for auth request");
        match self
            .state
            .conn
            .read_frame::<socks::UserPassAuthRequest>()
            .await
        {
            Ok(auth_request) => {
                log::debug!("Read auth request {:?}", auth_request);
                let conn = self.state.conn;
                let next = ServerAuthentication { conn, auth_request };
                ClientAuthenticationResult::ServerAuthentication(next.into())
            }
            Err(net_err) => {
                let error = socks::Error::from(net_err);
                let next = Error { error };
                ClientAuthenticationResult::Error(next.into())
            }
        }
    }
}

#[async_trait]
impl ServerAuthenticationState for Socks5Protocol<ServerAuthentication> {
    async fn auth_response(mut self) -> ServerAuthenticationResult {
        let err_msg_opt = {
            if self.state.auth_request.version != SOCKS_AUTH_USERPASS_VERSION {
                Some(String::from("Invalid username/password auth version"))
            } else if self.state.auth_request.username.is_empty() {
                Some(String::from("Username is empty"))
            } else if self.state.auth_request.password.is_empty() {
                Some(String::from("Password is empty"))
            } else {
                None
            }
        };

        if let Some(err_msg) = err_msg_opt {
            let response = socks::UserPassAuthResponse {
                version: SOCKS_AUTH_USERPASS_VERSION,
                status: SOCKS_AUTH_STATUS_FAILURE,
            };

            // Do not propagate any net error; the socks error is more precise.
            match self.state.conn.write_frame(&response).await {
                Ok(_) => log::debug!("Success writing auth failure message"),
                Err(e) => log::debug!("Error writing auth failure message: {}", e),
            }

            let error = socks::Error::Auth(String::from(err_msg));
            let next = Error { error };
            return ServerAuthenticationResult::Error(next.into());
        }

        let response = socks::UserPassAuthResponse {
            version: SOCKS_AUTH_USERPASS_VERSION,
            status: SOCKS_AUTH_STATUS_SUCCESS,
        };

        match self.state.conn.write_frame(&response).await {
            Ok(_) => {
                let conn = self.state.conn;
                let next = ClientCommand { conn };
                ServerAuthenticationResult::ClientCommand(next.into())
            }
            Err(net_err) => {
                let error = socks::Error::from(net_err);
                let next = Error { error };
                ServerAuthenticationResult::Error(next.into())
            }
        }
    }
}

#[async_trait]
impl ClientCommandState for Socks5Protocol<ClientCommand> {
    async fn connect_request(mut self) -> ClientCommandResult {
        log::debug!("Waiting for connect request");

        match self.state.conn.read_frame::<socks::ConnectRequest>().await {
            Ok(request) => {
                log::debug!("Read connect request {:?}", request);
                let conn = self.state.conn;
                let next = ServerCommand { conn, request };
                ClientCommandResult::ServerCommand(next.into())
            }
            Err(net_err) => {
                let error = socks::Error::from(net_err);
                let next = Error { error };
                ClientCommandResult::Error(next.into())
            }
        }
    }
}

async fn do_connect(addr: SocketAddr) -> io::Result<(TcpStream, SocketAddr)> {
    let stream = TcpStream::connect(addr).await?;
    let local_addr = stream.local_addr()?;
    Ok((stream, local_addr))
}

async fn connect_to_host(addr: Socks5Address, port: u16) -> Result<(TcpStream, SocketAddr), (socks::Error, u8)> {
    let dest_addr = match addr {
        Socks5Address::IpAddr(a) => SocketAddr::new(a, port),
        _ => {
            return Err((
                socks::Error::Connect(String::from("Address type not supported")),
                SOCKS_STATUS_ADDR_ERR,
            ));
        }
    };

    match do_connect(dest_addr).await {
        Ok((stream, local_addr)) => Ok((stream, local_addr)),
        Err(e) => {
            // TODO: check the network error and be more precise here.
            Err((
                socks::Error::Network(net::Error::IoError(e)),
                SOCKS_STATUS_GEN_FAILURE,
            ))
        }
    }
}

async fn try_write_connect_err(conn: &mut Connection, status: u8) {
    let response = socks::ConnectResponse {
        version: SOCKS_VERSION_5,
        status,
        reserved: SOCKS_NULL,
        bind_addr: Socks5Address::Unknown,
        bind_port: 0,
    };

    // Do not propagate any net error; the socks error is more precise.
    match conn.write_frame(&response).await {
        Ok(_) => log::debug!("Success writing connect failure message"),
        Err(e) => log::debug!("Error writing connect failure message: {}", e),
    }
}

#[async_trait]
impl ServerCommandState for Socks5Protocol<ServerCommand> {
    async fn connect_response(mut self) -> ServerCommandResult {
        if self.state.request.version != SOCKS_VERSION_5 {
            try_write_connect_err(&mut self.state.conn, SOCKS_STATUS_PROTO_ERR).await;
            let error = socks::Error::Version;
            return ServerCommandResult::Error(Error { error }.into());
        } else if self.state.request.command != SOCKS_COMMAND_CONNECT {
            try_write_connect_err(&mut self.state.conn, SOCKS_STATUS_PROTO_ERR).await;
            let error = socks::Error::ConnectMethod;
            return ServerCommandResult::Error(Error { error }.into());
        } else if self.state.request.reserved != SOCKS_NULL {
            try_write_connect_err(&mut self.state.conn, SOCKS_STATUS_PROTO_ERR).await;
            let error = socks::Error::Reserved;
            return ServerCommandResult::Error(Error { error }.into());
        }

        // TODO: we should follow the bind addr configured in the env, if any.
        match connect_to_host(self.state.request.dest_addr, self.state.request.dest_port).await {
            Ok((stream, local_addr)) => {
                let response = socks::ConnectResponse {
                    version: SOCKS_VERSION_5,
                    status: SOCKS_STATUS_REQ_GRANTED,
                    reserved: SOCKS_NULL,
                    bind_addr: Socks5Address::IpAddr(local_addr.ip()),
                    bind_port: local_addr.port(),
                };

                match self.state.conn.write_frame(&response).await {
                    Ok(_) => {
                        let conn = self.state.conn;
                        let next = Success { conn, dest: Connection::new(stream) };
                        ServerCommandResult::Success(next.into())
                    }
                    Err(net_err) => {
                        let error = socks::Error::from(net_err);
                        let next = Error { error };
                        ServerCommandResult::Error(next.into())
                    }
                }
            }
            Err((error, status)) => {
                try_write_connect_err(&mut self.state.conn, status).await;
                return ServerCommandResult::Error(Error { error }.into());
            }
        }
    }
}

impl SuccessState for Socks5Protocol<Success> {
    fn finish(self) -> (Connection, Connection) {
        (self.state.conn, self.state.dest)
    }
}

impl ErrorState for Socks5Protocol<Error> {
    fn finish(self) -> socks::Error {
        self.state.error
    }
}

pub async fn run_protocol(conn: Connection) -> Result<(Connection, Connection), socks::Error> {
    let proto = Socks5Protocol::new(conn).start();

    let proto = match proto.greeting().await {
        ClientHandshakeResult::ServerHandshake(s) => s,
        ClientHandshakeResult::Error(e) => return Err(e.finish()),
    };

    let proto = match proto.choice().await {
        ServerHandshakeResult::ClientAuthentication(s) => {
            let auth = match s.auth_request().await {
                ClientAuthenticationResult::ServerAuthentication(s) => s,
                ClientAuthenticationResult::Error(e) => return Err(e.finish()),
            };

            match auth.auth_response().await {
                ServerAuthenticationResult::ClientCommand(s) => s,
                ServerAuthenticationResult::Error(e) => return Err(e.finish()),
            }
        }
        ServerHandshakeResult::ClientCommand(s) => s,
        ServerHandshakeResult::Error(e) => return Err(e.finish()),
    };

    let proto = match proto.connect_request().await {
        ClientCommandResult::ServerCommand(s) => s,
        ClientCommandResult::Error(e) => return Err(e.finish()),
    };

    let proto = match proto.connect_response().await {
        ServerCommandResult::Success(s) => s,
        ServerCommandResult::Error(e) => return Err(e.finish()),
    };

    Ok(proto.finish())
}
