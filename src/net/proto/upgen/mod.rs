use std::fmt;
use typestate::typestate;

use crate::net;

pub mod client;
pub mod generator;
pub mod server;

#[typestate]
mod one_round_automaton {
    use crate::net::proto::upgen;
    use crate::net::Connection;

    use async_trait::async_trait;

    #[automaton]
    pub struct UpgenProtocol;

    #[state]
    pub struct Initialization {
        pub client_conn: Connection,
        pub bridge_conn: Connection,
    }
    pub trait Initialization {
        fn new(client_conn: Connection, bridge_conn: Connection) -> Initialization;
        fn start(self) -> ClientHandshake;
    }

    #[state]
    pub struct ClientHandshake {
        pub client_conn: Connection,
        pub bridge_conn: Connection,
    }
    #[async_trait]
    pub trait ClientHandshake {
        async fn request(self) -> ClientHandshakeResult;
    }
    pub enum ClientHandshakeResult {
        ServerHandshake,
        Error,
    }

    #[state]
    pub struct ServerHandshake {
        pub client_conn: Connection,
        pub bridge_conn: Connection,
    }
    #[async_trait]
    pub trait ServerHandshake {
        async fn response(self) -> ServerHandshakeResult;
    }
    pub enum ServerHandshakeResult {
        Data,
        Error,
    }

    #[state]
    pub struct Data {
        pub client_conn: Connection,
        pub bridge_conn: Connection,
    }
    #[async_trait]
    pub trait Data {
        async fn data(self) -> DataResult;
    }
    pub enum DataResult {
        Data,
        Success,
        Error,
    }

    #[state]
    pub struct Success {
        pub client_conn: Connection,
        pub bridge_conn: Connection,
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

    impl From<Initialization> for UpgenProtocol<Initialization> {
        fn from(state: Initialization) -> Self {
            UpgenProtocol::<Initialization> { state: state }
        }
    }

    impl From<ClientHandshake> for UpgenProtocol<ClientHandshake> {
        fn from(state: ClientHandshake) -> Self {
            UpgenProtocol::<ClientHandshake> { state: state }
        }
    }

    impl From<ServerHandshake> for UpgenProtocol<ServerHandshake> {
        fn from(state: ServerHandshake) -> Self {
            UpgenProtocol::<ServerHandshake> { state: state }
        }
    }

    impl From<Data> for UpgenProtocol<Data> {
        fn from(state: Data) -> Self {
            UpgenProtocol::<Data> { state: state }
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
