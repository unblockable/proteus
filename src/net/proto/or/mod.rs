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
    let proto = ExtOrProtocol::new(conn).start();

    let proto = match proto.greeting().await {
        ClientHandshakeResult::ServerHandshake(s) => s,
        ClientHandshakeResult::Error(e) => return Err(e.finish()),
    };

    let proto = match proto.choice().await {
        ServerHandshakeResult::ClientAuthNonce(s) => s,
        ServerHandshakeResult::Error(e) => return Err(e.finish()),
    };

    let proto = match proto.auth_nonce().await {
        ClientAuthNonceResult::ServerAuthNonceHash(s) => s,
        ClientAuthNonceResult::Error(e) => return Err(e.finish()),
    };

    let proto = match proto.auth_nonce_hash().await {
        ServerAuthNonceHashResult::ClientAuthHash(s) => s,
        ServerAuthNonceHashResult::Error(e) => return Err(e.finish()),
    };

    let proto = match proto.auth_hash().await {
        ClientAuthHashResult::ServerAuthStatus(s) => s,
        ClientAuthHashResult::Error(e) => return Err(e.finish()),
    };

    let proto = match proto.auth_status().await {
        ServerAuthStatusResult::ClientCommand(s) => s,
        ServerAuthStatusResult::Error(e) => return Err(e.finish()),
    };

    // Keep processing commands until done.
    let mut client_cmd = proto;
    loop {
        let server_cmd = match client_cmd.command().await {
            ClientCommandResult::ServerCommand(s) => s,
            ClientCommandResult::Error(e) => return Err(e.finish()),
        };

        client_cmd = match server_cmd.reply().await {
            ServerCommandResult::ClientCommand(s) => s,
            ServerCommandResult::Success(s) => return Ok(s.finish()),
            ServerCommandResult::Error(e) => return Err(e.finish()),
        };
    }
}
