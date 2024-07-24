use std::{fmt, net::SocketAddr};

use address::Socks5Address;
use anyhow::bail;
use formatter::Formatter;
use frames::{
    Choice, ConnectRequest, ConnectResponse, Greeting, UserPassAuthRequest, UserPassAuthResponse,
};

use crate::net::{self, proto::socks, Connection, Connector, Reader, Writer};

mod address;
mod formatter;
mod frames;

#[allow(dead_code)]
pub async fn run_socks5_client<R: Reader, W: Writer>(
    _conn: Connection<R, W>,
) -> anyhow::Result<(Connection<R, W>, Connection<R, W>)> {
    unimplemented!()
}

pub async fn run_socks5_server<R: Reader, W: Writer, C: Connector<R, W>>(
    conn: Connection<R, W>,
    connector: C,
) -> anyhow::Result<(Connection<R, W>, Connection<R, W>, Option<String>)> {
    let proto = Init::new(conn).start_server();

    let proto = proto.recv_greeting().await?;

    let proto = match proto.send_choice().await? {
        AuthOrCommand::Auth(s) => s.recv_auth_request().await?.send_auth_response().await?,
        AuthOrCommand::Command(s) => s,
    };

    let proto = proto.recv_connect_request().await?;
    let result = proto.send_connect_response(connector).await?;

    Ok(result)
}

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
const _SOCKS_STATUS_GEN_FAILURE: u8 = 0x01;
const SOCKS_STATUS_PROTO_ERR: u8 = 0x07;
const SOCKS_STATUS_ADDR_ERR: u8 = 0x08;

enum Error {
    Version,
    Reserved,
    AuthMethod,
    Auth(String),
    ConnectMethod,
    Connect(String),
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
            Error::Auth(s) => write!(f, "Chosen authentication method failed: {}", s),
            Error::ConnectMethod => write!(f, "No supported connect methods"),
            Error::Connect(s) => write!(f, "Chosen connect method failed: {}", s),
            Error::Network(e) => write!(f, "Network error: {}", e),
        }
    }
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

    fn start_server(self) -> ServerHandshake1<R, W> {
        ServerHandshake1 {
            conn: self.conn,
            fmt: self.fmt,
        }
    }
}

struct ServerHandshake1<R: Reader, W: Writer> {
    conn: Connection<R, W>,
    fmt: Formatter,
}

impl<R: Reader, W: Writer> ServerHandshake1<R, W> {
    async fn recv_greeting(mut self) -> anyhow::Result<ServerHandshake2<R, W>> {
        log::debug!("Waiting for greeting");

        match self
            .conn
            .src
            .read_frame::<Greeting, Formatter>(&mut self.fmt)
            .await
        {
            Ok(greeting) => {
                log::debug!("Read greeting {:?}", greeting);
                Ok(ServerHandshake2 {
                    conn: self.conn,
                    fmt: self.fmt,
                    greeting,
                })
            }
            Err(net_err) => bail!(net_err),
        }
    }
}

struct ServerHandshake2<R: Reader, W: Writer> {
    conn: Connection<R, W>,
    fmt: Formatter,
    greeting: Greeting,
}

