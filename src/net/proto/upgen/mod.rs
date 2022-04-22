use std::fmt;

use crate::net::{
    self,
    proto::upgen::{
        self,
        spec::upgen::*,
    },
    Connection,
};

mod generator;
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

async fn run_data_loop(mut proto: UpgenProtocol<Data>) -> Result<(), upgen::Error> {
    loop {
        proto = match proto.forward_data().await {
            DataResult::Data(s) => s,
            DataResult::Success(s) => return Ok(s.finish()),
            DataResult::Error(e) => return Err(e.finish()),
        };
    }
}

pub async fn run_upgen_client(
    client_conn: Connection,
    server_conn: Connection,
) -> Result<(), upgen::Error> {
    let seed: u64 = 123456;
    let proto = UpgenProtocol::new(client_conn, server_conn, seed).start_client();

    let proto = match proto.send_handshake1().await {
        ClientHandshake1Result::ClientHandshake2(s) => s,
        ClientHandshake1Result::Error(e) => return Err(e.finish()),
    };

    let proto = match proto.recv_handshake2().await {
        ClientHandshake2Result::Data(s) => s,
        ClientHandshake2Result::Error(e) => return Err(e.finish()),
    };
    
    run_data_loop(proto).await
}

pub async fn run_upgen_server(
    client_conn: Connection,
    server_conn: Connection,
) -> Result<(), upgen::Error> {
    let seed: u64 = 123456;
    let proto = UpgenProtocol::new(client_conn, server_conn, seed).start_server();

    let proto = match proto.recv_handshake1().await {
        ServerHandshake1Result::ServerHandshake2(s) => s,
        ServerHandshake1Result::Error(e) => return Err(e.finish()),
    };

    let proto = match proto.send_handshake2().await {
        ServerHandshake2Result::Data(s) => s,
        ServerHandshake2Result::Error(e) => return Err(e.finish()),
    };

    run_data_loop(proto).await
}