use std::{
    ops::Range,
    sync::{Arc, Mutex},
};

use anyhow::bail;
use bytes::Bytes;

use crate::{
    crypto::chacha::{Cipher, CipherKind, DecryptionCipher, EncryptionCipher},
    net::{Reader, Writer},
};

pub struct Forwarder<R: Reader, W: Writer> {
    src: R,
    n_recv_src: usize,
    dst: W,
    n_sent_dst: usize,
    state_owned: ForwardingState,
    state_shared: SharedForwardingState,
}

impl<R: Reader, W: Writer> Forwarder<R, W> {
    pub fn new(src: R, dst: W, state: Option<SharedForwardingState>) -> Self {
        Self {
            src,
            n_recv_src: 0,
            dst,
            n_sent_dst: 0,
            state_owned: ForwardingState::empty(),
            state_shared: state.unwrap_or(SharedForwardingState::empty()),
        }
    }

    pub fn share(&self) -> SharedForwardingState {
        self.state_shared.clone()
    }

    pub async fn send(&mut self, bytes: Bytes) -> anyhow::Result<usize> {
        log::trace!("trying to send {} bytes to dst", bytes.len());

        let num_written = match self.dst.write_bytes(&bytes).await {
            Ok(num) => num,
            Err(e) => bail!("Error sending to dst: {e}"),
        };

        self.n_sent_dst += num_written;
        log::trace!("sent {num_written} bytes to dst");

        Ok(num_written)
    }

    pub async fn recv(&mut self, len: Range<usize>) -> anyhow::Result<Bytes> {
        log::trace!("Trying to receive {len:?} bytes from src",);

        let data = match self.src.read_bytes(len).await {
            Ok(data) => data,
            Err(e) => bail!(e),
            // If we return an error on EOF, will the entire connection
            // close even if we could still possibly send?
            // Do we need to just go to sleep forever upon EOF, and let
            // an error on the other direction close us down?
            // Err(net_err) => match net_err {
            //     net::Error::Eof => break,
            //     _ => return Err(proteus::Error::from(net_err)),
            // },
        };

        let n_bytes = data.len();
        self.n_recv_src += n_bytes;
        log::trace!("Received {n_bytes} bytes from src");

        Ok(data.into())
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

    pub fn encrypt(&mut self, plaintext: &[u8]) -> anyhow::Result<(Vec<u8>, [u8; 16])> {
        // Check for cipher in our local state before checking shared state.
        let encryptor = match self.state_owned.encryptor.as_mut() {
            Some(cipher) => cipher,
            None => {
                let cipher_maybe = {
                    // Take care not to panic in this scope while holding the lock.
                    let mut crypt = self.state_shared.inner.lock().unwrap();
                    // Move an existing cipher out of shared state.
                    crypt.encryptor.take()
                };
                match cipher_maybe {
                    // Store an existing cipher in local state.
                    Some(cipher) => self.state_owned.encryptor.insert(cipher),
                    None => bail!("No cipher for encryption"),
                }
            }
        };
        Ok(encryptor.encrypt(plaintext))
    }

    pub fn decrypt(&mut self, ciphertext: &[u8], mac: &[u8; 16]) -> anyhow::Result<Vec<u8>> {
        // Check for cipher in our local state before checking shared state.
        let decryptor = match self.state_owned.decryptor.as_mut() {
            Some(cipher) => cipher,
            None => {
                let cipher_maybe = {
                    // Take care not to panic in this scope while holding the lock.
                    let mut crypt = self.state_shared.inner.lock().unwrap();
                    // Move an existing cipher out of shared state.
                    crypt.decryptor.take()
                };
                match cipher_maybe {
                    // Store an existing cipher in local state.
                    Some(cipher) => self.state_owned.decryptor.insert(cipher),
                    None => bail!("No cipher for decryption"),
                }
            }
        };
        Ok(decryptor.decrypt(&ciphertext, &mac))
    }
}

/// Wraps the ForwardingState allowing us to safely share the internal ciphers
/// across threads while concurrently executing instructions.
#[derive(Clone)]
pub struct SharedForwardingState {
    inner: Arc<Mutex<ForwardingState>>,
}

impl SharedForwardingState {
    fn empty() -> Self {
        Self {
            inner: Arc::new(Mutex::new(ForwardingState::empty())),
        }
    }
}

struct ForwardingState {
    encryptor: Option<EncryptionCipher>,
    decryptor: Option<DecryptionCipher>,
}

impl ForwardingState {
    fn empty() -> Self {
        Self {
            encryptor: None,
            decryptor: None,
        }
    }
}
