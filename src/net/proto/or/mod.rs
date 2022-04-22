use std::fmt;

use crate::net::{
    self,
    proto::or::{self, spec::extor::*},
    Connection,
};

mod frames;
mod spec;
mod states;

pub enum Error {
    AuthMethod,
    AuthStatusFailed,
    AuthStatusUnknown,
    Auth(String),
    Address(String),
    Transport(String),
    Command(String),
    Network(net::Error),
}

impl From<net::Error> for or::Error {
    fn from(e: net::Error) -> Self {
        Error::Network(e)
    }
}

impl fmt::Display for or::Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::AuthMethod => write!(f, "Ext OR authentication method unsupported"),
            Error::AuthStatusFailed => write!(f, "Ext OR authentication failed"),
            Error::AuthStatusUnknown => write!(f, "Ext OR authentication status unknown"),
            Error::Auth(s) => write!(f, "Chosen Ext OR authentication method failed: {}", s),
            Error::Address(s) => write!(f, "User address denied: {}", s),
            Error::Transport(s) => write!(f, "Transport denied: {}", s),
            Error::Command(s) => write!(f, "Command denied: {}", s),
            Error::Network(e) => write!(f, "Network error: {}", e),
        }
    }
}

pub async fn run_extor_client(conn: Connection) -> Result<Connection, or::Error> {
    let proto = ExtOrProtocol::new(conn).start_client();

    let proto = match proto.recv_greeting().await {
        ClientHandshake1Result::ClientHandshake2(s) => s,
        ClientHandshake1Result::Error(e) => return Err(e.finish()),
    };

    let proto = match proto.send_choice().await {
        ClientHandshake2Result::ClientAuth1(s) => s,
        ClientHandshake2Result::Error(e) => return Err(e.finish()),
    };

    let proto = match proto.send_nonce().await {
        ClientAuth1Result::ClientAuth2(s) => s,
        ClientAuth1Result::Error(e) => return Err(e.finish()),
    };

    let proto = match proto.recv_nonce_hash().await {
        ClientAuth2Result::ClientAuth3(s) => s,
        ClientAuth2Result::Error(e) => return Err(e.finish()),
    };

    let proto = match proto.send_hash().await {
        ClientAuth3Result::ClientAuth4(s) => s,
        ClientAuth3Result::Error(e) => return Err(e.finish()),
    };

    let proto = match proto.recv_status().await {
        ClientAuth4Result::ClientCommand1(s) => s,
        ClientAuth4Result::Error(e) => return Err(e.finish()),
    };

    // Keep processing commands until done.
    let mut part1 = proto;
    loop {
        let part2 = match part1.send_command().await {
            ClientCommand1Result::ClientCommand2(s) => s,
            ClientCommand1Result::Error(e) => return Err(e.finish()),
        };

        part1 = match part2.recv_reply().await {
            ClientCommand2Result::ClientCommand1(s) => s,
            ClientCommand2Result::Success(s) => return Ok(s.finish()),
            ClientCommand2Result::Error(e) => return Err(e.finish()),
        };
    }
}
