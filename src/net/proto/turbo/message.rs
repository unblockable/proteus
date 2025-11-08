use std::fmt::Debug;

use bytes::Bytes;

use crate::net::proto::socks::address::Socks5Target;

pub type DataCursor = u64;

#[derive(Debug, PartialEq, Clone)]
pub struct Message {
    pub session_id: u64,
    /// Similar to TCP sequence number.
    pub write: DataCursor,
    /// Similar to TCP acknowledgment number.
    pub read: DataCursor,
    pub command: Command,
}

#[derive(Debug, PartialEq, Clone)]
pub enum Command {
    Request(Request),
    Response(Response),
}

#[derive(Debug, PartialEq, Clone)]
pub enum Request {
    Open(Socks5Target),
    Forward(Payload),
    Rewind,
    Shut,
}

#[derive(Debug, PartialEq, Clone)]
pub enum Response {
    Open(Result),
    Forward(Result),
    Rewind(Result),
    Shut(Result),
}

#[derive(Debug, PartialEq, Clone)]
pub enum Result {
    Ok,
    Error,
}

#[derive(PartialEq, Clone)]
pub struct Payload {
    /// The application data payload bytes.
    pub data: Bytes,
}

impl From<Bytes> for Payload {
    fn from(value: Bytes) -> Self {
        Payload { data: value }
    }
}

impl Debug for Payload {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Payload(len: {}))", self.data.len())
    }
}

#[cfg(test)]
mod tests {
}