use crate::net::proto::upgen::crypto::{self, CryptoProtocol, Decrypt, Encrypt};

use bytes::{Bytes};

pub struct CryptoModule {
    // Not sure what's gonna go in here yet.
}

impl CryptoModule {
    pub fn new() -> CryptoModule {
        CryptoModule {}
    }
}

impl CryptoProtocol for CryptoModule {}

impl Encrypt for CryptoModule {
    fn encrypt(&mut self, plaintext: &Bytes) -> Result<Bytes, crypto::Error> {
        Ok(plaintext.clone())
    }
}

impl Decrypt for CryptoModule {
    fn decrypt(&mut self, ciphertext: &Bytes) -> Result<Bytes, crypto::Error> {
        Ok(ciphertext.clone())
    }
}
