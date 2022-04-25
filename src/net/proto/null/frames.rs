use bytes::{Buf, BufMut, Bytes, BytesMut};
use std::io::Cursor;

use crate::net::frame::Frame;

#[derive(Debug, PartialEq)]
pub struct Data {
    pub msg_len: u16,
    pub msg: Bytes,
}

impl Frame<Data> for Data {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<Data> {
        let len = (buf.remaining() >= 2).then(|| buf.get_u16() as usize)?;
        let msg = (buf.remaining() >= len).then(|| buf.copy_to_bytes(len))?;

        Some(Data {
            msg_len: len as u16, msg
        })
    }

    fn serialize(&self) -> Bytes {
        let len: u16 = match self.msg.len() < self.msg_len as usize {
            true => self.msg.len() as u16,
            false => self.msg_len
        };
        let size = len as usize;

        let mut buf = BytesMut::with_capacity(2 + size);
        buf.put_u16(len);
        buf.put(&self.msg[..size]);

        buf.freeze()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data() {
        let msg = Bytes::from(&b"hello world"[..]);

        let frame = Data {
            msg_len: msg.len() as u16,
            msg,
        };

        let mut buf = BytesMut::new();
        buf.put(frame.serialize());

        assert_eq!(
            frame,
            Data::deserialize(&mut Cursor::new(&buf)).unwrap()
        );
    }
}
