use std::cmp::Ordering;
use std::fmt::Debug;

use bytes::Bytes;

pub type DataCursor = u64;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TurboMessage {
    /// Similar to TCP sequence number.
    pub write: DataCursor,
    /// Similar to TCP acknowledgment number.
    pub read: DataCursor,
    pub command: Command,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Command {
    Forward(Payload),
    ForwardAck,
    Rewind,
    RewindAck,
    Shut,
    ShutAck,
    Reset,
}

#[derive(Clone, Eq, PartialEq)]
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
        write!(f, "Payload(len: {})", self.data.len())
    }
}

impl Ord for TurboMessage {
    fn cmp(&self, other: &Self) -> Ordering {
        self.write.cmp(&other.write)
    }
}

impl PartialOrd for TurboMessage {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl TurboMessage {
    pub fn new(write: DataCursor, read: DataCursor, command: Command) -> Self {
        Self {
            write,
            read,
            command,
        }
    }
}

#[cfg(test)]
impl TurboMessage{
    pub fn forward(write: DataCursor, read: DataCursor, payload: Bytes) -> Self {
        Self::new(write, read, Command::Forward(payload.into()))
    }

    pub fn forward_ack(write: DataCursor, read: DataCursor) -> Self {
        Self::new(write, read, Command::ForwardAck)
    }

    pub fn rewind(write: DataCursor, read: DataCursor) -> Self {
        Self::new(write, read, Command::Rewind)
    }

    pub fn rewind_ack(write: DataCursor, read: DataCursor) -> Self {
        Self::new(write, read, Command::RewindAck)
    }

    pub fn shut(write: DataCursor, read: DataCursor) -> Self {
        Self::new(write, read, Command::Shut)
    }

    pub fn shut_ack(write: DataCursor, read: DataCursor) -> Self {
        Self::new(write, read, Command::ShutAck)
    }

    pub fn reset(write: DataCursor, read: DataCursor) -> Self {
        Self {
            write,
            read,
            command: Command::Reset,
        }
    }
}

#[cfg(test)]
mod tests {}