impl<R: Reader, W: Writer> ServerHandshake2<R, W> {
    async fn send_choice(mut self) -> anyhow::Result<AuthOrCommand<R, W>> {
        // Must be socks version 5, or we close the connection without a response.
        if self.greeting.version != SOCKS_VERSION_5 {
            bail!("{}", Error::Version);
        }

        // Check the auth methods supported by the client.
        let methods = self.greeting.supported_auth_methods;

        // We support user/pass or none; prefer user/pass.
        if methods.iter().any(|&val| val == SOCKS_AUTH_USERPASS) {
            log::debug!("Choosing username/password authentication");

            let choice = Choice {
                version: SOCKS_VERSION_5,
                auth_method: SOCKS_AUTH_USERPASS,
            };

            match self
                .conn
                .dst
                .write_frame::<Choice, Formatter>(&mut self.fmt, choice)
                .await
            {
                Ok(_) => Ok(AuthOrCommand::Auth(ServerAuth1 {
                    conn: self.conn,
                    fmt: self.fmt,
                })),
                Err(net_err) => bail!(net_err),
            }
        } else if methods.iter().any(|&val| val == SOCKS_AUTH_NONE) {
            log::debug!("Choosing no authentication");

            let choice = Choice {
                version: SOCKS_VERSION_5,
                auth_method: SOCKS_AUTH_NONE,
            };

            match self
                .conn
                .dst
                .write_frame::<Choice, Formatter>(&mut self.fmt, choice)
                .await
            {
                Ok(_) => {
                    log::debug!("Wrote choice");
                    Ok(AuthOrCommand::Command(ServerCommand1 {
                        conn: self.conn,
                        fmt: self.fmt,
                        username: None,
                    }))
                }
                Err(net_err) => bail!("Error writing choice: {}", net_err),
            }
        } else {
            log::debug!("Authentication methods are unsupported");

            let choice = Choice {
                version: SOCKS_VERSION_5,
                auth_method: SOCKS_AUTH_UNSUPPORTED,
            };

            // Do not propagate any net error; the socks error is more precise.
            match self
                .conn
                .dst
                .write_frame::<Choice, Formatter>(&mut self.fmt, choice)
                .await
            {
                Ok(_) => log::debug!("Success writing choice failure message"),
                Err(e) => log::debug!("Error writing choice failure message: {}", e),
            }

            bail!("{}", Error::AuthMethod);
        }
    }
}

enum AuthOrCommand<R: Reader, W: Writer> {
    Auth(ServerAuth1<R, W>),
    Command(ServerCommand1<R, W>),
}

struct ServerAuth1<R: Reader, W: Writer> {
    conn: Connection<R, W>,
    fmt: Formatter,
}

impl<R: Reader, W: Writer> ServerAuth1<R, W> {
    async fn recv_auth_request(mut self) -> anyhow::Result<ServerAuth2<R, W>> {
        log::debug!("Waiting for auth request");

        match self
            .conn
            .src
            .read_frame::<UserPassAuthRequest, Formatter>(&mut self.fmt)
            .await
        {
            Ok(auth_request) => {
                log::debug!("Read auth request {:?}", auth_request);
                Ok(ServerAuth2 {
                    conn: self.conn,
                    fmt: self.fmt,
                    auth_request,
                })
            }
            Err(net_err) => bail!(net_err),
        }
    }
}

struct ServerAuth2<R: Reader, W: Writer> {
    conn: Connection<R, W>,
    fmt: Formatter,
    auth_request: UserPassAuthRequest,
}

