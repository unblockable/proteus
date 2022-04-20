use crate::net::frame::FrameFmt;

pub struct OvertProtocolSpec {
    // 1-RTT handshake phase and then data phase.
    request: FrameFmt,
    response: FrameFmt,
    data: FrameFmt,
}

impl OvertProtocolSpec {
    pub fn new(seed: u64) -> OvertProtocolSpec {
        OvertProtocolSpec {
            request: FrameFmt {},
            response: FrameFmt {},
            data: FrameFmt {},
        }
    }

    // TODO
}
