use std::{collections::HashMap, fmt};

use crate::{
    lang::ProteusSpecification,
    net::{self, proto::proteus::{self, protocol::ProteusProtocol}, Connection},
};

pub mod action;
pub mod format;
mod message;
pub mod protocol;

pub enum Role {
    Client,
    Server,
}

#[derive(Debug)]
pub enum Error {
    Option(String),
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
            Error::Option(s) => write!(f, "Option failed: {}", s),
            Error::Protocol(s) => write!(f, "Protocol failed: {}", s),
            Error::Network(e) => write!(f, "Network error: {}", e),
        }
    }
}

pub async fn run_proteus(
    proteus_conn: Connection,
    other_conn: Connection,
    options: HashMap<String, String>,
    spec: &ProteusSpecification,
    role: Role,
) -> Result<(), proteus::Error> {
    let mut proto = ProteusProtocol::new(role);
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test() {}
}