impl<R: Reader, W: Writer> ServerAuth2<R, W> {
    async fn send_auth_response(mut self) -> anyhow::Result<ServerCommand1<R, W>> {
        let err_msg_opt = {
            if self.auth_request.version != SOCKS_AUTH_USERPASS_VERSION {
                Some(String::from("Invalid username/password auth version"))
            } else if self.auth_request.username.is_empty() {
                Some(String::from("Username is empty"))
            } else if self.auth_request.password.is_empty() {
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
                .conn
                .dst
                .write_frame::<UserPassAuthResponse, Formatter>(&mut self.fmt, response)
                .await
            {
                Ok(_) => log::debug!("Success writing auth failure message"),
                Err(e) => log::debug!("Error writing auth failure message: {}", e),
            }

            bail!("{}", Error::Auth(err_msg));
        }

        let response = UserPassAuthResponse {
            version: SOCKS_AUTH_USERPASS_VERSION,
            status: SOCKS_AUTH_STATUS_SUCCESS,
        };

        match self
            .conn
            .dst
            .write_frame::<UserPassAuthResponse, Formatter>(&mut self.fmt, response)
            .await
        {
            Ok(_) => Ok(ServerCommand1 {
                conn: self.conn,
                fmt: self.fmt,
                username: Some(self.auth_request.username),
            }),
            Err(net_err) => bail!(net_err),
        }
    }
}

struct ServerCommand1<R: Reader, W: Writer> {
    conn: Connection<R, W>,
    fmt: Formatter,
    username: Option<String>,
}

impl<R: Reader, W: Writer> ServerCommand1<R, W> {
    async fn recv_connect_request(mut self) -> anyhow::Result<ServerCommand2<R, W>> {
        log::debug!("Waiting for connect request");

        match self
            .conn
            .src
            .read_frame::<ConnectRequest, Formatter>(&mut self.fmt)
            .await
        {
            Ok(request) => {
                log::debug!("Read connect request {:?}", request);
                Ok(ServerCommand2 {
                    conn: self.conn,
                    fmt: self.fmt,
                    username: self.username,
                    request,
                })
            }
            Err(net_err) => bail!(net_err),
        }
    }
}

struct ServerCommand2<R: Reader, W: Writer> {
    conn: Connection<R, W>,
    fmt: Formatter,
    username: Option<String>,
    request: ConnectRequest,
}

impl<R: Reader, W: Writer> ServerCommand2<R, W> {
    async fn send_connect_response<C: Connector<R, W>>(
        mut self,
        connector: C,
    ) -> anyhow::Result<(Connection<R, W>, Connection<R, W>, Option<String>)> {
        if self.request.version != SOCKS_VERSION_5 {
            try_write_connect_err(&mut self.conn, &mut self.fmt, SOCKS_STATUS_PROTO_ERR).await;
            bail!("{}", Error::Version);
        } else if self.request.command != SOCKS_COMMAND_CONNECT {
            try_write_connect_err(&mut self.conn, &mut self.fmt, SOCKS_STATUS_PROTO_ERR).await;
            bail!("{}", Error::ConnectMethod);
        } else if self.request.reserved != SOCKS_NULL {
            try_write_connect_err(&mut self.conn, &mut self.fmt, SOCKS_STATUS_PROTO_ERR).await;
            bail!("{}", Error::Reserved);
        }

        // TODO: we should follow the bind addr configured in the env, if any.

        let dest_addr = match self.request.dest_addr {
            Socks5Address::IpAddr(a) => SocketAddr::new(a, self.request.dest_port),
            _ => {
                try_write_connect_err(&mut self.conn, &mut self.fmt, SOCKS_STATUS_ADDR_ERR).await;
                bail!(
                    "{}",
                    Error::Connect(String::from("Address type not supported"))
                );
            }
        };

        let (new_conn, local_addr) = connector.connect(dest_addr).await?;

        let response = ConnectResponse {
            version: SOCKS_VERSION_5,
            status: SOCKS_STATUS_REQ_GRANTED,
            reserved: SOCKS_NULL,
            bind_addr: Socks5Address::IpAddr(local_addr.ip()),
            bind_port: local_addr.port(),
        };

        self.conn
            .dst
            .write_frame::<ConnectResponse, Formatter>(&mut self.fmt, response)
            .await?;
        Ok((self.conn, new_conn, self.username))
    }
}

async fn try_write_connect_err<R: Reader, W: Writer>(
    conn: &mut Connection<R, W>,
    fmt: &mut Formatter,
    status: u8,
) {
    let response = ConnectResponse {
        version: SOCKS_VERSION_5,
        status,
        reserved: SOCKS_NULL,
        bind_addr: Socks5Address::Unknown,
        bind_port: 0,
    };

    // Do not propagate any net error; the socks error is more precise.
    match conn
        .dst
        .write_frame::<ConnectResponse, Formatter>(fmt, response)
        .await
    {
        Ok(_) => log::debug!("Success writing connect failure message"),
        Err(e) => log::debug!("Error writing connect failure message: {}", e),
    }
}
