use bytes::Buf;
use std::io::{self, Cursor, BufWriter, Write, BufRead};
use std::net::TcpStream;
use typestate::typestate;

use tokio::io::AsyncWriteExt;

use crate::net::{self, Frame};

pub mod server;

pub enum Error {
    IoError(io::Error),
}

pub enum Socks5AddressData {
    Ipv4([u8; 4]),
    Ipv6([u8; 16]),
    Name {
        name_len: u8,
        name: String
    }
}

pub struct Socks5Address {
    addr_type: u8,
    addr: Socks5AddressData,
}

pub struct Greeting {
    version: u8,
    num_auth_methods: u8,
    supported_auth_methods: Vec<u8>
}

pub struct Choice {
    version: u8,
    chosen_auth_method: u8
}

pub struct UserPassAuthRequest{
    version: u8,
    username_len: u8,
    username: String,
    password_len: u8,
    password: String
}

pub struct UserPassAuthResponse {
    version: u8,
    status: u8,
}

pub struct ConnectRequest {
    version: u8,
    command: u8,
    reserved: u8,
    dest_addr: Socks5Address,
    dest_port: u16
}

pub struct ConnectResponse {
    version: u8,
    status: u8,
    reserved: u8,
    bind_addr: Socks5Address,
    bind_port: u16
}

#[typestate]
mod socks5_protocol {
    use super::*;
    use crate::net::Connection;

    use async_trait::async_trait;

    #[automaton]
    pub struct Socks5Protocol;

    #[state] pub struct Initialization {
        pub conn: Connection
    }
    pub trait Initialization {
        fn new(conn: Connection) -> Initialization;
        fn start(self) -> ClientHandshake;
    }

    #[state] pub struct ClientHandshake {
        pub conn: Connection
    }
    #[async_trait]
    pub trait ClientHandshake {
        async fn greeting(self) -> ClientHandshakeResult;
    }
    pub enum ClientHandshakeResult {
        ServerHandshake, Error
    }

    #[state] pub struct ServerHandshake {
        pub conn: Connection,
        pub greeting: Greeting,
    }
    #[async_trait]
    pub trait ServerHandshake {
        async fn choice(self) -> ServerHandshakeResult;
    }
    pub enum ServerHandshakeResult {
        ClientAuthentication, ClientCommand, Error
    }

    #[state] pub struct ClientAuthentication {
        pub conn: Connection,
        pub choice: Choice,
    }
    #[async_trait]
    pub trait ClientAuthentication {
        async fn auth_request(self) -> ClientAuthenticationResult;
    }
    pub enum ClientAuthenticationResult {
        ServerAuthentication, Error
    }

    #[state] pub struct ServerAuthentication {
        pub conn: Connection,
        pub auth_request: UserPassAuthRequest,
    }
    #[async_trait]
    pub trait ServerAuthentication {
        async fn auth_response(self) -> ServerAuthenticationResult;
    }
    pub enum ServerAuthenticationResult {
        ClientCommand, Error
    }

    #[state] pub struct ClientCommand {
        pub conn: Connection,
        pub auth_response: Option<UserPassAuthResponse>,
    }
    #[async_trait]
    pub trait ClientCommand {
        async fn connect_request(self) -> ClientCommandResult;
    }
    pub enum ClientCommandResult {
        ServerCommand, Error
    }

    #[state] pub struct ServerCommand {
        pub conn: Connection,
        pub request: ConnectRequest,
    }
    #[async_trait]
    pub trait ServerCommand {
        async fn connect_response(self) -> ServerCommandResult;
    }
    pub enum ServerCommandResult {
        Success, Error
    }

    #[state] pub struct Success {
        pub conn: Connection,
        pub response: ConnectResponse
    }
    pub trait Success {
        fn take(self) -> Connection;
    }

    #[state] pub struct Error {
        pub message: String
    }
    pub trait Error {
        fn take(self) -> String;
    }
}

fn get_u8(src: &mut Cursor<&[u8]>) -> Option<u8> {
    if !src.has_remaining() {
        return None;
    }
    Some(src.get_u8())
}

impl Frame<Greeting> for Greeting {
    fn parse(src: &mut Cursor<&[u8]>) -> Option<Greeting> {
        let version = get_u8(src)?;
        let num_auth_methods = get_u8(src)?;

        let mut supported_auth_methods = Vec::new();
        for _ in 0..num_auth_methods {
            supported_auth_methods.push(get_u8(src)?);
        }

        Some(Greeting {
            version,
            num_auth_methods,
            supported_auth_methods
        })
    }

    fn write<W: AsyncWriteExt>(&self, dst: &W) -> Result<(), net::Error> {
        todo!()
    }
}
