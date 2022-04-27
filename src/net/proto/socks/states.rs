use async_trait::async_trait;
use std::io;
use std::net::SocketAddr;
use tokio::net::TcpStream;

use crate::net::{
    self,
    proto::socks::{
        self, address::Socks5Address, formatter::Formatter, frames::*, spec::socks5::*,
    },
    Connection,
};

impl InitState for Socks5Protocol<Init> {
    fn new(conn: Connection) -> Socks5Protocol<Init> {
        Init {
            conn,
            fmt: Formatter::new(),
        }
        .into()
    }

    fn start_server(self) -> Socks5Protocol<ServerHandshake1> {
        ServerHandshake1 {
            conn: self.state.conn,
            fmt: self.state.fmt,
        }
        .into()
    }
}

#[async_trait]
impl ServerHandshake1State for Socks5Protocol<ServerHandshake1> {
    async fn recv_greeting(mut self) -> ServerHandshake1Result {
        log::debug!("Waiting for greeting");

        match self
            .state
            .conn
            .read_frame::<Greeting, Formatter>(&self.state.fmt)
            .await
        {
            Ok(greeting) => {
                log::debug!("Read greeting {:?}", greeting);
                let next = ServerHandshake2 {
                    conn: self.state.conn,
                    fmt: self.state.fmt,
                    greeting,
                };
                ServerHandshake1Result::ServerHandshake2(next.into())
            }
            Err(net_err) => {
                let error = socks::Error::from(net_err);
                let next = Error { error };
                ServerHandshake1Result::Error(next.into())
            }
        }
    }
}

#[async_trait]
impl ServerHandshake2State for Socks5Protocol<ServerHandshake2> {
    async fn send_choice(mut self) -> ServerHandshake2Result {
        // Must be socks version 5, or we close the connection without a response.
        if self.state.greeting.version != SOCKS_VERSION_5 {
            let error = socks::Error::Version;
            let next = Error { error };
            return ServerHandshake2Result::Error(next.into());
        }

        // Check the auth methods supported by the client.
        let methods = self.state.greeting.supported_auth_methods;

        // We support user/pass or none; prefer user/pass.
        if let Some(_) = methods.iter().find(|&&val| val == SOCKS_AUTH_USERPASS) {
            log::debug!("Choosing username/password authentication");

            let choice = Choice {
                version: SOCKS_VERSION_5,
                auth_method: SOCKS_AUTH_USERPASS,
            };

            match self
                .state
                .conn
                .write_frame::<Choice, Formatter>(&self.state.fmt, choice)
                .await
            {
                Ok(_) => {
                    let next = ServerAuth1 {
                        conn: self.state.conn,
                        fmt: self.state.fmt,
                    };
                    ServerHandshake2Result::ServerAuth1(next.into())
                }
                Err(net_err) => {
                    let error = socks::Error::from(net_err);
                    let next = Error { error };
                    ServerHandshake2Result::Error(next.into())
                }
            }
        } else if let Some(_) = methods.iter().find(|&&val| val == SOCKS_AUTH_NONE) {
            log::debug!("Choosing no authentication");

            let choice = Choice {
                version: SOCKS_VERSION_5,
                auth_method: SOCKS_AUTH_NONE,
            };

            match self
                .state
                .conn
                .write_frame::<Choice, Formatter>(&self.state.fmt, choice)
                .await
            {
                Ok(_) => {
                    log::debug!("Wrote choice");
                    let next = ServerCommand1 {
                        conn: self.state.conn,
                        fmt: self.state.fmt,
                    };
                    ServerHandshake2Result::ServerCommand1(next.into())
                }
                Err(net_err) => {
                    log::debug!("Error writing choice");
                    let error = socks::Error::from(net_err);
                    let next = Error { error };
                    ServerHandshake2Result::Error(next.into())
                }
            }
        } else {
            log::debug!("Authentication methods are unsupported");

            let choice = Choice {
                version: SOCKS_VERSION_5,
                auth_method: SOCKS_AUTH_UNSUPPORTED,
            };

            // Do not propagate any net error; the socks error is more precise.
            match self
                .state
                .conn
                .write_frame::<Choice, Formatter>(&self.state.fmt, choice)
                .await
            {
                Ok(_) => log::debug!("Success writing choice failure message"),
                Err(e) => log::debug!("Error writing choice failure message: {}", e),
            }

            let error = socks::Error::AuthMethod;
            let next = Error { error };
            ServerHandshake2Result::Error(next.into())
        }
    }
}

#[async_trait]
impl ServerAuth1State for Socks5Protocol<ServerAuth1> {
    async fn recv_auth_request(mut self) -> ServerAuth1Result {
        log::debug!("Waiting for auth request");

        match self
            .state
            .conn
            .read_frame::<UserPassAuthRequest, Formatter>(&self.state.fmt)
            .await
        {
            Ok(auth_request) => {
                log::debug!("Read auth request {:?}", auth_request);
                let next = ServerAuth2 {
                    conn: self.state.conn,
                    fmt: self.state.fmt,
                    auth_request,
                };
                ServerAuth1Result::ServerAuth2(next.into())
            }
            Err(net_err) => {
                let error = socks::Error::from(net_err);
                let next = Error { error };
                ServerAuth1Result::Error(next.into())
            }
        }
    }
}

