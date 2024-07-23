use bytes::{Buf, BufMut, Bytes, BytesMut};
use net::{Deserialize, Serialize};
use std::io::Cursor;

use crate::net::proto::or::*;

#[derive(Debug, PartialEq)]
pub struct Greeting {
    pub auth_types: Vec<u8>,
}

#[derive(Debug, PartialEq)]
pub struct Choice {
    pub auth_type: u8,
}

#[derive(Debug, PartialEq, Clone)]
pub struct ClientNonce {
    pub nonce: [u8; 32],
}

#[derive(Debug, PartialEq)]
pub struct ServerHashNonce {
    pub hash: [u8; 32],
    pub nonce: [u8; 32],
}

#[derive(Debug, PartialEq)]
pub struct ClientHash {
    pub hash: [u8; 32],
}

#[derive(Debug, PartialEq)]
pub struct ServerStatus {
    pub status: u8,
}

#[derive(Debug, PartialEq)]
pub struct Command {
    pub command: u16,
    pub body: String,
}

#[derive(Debug, PartialEq)]
pub struct Reply {
    pub reply: u16,
}

impl Serialize<Greeting> for Greeting {
    fn serialize(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(4);

        for method in self.auth_types.iter() {
            buf.put_u8(*method);
        }
        buf.put_u8(EXTOR_AUTH_TYPE_END);

        buf.freeze()
    }
}

impl Deserialize<Greeting> for Greeting {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<Greeting> {
        let mut auth_types = Vec::new();

        while buf.remaining() > 0 {
            match buf.get_u8() {
                EXTOR_AUTH_TYPE_END => return Some(Greeting { auth_types }),
                t => auth_types.push(t),
            }
        }

        None
    }
}

impl Serialize<Choice> for Choice {
    fn serialize(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(4);
        buf.put_u8(self.auth_type);
        buf.freeze()
    }
}

impl Deserialize<Choice> for Choice {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<Choice> {
        Some(Choice {
            auth_type: (buf.remaining() > 0).then(|| buf.get_u8())?,
        })
    }
}

impl Serialize<ClientNonce> for ClientNonce {
    fn serialize(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(32);
        buf.put_slice(&self.nonce);
        buf.freeze()
    }
}

impl Deserialize<ClientNonce> for ClientNonce {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<ClientNonce> {
        let mut nonce: [u8; 32] = [0; 32];
        (buf.remaining() >= 32).then(|| buf.copy_to_slice(&mut nonce))?;
        Some(ClientNonce { nonce })
    }
}

impl Serialize<ServerHashNonce> for ServerHashNonce {
    fn serialize(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(64);
        buf.put_slice(&self.hash);
        buf.put_slice(&self.nonce);
        buf.freeze()
    }
}

impl Deserialize<ServerHashNonce> for ServerHashNonce {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<ServerHashNonce> {
        let mut hash: [u8; 32] = [0; 32];
        (buf.remaining() >= 32).then(|| buf.copy_to_slice(&mut hash))?;
        let mut nonce: [u8; 32] = [0; 32];
        (buf.remaining() >= 32).then(|| buf.copy_to_slice(&mut nonce))?;
        Some(ServerHashNonce { hash, nonce })
    }
}

impl Serialize<ClientHash> for ClientHash {
    fn serialize(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(32);
        buf.put_slice(&self.hash);
        buf.freeze()
    }
}

impl Deserialize<ClientHash> for ClientHash {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<ClientHash> {
        let mut hash: [u8; 32] = [0; 32];
        (buf.remaining() >= 32).then(|| buf.copy_to_slice(&mut hash))?;
        Some(ClientHash { hash })
    }
}

impl Serialize<ServerStatus> for ServerStatus {
    fn serialize(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(4);
        buf.put_u8(self.status);
        buf.freeze()
    }
}

impl Deserialize<ServerStatus> for ServerStatus {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<ServerStatus> {
        Some(ServerStatus {
            status: (buf.remaining() > 0).then(|| buf.get_u8())?,
        })
    }
}

impl Serialize<Command> for Command {
    fn serialize(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(4);
        buf.put_u16(self.command);
        buf.put_u16(self.body.len() as u16);
        buf.put_slice(self.body.as_bytes());
        buf.freeze()
    }
}

