use std::ops::Range;

use bytes::Bytes;
use types::Identifier;

use crate::crypto::chacha::CipherKind;
use crate::lang::data::Data;

pub mod compiler;
mod data;
pub mod interpreter;
pub mod ir;
mod message;
mod types;

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Role {
    Client,
    Server,
}

trait Runtime {
    fn store<T: Into<Data>>(&mut self, addr: Identifier, data: T) -> anyhow::Result<()>;
    fn load<'a, T: TryFrom<&'a Data>>(&'a self, addr: &Identifier) -> anyhow::Result<T>;
    fn drop<T: TryFrom<Data>>(&mut self, addr: &Identifier) -> anyhow::Result<T>;
    fn init_key(&mut self, key: &[u8]) -> anyhow::Result<()>;
    fn create_cipher(&mut self, secret_key: [u8; 32], kind: CipherKind);
    fn encrypt(&mut self, plaintext: &[u8]) -> anyhow::Result<(Vec<u8>, [u8; 16])>;
    fn encrypt_unauth(&mut self, plaintext: &[u8]) -> anyhow::Result<Vec<u8>>;
    fn decrypt(&mut self, ciphertext: &[u8], mac: &[u8; 16]) -> anyhow::Result<Vec<u8>>;
    fn decrypt_unauth(&mut self, ciphertext: &[u8]) -> anyhow::Result<Vec<u8>>;
    async fn recv(&mut self, len: Range<usize>) -> anyhow::Result<Bytes>;
    async fn send(&mut self, bytes: Bytes) -> anyhow::Result<usize>;
    async fn flush(&mut self) -> anyhow::Result<()>;
}

trait Execute {
    async fn execute(&self, runtime: &mut impl Runtime) -> anyhow::Result<()>;
}

// TODO: remove when the compiler implements this trait.
#[allow(dead_code)]
trait Compile {
    fn compile(content: &str, role: Role) -> anyhow::Result<Vec<impl Execute>>;
}

// TODO, does this belong somewhere more relevant than in this mod?
pub fn padding_nbytes(payload_nbytes: usize, block_nbytes: usize) -> usize {
    let rem_nbytes = payload_nbytes % block_nbytes;
    (block_nbytes - rem_nbytes) % block_nbytes
}
