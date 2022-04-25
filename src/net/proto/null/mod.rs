use std::fmt;

use crate::net::{
    self,
    proto::null::{self, spec::null::*},
    Connection,
};

mod frames;
mod spec;
mod states;

pub enum Error {
    Network(net::Error),
}

impl From<net::Error> for null::Error {
    fn from(e: net::Error) -> Self {
        Error::Network(e)
    }
}

impl fmt::Display for null::Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Network(e) => write!(f, "Network error: {}", e),
        }
    }
}

async fn run_data_loop(mut proto: NullProtocol<Data>) -> Result<(), null::Error> {
    loop {
        proto = match proto.forward_data().await {
            DataResult::Data(s) => s,
            DataResult::Success(s) => return Ok(s.finish()),
            DataResult::Error(e) => return Err(e.finish()),
        };
    }
}

pub async fn run_null_client(
    client_conn: Connection,
    server_conn: Connection,
) -> Result<(), null::Error> {
    run_data_loop(NullProtocol::new(client_conn, server_conn).start_client()).await
}

pub async fn run_null_server(
    client_conn: Connection,
    server_conn: Connection,
) -> Result<(), null::Error> {
    run_data_loop(NullProtocol::new(client_conn, server_conn).start_server()).await
}
