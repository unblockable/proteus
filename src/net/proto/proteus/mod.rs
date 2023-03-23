use std::{collections::HashMap, fmt};

use crate::{
    lang::{ProteusSpecification, Role},
    net::{
        self,
        proto::proteus::{self, formatter::Formatter, spec::proteus::*},
        Connection,
    },
};

mod formatter;
mod frames;
mod spec;
mod states;

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
    spec: ProteusSpecification,
    role: Role,
) -> Result<(), proteus::Error> {
    let fmt = Formatter::new();
    let proto = ProteusProtocol::new(other_conn, proteus_conn, spec, fmt).start(role);

    match proto.run().await {
        ActionResult::Success(s) => Ok(s.finish()),
        ActionResult::Error(e) => Err(e.finish()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test() {}
}
