use bytes::{Buf, BufMut, BytesMut};
use std::io::Cursor;

use crate::net::{self, proto::or::spec::extor::*, Frame};

#[derive(Debug, PartialEq)]
pub struct Greeting {
    pub auth_types: Vec<u8>,
}

#[derive(Debug, PartialEq)]
pub struct Choice {
    pub auth_type: u8,
}

#[derive(Debug, PartialEq)]
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

impl Frame<Greeting> for Greeting {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<Greeting> {
        let mut auth_types = Vec::new();

        while buf.has_remaining() {
            match buf.get_u8() {
                EXTOR_AUTH_TYPE_END => return Some(Greeting { auth_types }),
                t => auth_types.push(t),
            }
        }

        None
    }

    fn serialize(&self) -> BytesMut {
        let mut buf = BytesMut::with_capacity(4);

        for method in self.auth_types.iter() {
            buf.put_u8(*method);
        }
        buf.put_u8(EXTOR_AUTH_TYPE_END);

        buf
    }
}

impl Frame<Choice> for Choice {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<Choice> {
        Some(Choice {
            auth_type: buf.has_remaining().then(|| buf.get_u8())?,
        })
    }

    fn serialize(&self) -> BytesMut {
        let mut buf = BytesMut::with_capacity(4);
        buf.put_u8(self.auth_type);
        buf
    }
}

impl Frame<ClientNonce> for ClientNonce {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<ClientNonce> {
        let mut nonce: [u8; 32] = [0; 32];
        for i in 0..32 {
            nonce[i] = buf.has_remaining().then(|| buf.get_u8())?;
        }
        Some(ClientNonce { nonce })
    }

    fn serialize(&self) -> BytesMut {
        let mut buf = BytesMut::with_capacity(32);
        for i in 0..32 {
            buf.put_u8(self.nonce[i]);
        }
        buf
    }
}

impl Frame<ServerHashNonce> for ServerHashNonce {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<ServerHashNonce> {
        let mut hash: [u8; 32] = [0; 32];
        for i in 0..32 {
            hash[i] = buf.has_remaining().then(|| buf.get_u8())?;
        }
        let mut nonce: [u8; 32] = [0; 32];
        for i in 0..32 {
            nonce[i] = buf.has_remaining().then(|| buf.get_u8())?;
        }
        Some(ServerHashNonce { hash, nonce })
    }

    fn serialize(&self) -> BytesMut {
        let mut buf = BytesMut::with_capacity(64);
        for i in 0..32 {
            buf.put_u8(self.hash[i]);
        }
        for i in 0..32 {
            buf.put_u8(self.nonce[i]);
        }
        buf
    }
}

impl Frame<ClientHash> for ClientHash {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<ClientHash> {
        let mut hash: [u8; 32] = [0; 32];
        for i in 0..32 {
            hash[i] = buf.has_remaining().then(|| buf.get_u8())?
        }
        Some(ClientHash { hash })
    }

    fn serialize(&self) -> BytesMut {
        let mut buf = BytesMut::with_capacity(32);
        for i in 0..32 {
            buf.put_u8(self.hash[i]);
        }
        buf
    }
}

impl Frame<ServerStatus> for ServerStatus {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<ServerStatus> {
        Some(ServerStatus {
            status: buf.has_remaining().then(|| buf.get_u8())?,
        })
    }

    fn serialize(&self) -> BytesMut {
        let mut buf = BytesMut::with_capacity(4);
        buf.put_u8(self.status);
        buf
    }
}

impl Frame<Command> for Command {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<Command> {
        let command = buf.has_remaining().then(|| buf.get_u16())?;
        let body_len = buf.has_remaining().then(|| buf.get_u16())?;
        let body_bytes = net::get_bytes_vec(buf, body_len as usize)?;

        Some(Command {
            command,
            body: String::from_utf8_lossy(&body_bytes).to_string(),
        })
    }

    fn serialize(&self) -> BytesMut {
        let mut buf = BytesMut::with_capacity(4);
        buf.put_u16(self.command);
        buf.put_u16(self.body.len() as u16);
        buf.put_slice(self.body.as_bytes());
        buf
    }
}

impl Frame<Reply> for Reply {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<Reply> {
        Some(Reply {
            reply: buf.has_remaining().then(|| buf.get_u16())?,
        })
    }

    fn serialize(&self) -> BytesMut {
        let mut buf = BytesMut::with_capacity(4);
        buf.put_u16(self.reply);
        buf
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
        assert_eq!(
            frame,
            Greeting::deserialize(&mut Cursor::new(&frame.serialize())).unwrap()
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
        assert_eq!(
            frame,
            Choice::deserialize(&mut Cursor::new(&frame.serialize())).unwrap()
        );
    }

    #[test]
    fn client_nonce() {
        let nonce: [u8; 32] = [111; 32];
        let frame = ClientNonce { nonce };
        assert_eq!(
            frame,
            ClientNonce::deserialize(&mut Cursor::new(&frame.serialize())).unwrap()
        );
    }

    #[test]
    fn server_hashnonce() {
        let hash: [u8; 32] = [222; 32];
        let nonce: [u8; 32] = [111; 32];
        let frame = ServerHashNonce { hash, nonce };
        assert_eq!(
            frame,
            ServerHashNonce::deserialize(&mut Cursor::new(&frame.serialize())).unwrap()
        );
    }

    #[test]
    fn client_hash() {
        let hash: [u8; 32] = [222; 32];
        let frame = ClientHash { hash };
        assert_eq!(
            frame,
            ClientHash::deserialize(&mut Cursor::new(&frame.serialize())).unwrap()
        );
    }

    #[test]
    fn server_status() {
        let frame = ServerStatus { status: 1 };
        assert_eq!(
            frame,
            ServerStatus::deserialize(&mut Cursor::new(&frame.serialize())).unwrap()
        );
    }

    #[test]
    fn command() {
        let frame = Command {
            command: EXTOR_COMMAND_TRANSPORT,
            body: String::from("upgen"),
        };
        assert_eq!(
            frame,
            Command::deserialize(&mut Cursor::new(&frame.serialize())).unwrap()
        );
    }

    #[test]
    fn reply() {
        let frame = Reply {
            reply: EXTOR_REPLY_OK,
        };
        assert_eq!(
            frame,
            Reply::deserialize(&mut Cursor::new(&frame.serialize())).unwrap()
        );
    }
}
