pub mod null;
pub mod prototype;

use std::fmt;

use bytes::Bytes;

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

#[derive(Clone)]
// These are all fixed-length, variable value
pub enum CryptoMaterialKind {
    IV,
    KeyMaterialSent,
    KeyMaterialReceived,
    EncryptedHeader(usize), // Holds the size in bytes of the field
}

// Super-trait that defines everything needed for a crypto protocol.
pub trait CryptoProtocol {
    fn encrypt(&mut self, plaintext: &Bytes) -> Result<Bytes, crypto::Error>;
    fn decrypt(&mut self, ciphertext: &Bytes) -> Result<Bytes, crypto::Error>;
    fn len(&self, material: CryptoMaterialKind) -> usize;
}
