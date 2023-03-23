use std::{collections::HashMap, fmt};

use bytes::Buf;

use crate::{
    lang::{ProteusSpecification, Role, action::ActionKind},
    net::{self, proto::proteus::{self, spec::ProteusProtocol, message::CovertPayload, formatter::Formatter}, Connection},
};

mod formatter;
mod message;
pub mod spec;

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
    let mut proto = ProteusProtocol::new(spec.clone(), role.clone());

    // Get the source and sink ends so we can forward data concurrently.
    let (mut proteus_source, mut proteus_sink) = proteus_conn.into_split();
    let (mut other_source, mut other_sink) = other_conn.into_split();

    let mut formatter = Formatter::new();

    loop {
        let action = proto.get_next_action();

        match action.get_kind(role.clone()) {
            ActionKind::Send => {
                // Read the raw covert data stream.
                let bytes = match other_source.read_bytes().await {
                    Ok(b) => b,
                    Err(net_err) => match net_err {
                        net::Error::Eof => break,
                        _ => return Err(proteus::Error::from(net_err)),
                    },
                };

                log::trace!("obfuscate: read {} app bytes", bytes.len());

                if bytes.has_remaining() {
                    let payload = CovertPayload { data: bytes };
                    let message = proto.pack_message(payload);
        
                    let num_written = match proteus_sink.write_frame(&mut formatter, message).await {
                        Ok(num) => num,
                        Err(e) => return Err(proteus::Error::from(e)),
                    };
        
                    log::trace!("obfuscate: wrote {} covert bytes", num_written);
                }
            },
            ActionKind::Receive => {
                todo!()
            }
        }
    }
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test() {}
}
