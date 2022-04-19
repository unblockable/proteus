#[derive(Debug, PartialEq)]
pub struct FrameFormatSpec {
    // TODO: json spec for a single frame
}

pub struct OvertProtocolSpec {
    // 1-RTT handshake phase and then data phase.
    request: FrameFormatSpec,
    response: FrameFormatSpec,
    data: FrameFormatSpec,
}

impl OvertProtocolSpec {
    pub fn new(seed: u64) -> OvertProtocolSpec {
        OvertProtocolSpec {
            request: FrameFormatSpec {},
            response: FrameFormatSpec {},
            data: FrameFormatSpec {},
        }
    }

    // TODO
}
