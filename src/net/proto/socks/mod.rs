use std::fmt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use address::Socks5Address;
use anyhow::bail;
use formatter::Formatter;
use frames::{
    Choice, ConnectRequest, ConnectResponse, Greeting, UserPassAuthRequest, UserPassAuthResponse,
};

use crate::net::proto::socks;
use crate::net::{self, Connection, Reader, Writer};

pub mod address;
mod formatter;
mod frames;

const SOCKS_NULL: u8 = 0x00;
const SOCKS_VERSION_5: u8 = 0x05;
const SOCKS_AUTH_NONE: u8 = 0x00;
const SOCKS_AUTH_USERPASS: u8 = 0x02;
const SOCKS_AUTH_UNSUPPORTED: u8 = 0xff;
const SOCKS_AUTH_USERPASS_VERSION: u8 = 0x01;
const SOCKS_AUTH_STATUS_SUCCESS: u8 = 0x00;
const SOCKS_AUTH_STATUS_FAILURE: u8 = 0x01;
const SOCKS_COMMAND_CONNECT: u8 = 0x01;
const SOCKS_STATUS_REQ_GRANTED: u8 = 0x00;
const SOCKS_STATUS_GEN_FAILURE: u8 = 0x01;
const SOCKS_STATUS_PROTO_ERR: u8 = 0x07;
const SOCKS_STATUS_ADDR_ERR: u8 = 0x08;

/// A successful protocol run will yield these components.
type SocksConnectInfo<R, W> = (
    Connection<R, W>,
    Option<std::string::String>,
    Option<std::string::String>,
    std::net::SocketAddr,
);

enum Error {
    Version,
    Reserved,
    AuthMethod,
    AuthUserPassVersion,
    AuthUsernameEmpty,
    AuthPasswordEmpty,
    ConnectMethod,
    ConnectAddress,
    Network(net::Error),
}

impl From<net::Error> for socks::Error {
    fn from(e: net::Error) -> Self {
        Error::Network(e)
    }
}

impl fmt::Display for socks::Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Version => write!(f, "Socks version mismatch"),
            Error::Reserved => write!(f, "Socks non-zero reserved field"),
            Error::AuthMethod => write!(f, "No supported authentication methods"),
            Error::AuthUserPassVersion => {
                write!(f, "User/pass authentication failed: invalid version")
            }
            Error::AuthUsernameEmpty => {
                write!(f, "User/pass authentication failed: empty username")
            }
            Error::AuthPasswordEmpty => {
                write!(f, "User/pass authentication failed: empty password")
            }
            Error::ConnectMethod => write!(f, "No supported connect methods"),
            Error::ConnectAddress => write!(f, "Address type not supported"),
            Error::Network(e) => write!(f, "Network error: {}", e),
        }
    }
}

#[allow(dead_code)]
pub async fn run_socks5_client<R: Reader, W: Writer>(
    _conn: Connection<R, W>,
) -> anyhow::Result<Connection<R, W>> {
    unimplemented!()
}

pub async fn run_socks5_server<R: Reader, W: Writer>(
    conn: Connection<R, W>,
) -> anyhow::Result<SocksConnectInfo<R, W>> {
    let proto = Init::new(conn).start_server();

    let auth_method = proto
        .recv_greeting()
        .await?
        .prepare_choice()?
        .send_choice()
        .await?;

    let proto = match auth_method {
        AuthOrCommand::Auth(s) => {
            s.recv_auth_request()
                .await?
                .prepare_auth_response()
                .send_auth_response()
                .await?
        }
        AuthOrCommand::Command(s) => s,
    };

    proto
        .recv_connect_request()
        .await?
        .prepare_connect_response()
        .send_connect_response()
        .await
}

struct Init<R: Reader, W: Writer> {
    conn: Connection<R, W>,
    fmt: Formatter,
}

