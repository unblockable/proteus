use bytes::{Buf, Bytes};
use std::io::Cursor;

use crate::net::proto::upgen::crypto::{self, CryptoProtocol};

pub struct CryptoModule {}

impl CryptoModule {
    pub fn new() -> CryptoModule {
        CryptoModule {}
    }
}

impl CryptoProtocol for CryptoModule {
    fn material_len(&self, material_kind: crypto::CryptoMaterialKind) -> usize {
        match material_kind {
            _ => 0,
        }
    }

    fn get_ciphertext_len(&self, plaintext_len: usize) -> usize {
        plaintext_len
    }

    fn encrypt(
        &mut self,
        plaintext: &mut Cursor<Bytes>,
        ciphertext_len: usize,
    ) -> Result<Bytes, crypto::Error> {
        Ok(plaintext.copy_to_bytes(ciphertext_len))
    }

    fn decrypt(&mut self, ciphertext: &Bytes) -> Result<Bytes, crypto::Error> {
        Ok(ciphertext.clone())
    }

    fn get_material(&mut self, material_kind: crypto::CryptoMaterialKind) -> Bytes {
        match material_kind {
            _ => Bytes::new(),
        }
    }

    fn set_material(&mut self, _material_kind: crypto::CryptoMaterialKind, _data: Bytes) {
        // Don't need to store anything.
    }
}
