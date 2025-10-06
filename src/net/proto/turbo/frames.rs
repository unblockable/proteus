use std::fmt::Debug;
use std::io::Cursor;
use std::net::{IpAddr, SocketAddr};

use bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::net::proto::socks::address::Socks5Address;
use crate::net::{Deserialize, Serialize};

pub type DataCursor = u64;

#[derive(Debug, PartialEq)]
pub struct Message {
    pub session_id: u64,
    /// Similar to TCP sequence number.
    pub write: DataCursor,
    /// Similar to TCP acknowledgment number.
    pub read: DataCursor,
    pub command: Command,
}

#[derive(Debug, PartialEq)]
pub enum Command {
    Connect(Target),
    ConnectOk,
    Resume,
    ResumeOk,
    Forward(Payload),
    ForwardOk,
    Shut,
    ShutOk,
    /// An invalid value found during deserialization.
    Invalid,
}

#[derive(Debug, PartialEq)]
pub struct Target {
    /// The target host address to which the server side of the tunnel should connect.
    pub addr: Socks5Address,
    /// The target host port to which the server side of the tunnel should connect.
    pub port: u16,
}

#[derive(PartialEq)]
pub struct Payload {
    /// The application data payload bytes.
    pub data: Bytes,
}

impl Serialize<Message> for Message {
    fn serialize(&self) -> Bytes {
        let mut buf = BytesMut::new();

        buf.put_u64(self.session_id);
        buf.put_slice(&self.write.serialize());
        buf.put_slice(&self.read.serialize());
        buf.put_slice(&self.command.serialize());

        buf.freeze()
    }
}

impl Deserialize<Message> for Message {
    fn deserialize(src: &mut Cursor<&BytesMut>) -> Option<Message> {
        let session_id = (src.remaining() >= 8).then(|| src.get_u64())?;
        let write = DataCursor::deserialize(src)?;
        let read = DataCursor::deserialize(src)?;
        let command = Command::deserialize(src)?;
        Some(Message {
            session_id,
            write,
            read,
            command,
        })
    }
}

impl Serialize<Command> for Command {
    fn serialize(&self) -> Bytes {
        let mut buf = BytesMut::new();

        let command_type = match self {
            Command::Connect(_) => 0,
            Command::ConnectOk => 1,
            Command::Resume => 2,
            Command::ResumeOk => 3,
            Command::Forward(_) => 4,
            Command::ForwardOk => 5,
            Command::Shut => 6,
            Command::ShutOk => 7,
            Command::Invalid => u8::MAX,
        };
        buf.put_u8(command_type);

        let bytes = match self {
            Command::Connect(target) => target.serialize(),
            Command::Forward(payload) => payload.data.serialize(),
            _ => Bytes::new(),
        };

        buf.put_slice(&bytes);
        buf.freeze()
    }
}

impl Deserialize<Command> for Command {
    fn deserialize(src: &mut Cursor<&BytesMut>) -> Option<Command> {
        let command_type = (src.remaining() >= 1).then(|| src.get_u8())?;

        let command = match command_type {
            0 => Command::Connect(Target::deserialize(src)?),
            1 => Command::ConnectOk,
            2 => Command::Resume,
            3 => Command::ResumeOk,
            4 => Command::Forward(Payload::from(Bytes::deserialize(src)?)),
            5 => Command::ForwardOk,
            6 => Command::Shut,
            7 => Command::ShutOk,
            _ => Command::Invalid,
        };

        Some(command)
    }
}

impl Serialize<Target> for Target {
    fn serialize(&self) -> Bytes {
        let mut buf = BytesMut::new();

        buf.put_slice(&self.addr.serialize());
        buf.put_u16(self.port);

        buf.freeze()
    }
}

impl Deserialize<Target> for Target {
    fn deserialize(src: &mut Cursor<&BytesMut>) -> Option<Target> {
        Some(Target {
            addr: Socks5Address::deserialize(src)?,
            port: (src.remaining() >= 2).then(|| src.get_u16())?,
        })
    }
}

impl Serialize<DataCursor> for DataCursor {
    fn serialize(&self) -> Bytes {
        let mut buf = BytesMut::new();
        buf.put_u64(*self);
        buf.freeze()
    }
}

