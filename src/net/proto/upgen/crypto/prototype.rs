use crate::net::proto::upgen::crypto::{self, CryptoProtocol};

use bytes::Bytes;
use std::rc::Rc;

use rand::rngs::OsRng;
use rand::RngCore;

use chacha20poly1305::aead::{Aead, NewAead};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};

use salsa20::cipher::{KeyIvInit, StreamCipher};
use salsa20::Salsa20;

use x25519_dalek::{x25519, PublicKey, SharedSecret, StaticSecret};

enum CipherKind {
    Sender,
    Receiver,
}

struct Cipher {
    encryption_nonce_gen: Salsa20,
    decryption_nonce_gen: Salsa20,
    encryption_cipher: ChaCha20Poly1305,
    decryption_cipher: ChaCha20Poly1305,
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
        }
    }

    fn get_nonce(nonce_gen: &mut Salsa20) -> [u8; 12] {
        let mut buf: [u8; 12] = [0x00; 12];
        nonce_gen.apply_keystream(&mut buf);
        buf
    }

    pub fn encrypt(&mut self, plaintext: &[u8]) -> Vec<u8> {
        let nonce = Cipher::get_nonce(&mut self.encryption_nonce_gen);

        self.encryption_cipher
            .encrypt(&nonce.into(), plaintext)
            .expect("encryption failure")
    }

    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Vec<u8> {
        let nonce = Cipher::get_nonce(&mut self.decryption_nonce_gen);

        self.decryption_cipher
            .decrypt(&nonce.into(), ciphertext)
            .expect("decryption failure")
    }
}

#[derive(Clone)]
pub struct CryptoModule {
    my_secret_key: StaticSecret,
    my_public_key: Option<PublicKey>,
    our_shared_secret_key: Option<[u8; 32]>,
    cipher: Option<Rc<Cipher>>,
}

impl CryptoModule {
    pub fn new() -> CryptoModule {
        let mut key: [u8; 32] = [0; 32];
        OsRng.fill_bytes(&mut key);

        CryptoModule {
            my_secret_key: StaticSecret::from(key),
            my_public_key: None,
            our_shared_secret_key: None,
            cipher: None,
        }
    }

    fn produce_my_half_handshake(&mut self) -> [u8; 32] {
        self.my_public_key = Some(PublicKey::from(&self.my_secret_key));
        self.my_public_key.unwrap().to_bytes()
    }

    /*
     * Produces the shared secret and instantiates the ciphers.
     */
    fn receive_their_half_handshake(&mut self, their_public_key: [u8; 32]) {
        self.our_shared_secret_key = Some(x25519(self.my_secret_key.to_bytes(), their_public_key));

        let mut my_role = CipherKind::Sender;

        if their_public_key < self.my_public_key.unwrap().to_bytes() {
            my_role = CipherKind::Receiver;
        } else {
        }

        self.cipher = Some(Rc::new(Cipher::new(
            self.our_shared_secret_key.unwrap(),
            my_role,
        )));
    }

    fn encrypt_impl(&mut self, plaintext: &[u8]) -> Vec<u8> {
        let mut cipher = self.cipher.as_mut().unwrap();
        Rc::get_mut(&mut cipher).unwrap().encrypt(plaintext)
    }

    fn decrypt_impl(&mut self, ciphertext: &[u8]) -> Vec<u8> {
        let mut cipher = self.cipher.as_mut().unwrap();
        Rc::get_mut(&mut cipher).unwrap().decrypt(ciphertext)
    }
}

impl CryptoProtocol for CryptoModule {
    fn encrypt(&mut self, plaintext: &Bytes) -> Result<Bytes, crypto::Error> {
        todo!()
    }

    fn decrypt(&mut self, ciphertext: &Bytes) -> Result<Bytes, crypto::Error> {
        todo!()
    }

    fn len(&self, material: crypto::CryptoMaterialKind) -> usize {
        todo!()
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    fn get_connected_pair() -> (CryptoModule, CryptoModule) {
        let mut alice = CryptoModule::new();
        let mut bob = CryptoModule::new();

        let alice_key = alice.produce_my_half_handshake();
        let bob_key = bob.produce_my_half_handshake();

        alice.receive_their_half_handshake(bob_key);
        bob.receive_their_half_handshake(alice_key);

        (alice, bob)
    }

    // #[test]
    fn test_key_exchange() {
        let (alice, bob) = get_connected_pair();

        assert_eq!(alice.our_shared_secret_key, bob.our_shared_secret_key);
        assert!(!alice.our_shared_secret_key.is_none());
    }

    #[test]
    fn test_encryption_decryption() {
        let (mut alice, mut bob) = get_connected_pair();

        let plaintext = b"hello world";

        let ciphertext = alice.encrypt_impl(plaintext);

        let plaintext_prime = bob.decrypt_impl(&ciphertext);

        assert_eq!(plaintext, &plaintext_prime[..]);
    }

    #[test]
    fn test_crypto_conversation() {
        let (mut alice, mut bob) = get_connected_pair();

        let alice_plaintext = b"hello world";
        let bob_plaintext = b"lorem ipsum";

        let alice_ciphertext = alice.encrypt_impl(alice_plaintext);
        let bob_ciphertext = bob.encrypt_impl(bob_plaintext);

        let alice_plaintext_prime = bob.decrypt_impl(&alice_ciphertext);
        assert_eq!(alice_plaintext, &alice_plaintext_prime[..]);

        let bob_plaintext_prime = alice.decrypt_impl(&bob_ciphertext);
        assert_eq!(bob_plaintext, &bob_plaintext_prime[..]);
    }

    #[test]
    fn test_two_message_encryption() {
        let (mut alice, mut bob) = get_connected_pair();

        let plaintext = b"hello world";

        let ciphertext1 = alice.encrypt_impl(plaintext);
        let ciphertext2 = alice.encrypt_impl(plaintext);
        assert_ne!(ciphertext1, ciphertext2);

        let plaintext_prime1 = bob.decrypt_impl(&ciphertext1);
        let plaintext_prime2 = bob.decrypt_impl(&ciphertext2);

        assert_eq!(plaintext, &plaintext_prime1[..]);
        assert_eq!(plaintext, &plaintext_prime2[..]);
    }
}
