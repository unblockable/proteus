use std::{collections::HashMap, fmt};

use crate::{
    lang::spec::proteus::ProteusSpec,
    net::{
        self,
        proto::proteus::{self, spec::proteus::*},
        Connection,
    },
};

mod formatter;
mod frames;
mod spec;
mod states;

#[derive(Debug)]
pub enum Error {
    Protocol(String),
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
            Error::Protocol(s) => write!(f, "Protocol failed: {}", s),
            Error::Network(e) => write!(f, "Network error: {}", e),
        }
    }
}

pub async fn run_proteus(
    proteus_conn: Connection,
    other_conn: Connection,
    _options: HashMap<String, String>,
    spec: ProteusSpec,
) -> Result<(), proteus::Error> {
    match ProteusProtocol::new(other_conn, proteus_conn, spec)
        .run()
        .await
    {
        RunResult::Success(s) => Ok(s.finish()),
        RunResult::Error(e) => Err(e.finish()),
    }
}
