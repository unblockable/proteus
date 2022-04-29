use bytes::Buf;
use bytes::Bytes;

use bytes::BytesMut;
use crate::net::{Serializer, Deserializer};

use super::frames::{CovertPayload, OvertFrameSpec};

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

impl Serializer<CovertPayload> for Formatter {
    fn serialize_frame(&self, src: CovertPayload) -> Bytes {
        // Write as many frames as needed write all of the payload.
        src.data
    }
}

impl Deserializer<CovertPayload> for Formatter {
    fn deserialize_frame(&self, src: &mut std::io::Cursor<&BytesMut>) -> Option<CovertPayload> {
        // Read as many frames as are available and return payload.
        let data = src.copy_to_bytes(src.remaining());
        Some(CovertPayload { data })
    }
}
