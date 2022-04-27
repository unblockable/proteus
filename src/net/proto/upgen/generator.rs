use crate::net::frame::FrameFmt;



pub struct Generator {
    // Currently support a 1-RTT handshake phase and then data phase.
    handshake1: OvertFrameSpec,
    handshake2: OvertFrameSpec,
    data: OvertFrameSpec,
}

// TODO
impl Generator {
    pub fn new(seed: u64) -> Generator {
        // Use the seed to generate all of our protocol decisions and
        // create/store the various frame types.
        Generator {
            handshake1: OvertFrameSpec::new(),
            handshake2: OvertFrameSpec::new(),
            data: OvertFrameSpec::new(),
        }
    }

    pub fn get_overt_frame_spec(&self, frame_type: OvertFrameType) -> &OvertFrameSpec {
        match frame_type {
            OvertFrameType::Handshake1 => &self.handshake1,
            OvertFrameType::Handshake2 => &self.handshake2,
            OvertFrameType::Data => &self.data,
        }
    }
}
