use bytes::{Bytes, BytesMut};

use crate::net::proto::turbo::frames::Message;
use crate::net::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Copy, Default)]
pub struct Formatter {
    // All turbo frames can be formatted without extra state.
}

impl Serializer<Message> for Formatter {
    fn serialize_frame(&mut self, src: Message) -> Bytes {
        src.serialize()
    }
}

impl Deserializer<Message> for Formatter {
    fn deserialize_frame(&mut self, src: &mut std::io::Cursor<&BytesMut>) -> Option<Message> {
        Message::deserialize(src)
    }
}
