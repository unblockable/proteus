use crate::net::proto::upgen::crypto::{self, Decrypt, Encrypt};

use bytes::Bytes;
use std::rc::Rc;

use rand::rngs::OsRng;
use rand::RngCore;

use salsa20::cipher::{KeyIvInit, StreamCipher};
use salsa20::Salsa20;
use x25519_dalek::{x25519, PublicKey, SharedSecret, StaticSecret};

#[derive(Clone)]
struct Cipher {
    secret_key: [u8; 32],
    nonce_gen: Rc<Salsa20>,
}

impl Cipher {
    pub fn new(secret_key: [u8; 32]) -> Cipher {
        let nonce = [0x0; 8];

        Cipher {
            secret_key: secret_key,
            nonce_gen: Rc::new(Salsa20::new(&secret_key.into(), &nonce.into())),
        }
    }
}

#[derive(Clone)]
pub struct CryptoModule {
    my_secret_key: StaticSecret,
    my_public_key: Option<PublicKey>,
    our_shared_secret_key: Option<[u8; 32]>,
}

impl CryptoModule {
    pub fn new() -> CryptoModule {
        let mut key: [u8; 32] = [0; 32];
        OsRng.fill_bytes(&mut key);

        CryptoModule {
            my_secret_key: StaticSecret::from(key),
            my_public_key: None,
            our_shared_secret_key: None,
        }
    }

    pub fn produce_my_half_handshake(&mut self) -> [u8; 32] {
        self.my_public_key = Some(PublicKey::from(&self.my_secret_key));
        self.my_public_key.unwrap().to_bytes()
    }

    /*
     * Produces the shared secret.
     */
    pub fn receive_their_half_handshake(&mut self, their_public_key: [u8; 32]) {
        self.our_shared_secret_key = Some(x25519(self.my_secret_key.to_bytes(), their_public_key));
    }
}

impl Encrypt for CryptoModule {
    fn encrypt(&mut self, plaintext: &Bytes) -> Result<Bytes, crypto::Error> {
        todo!()
    }
}

impl Decrypt for CryptoModule {
    fn decrypt(&mut self, ciphertext: &Bytes) -> Result<Bytes, crypto::Error> {
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

    #[test]
    fn test_key_exchange() {
        let (alice, bob) = get_connected_pair();

        assert_eq!(alice.our_shared_secret_key, bob.our_shared_secret_key);
        assert!(!alice.our_shared_secret_key.is_none());
    }
}
