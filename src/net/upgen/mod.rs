use bytes::{Buf, BufMut, BytesMut};
use std::{
    fmt,
    io::{Cursor},
};
use typestate::typestate;

use crate::net::{self, Frame, upgen::generator::FrameFormatSpec};

pub mod client;
pub mod generator;
pub mod server;

#[typestate]
mod upgen_protocol {
    use super::*;
    use crate::net::upgen;
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

#[derive(Debug, PartialEq)]
pub struct Handshake {
    spec: FrameFormatSpec,
}

#[derive(Debug, PartialEq)]
pub struct Data {
    spec: FrameFormatSpec,
    payload: BytesMut
}

impl Frame<Handshake> for Handshake {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<Handshake> {
        // TODO this isn't going to work.
        // We probably want a new trait called FrameSpec or something
        // where the derserialize function takes a frame spec in addition
        // to the buf.
        todo!()
    }

    fn serialize(&self) -> BytesMut {
        todo!()
    }
}

impl Frame<Data> for Data {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<Data> {
        todo!()
    }

    fn serialize(&self) -> BytesMut {
        todo!()
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
