use std::fmt;

use bytes::Bytes;

use crate::crypto;
use crate::lang::spec::crypto::CryptoSpec;

mod chacha;
mod null;

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

pub trait CryptoProtocol {
    fn encrypt(&mut self, data: Bytes, spec: CryptoSpec) -> Result<Bytes, crypto::Error>;
    fn decrypt(&mut self, data: Bytes, spec: CryptoSpec) -> Result<Bytes, crypto::Error>;
}
