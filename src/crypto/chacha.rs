use chacha20poly1305::aead::{Aead, NewAead};
use chacha20poly1305::ChaCha20Poly1305;

use salsa20::cipher::{KeyIvInit, StreamCipher};
use salsa20::Salsa20;

const MAC_NBYTES: usize = 16;
type Payload = Vec<u8>;
type MAC = [u8; MAC_NBYTES];

pub enum CipherKind {
    Sender,
    Receiver,
}

pub struct Cipher {
    encryption_nonce_gen: Salsa20,
    decryption_nonce_gen: Salsa20,
    encryption_cipher: ChaCha20Poly1305,
    decryption_cipher: ChaCha20Poly1305,
    nbytes_encrypted: usize,
    nbytes_decrypted: usize,
}

impl Cipher {
    pub fn new(secret_key: [u8; 32], cipher_kind: CipherKind) -> Cipher {
        const NONCE_A: [u8; 8] = [0xAA; 8];
        const NONCE_B: [u8; 8] = [0xBB; 8];

        let encryption_nonce = match cipher_kind {
            CipherKind::Sender => NONCE_A,
            CipherKind::Receiver => NONCE_B,
        };

        let decryption_nonce = match cipher_kind {
            CipherKind::Sender => NONCE_B,
            CipherKind::Receiver => NONCE_A,
        };

        Cipher {
            encryption_nonce_gen: Salsa20::new(&secret_key.into(), &encryption_nonce.into()),
            decryption_nonce_gen: Salsa20::new(&secret_key.into(), &decryption_nonce.into()),
            encryption_cipher: ChaCha20Poly1305::new(&secret_key.into()),
            decryption_cipher: ChaCha20Poly1305::new(&secret_key.into()),
            nbytes_encrypted: 0,
            nbytes_decrypted: 0,
        }
    }

    fn get_nonce(nonce_gen: &mut Salsa20) -> [u8; 12] {
        let mut buf: [u8; 12] = [0x00; 12];
        nonce_gen.apply_keystream(&mut buf);
        buf
    }

    pub fn encrypt(&mut self, plaintext: &[u8]) -> (Payload, MAC) {
        self.nbytes_encrypted += plaintext.len();

        let nonce = Cipher::get_nonce(&mut self.encryption_nonce_gen);

        let mut ciphertext = self
            .encryption_cipher
            .encrypt(&nonce.into(), plaintext)
            .expect("encryption failure");

        let mac: MAC;
        mac = ciphertext
            .drain(ciphertext.len() - 16..ciphertext.len())
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        assert!(plaintext.len() == ciphertext.len());

        (ciphertext, mac)
    }

    pub fn decrypt(&mut self, ciphertext: &[u8], mac: &MAC) -> Vec<u8> {
        let ctext_and_mac: Vec<u8> = ciphertext.iter().chain(mac.iter()).map(|e| *e).collect();

        self.nbytes_decrypted += ciphertext.len();

        let nonce = Cipher::get_nonce(&mut self.decryption_nonce_gen);

        self.decryption_cipher
            .decrypt(&nonce.into(), &ctext_and_mac[..])
            .expect("decryption failure")
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::crypto::kdf::derive_key_256;

    #[test]
    fn test_encryption_decryption() {
        let password = "hunter2";
        let salt = "pepper pepper pepper";

        let secret_key = derive_key_256(password, salt);

        let mut send_cipher = Cipher::new(secret_key, CipherKind::Sender);
        let mut recv_cipher = Cipher::new(secret_key, CipherKind::Receiver);

        let original_plain_text: Vec<u8> = b"hello world".iter().map(|e| *e).collect();

        let (ctext, mac) = send_cipher.encrypt(&original_plain_text[..]);
        let recovered_plain_text = recv_cipher.decrypt(&ctext[..], &mac);

        assert_eq!(original_plain_text, recovered_plain_text);
    }
}
