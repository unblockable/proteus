use std::fmt;
use typestate::typestate;

use crate::net;

pub mod client;
pub mod generator;
pub mod server;

#[typestate]
mod upgen_protocol {
    use crate::net::proto::upgen;
    use crate::net::Connection;

    use async_trait::async_trait;

    #[automaton]
    pub struct UpgenProtocol;

    #[state]
    pub struct Init {
        pub client_conn: Connection,
        pub server_conn: Connection,
    }
    pub trait Init {
        fn new(client_conn: Connection, server_conn: Connection) -> Init;
        fn start_client(self) -> ClientHandshake1;
        fn start_server(self) -> ServerHandshake1;
    }

    #[state]
    pub struct ClientHandshake1 {
        pub client_conn: Connection,
        pub server_conn: Connection,
    }
    #[async_trait]
    pub trait ClientHandshake1 {
        async fn send_handshake1(self) -> ClientHandshake1Result;
    }
    pub enum ClientHandshake1Result {
        ClientHandshake2,
        Error,
    }

    #[state]
    pub struct ClientHandshake2 {
        pub client_conn: Connection,
        pub server_conn: Connection,
    }
    #[async_trait]
    pub trait ClientHandshake2 {
        async fn recv_handshake2(self) -> ClientHandshake2Result;
    }
    pub enum ClientHandshake2Result {
        ClientData,
        Error,
    }

    #[state]
    pub struct ClientData {
        pub client_conn: Connection,
        pub server_conn: Connection,
    }
    #[async_trait]
    pub trait ClientData {
        async fn forward_data(self) -> ClientDataResult;
    }
    pub enum ClientDataResult {
        ClientData,
        Success,
        Error,
    }

    #[state]
    pub struct ServerHandshake1 {
        pub client_conn: Connection,
        pub server_conn: Connection,
    }
    #[async_trait]
    pub trait ServerHandshake1 {
        async fn recv_handshake1(self) -> ServerHandshake1Result;
    }
    pub enum ServerHandshake1Result {
        ServerHandshake2,
        Error,
    }

    #[state]
    pub struct ServerHandshake2 {
        pub client_conn: Connection,
        pub server_conn: Connection,
    }
    #[async_trait]
    pub trait ServerHandshake2 {
        async fn send_handshake2(self) -> ServerHandshake2Result;
    }
    pub enum ServerHandshake2Result {
        ServerData,
        Error,
    }

    #[state]
    pub struct ServerData {
        pub client_conn: Connection,
        pub server_conn: Connection,
    }
    #[async_trait]
    pub trait ServerData {
        async fn forward_data(self) -> ServerDataResult;
    }
    pub enum ServerDataResult {
        ServerData,
        Success,
        Error,
    }

    #[state]
    pub struct Success {
        pub client_conn: Connection,
        pub server_conn: Connection,
    }
    pub trait Success {
        fn finish(self);
    }

    #[state]
    pub struct Error {
        pub error: upgen::Error,
    }
    pub trait Error {
        fn finish(self) -> upgen::Error;
    }

    impl From<Init> for UpgenProtocol<Init> {
        fn from(state: Init) -> Self {
            UpgenProtocol::<Init> { state: state }
        }
    }

    impl From<ClientHandshake1> for UpgenProtocol<ClientHandshake1> {
        fn from(state: ClientHandshake1) -> Self {
            UpgenProtocol::<ClientHandshake1> { state: state }
        }
    }

    impl From<ClientHandshake2> for UpgenProtocol<ClientHandshake2> {
        fn from(state: ClientHandshake2) -> Self {
            UpgenProtocol::<ClientHandshake2> { state: state }
        }
    }

    impl From<ServerHandshake1> for UpgenProtocol<ServerHandshake1> {
        fn from(state: ServerHandshake1) -> Self {
            UpgenProtocol::<ServerHandshake1> { state: state }
        }
    }

    impl From<ServerHandshake2> for UpgenProtocol<ServerHandshake2> {
        fn from(state: ServerHandshake2) -> Self {
            UpgenProtocol::<ServerHandshake2> { state: state }
        }
    }

    impl From<ClientData> for UpgenProtocol<ClientData> {
        fn from(state: ClientData) -> Self {
            UpgenProtocol::<ClientData> { state: state }
        }
    }

    impl From<ServerData> for UpgenProtocol<ServerData> {
        fn from(state: ServerData) -> Self {
            UpgenProtocol::<ServerData> { state: state }
        }
    }

    impl From<Success> for UpgenProtocol<Success> {
        fn from(state: Success) -> Self {
            UpgenProtocol::<Success> { state: state }
        }
    }

    impl From<Error> for UpgenProtocol<Error> {
        fn from(state: Error) -> Self {
            UpgenProtocol::<Error> { state: state }
        }
    }
}

pub enum Error {
    ClientHandshake(String),
    ServerHandshake(String),
    Network(net::Error),
}

impl From<net::Error> for Error {
    fn from(e: net::Error) -> Self {
        Error::Network(e)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::ClientHandshake(s) => write!(f, "Client handshake failed: {}", s),
            Error::ServerHandshake(s) => write!(f, "Server handshake failed: {}", s),
            Error::Network(e) => write!(f, "Network error: {}", e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_handshake() {
    }

    #[test]
    fn server_handshake() {
    }

    #[test]
    fn data() {
    }
}
