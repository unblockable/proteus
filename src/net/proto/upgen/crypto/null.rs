use crate::net::proto::upgen::crypto::{self, CryptoProtocol};

use bytes::Bytes;

pub struct CryptoModule {
    // Not sure what's gonna go in here yet.
}

impl CryptoModule {
    pub fn new() -> CryptoModule {
        CryptoModule {}
    }
}

impl CryptoProtocol for CryptoModule {
    fn encrypt(&mut self, plaintext: &Bytes) -> Result<Bytes, crypto::Error> {
        Ok(plaintext.clone())
    }

    fn decrypt(&mut self, ciphertext: &Bytes) -> Result<Bytes, crypto::Error> {
        Ok(ciphertext.clone())
    }

    fn len(&self, material: crypto::CryptoMaterialKind) -> usize {
        todo!()
    }
}