use bytes::Bytes;

use bytes::BytesMut;
use crate::net::{Serializer, Deserializer};

use super::frames::OvertFrameSpec;
use super::message::OvertMessage;

pub struct Formatter {
    frame_spec: OvertFrameSpec
}

impl Formatter {
    pub fn new() -> Formatter {
        Formatter{ frame_spec: OvertFrameSpec::new()}
    }

    /// Sets the frame spec that we'll continue to follow when serializing or
    /// deserializing frames until a new frame spec is set.
    pub fn set_frame_spec(&mut self, frame_spec: OvertFrameSpec) {
        self.frame_spec = frame_spec;
    }
}

impl Serializer<OvertMessage> for Formatter {
    fn serialize_frame(&self, src: OvertMessage) -> Bytes {
        todo!()
    }
}

impl Deserializer<OvertMessage> for Formatter {
    fn deserialize_frame(&self, src: &mut std::io::Cursor<&BytesMut>) -> Option<OvertMessage> {
        todo!()
    }
}
