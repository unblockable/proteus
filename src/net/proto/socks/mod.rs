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
    let proto = Socks5Protocol::new(conn).start_server();

    let proto = match proto.recv_greeting().await {
        ServerHandshake1Result::ServerHandshake2(s) => s,
        ServerHandshake1Result::Error(e) => return Err(e.finish()),
    };

    let proto = match proto.send_choice().await {
        ServerHandshake2Result::ServerAuth1(s) => {
            let auth = match s.recv_auth_request().await {
                ServerAuth1Result::ServerAuth2(s) => s,
                ServerAuth1Result::Error(e) => return Err(e.finish()),
            };

            match auth.send_auth_response().await {
                ServerAuth2Result::ServerCommand1(s) => s,
                ServerAuth2Result::Error(e) => return Err(e.finish()),
            }
        }
        ServerHandshake2Result::ServerCommand1(s) => s,
        ServerHandshake2Result::Error(e) => return Err(e.finish()),
    };

    let proto = match proto.recv_connect_request().await {
        ServerCommand1Result::ServerCommand2(s) => s,
        ServerCommand1Result::Error(e) => return Err(e.finish()),
    };

    let proto = match proto.send_connect_response().await {
        ServerCommand2Result::Success(s) => s,
        ServerCommand2Result::Error(e) => return Err(e.finish()),
    };

    Ok(proto.finish())
}
