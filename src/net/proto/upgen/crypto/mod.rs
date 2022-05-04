pub mod prototype;

use std::fmt;

use bytes::Bytes;

use crate::net::proto::upgen::crypto;

enum Error {
    CryptFailure
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::CryptFailure => write!(f, "Error encrypting/decrypting payload"),
        }
    }
}

trait Encrypt {
    fn encrypt(&mut self, plaintext: &Bytes) -> Result<Bytes, crypto::Error>;
}

trait Decrypt {
    fn decrypt(&mut self, ciphertext: &Bytes) -> Result<Bytes, crypto::Error>;
}

pub enum CryptoProtocol {
    Prototype(prototype::CryptoModule),
}
