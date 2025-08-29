use std::fmt::Debug;
use std::io::Cursor;

use bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::net::proto::socks::address::Socks5Address;
use crate::net::{Deserialize, Serialize};

pub type DataCursor = u64;

#[derive(Debug, PartialEq)]
pub struct Message {
    pub session_id: u64,
    pub command: Command,
}

#[derive(Debug, PartialEq)]
pub enum Command {
    Connect(Target),
    ConnectOk,
    Resume(DataCursor),
    ResumeOk(DataCursor),
    Forward(Payload),
    ForwardOk(DataCursor),
    Shut(DataCursor),
    ShutOk(DataCursor),
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
    /// The byte location to write this data in the stream. Similar to TCP sequence number.
    pub write: DataCursor,
    /// The byte location we have read in the stream. Similar to TCP acknowledgment number.
    pub read: DataCursor,
    /// The application data payload bytes.
    pub data: Bytes,
}

impl Serialize<Message> for Message {
    fn serialize(&self) -> Bytes {
        let mut buf = BytesMut::new();

        buf.put_u64(self.session_id);
        buf.put_slice(&self.command.serialize());

        buf.freeze()
    }
}

impl Deserialize<Message> for Message {
    fn deserialize(src: &mut Cursor<&BytesMut>) -> Option<Message> {
        let session_id = (src.remaining() >= 8).then(|| src.get_u64())?;
        let command = Command::deserialize(src)?;
        Some(Message {
            session_id,
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
            Command::Resume(_) => 2,
            Command::ResumeOk(_) => 3,
            Command::Forward(_) => 4,
            Command::ForwardOk(_) => 5,
            Command::Shut(_) => 6,
            Command::ShutOk(_) => 7,
            Command::Invalid => u8::MAX,
        };
        buf.put_u8(command_type);

        let bytes = match self {
            Command::Connect(target) => target.serialize(),
            Command::ConnectOk => Bytes::new(),
            Command::Resume(sequence) => sequence.serialize(),
            Command::ResumeOk(cursor) => cursor.serialize(),
            Command::Forward(payload) => payload.serialize(),
            Command::ForwardOk(cursor) => cursor.serialize(),
            Command::Shut(cursor) => cursor.serialize(),
            Command::ShutOk(cursor) => cursor.serialize(),
            Command::Invalid => Bytes::new(),
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
            2 => Command::Resume(DataCursor::deserialize(src)?),
            3 => Command::ResumeOk(DataCursor::deserialize(src)?),
            4 => Command::Forward(Payload::deserialize(src)?),
            5 => Command::ForwardOk(DataCursor::deserialize(src)?),
            6 => Command::Shut(DataCursor::deserialize(src)?),
            7 => Command::ShutOk(DataCursor::deserialize(src)?),
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

impl Serialize<Payload> for Payload {
    fn serialize(&self) -> Bytes {
        let mut buf = BytesMut::new();

        buf.put_slice(&self.write.serialize());
        buf.put_slice(&self.read.serialize());

        assert!(self.data.len() <= u16::MAX as usize);
        buf.put_u16(self.data.len() as u16);
        buf.put_slice(&self.data);

        buf.freeze()
    }
}

impl Deserialize<Payload> for Payload {
    fn deserialize(src: &mut Cursor<&BytesMut>) -> Option<Payload> {
        let write = DataCursor::deserialize(src)?;
        let read = DataCursor::deserialize(src)?;

        let data_len = (src.remaining() >= 2).then(|| src.get_u16() as usize)?;
        let data = (src.remaining() >= data_len).then(|| src.copy_to_bytes(data_len))?;

        Some(Payload { write, read, data })
    }
}

impl Debug for Payload {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "Payload {{write: {}, read: {}, data: [len={}] }}",
            self.write,
            self.read,
            self.data.len()
        )
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
                command: Command::Connect(Target { addr, port: 12345 }),
            };
            assert_serialize_deserialize(msg);
        }
    }

    #[test]
    fn connect_ok() {
        let msg = Message {
            session_id: 123456789,
            command: Command::ConnectOk,
        };
        assert_serialize_deserialize(msg);
    }

    #[test]
    fn resume() {
        let msg = Message {
            session_id: 123456789,
            command: Command::Resume(987654321),
        };
        assert_serialize_deserialize(msg);
    }

    #[test]
    fn resume_ok() {
        let msg = Message {
            session_id: 123456789,
            command: Command::ResumeOk(12345),
        };
        assert_serialize_deserialize(msg);
    }

    #[test]
    fn forward() {
        let msg = Message {
            session_id: 123456789,
            command: Command::Forward(Payload {
                write: 12345,
                read: 54321,
                data: Bytes::from("This is the payload."),
            }),
        };
        assert_serialize_deserialize(msg);
    }

    #[test]
    fn forward_ok() {
        let msg = Message {
            session_id: 123456789,
            command: Command::ForwardOk(12345),
        };
        assert_serialize_deserialize(msg);
    }

    #[test]
    fn shut() {
        let msg = Message {
            session_id: 123456789,
            command: Command::Shut(12345),
        };
        assert_serialize_deserialize(msg);
    }

    #[test]
    fn shut_ok() {
        let msg = Message {
            session_id: 123456789,
            command: Command::ShutOk(12345),
        };
        assert_serialize_deserialize(msg);
    }

    #[test]
    fn invalid() {
        let msg = Message {
            session_id: 123456789,
            command: Command::Invalid,
        };
        assert_serialize_deserialize(msg);
    }
}