impl<R: Reader, W: Writer> Init<R, W> {
    fn new(conn: Connection<R, W>) -> Init<R, W> {
        Init {
            conn,
            fmt: Formatter::new(),
        }
    }

    fn start_server(self) -> ServerBegin<R, W> {
        ServerBegin {
            conn: self.conn,
            fmt: self.fmt,
        }
    }
}

struct ServerBegin<R: Reader, W: Writer> {
    conn: Connection<R, W>,
    fmt: Formatter,
}

impl<R: Reader, W: Writer> ServerBegin<R, W> {
    async fn recv_greeting(mut self) -> anyhow::Result<ServerGreeting<R, W>> {
        log::debug!("Waiting for greeting");

        match self
            .conn
            .src
            .read_frame::<Greeting, Formatter>(&mut self.fmt)
            .await
        {
            Ok(greeting) => {
                log::debug!("Read greeting {:?}", greeting);
                Ok(ServerGreeting {
                    conn: self.conn,
                    fmt: self.fmt,
                    greeting,
                })
            }
            Err(net_err) => bail!(net_err),
        }
    }
}

struct ServerGreeting<R: Reader, W: Writer> {
    conn: Connection<R, W>,
    fmt: Formatter,
    greeting: Greeting,
}

impl<R: Reader, W: Writer> ServerGreeting<R, W> {
    fn prepare_choice(self) -> anyhow::Result<ServerChoice<R, W>> {
        // Must be socks version 5, or we close the connection without a response.
        if self.greeting.version != SOCKS_VERSION_5 {
            bail!("{}", Error::Version);
        }

        // Check the auth methods supported by the client.
        let methods = self.greeting.supported_auth_methods;

        // We support user/pass or none; prefer user/pass.
        let choice = if methods.contains(&SOCKS_AUTH_USERPASS) {
            log::debug!("Choosing username/password authentication");
            Choice {
                version: SOCKS_VERSION_5,
                auth_method: SOCKS_AUTH_USERPASS,
            }
        } else if methods.contains(&SOCKS_AUTH_NONE) {
            log::debug!("Choosing no authentication");
            Choice {
                version: SOCKS_VERSION_5,
                auth_method: SOCKS_AUTH_NONE,
            }
        } else {
            log::debug!("Authentication methods are unsupported");
            Choice {
                version: SOCKS_VERSION_5,
                auth_method: SOCKS_AUTH_UNSUPPORTED,
            }
        };

        Ok(ServerChoice {
            conn: self.conn,
            fmt: self.fmt,
            choice,
        })
    }
}

struct ServerChoice<R: Reader, W: Writer> {
    conn: Connection<R, W>,
    fmt: Formatter,
    choice: Choice,
}

impl<R: Reader, W: Writer> ServerChoice<R, W> {
    async fn send_choice(mut self) -> anyhow::Result<AuthOrCommand<R, W>> {
        let auth_method = self.choice.auth_method;

        match self
            .conn
            .dst
            .write_frame::<Choice, Formatter>(&mut self.fmt, self.choice)
            .await
        {
            Ok(_) => log::debug!("Success writing choice"),
            Err(net_err) => bail!("Error writing choice: {}", net_err),
        };

        let next = match auth_method {
            SOCKS_AUTH_USERPASS => AuthOrCommand::Auth(ServerAuth {
                conn: self.conn,
                fmt: self.fmt,
            }),
            SOCKS_AUTH_NONE => AuthOrCommand::Command(ServerCommand {
                conn: self.conn,
                fmt: self.fmt,
                username: None,
                password: None,
            }),
            _ => bail!("{}", Error::AuthMethod),
        };

        Ok(next)
    }
}

enum AuthOrCommand<R: Reader, W: Writer> {
    Auth(ServerAuth<R, W>),
    Command(ServerCommand<R, W>),
}

struct ServerAuth<R: Reader, W: Writer> {
    conn: Connection<R, W>,
    fmt: Formatter,
}

