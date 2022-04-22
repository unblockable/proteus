use std::fmt;

use crate::net::{
    self,
    proto::socks::{
        self,
        spec::socks5::*,
    },
    Connection,
};

mod address;
mod frames;
mod states;
mod spec;

pub enum Error {
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

#[allow(dead_code)]
pub async fn run_socks5_client(_conn: Connection) -> Result<(Connection, Connection), socks::Error> {
    unimplemented!()
}

pub async fn run_socks5_server(conn: Connection) -> Result<(Connection, Connection), socks::Error> {
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