impl Deserialize<DataCursor> for DataCursor {
    fn deserialize(src: &mut Cursor<&BytesMut>) -> Option<DataCursor> {
        let cursor = (src.remaining() >= 8).then(|| src.get_u64())?;
        Some(cursor as DataCursor)
    }
}

impl Serialize<Bytes> for Bytes {
    fn serialize(&self) -> Bytes {
        let mut buf = BytesMut::new();

        assert!(self.len() <= u16::MAX as usize);
        buf.put_u16(self.len() as u16);
        buf.put_slice(&self);

        buf.freeze()
    }
}

impl Deserialize<Bytes> for Bytes {
    fn deserialize(src: &mut Cursor<&BytesMut>) -> Option<Bytes> {
        let len = (src.remaining() >= 2).then(|| src.get_u16() as usize)?;
        let payload = (src.remaining() >= len).then(|| src.copy_to_bytes(len))?;
        Some(payload)
    }
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

impl From<SocketAddr> for Target {
    fn from(value: SocketAddr) -> Self {
        match value {
            SocketAddr::V4(socket_addr_v4) => Target {
                addr: Socks5Address::IpAddr(IpAddr::V4(*socket_addr_v4.ip())),
                port: socket_addr_v4.port(),
            },
            SocketAddr::V6(socket_addr_v6) => Target {
                addr: Socks5Address::IpAddr(IpAddr::V6(*socket_addr_v6.ip())),
                port: socket_addr_v6.port(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    use super::*;

    fn assert_serialize_deserialize(msg: Message) {
        let mut buf = BytesMut::new();
        buf.put(msg.serialize());
        assert_eq!(msg, Message::deserialize(&mut Cursor::new(&buf)).unwrap());
    }

    #[test]
    fn connect() {
        let addresses = vec![
            Socks5Address::from_name(String::from("test.com")),
            Socks5Address::from_addr(IpAddr::V4(Ipv4Addr::new(4, 3, 2, 1))),
            Socks5Address::from_addr(IpAddr::V6(Ipv6Addr::new(8, 7, 6, 5, 4, 3, 2, 1))),
            Socks5Address::Unknown,
        ];

        for addr in addresses {
            let msg = Message {
                session_id: 123456789,
                write: 123,
                read: 321,
                command: Command::Connect(Target { addr, port: 12345 }),
            };
            assert_serialize_deserialize(msg);
        }
    }

    #[test]
    fn connect_ok() {
        let msg = Message {
            session_id: 123456789,
            write: 123,
            read: 321,
            command: Command::ConnectOk,
        };
        assert_serialize_deserialize(msg);
    }

    #[test]
    fn resume() {
        let msg = Message {
            session_id: 123456789,
            write: 123,
            read: 321,
            command: Command::Resume,
        };
        assert_serialize_deserialize(msg);
    }

    #[test]
    fn resume_ok() {
        let msg = Message {
            session_id: 123456789,
            write: 123,
            read: 321,
            command: Command::ResumeOk,
        };
        assert_serialize_deserialize(msg);
    }

    #[test]
    fn forward() {
        let msg = Message {
            session_id: 123456789,
            write: 123,
            read: 321,
            command: Command::Forward(Payload::from(Bytes::from("This is the payload."))),
        };
        assert_serialize_deserialize(msg);
    }

    #[test]
    fn forward_ok() {
        let msg = Message {
            session_id: 123456789,
            write: 123,
            read: 321,
            command: Command::ForwardOk,
        };
        assert_serialize_deserialize(msg);
    }

    #[test]
    fn shut() {
        let msg = Message {
            session_id: 123456789,
            write: 123,
            read: 321,
            command: Command::Shut,
        };
        assert_serialize_deserialize(msg);
    }

    #[test]
    fn shut_ok() {
        let msg = Message {
            session_id: 123456789,
            write: 123,
            read: 321,
            command: Command::ShutOk,
        };
        assert_serialize_deserialize(msg);
    }

    #[test]
    fn invalid() {
        let msg = Message {
            session_id: 123456789,
            write: 123,
            read: 321,
            command: Command::Invalid,
        };
        assert_serialize_deserialize(msg);
    }
}
