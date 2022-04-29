use std::fmt;

use crate::net::{
    self,
    proto::upgen::{
        self,
        spec::upgen::*,
    },
    Connection,
};

use self::generator::Generator;

mod formatter;
mod frames;
mod generator;
mod protocols;
mod spec;
mod states;

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

pub async fn run_upgen_client(
    upgen_conn: Connection,
    other_conn: Connection,
    seed: u64,
) -> Result<(), upgen::Error> {
    let overt_proto = Generator::new(seed).generate_overt_protocol();
    let proto = UpgenProtocol::new(upgen_conn, other_conn, overt_proto).start_client();

    let proto = match proto.send_handshake1().await {
        ClientHandshake1Result::ClientHandshake2(s) => s,
        ClientHandshake1Result::Error(e) => return Err(e.finish()),
    };

    let proto = match proto.recv_handshake2().await {
        ClientHandshake2Result::Data(s) => s,
        ClientHandshake2Result::Error(e) => return Err(e.finish()),
    };

    match proto.forward_data().await {
        DataResult::Success(s) => Ok(s.finish()),
        DataResult::Error(e) => Err(e.finish()),
    }
}

pub async fn run_upgen_server(
    upgen_conn: Connection,
    other_conn: Connection,
    seed: u64,
) -> Result<(), upgen::Error> {
    let overt_proto = Generator::new(seed).generate_overt_protocol();
    let proto = UpgenProtocol::new(upgen_conn, other_conn, overt_proto).start_server();

    let proto = match proto.recv_handshake1().await {
        ServerHandshake1Result::ServerHandshake2(s) => s,
        ServerHandshake1Result::Error(e) => return Err(e.finish()),
    };

    let proto = match proto.send_handshake2().await {
        ServerHandshake2Result::Data(s) => s,
        ServerHandshake2Result::Error(e) => return Err(e.finish()),
    };

    match proto.forward_data().await {
        DataResult::Success(s) => Ok(s.finish()),
        DataResult::Error(e) => Err(e.finish()),
    }
}