impl<R: Reader, W: Writer> ServerAuth<R, W> {
    async fn recv_auth_request(mut self) -> anyhow::Result<ServerAuthRequest<R, W>> {
        log::debug!("Waiting for auth request");

        match self
            .conn
            .src
            .read_frame::<UserPassAuthRequest, Formatter>(&mut self.fmt)
            .await
        {
            Ok(auth_request) => {
                log::debug!("Read auth request {:?}", auth_request);
                Ok(ServerAuthRequest {
                    conn: self.conn,
                    fmt: self.fmt,
                    auth_request,
                })
            }
            Err(net_err) => bail!(net_err),
        }
    }
}

struct ServerAuthRequest<R: Reader, W: Writer> {
    conn: Connection<R, W>,
    fmt: Formatter,
    auth_request: UserPassAuthRequest,
}

impl<R: Reader, W: Writer> ServerAuthRequest<R, W> {
    fn prepare_auth_response(self) -> ServerAuthResponse<R, W> {
        let (status, auth_err) = if self.auth_request.version != SOCKS_AUTH_USERPASS_VERSION {
            log::debug!("Incorrect version in authentication request");
            (SOCKS_AUTH_STATUS_FAILURE, Some(Error::AuthUserPassVersion))
        } else if self.auth_request.username.is_empty() {
            log::debug!("Empty username in authentication request");
            (SOCKS_AUTH_STATUS_FAILURE, Some(Error::AuthUsernameEmpty))
        } else if self.auth_request.password.is_empty() {
            log::debug!("Empty password in authentication request");
            (SOCKS_AUTH_STATUS_FAILURE, Some(Error::AuthPasswordEmpty))
        } else {
            log::debug!("Authentication request OK");
            (SOCKS_AUTH_STATUS_SUCCESS, None)
        };

        let auth_response = UserPassAuthResponse {
            version: SOCKS_AUTH_USERPASS_VERSION,
            status,
        };

        ServerAuthResponse {
            conn: self.conn,
            fmt: self.fmt,
            auth_response,
            auth_err,
            username: Some(self.auth_request.username),
            password: Some(self.auth_request.password),
        }
    }
}

struct ServerAuthResponse<R: Reader, W: Writer> {
    conn: Connection<R, W>,
    fmt: Formatter,
    auth_response: UserPassAuthResponse,
    auth_err: Option<Error>,
    username: Option<String>,
    password: Option<String>,
}

impl<R: Reader, W: Writer> ServerAuthResponse<R, W> {
    async fn send_auth_response(mut self) -> anyhow::Result<ServerCommand<R, W>> {
        log::debug!("Sending authentication response");
        let net_err = self
            .conn
            .dst
            .write_frame::<UserPassAuthResponse, Formatter>(&mut self.fmt, self.auth_response)
            .await
            .err();

        match (self.auth_err, net_err) {
            (None, None) => {}
            (None, Some(e)) => bail!("Authentication error: net:'{e}'"),
            (Some(e), None) => bail!("Authentication error: auth:'{e}'"),
            (Some(ae), Some(ne)) => bail!("Authentication error: auth:'{ae}' net:'{ne}'"),
        };

        Ok(ServerCommand {
            conn: self.conn,
            fmt: self.fmt,
            username: self.username,
            password: self.password,
        })
    }
}

struct ServerCommand<R: Reader, W: Writer> {
    conn: Connection<R, W>,
    fmt: Formatter,
    username: Option<String>,
    password: Option<String>,
}

impl<R: Reader, W: Writer> ServerCommand<R, W> {
    async fn recv_connect_request(mut self) -> anyhow::Result<ServerConnectRequest<R, W>> {
        log::debug!("Waiting for connect request");

        match self
            .conn
            .src
            .read_frame::<ConnectRequest, Formatter>(&mut self.fmt)
            .await
        {
            Ok(request) => {
                log::debug!("Read connect request {:?}", request);
                Ok(ServerConnectRequest {
                    conn: self.conn,
                    fmt: self.fmt,
                    username: self.username,
                    password: self.password,
                    request,
                })
            }
            Err(net_err) => bail!(net_err),
        }
    }
}

