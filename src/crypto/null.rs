use bytes::Bytes;

use crate::{
    crypto::{self, CryptoProtocol},
    lang::spec::crypto::CryptoSpec,
};

pub struct NullCipher {}

impl NullCipher {
    pub fn new() -> Self {
        Self {}
    }
}

impl CryptoProtocol for NullCipher {
    fn encrypt(&mut self, data: Bytes, _spec: CryptoSpec) -> Result<Bytes, crypto::Error> {
        Ok(data.clone())
    }

    fn decrypt(&mut self, data: Bytes, _spec: CryptoSpec) -> Result<Bytes, crypto::Error> {
        Ok(data.clone())
    }
}
