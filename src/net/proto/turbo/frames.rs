use std::io::Cursor;

use bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::net::proto::socks::address::Socks5Address;
use crate::net::{Deserialize, Serialize};

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum ShutWhich {
    /// Shutdown reads after receiving all data up to the given write cursor.
    Read(DataCursor),
    /// Shutdown writes, data after the given read cursor will be dropped.
    Write(DataCursor),
    /// Shutdown reads and writes, as in `Self::Read` and `Self::Write`.
    ReadWrite((DataCursor, DataCursor)),
    /// An invalid value found during deserialization.
    Invalid,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Status {
    ConnectOk,
    ConnectError,
    ResumeOk,
    ResumeError,
    ForwardOk,
    ForwardError,
    ShutdownOk,
    ShutdownError,
    /// An invalid value found during deserialization.
    Invalid,
}

pub type DataCursor = u64;

#[derive(Debug, PartialEq)]
pub struct Message {
    pub session_id: u64,
    pub command: Command,
}

#[derive(Debug, PartialEq)]
pub enum Command {
    Connect(Target),
    Resume(DataCursor),
    Forward(Payload),
    Shutdown(ShutWhich),
    Notify(Status),
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

#[derive(Debug, PartialEq)]
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
            Command::Resume(_) => 1,
            Command::Forward(_) => 2,
            Command::Shutdown(_) => 3,
            Command::Notify(_) => 4,
            Command::Invalid => u8::MAX,
        };
        buf.put_u8(command_type);

        let bytes = match self {
            Command::Connect(target) => target.serialize(),
            Command::Resume(sequence) => sequence.serialize(),
            Command::Forward(payload) => payload.serialize(),
            Command::Shutdown(which) => which.serialize(),
            Command::Notify(status) => status.serialize(),
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
            1 => Command::Resume(DataCursor::deserialize(src)?),
            2 => Command::Forward(Payload::deserialize(src)?),
            3 => Command::Shutdown(ShutWhich::deserialize(src)?),
            4 => Command::Notify(Status::deserialize(src)?),
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

impl Serialize<ShutWhich> for ShutWhich {
    fn serialize(&self) -> Bytes {
        let mut buf = BytesMut::new();

        let code = match self {
            ShutWhich::Read(_) => 0,
            ShutWhich::Write(_) => 1,
            ShutWhich::ReadWrite(_) => 2,
            ShutWhich::Invalid => u8::MAX,
        };
        buf.put_u8(code);

        match self {
            ShutWhich::Read(wr_cursor) => buf.put_slice(&wr_cursor.serialize()),
            ShutWhich::Write(rd_cursor) => buf.put_slice(&rd_cursor.serialize()),
            ShutWhich::ReadWrite((wr_cursor, rd_cursor)) => {
                buf.put_slice(&wr_cursor.serialize());
                buf.put_slice(&rd_cursor.serialize());
            }
            ShutWhich::Invalid => {}
        };

        buf.freeze()
    }
}

impl Deserialize<ShutWhich> for ShutWhich {
    fn deserialize(src: &mut Cursor<&BytesMut>) -> Option<ShutWhich> {
        let code = (src.remaining() >= 1).then(|| src.get_u8())?;

        let which = match code {
            0 => ShutWhich::Read(DataCursor::deserialize(src)?),
            1 => ShutWhich::Write(DataCursor::deserialize(src)?),
            2 => {
                ShutWhich::ReadWrite((DataCursor::deserialize(src)?, DataCursor::deserialize(src)?))
            }
            _ => ShutWhich::Invalid,
        };

        Some(which)
    }
}

impl Serialize<Status> for Status {
    fn serialize(&self) -> Bytes {
        let mut buf = BytesMut::new();

        let code = match self {
            Status::ConnectOk => 0,
            Status::ConnectError => 1,
            Status::ResumeOk => 2,
            Status::ResumeError => 3,
            Status::ForwardOk => 4,
            Status::ForwardError => 5,
            Status::ShutdownOk => 6,
            Status::ShutdownError => 7,
            Status::Invalid => u8::MAX,
        };

        buf.put_u8(code);
        buf.freeze()
    }
}

impl Deserialize<Status> for Status {
    fn deserialize(src: &mut Cursor<&BytesMut>) -> Option<Status> {
        let code = (src.remaining() >= 1).then(|| src.get_u8())?;

        let status = match code {
            0 => Status::ConnectOk,
            1 => Status::ConnectError,
            2 => Status::ResumeOk,
            3 => Status::ResumeError,
            4 => Status::ForwardOk,
            5 => Status::ForwardError,
            6 => Status::ShutdownOk,
            7 => Status::ShutdownError,
            _ => Status::Invalid,
        };

        Some(status)
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
    fn resume() {
        let msg = Message {
            session_id: 123456789,
            command: Command::Resume(987654321),
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
    fn shutdown() {
        for which in [
            ShutWhich::Read(123456),
            ShutWhich::Write(654321),
            ShutWhich::ReadWrite((123456, 654321)),
            ShutWhich::Invalid,
        ] {
            let msg = Message {
                session_id: 123456789,
                command: Command::Shutdown(which),
            };
            assert_serialize_deserialize(msg);
        }
    }

    #[test]
    fn notify() {
        for status in [
            Status::ConnectOk,
            Status::ConnectError,
            Status::ResumeOk,
            Status::ResumeError,
            Status::ForwardOk,
            Status::ForwardError,
            Status::ShutdownOk,
            Status::ShutdownError,
            Status::Invalid,
        ] {
            let msg = Message {
                session_id: 123456789,
                command: Command::Notify(status),
            };
            assert_serialize_deserialize(msg);
        }
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
