use chacha20poly1305::ChaCha20Poly1305;
use chacha20poly1305::aead::{Aead, NewAead};
use salsa20::Salsa20;
use salsa20::cipher::{KeyIvInit, StreamCipher};

const MAC_NBYTES: usize = 16;
const NONCE_A: [u8; 8] = [0xAA; 8];
const NONCE_B: [u8; 8] = [0xBB; 8];

type Payload = Vec<u8>;
type Mac = [u8; MAC_NBYTES];

type Key = [u8; 32];

#[derive(PartialEq, Eq, Copy, Clone, Debug)]
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
    kind: CipherKind,
}

pub struct DecryptionCipher {
    inner: CipherInner,
    kind: CipherKind,
}

struct CipherInner {
    no_mac: Salsa20,
    nonce_gen: Salsa20,
    cipher: ChaCha20Poly1305,
    n_bytes_ciphered: usize,
    key: Option<Key>,
}

impl Cipher {
    pub fn new(secret_key: [u8; 32], kind: CipherKind) -> Self {
        Self {
            encryptor: EncryptionCipher::new(secret_key, kind),
            decryptor: DecryptionCipher::new(secret_key, kind),
        }
    }

    #[cfg(test)]
    pub fn encrypt(&mut self, plaintext: &[u8]) -> (Payload, Mac) {
        self.encryptor.encrypt(plaintext)
    }

    #[cfg(test)]
    pub fn encrypt_unauth(&mut self, plaintext: &[u8]) -> Vec<u8> {
        self.encryptor.encrypt_unauth(plaintext)
    }

    #[cfg(test)]
    pub fn decrypt(&mut self, ciphertext: &[u8], mac: &Mac) -> Vec<u8> {
        self.decryptor.decrypt(ciphertext, mac)
    }

    #[cfg(test)]
    pub fn decrypt_unauth(&mut self, ciphertext: &[u8]) -> Vec<u8> {
        self.decryptor.decrypt_unauth(ciphertext)
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
            no_mac: Salsa20::new(&secret_key.into(), &nonce.into()),
            nonce_gen: Salsa20::new(&secret_key.into(), &nonce.into()),
            cipher: ChaCha20Poly1305::new(&secret_key.into()),
            n_bytes_ciphered: 0,
            key: None,
        }
    }

    fn generate_nonce(&mut self) -> [u8; 12] {
        let mut buf: [u8; 12] = [0x00; 12];
        self.nonce_gen.apply_keystream(&mut buf);
        buf
    }

    fn init_key(&mut self, key_bytes: &[u8], nonce: &[u8; 8]) {
        if self.key.is_none() {
            let mut key: Key = [0; 32];
            let copy_len = core::cmp::min(key.len(), key_bytes.len());

            key[..copy_len].clone_from_slice(&key_bytes[..copy_len]);
            self.key = Some(key);

            self.nonce_gen = Salsa20::new(&key.into(), nonce.into());
            self.no_mac = Salsa20::new(&key.into(), nonce.into());
        }
    }
}

impl EncryptionCipher {
    fn new(secret_key: [u8; 32], kind: CipherKind) -> Self {
        Self {
            inner: CipherInner::new(secret_key, Self::fixed_nonce(kind)),
            kind,
        }
    }

    fn fixed_nonce(kind: CipherKind) -> [u8; 8] {
        match kind {
            CipherKind::Sender => NONCE_A,
            CipherKind::Receiver => NONCE_B,
        }
    }

    pub fn init_key(&mut self, key_bytes: &[u8]) {
        self.inner
            .init_key(key_bytes, &Self::fixed_nonce(self.kind));
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

    pub fn encrypt_unauth(&mut self, plaintext: &[u8]) -> Vec<u8> {
        let mut ciphertext: Vec<u8> = plaintext.to_vec();
        self.inner.no_mac.apply_keystream(&mut ciphertext);
        ciphertext
    }
}

impl DecryptionCipher {
    fn new(secret_key: [u8; 32], kind: CipherKind) -> Self {
        Self {
            inner: CipherInner::new(secret_key, Self::fixed_nonce(kind)),
            kind,
        }
    }

    fn fixed_nonce(kind: CipherKind) -> [u8; 8] {
        match kind {
            CipherKind::Sender => NONCE_B,
            CipherKind::Receiver => NONCE_A,
        }
    }

    pub fn init_key(&mut self, key_bytes: &[u8]) {
        self.inner
            .init_key(key_bytes, &Self::fixed_nonce(self.kind));
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

    pub fn decrypt_unauth(&mut self, ciphertext: &[u8]) -> Vec<u8> {
        let mut plaintext: Vec<u8> = ciphertext.to_vec();
        self.inner.no_mac.apply_keystream(&mut plaintext);
        plaintext
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::crypto::kdf::derive_key_256;

    fn make_key() -> [u8; 32] {
        let password = "hunter2";
        let salt = "pepper pepper pepper";
        derive_key_256(password, salt)
    }

    #[test]
    fn test_encryption_decryption() {
        let secret_key = make_key();

        let mut send_cipher = Cipher::new(secret_key, CipherKind::Sender);
        let mut recv_cipher = Cipher::new(secret_key, CipherKind::Receiver);

        let original_plain_text: Vec<u8> = b"hello world".to_vec();

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

        let original_plain_text: Vec<u8> = b"hello world".to_vec();

        let (ctext, mac) = send_enc.encrypt(&original_plain_text[..]);
        let recovered_plain_text = recv_dec.decrypt(&ctext[..], &mac);
        assert_eq!(original_plain_text, recovered_plain_text);

        let (ctext, mac) = recv_enc.encrypt(&original_plain_text[..]);
        let recovered_plain_text = send_dec.decrypt(&ctext[..], &mac);
        assert_eq!(original_plain_text, recovered_plain_text);
    }

    #[test]
    fn test_encryption_decryption_unauth() {
        let secret_key = make_key();

        let mut send_cipher = Cipher::new(secret_key, CipherKind::Sender);
        let mut recv_cipher = Cipher::new(secret_key, CipherKind::Receiver);

        let original_plain_text: Vec<u8> = b"hello world".to_vec();

        let ctext = send_cipher.encrypt_unauth(&original_plain_text[..]);
        let recovered_plain_text = recv_cipher.decrypt_unauth(&ctext[..]);

        assert_eq!(original_plain_text, recovered_plain_text);
    }
}
