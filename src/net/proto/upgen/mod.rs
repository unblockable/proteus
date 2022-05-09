use std::{collections::HashMap, fmt};

use crate::net::{
    self,
    proto::upgen::{self, spec::upgen::*},
    Connection,
};

use self::formatter::Formatter;
use self::generator::Generator;

mod crypto;
mod formatter;
mod frames;
mod generator;
mod protocols;
mod spec;
mod states;

#[derive(Debug)]
pub enum Error {
    Option(String),
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
            Error::Option(s) => write!(f, "Option failed: {}", s),
            Error::ClientHandshake(s) => write!(f, "Client handshake failed: {}", s),
            Error::ServerHandshake(s) => write!(f, "Server handshake failed: {}", s),
            Error::Network(e) => write!(f, "Network error: {}", e),
        }
    }
}

fn get_seed_option(options: &HashMap<String, String>) -> Result<u64, upgen::Error> {
    match options.get("seed") {
        Some(value) => match value.parse::<u64>() {
            Ok(seed) => Ok(seed),
            Err(e) => {
                Err(upgen::Error::Option(String::from(format!(
                    "error parsing seed from {}",
                    value
                ))))
            }
        },
        None => Err(upgen::Error::Option(String::from("missing seed option"))),
    }
}

pub async fn run_upgen_client(
    upgen_conn: Connection,
    other_conn: Connection,
    options: HashMap<String, String>,
) -> Result<(), upgen::Error> {
    let seed = get_seed_option(&options)?;

    let (overt_proto, crypto_proto) = Generator::new(seed).generate_overt_protocol();
    let fmt = Formatter::new(crypto_proto);

    let proto = UpgenProtocol::new(upgen_conn, other_conn, overt_proto, fmt).start_client();

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
    options: HashMap<String, String>,
) -> Result<(), upgen::Error> {
    let seed = get_seed_option(&options)?;

    let (overt_proto, crypto_proto) = Generator::new(seed).generate_overt_protocol();
    let fmt = Formatter::new(crypto_proto);

    let proto = UpgenProtocol::new(upgen_conn, other_conn, overt_proto, fmt).start_server();

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn option_seed() {
        let mut map = HashMap::new();
        map.insert(String::from("seed"), String::from("123"));
        assert_eq!(get_seed_option(&map).unwrap(), 123u64);
        map.insert(String::from("seed"), String::from("abc"));
        assert!(get_seed_option(&map).is_err());
    }
}
