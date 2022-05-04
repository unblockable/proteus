pub mod null;
pub mod prototype;

use std::fmt;

use bytes::Bytes;

use crate::net::proto::upgen::crypto;

#[derive(Debug)]
enum Error {
    CryptFailure,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::CryptFailure => write!(f, "Error encrypting/decrypting payload"),
        }
    }
}

pub trait Encrypt {
    fn encrypt(&mut self, plaintext: &Bytes) -> Result<Bytes, crypto::Error>;
}

pub trait Decrypt {
    fn decrypt(&mut self, ciphertext: &Bytes) -> Result<Bytes, crypto::Error>;
}

// Super-trait that defines everything needed for a crypto protocol.
pub trait CryptoProtocol: Encrypt + Decrypt {}