#[async_trait]
impl ServerAuth2State for Socks5Protocol<ServerAuth2> {
    async fn send_auth_response(mut self) -> ServerAuth2Result {
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
            let response = UserPassAuthResponse {
                version: SOCKS_AUTH_USERPASS_VERSION,
                status: SOCKS_AUTH_STATUS_FAILURE,
            };

            // Do not propagate any net error; the socks error is more precise.
            match self
                .state
                .conn
                .write_frame::<UserPassAuthResponse, Formatter>(&self.state.fmt, response)
                .await
            {
                Ok(_) => log::debug!("Success writing auth failure message"),
                Err(e) => log::debug!("Error writing auth failure message: {}", e),
            }

            let error = socks::Error::Auth(String::from(err_msg));
            let next = Error { error };
            return ServerAuth2Result::Error(next.into());
        }

        let response = UserPassAuthResponse {
            version: SOCKS_AUTH_USERPASS_VERSION,
            status: SOCKS_AUTH_STATUS_SUCCESS,
        };

        match self
            .state
            .conn
            .write_frame::<UserPassAuthResponse, Formatter>(&self.state.fmt, response)
            .await
        {
            Ok(_) => {
                let next = ServerCommand1 {
                    conn: self.state.conn,
                    fmt: self.state.fmt,
                };
                ServerAuth2Result::ServerCommand1(next.into())
            }
            Err(net_err) => {
                let error = socks::Error::from(net_err);
                let next = Error { error };
                ServerAuth2Result::Error(next.into())
            }
        }
    }
}

#[async_trait]
impl ServerCommand1State for Socks5Protocol<ServerCommand1> {
    async fn recv_connect_request(mut self) -> ServerCommand1Result {
        log::debug!("Waiting for connect request");

        match self
            .state
            .conn
            .read_frame::<ConnectRequest, Formatter>(&self.state.fmt)
            .await
        {
            Ok(request) => {
                log::debug!("Read connect request {:?}", request);
                let next = ServerCommand2 {
                    conn: self.state.conn,
                    fmt: self.state.fmt,
                    request,
                };
                ServerCommand1Result::ServerCommand2(next.into())
            }
            Err(net_err) => {
                let error = socks::Error::from(net_err);
                let next = Error { error };
                ServerCommand1Result::Error(next.into())
            }
        }
    }
}

async fn do_connect(addr: SocketAddr) -> io::Result<(TcpStream, SocketAddr)> {
    let stream = TcpStream::connect(addr).await?;
    let local_addr = stream.local_addr()?;
    Ok((stream, local_addr))
}

async fn connect_to_host(
    addr: Socks5Address,
    port: u16,
) -> Result<(TcpStream, SocketAddr), (socks::Error, u8)> {
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

async fn try_write_connect_err(conn: &mut Connection, fmt: &Formatter, status: u8) {
    let response = ConnectResponse {
        version: SOCKS_VERSION_5,
        status,
        reserved: SOCKS_NULL,
        bind_addr: Socks5Address::Unknown,
        bind_port: 0,
    };

    // Do not propagate any net error; the socks error is more precise.
    match conn
        .write_frame::<ConnectResponse, Formatter>(fmt, response)
        .await
    {
        Ok(_) => log::debug!("Success writing connect failure message"),
        Err(e) => log::debug!("Error writing connect failure message: {}", e),
    }
}

#[async_trait]
impl ServerCommand2State for Socks5Protocol<ServerCommand2> {
    async fn send_connect_response(mut self) -> ServerCommand2Result {
        if self.state.request.version != SOCKS_VERSION_5 {
            try_write_connect_err(
                &mut self.state.conn,
                &self.state.fmt,
                SOCKS_STATUS_PROTO_ERR,
            )
            .await;
            let error = socks::Error::Version;
            return ServerCommand2Result::Error(Error { error }.into());
        } else if self.state.request.command != SOCKS_COMMAND_CONNECT {
            try_write_connect_err(
                &mut self.state.conn,
                &self.state.fmt,
                SOCKS_STATUS_PROTO_ERR,
            )
            .await;
            let error = socks::Error::ConnectMethod;
            return ServerCommand2Result::Error(Error { error }.into());
        } else if self.state.request.reserved != SOCKS_NULL {
            try_write_connect_err(
                &mut self.state.conn,
                &self.state.fmt,
                SOCKS_STATUS_PROTO_ERR,
            )
            .await;
            let error = socks::Error::Reserved;
            return ServerCommand2Result::Error(Error { error }.into());
        }

        // TODO: we should follow the bind addr configured in the env, if any.
        match connect_to_host(self.state.request.dest_addr, self.state.request.dest_port).await {
            Ok((stream, local_addr)) => {
                let response = ConnectResponse {
                    version: SOCKS_VERSION_5,
                    status: SOCKS_STATUS_REQ_GRANTED,
                    reserved: SOCKS_NULL,
                    bind_addr: Socks5Address::IpAddr(local_addr.ip()),
                    bind_port: local_addr.port(),
                };

                match self
                    .state
                    .conn
                    .write_frame::<ConnectResponse, Formatter>(&self.state.fmt, response)
                    .await
                {
                    Ok(_) => {
                        let conn = self.state.conn;
                        let next = Success {
                            conn,
                            dest: Connection::new(stream),
                        };
                        ServerCommand2Result::Success(next.into())
                    }
                    Err(net_err) => {
                        let error = socks::Error::from(net_err);
                        let next = Error { error };
                        ServerCommand2Result::Error(next.into())
                    }
                }
            }
            Err((error, status)) => {
                try_write_connect_err(&mut self.state.conn, &self.state.fmt, status).await;
                return ServerCommand2Result::Error(Error { error }.into());
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
