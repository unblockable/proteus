use chacha20poly1305::aead::{Aead, NewAead};
use chacha20poly1305::ChaCha20Poly1305;

use salsa20::cipher::{KeyIvInit, StreamCipher};
use salsa20::Salsa20;

const MAC_NBYTES: usize = 16;
const NONCE_A: [u8; 8] = [0xAA; 8];
const NONCE_B: [u8; 8] = [0xBB; 8];

type Payload = Vec<u8>;
type Mac = [u8; MAC_NBYTES];

pub enum CipherKind {
    Sender,
    Receiver,
}

pub struct Cipher {
    encryptor: EncryptionCipher,
    decryptor: DecryptionCipher,
}

pub struct EncryptionCipher {
    inner: CipherInner,
}

pub struct DecryptionCipher {
    inner: CipherInner,
}

struct CipherInner {
    nonce_gen: Salsa20,
    cipher: ChaCha20Poly1305,
    n_bytes_ciphered: usize,
}

impl Cipher {
    pub fn new(secret_key: [u8; 32], cipher_kind: CipherKind) -> Self {
        let (enc_nonce, dec_nonce) = match cipher_kind {
            CipherKind::Sender => (NONCE_A, NONCE_B),
            CipherKind::Receiver => (NONCE_B, NONCE_A),
        };

        Self {
            encryptor: EncryptionCipher::new(secret_key, enc_nonce),
            decryptor: DecryptionCipher::new(secret_key, dec_nonce),
        }
    }

    #[allow(dead_code)]
    pub fn encrypt(&mut self, plaintext: &[u8]) -> (Payload, Mac) {
        self.encryptor.encrypt(plaintext)
    }

    #[allow(dead_code)]
    pub fn decrypt(&mut self, ciphertext: &[u8], mac: &Mac) -> Vec<u8> {
        self.decryptor.decrypt(ciphertext, mac)
    }

    pub fn into_split(self) -> (EncryptionCipher, DecryptionCipher) {
        (self.encryptor, self.decryptor)
    }

    #[allow(dead_code)]
    pub fn reunite(encryptor: EncryptionCipher, decryptor: DecryptionCipher) -> Cipher {
        Cipher {
            encryptor,
            decryptor,
        }
    }
}

impl CipherInner {
    fn new(secret_key: [u8; 32], nonce: [u8; 8]) -> Self {
        Self {
            nonce_gen: Salsa20::new(&secret_key.into(), &nonce.into()),
            cipher: ChaCha20Poly1305::new(&secret_key.into()),
            n_bytes_ciphered: 0,
        }
    }

    fn generate_nonce(&mut self) -> [u8; 12] {
        let mut buf: [u8; 12] = [0x00; 12];
        self.nonce_gen.apply_keystream(&mut buf);
        buf
    }
}

impl EncryptionCipher {
    fn new(secret_key: [u8; 32], nonce: [u8; 8]) -> Self {
        Self {
            inner: CipherInner::new(secret_key, nonce),
        }
    }

    pub fn encrypt(&mut self, plaintext: &[u8]) -> (Payload, Mac) {
        self.inner.n_bytes_ciphered += plaintext.len();

        let nonce = self.inner.generate_nonce();

        let mut ciphertext = self
            .inner
            .cipher
            .encrypt(&nonce.into(), plaintext)
            .expect("encryption failure");

        let mac: Mac = ciphertext
            .drain(ciphertext.len() - 16..ciphertext.len())
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        assert!(plaintext.len() == ciphertext.len());

        (ciphertext, mac)
    }
}

impl DecryptionCipher {
    fn new(secret_key: [u8; 32], nonce: [u8; 8]) -> Self {
        Self {
            inner: CipherInner::new(secret_key, nonce),
        }
    }

    pub fn decrypt(&mut self, ciphertext: &[u8], mac: &Mac) -> Vec<u8> {
        let ctext_and_mac: Vec<u8> = ciphertext.iter().chain(mac.iter()).copied().collect();

        self.inner.n_bytes_ciphered += ciphertext.len();

        let nonce = self.inner.generate_nonce();

        self.inner
            .cipher
            .decrypt(&nonce.into(), &ctext_and_mac[..])
            .expect("decryption failure")
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::crypto::kdf::derive_key_256;

    fn make_key() -> [u8;32] {
        let password = "hunter2";
        let salt = "pepper pepper pepper";
        derive_key_256(password, salt)
    }

    #[test]
    fn test_encryption_decryption() {
        let secret_key = make_key();

        let mut send_cipher = Cipher::new(secret_key, CipherKind::Sender);
        let mut recv_cipher = Cipher::new(secret_key, CipherKind::Receiver);

        let original_plain_text: Vec<u8> = b"hello world".iter().map(|e| *e).collect();

        let (ctext, mac) = send_cipher.encrypt(&original_plain_text[..]);
        let recovered_plain_text = recv_cipher.decrypt(&ctext[..], &mac);

        assert_eq!(original_plain_text, recovered_plain_text);
    }

    #[test]
    fn test_split_encryption_decryption() {
        let secret_key = make_key();

        let (mut send_enc, mut send_dec) = Cipher::new(secret_key, CipherKind::Sender).into_split();
        let (mut recv_enc, mut recv_dec) =
            Cipher::new(secret_key, CipherKind::Receiver).into_split();

        let original_plain_text: Vec<u8> = b"hello world".iter().map(|e| *e).collect();

        let (ctext, mac) = send_enc.encrypt(&original_plain_text[..]);
        let recovered_plain_text = recv_dec.decrypt(&ctext[..], &mac);
        assert_eq!(original_plain_text, recovered_plain_text);

        let (ctext, mac) = recv_enc.encrypt(&original_plain_text[..]);
        let recovered_plain_text = send_dec.decrypt(&ctext[..], &mac);
        assert_eq!(original_plain_text, recovered_plain_text);
    }
}
