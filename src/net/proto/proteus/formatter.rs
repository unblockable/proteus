use bytes::{Bytes, BytesMut};

use crate::net::{
    proto::proteus::frames::OvertMessage, Deserialize, Deserializer, Serialize, Serializer,
};

#[derive(Clone, Copy)]
pub struct Formatter {
    // The proteus message is just bytes and can be formatted without extra state.
    // Do we want to specify the min and max lengths here, and set for each read?
}

impl Formatter {
    pub fn new() -> Formatter {
        Formatter {}
    }
}

impl Serializer<OvertMessage> for Formatter {
    fn serialize_frame(&mut self, src: OvertMessage) -> Bytes {
        src.serialize()
    }
}

impl Deserializer<OvertMessage> for Formatter {
    fn deserialize_frame(&mut self, src: &mut std::io::Cursor<&BytesMut>) -> Option<OvertMessage> {
        OvertMessage::deserialize(src)
    }
}
