pub mod null;
pub mod prototype;

use std::fmt;

use bytes::Bytes;
use std::io::Cursor;

use crate::net::proto::upgen::crypto;

#[derive(Debug)]
pub enum Error {
    CryptFailure,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::CryptFailure => write!(f, "Error encrypting/decrypting payload"),
        }
    }
}

#[derive(Clone, Copy)]
// These are all fixed-length, variable value
pub enum CryptoMaterialKind {
    IV,
    KeyMaterial,
    EncryptedHeader(usize), // Holds the size in bytes of the field
    MAC,
}

// Super-trait that defines everything needed for a crypto protocol.
pub trait CryptoProtocol {
    fn get_ciphertext_len(&self, plaintext_len: usize) -> usize;
    
    fn encrypt(
        &mut self,
        plaintext: &mut Cursor<Bytes>,
        ciphertext_len: usize,
    ) -> Result<Bytes, crypto::Error>;
    
    fn decrypt(&mut self, ciphertext: &Bytes) -> Result<Bytes, crypto::Error>;
    
    fn material_len(&self, material_kind: CryptoMaterialKind) -> usize;
    fn get_material(&mut self, material_kind: CryptoMaterialKind) -> Bytes;
    fn set_material(&mut self, material_kind: CryptoMaterialKind, data: Bytes);

    // Use the material_len() function to query the output length, if desired.
    // fn generate_ephemeral_public_key(&mut self) -> Bytes;
    // fn receive_ephemeral_public_key(&mut self, key: Bytes);
    // fn get_iv(&mut self) -> Bytes;
    // fn get_encrypted_header(&mut self, nbytes: usize) -> Bytes;
}
