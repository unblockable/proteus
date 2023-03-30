use std::ops::Range;

use bytes::{Buf, Bytes, BytesMut};

use crate::net::{proto::proteus::frames::NetworkData, Deserializer, Serialize, Serializer};

pub struct Formatter {
    valid_read_range: Range<usize>,
}

impl Formatter {
    pub fn new(valid_read_range: Range<usize>) -> Formatter {
        Formatter { valid_read_range }
    }
}

impl Serializer<NetworkData> for Formatter {
    fn serialize_frame(&mut self, src: NetworkData) -> Bytes {
        src.serialize()
    }
}

impl Deserializer<NetworkData> for Formatter {
    fn deserialize_frame(&mut self, src: &mut std::io::Cursor<&BytesMut>) -> Option<NetworkData> {
        match self.valid_read_range.contains(&src.remaining()) {
            true => Some(NetworkData::from(src.copy_to_bytes(src.remaining()))),
            false => None,
        }
    }
}
