use crate::net::proto::upgen::frames::OvertFrameSpec;

pub enum ProtocolPhase {
    Handshake1, // client to server
    Handshake2, // server to client
    Data,
}

#[derive(Clone)]
pub struct ProtocolSpec {
    frames: ProtocolFrames,
}

#[derive(Clone)]
struct ProtocolFrames {
    handshake1: OvertFrameSpec,
    handshake2: OvertFrameSpec,
    data: OvertFrameSpec,
}

impl ProtocolSpec {
    pub fn new(
        handshake1: OvertFrameSpec,
        handshake2: OvertFrameSpec,
        data: OvertFrameSpec,
    ) -> ProtocolSpec {
        ProtocolSpec {
            frames: ProtocolFrames {
                handshake1,
                handshake2,
                data,
            },
        }
    }

    pub fn get_frame_spec(&self, phase: ProtocolPhase) -> OvertFrameSpec {
        match phase {
            ProtocolPhase::Handshake1 => self.frames.handshake1.clone(),
            ProtocolPhase::Handshake2 => self.frames.handshake2.clone(),
            ProtocolPhase::Data => self.frames.data.clone(),
        }
    }
}