struct ServerConnectRequest<R: Reader, W: Writer> {
    conn: Connection<R, W>,
    fmt: Formatter,
    username: Option<String>,
    password: Option<String>,
    request: ConnectRequest,
}

fn parse_connect_request(request: &ConnectRequest) -> Result<SocketAddr, Error> {
    if request.version != SOCKS_VERSION_5 {
        return Err(Error::Version);
    }

    if request.command != SOCKS_COMMAND_CONNECT {
        return Err(Error::ConnectMethod);
    }

    if request.reserved != SOCKS_NULL {
        return Err(Error::Reserved);
    }

    match request.dest_addr {
        Socks5Address::IpAddr(a) => Ok(SocketAddr::new(a, request.dest_port)),
        _ => Err(Error::ConnectAddress),
    }
}

impl<R: Reader, W: Writer> ServerConnectRequest<R, W> {
    fn prepare_connect_response(self) -> ServerConnectResponse<R, W> {
        let mut result = parse_connect_request(&self.request);

        let response = if result.is_ok() {
            ConnectResponse {
                version: SOCKS_VERSION_5,
                status: SOCKS_STATUS_REQ_GRANTED,
                reserved: SOCKS_NULL,
                // Normally we would set to addr:port of outgoing connection, but we defer
                // the connection attempt to a later stage, and it might change if we
                // eventually need to reconnect. I think using zeros here is common.
                bind_addr: Socks5Address::IpAddr(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0))),
                bind_port: 0,
            }
        } else {
            let mut error_status = SOCKS_STATUS_GEN_FAILURE;

            result = result.inspect_err(|e| {
                if let Error::ConnectAddress = e {
                    error_status = SOCKS_STATUS_ADDR_ERR
                } else {
                    error_status = SOCKS_STATUS_PROTO_ERR
                }
            });

            ConnectResponse {
                version: SOCKS_VERSION_5,
                status: error_status,
                reserved: SOCKS_NULL,
                bind_addr: Socks5Address::Unknown,
                bind_port: 0,
            }
        };

        ServerConnectResponse {
            conn: self.conn,
            fmt: self.fmt,
            username: self.username,
            password: self.password,
            response,
            result,
        }
    }
}

struct ServerConnectResponse<R: Reader, W: Writer> {
    conn: Connection<R, W>,
    fmt: Formatter,
    username: Option<String>,
    password: Option<String>,
    response: ConnectResponse,
    result: Result<SocketAddr, Error>,
}

