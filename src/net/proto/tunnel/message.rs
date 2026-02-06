use std::fmt::Debug;

use bytes::Bytes;

use crate::net::proto::socks::address::Socks5Target;

#[derive(Debug, PartialEq, Clone)]
pub struct TunnelMessage {
    pub id: u64,
    pub kind: TunnelMessageKind,
}

#[derive(PartialEq, Clone)]
pub enum TunnelMessageKind {
    /// Client requests tunnel id from server for later tunnel resumption.
    /// Use id=0 to start a new tunnel, or id>0 to attempt to resume a previous.
    Open,
    Opened,
    /// Peers notifying a close event occurred.
    Close,
    Closed,
    /// Client requests the server to open a stream.
    Connect(Socks5Target),
    /// Peers forwarding encapsulated stream bytes to each other.
    Encapsulate(Bytes),
}

impl TunnelMessage {
    pub fn open(tunnel_id: u64) -> Self {
        Self {
            id: tunnel_id,
            kind: TunnelMessageKind::Open,
        }
    }

    pub fn opened(tunnel_id: u64) -> Self {
        Self {
            id: tunnel_id,
            kind: TunnelMessageKind::Opened,
        }
    }

    pub fn close() -> Self {
        Self {
            id: 0,
            kind: TunnelMessageKind::Close,
        }
    }

    pub fn closed() -> Self {
        Self {
            id: 0,
            kind: TunnelMessageKind::Closed,
        }
    }

    pub fn connect(session_id: u64, target: Socks5Target) -> Self {
        Self {
            id: session_id,
            kind: TunnelMessageKind::Connect(target),
        }
    }

    pub fn encapsulate(session_id: u64, bytes: Bytes) -> Self {
        Self {
            id: session_id,
            kind: TunnelMessageKind::Encapsulate(bytes),
        }
    }
}

impl Debug for TunnelMessageKind {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            TunnelMessageKind::Open => write!(f, "Open"),
            TunnelMessageKind::Opened => write!(f, "Opened"),
            TunnelMessageKind::Close => write!(f, "Close"),
            TunnelMessageKind::Closed => write!(f, "Closed"),
            TunnelMessageKind::Connect(target) => write!(f, "Connect({target:?})"),
            TunnelMessageKind::Encapsulate(bytes) => {
                write!(f, "Encapsulate(len: {})", bytes.len())
            }
        }
    }
}