impl Deserialize<Command> for Command {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<Command> {
        let command = (buf.remaining() >= 2).then(|| buf.get_u16())?;
        let body_len = (buf.remaining() >= 2).then(|| buf.get_u16() as usize)?;
        let body_bytes = (buf.remaining() >= body_len).then(|| buf.copy_to_bytes(body_len))?;

        Some(Command {
            command,
            body: String::from_utf8_lossy(&body_bytes).to_string(),
        })
    }
}

impl Serialize<Reply> for Reply {
    fn serialize(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(4);
        buf.put_u16(self.reply);
        buf.freeze()
    }
}

impl Deserialize<Reply> for Reply {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<Reply> {
        Some(Reply {
            reply: (buf.remaining() >= 2).then(|| buf.get_u16())?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greeting() {
        let frame = Greeting {
            auth_types: vec![1],
        };

        let mut buf = BytesMut::new();
        buf.put(frame.serialize());

        assert_eq!(
            frame,
            Greeting::deserialize(&mut Cursor::new(&buf)).unwrap()
        );
    }

    #[test]
    fn greeting_partial() {
        let mut bytes = BytesMut::new();
        bytes.put_u8(1);
        bytes.put_u8(2);

        assert!(Greeting::deserialize(&mut Cursor::new(&bytes)).is_none());

        bytes.put_u8(EXTOR_AUTH_TYPE_END);

        let frame = Greeting {
            auth_types: vec![1, 2],
        };

        assert_eq!(
            frame,
            Greeting::deserialize(&mut Cursor::new(&bytes)).unwrap()
        );

        bytes.advance(1);

        let frame2 = Greeting {
            auth_types: vec![2],
        };

        assert_eq!(
            frame2,
            Greeting::deserialize(&mut Cursor::new(&bytes)).unwrap()
        );
    }

    #[test]
    fn choice() {
        let frame = Choice { auth_type: 1 };

        let mut buf = BytesMut::new();
        buf.put(frame.serialize());

        assert_eq!(frame, Choice::deserialize(&mut Cursor::new(&buf)).unwrap());
    }

    #[test]
    fn client_nonce() {
        let nonce: [u8; 32] = [111; 32];
        let frame = ClientNonce { nonce };

        let mut buf = BytesMut::new();
        buf.put(frame.serialize());

        assert_eq!(
            frame,
            ClientNonce::deserialize(&mut Cursor::new(&buf)).unwrap()
        );
    }

    #[test]
    fn server_hashnonce() {
        let hash: [u8; 32] = [222; 32];
        let nonce: [u8; 32] = [111; 32];
        let frame = ServerHashNonce { hash, nonce };

        let mut buf = BytesMut::new();
        buf.put(frame.serialize());

        assert_eq!(
            frame,
            ServerHashNonce::deserialize(&mut Cursor::new(&buf)).unwrap()
        );
    }

    #[test]
    fn client_hash() {
        let hash: [u8; 32] = [222; 32];
        let frame = ClientHash { hash };

        let mut buf = BytesMut::new();
        buf.put(frame.serialize());

        assert_eq!(
            frame,
            ClientHash::deserialize(&mut Cursor::new(&buf)).unwrap()
        );
    }

    #[test]
    fn server_status() {
        let frame = ServerStatus { status: 1 };

        let mut buf = BytesMut::new();
        buf.put(frame.serialize());

        assert_eq!(
            frame,
            ServerStatus::deserialize(&mut Cursor::new(&buf)).unwrap()
        );
    }

    #[test]
    fn command() {
        let frame = Command {
            command: EXTOR_COMMAND_TRANSPORT,
            body: String::from("proteus"),
        };

        let mut buf = BytesMut::new();
        buf.put(frame.serialize());

        assert_eq!(frame, Command::deserialize(&mut Cursor::new(&buf)).unwrap());
    }

    #[test]
    fn reply() {
        let frame = Reply {
            reply: EXTOR_REPLY_OK,
        };

        let mut buf = BytesMut::new();
        buf.put(frame.serialize());

        assert_eq!(frame, Reply::deserialize(&mut Cursor::new(&buf)).unwrap());
    }
}
