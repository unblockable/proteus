use bytes::Bytes;

use crate::{
    crypto::{self, CryptoProtocol},
    lang::spec::crypto::CryptoSpec,
};

pub struct ChaChaCipher {}

impl ChaChaCipher {
    pub fn new() -> Self {
        Self {}
    }
}

impl CryptoProtocol for ChaChaCipher {
    fn encrypt(&mut self, data: Bytes, spec: CryptoSpec) -> Result<Bytes, crypto::Error> {
        todo!()
    }

    fn decrypt(&mut self, data: Bytes, spec: CryptoSpec) -> Result<Bytes, crypto::Error> {
        todo!()
    }
}
