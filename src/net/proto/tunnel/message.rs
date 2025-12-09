use std::fmt::Debug;

use bytes::Bytes;

use crate::net::proto::socks::address::Socks5Target;

#[derive(Debug, PartialEq, Clone)]
pub struct TunnelMessage {
    pub session_id: u64,
    pub kind: TunnelMessageKind,
}

#[derive(PartialEq, Clone)]
pub enum TunnelMessageKind {
    Open(Socks5Target),
    Encapsulated(Bytes),
    Close,
}

impl TunnelMessage {
    pub fn open(session_id: u64, target: Socks5Target) -> Self {
        Self {
            session_id,
            kind: TunnelMessageKind::Open(target),
        }
    }

    pub fn encapsulated(session_id: u64, bytes: Bytes) -> Self {
        Self {
            session_id,
            kind: TunnelMessageKind::Encapsulated(bytes),
        }
    }

    pub fn close() -> Self {
        Self {
            session_id: 0,
            kind: TunnelMessageKind::Close,
        }
    }
}

impl Debug for TunnelMessageKind {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            TunnelMessageKind::Open(target) => write!(f, "Open({target:?})"),
            TunnelMessageKind::Encapsulated(bytes) => {
                write!(f, "Encapsulated(len: {})", bytes.len())
            }
            TunnelMessageKind::Close => write!(f, "Close"),
        }
    }
}