impl<R: Reader, W: Writer> ServerConnectResponse<R, W> {
    async fn send_connect_response(mut self) -> anyhow::Result<SocksConnectInfo<R, W>> {
        log::debug!("Sending connect response");
        let net_err = self
            .conn
            .dst
            .write_frame::<ConnectResponse, Formatter>(&mut self.fmt, self.response)
            .await
            .err();

        let connect_addr = match (self.result, net_err) {
            (Ok(addr), None) => addr,
            (Ok(_), Some(e)) => bail!("Connect error: net:'{e}'"),
            (Err(e), None) => bail!("Connect error: connect:'{e}'"),
            (Err(ce), Some(ne)) => bail!("Connect error: connect:'{ce}' net:'{ne}'"),
        };

        log::debug!("Socks completed successfully");
        Ok((self.conn, self.username, self.password, connect_addr))
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use bytes::Bytes;
    use net::{BufReader, Serialize};
    use tokio_test::io::{Builder, Mock};

    use super::*;
    use crate::net::Connector;

    pub struct MockConnector {}

    impl MockConnector {
        fn default_addr() -> SocketAddr {
            // We return zeros for our localhost bind address.
            "0.0.0.0:0".parse().expect("Valid socket addr")
        }
    }

    #[async_trait]
    impl Connector<BufReader<Mock>, Mock> for MockConnector {
        async fn connect(
            &self,
        ) -> anyhow::Result<(Connection<BufReader<Mock>, Mock>, SocketAddr)> {
            let client = Connection::new(
                BufReader::new(Builder::new().build()),
                Builder::new().build(),
            );
            Ok((client, MockConnector::default_addr()))
        }

        fn into_self(self, _addr: SocketAddr) -> Self {
            Self {}
        }
    }

    fn greeting(auth_method: u8) -> Greeting {
        Greeting {
            version: SOCKS_VERSION_5,
            num_auth_methods: 1,
            supported_auth_methods: Bytes::from(vec![auth_method]),
        }
    }

    fn choice(auth_method: u8) -> Choice {
        Choice {
            version: SOCKS_VERSION_5,
            auth_method,
        }
    }

    fn userpass_auth_request() -> UserPassAuthRequest {
        UserPassAuthRequest {
            version: SOCKS_AUTH_USERPASS_VERSION,
            username: String::from("my_username"),
            password: String::from("my_password"),
        }
    }

    fn userpass_auth_response() -> UserPassAuthResponse {
        UserPassAuthResponse {
            version: SOCKS_AUTH_USERPASS_VERSION,
            status: SOCKS_AUTH_STATUS_SUCCESS,
        }
    }

    fn connect_request() -> ConnectRequest {
        let sock_addr: SocketAddr = "127.0.0.1:54321".parse().expect("Valid socket addr");
        ConnectRequest {
            version: SOCKS_VERSION_5,
            command: SOCKS_COMMAND_CONNECT,
            reserved: SOCKS_NULL,
            dest_addr: Socks5Address::IpAddr(sock_addr.ip()),
            dest_port: sock_addr.port(),
        }
    }

    fn connect_response() -> ConnectResponse {
        let sock_addr = MockConnector::default_addr();
        ConnectResponse {
            version: SOCKS_VERSION_5,
            status: SOCKS_STATUS_REQ_GRANTED,
            reserved: SOCKS_NULL,
            bind_addr: Socks5Address::IpAddr(sock_addr.ip()),
            bind_port: sock_addr.port(),
        }
    }

    #[tokio::test]
    async fn userpass_auth_method() {
        let reader = Builder::new()
            .read(&greeting(SOCKS_AUTH_USERPASS).serialize())
            .read(&userpass_auth_request().serialize())
            .read(&connect_request().serialize())
            .build();
        let writer = Builder::new()
            .write(&choice(SOCKS_AUTH_USERPASS).serialize())
            .write(&userpass_auth_response().serialize())
            .write(&connect_response().serialize())
            .build();
        let conn = Connection::new(BufReader::new(reader), writer);

        let s = run_socks5_server(conn).await;
        assert!(s.is_ok())
    }

    #[tokio::test]
    async fn none_auth_method() {
        let reader = Builder::new()
            .read(&greeting(SOCKS_AUTH_NONE).serialize())
            .read(&connect_request().serialize())
            .build();
        let writer = Builder::new()
            .write(&choice(SOCKS_AUTH_NONE).serialize())
            .write(&connect_response().serialize())
            .build();
        let conn = Connection::new(BufReader::new(reader), writer);

        let s = run_socks5_server(conn).await;
        assert!(s.is_ok())
    }

    #[tokio::test]
    async fn unsupported_auth_method() {
        let reader = Builder::new()
            .read(&greeting(SOCKS_AUTH_UNSUPPORTED).serialize())
            .build();
        let writer = Builder::new()
            .write(&choice(SOCKS_AUTH_UNSUPPORTED).serialize())
            .build();
        let conn = Connection::new(BufReader::new(reader), writer);

        let s = run_socks5_server(conn).await;
        assert!(s.is_err())
    }
}
