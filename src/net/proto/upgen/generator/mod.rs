use crate::net::frame::FrameFmt;

// We may provide distinct formats for different frame types.
pub enum OvertFrameType {
    Handshake1, // client to server
    Handshake2, // server to client
    Data,
}

pub struct OvertProtocolSpec {
    // Currently support a 1-RTT handshake phase and then data phase.
    handshake1: FrameFmt,
    handshake2: FrameFmt,
    data: FrameFmt,
}

// TODO
impl OvertProtocolSpec {
    pub fn new(seed: u64) -> OvertProtocolSpec {
        // Use the seed to generate all of our protocol decisions and
        // create/store the various frame types.
        OvertProtocolSpec {
            handshake1: FrameFmt::new(),
            handshake2: FrameFmt::new(),
            data: FrameFmt::new(),
        }
    }

    pub fn get_frame_fmt(&self, frame_type: OvertFrameType) -> &FrameFmt {
        match frame_type {
            OvertFrameType::Handshake1 => &self.handshake1,
            OvertFrameType::Handshake2 => &self.handshake2,
            OvertFrameType::Data => &self.data,
        }
    }
}
