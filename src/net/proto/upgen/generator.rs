use bytes::Bytes;

use crate::net::proto::upgen::frames::{FieldKind, FrameField, OvertFrameSpec};
use crate::net::proto::upgen::protocols::*;

pub struct Generator {
    seed: u64,
}

impl Generator {
    pub fn new(seed: u64) -> Generator {
        // Use the seed to generate an overt protocol which specifies the format
        // of all frames that are transferred over the network.
        Generator { seed }
    }

    fn create_frame_spec(&self) -> OvertFrameSpec {
        let mut frame_spec = OvertFrameSpec::new();
        frame_spec.push_field(FrameField::new(FieldKind::Fixed(Bytes::from("UPGen v1"))));
        frame_spec.push_field(FrameField::new(FieldKind::Length(2)));
        frame_spec.push_field(FrameField::new(FieldKind::Payload));
        frame_spec
    }

    pub fn generate_overt_protocol(&self) -> OvertProtocol {
        // XXX just an example, should be updated.

        // Use the same frame spec for all messages
        let handshake1 = self.create_frame_spec();
        let handshake2 = self.create_frame_spec();
        let data = self.create_frame_spec();

        let proto_spec = onertt::ProtocolSpec::new(handshake1, handshake2, data);
        OvertProtocol::OneRtt(proto_spec)
    }
}
