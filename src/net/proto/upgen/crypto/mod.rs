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
    // TODO(rwails) Remove these functions vvvvv
    fn encrypt(&mut self, plaintext: &Bytes) -> Result<Bytes, crypto::Error>;
    fn decrypt(&mut self, ciphertext: &Bytes) -> Result<Bytes, crypto::Error>;
    // TODO(rwails) Remove these functions ^^^^^

    fn material_len(&self, material_kind: CryptoMaterialKind) -> usize;

    // TODO(rwails) Promote these functions vvvvv
    fn encrypt_tmp(
        &mut self,
        plaintext: &mut Cursor<Bytes>,
        ciphertext_len: usize,
    ) -> Result<Bytes, crypto::Error>;

    fn decrypt_tmp(&mut self, ciphertext: &Bytes) -> Result<Bytes, crypto::Error>;
    // TODO(rwails) Promote these functions ^^^^^

    // Use the material_len() function to query the output length, if desired.
    fn generate_ephemeral_public_key(&mut self) -> Bytes;
    fn receive_ephemeral_public_key(&mut self, key: Bytes);
    fn get_iv(&mut self) -> Bytes;
    fn get_encrypted_header(&mut self, nbytes: usize) -> Bytes;
}
