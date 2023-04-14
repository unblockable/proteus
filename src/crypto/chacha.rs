use bytes::{Buf, Bytes, BytesMut};

use rand::rngs::OsRng;
use rand::RngCore;

use chacha20poly1305::aead::{Aead, NewAead};
use chacha20poly1305::ChaCha20Poly1305;

use salsa20::cipher::{KeyIvInit, StreamCipher};
use salsa20::Salsa20;

enum CipherKind {
    Sender,
    Receiver,
}

struct Cipher {
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

        let mut fingerprint: [u8; 8] = [0; 8];
        OsRng.fill_bytes(&mut fingerprint);

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

    pub fn encrypt(&mut self, plaintext: &[u8]) -> Vec<u8> {
        self.nbytes_encrypted += plaintext.len();

        let nonce = Cipher::get_nonce(&mut self.encryption_nonce_gen);

        self.encryption_cipher
            .encrypt(&nonce.into(), plaintext)
            .expect("encryption failure")
    }

    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Vec<u8> {
        self.nbytes_decrypted += ciphertext.len();

        let nonce = Cipher::get_nonce(&mut self.decryption_nonce_gen);

        self.decryption_cipher
            .decrypt(&nonce.into(), ciphertext)
            .expect("decryption failure")
    }
}
