use std::sync::{Arc, Mutex};

use anyhow::anyhow;

use crate::crypto::chacha::{Cipher, CipherKind, DecryptionCipher, EncryptionCipher};

pub struct CryptoState {
    encryptor: Option<EncryptionCipher>,
    decryptor: Option<DecryptionCipher>,
}

impl CryptoState {
    pub fn empty() -> Self {
        Self {
            encryptor: None,
            decryptor: None,
        }
    }
}

/// Wrapper to allow us to safely share the internal ciphers across threads
/// while concurrently executing instructions in both forwarding directions.
#[derive(Clone)]
pub struct SharedCryptoState {
    inner: Arc<Mutex<CryptoState>>,
}

impl SharedCryptoState {
    fn empty() -> Self {
        Self {
            inner: Arc::new(Mutex::new(CryptoState::empty())),
        }
    }
}

pub struct CryptoStream {
    state_owned: CryptoState,
    state_shared: SharedCryptoState,
}

impl CryptoStream {
    pub fn new(state: Option<SharedCryptoState>) -> Self {
        // The crypto state starts out empty and is later populated by the
        // thread processing the app-to-net forwarding direction. When
        // populated, it is put into the shared state. Then, each direction
        // pulls out just the encryptor or decryptor needed for its forwarding
        // direction.
        Self {
            state_owned: CryptoState::empty(),
            state_shared: state.unwrap_or(SharedCryptoState::empty()),
        }
    }

    pub fn share(&self) -> SharedCryptoState {
        self.state_shared.clone()
    }

    pub fn create_cipher(&mut self, secret_key: [u8; 32], kind: CipherKind) {
        let cipher = Cipher::new(secret_key, kind);
        let (enc, dec) = cipher.into_split();

        // Store the ciphers in the _shared_ state so later each forwarding
        // direction can grab _only_ the enc or dec one they need.
        {
            // Take care not to panic while holding the lock.
            let mut crypt = self.state_shared.inner.lock().unwrap();
            crypt.encryptor = Some(enc);
            crypt.decryptor = Some(dec);
        }
    }

    pub fn init_key(&mut self, key: &[u8]) -> anyhow::Result<()> {
        if let Some(enc) = self.state_owned.encryptor.as_mut() {
            enc.init_key(key);
        }
        if let Some(dec) = self.state_owned.decryptor.as_mut() {
            dec.init_key(key);
        }
        Ok(())
    }

    fn take_shared_encryptor(&self) -> anyhow::Result<EncryptionCipher> {
        // Take care not to panic in this scope while holding the lock.
        let mut crypt = self.state_shared.inner.lock().unwrap();
        // Move an existing cipher out of shared state.
        crypt
            .encryptor
            .take()
            .ok_or_else(|| anyhow!("No shared encryption cipher"))
    }

    fn load_owned_encryptor(&mut self) -> anyhow::Result<&mut EncryptionCipher> {
        if self.state_owned.encryptor.is_none() {
            // Move an existing cipher from shared to local state.
            self.state_owned.encryptor = Some(self.take_shared_encryptor()?);
        }
        self.state_owned
            .encryptor
            .as_mut()
            .ok_or_else(|| anyhow!("No local encryption cipher"))
    }

    pub fn encrypt(&mut self, plaintext: &[u8]) -> anyhow::Result<(Vec<u8>, [u8; 16])> {
        Ok(self.load_owned_encryptor()?.encrypt(plaintext))
    }

    pub fn encrypt_unauth(&mut self, plaintext: &[u8]) -> anyhow::Result<Vec<u8>> {
        Ok(self.load_owned_encryptor()?.encrypt_unauth(plaintext))
    }

    fn take_shared_decryptor(&self) -> anyhow::Result<DecryptionCipher> {
        // Take care not to panic in this scope while holding the lock.
        let mut crypt = self.state_shared.inner.lock().unwrap();
        // Move an existing cipher out of shared state.
        crypt
            .decryptor
            .take()
            .ok_or_else(|| anyhow!("No shared decryption cipher"))
    }

    fn load_owned_decryptor(&mut self) -> anyhow::Result<&mut DecryptionCipher> {
        if self.state_owned.decryptor.is_none() {
            // Move an existing cipher from shared to local state.
            self.state_owned.decryptor = Some(self.take_shared_decryptor()?);
        }
        self.state_owned
            .decryptor
            .as_mut()
            .ok_or_else(|| anyhow!("No local decryption cipher"))
    }

    pub fn decrypt(&mut self, ciphertext: &[u8], mac: &[u8; 16]) -> anyhow::Result<Vec<u8>> {
        Ok(self.load_owned_decryptor()?.decrypt(ciphertext, mac))
    }

    pub fn decrypt_unauth(&mut self, ciphertext: &[u8]) -> anyhow::Result<Vec<u8>> {
        Ok(self.load_owned_decryptor()?.decrypt_unauth(ciphertext))
    }
}
