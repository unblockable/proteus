use crate::net::proto::upgen::crypto::{self, Decrypt, Encrypt};

use bytes::Bytes;

#[derive(Clone)]
pub struct CryptoModule {
    // Not sure what's gonna go in here yet.
}

impl CryptoModule {
    fn new() {}
}

impl Encrypt for CryptoModule {
    fn encrypt(&mut self, plaintext: &Bytes) -> Result<Bytes, crypto::Error> {
        todo!()
    }
}

impl Decrypt for CryptoModule {
    fn decrypt(&mut self, ciphertext: &Bytes) -> Result<Bytes, crypto::Error> {
        todo!()
    }
}

/*
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
*/
