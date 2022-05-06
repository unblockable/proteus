use crate::net::proto::upgen::crypto::{self, CryptoProtocol};

use bytes::Bytes;
use std::io::Cursor;

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

    fn material_len(&self, material_kind: crypto::CryptoMaterialKind) -> usize {
        todo!()
    }

    fn encrypt_tmp(
        &mut self,
        plaintext: &mut Cursor<Bytes>,
        ciphertext_len: usize,
    ) -> Result<Bytes, crypto::Error> {
        todo!();
    }
    fn decrypt_tmp(&mut self, ciphertext: &Bytes) -> Result<Bytes, crypto::Error> {
        todo!();
    }
    fn generate_ephemeral_public_key(&mut self) -> Bytes {
        todo!();
    }
    fn receive_ephemeral_public_key(&mut self, key: Bytes) {
        todo!();
    }
    fn get_iv(&mut self) -> Bytes {
        todo!();
    }
    fn get_encrypted_header(&mut self, nbytes: usize) -> Bytes {
        todo!();
    }
}
