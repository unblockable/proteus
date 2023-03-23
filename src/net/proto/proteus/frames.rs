use std::io::Cursor;

use bytes::{Buf, Bytes, BytesMut};

use crate::net::{Deserialize, Serialize};

pub struct CovertPayload {
    pub data: Bytes,
}

pub struct OvertMessage {
    pub data: Bytes,
}

impl Serialize<OvertMessage> for OvertMessage {
    fn serialize(&self) -> Bytes {
        self.data.clone()
    }
}

impl Deserialize<OvertMessage> for OvertMessage {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<OvertMessage> {
        match buf.remaining() > 0 {
            true => Some(OvertMessage {
                data: buf.copy_to_bytes(buf.remaining()),
            }),
            false => None,
        }
    }
}
