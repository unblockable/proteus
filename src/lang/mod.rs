use bytes::Bytes;
use types::Identifier;

use crate::crypto::chacha::CipherKind;
use crate::lang;
use crate::lang::data::Data;

pub mod compiler;
mod data;
pub mod interpreter;
pub mod ir;
mod message;
mod result;
mod types;

pub use result::{Error, Result, ResultExt};

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Role {
    Client,
    Server,
}

trait Runtime {
    fn store<T: Into<Data>>(&mut self, addr: Identifier, data: T) -> lang::Result<()>;
    fn load<'a, T: TryFrom<&'a Data>>(&'a self, addr: &Identifier) -> lang::Result<T>;
    fn drop<T: TryFrom<Data>>(&mut self, addr: &Identifier) -> lang::Result<T>;
    fn init_key(&mut self, key: &[u8]) -> lang::Result<()>;
    fn create_cipher(&mut self, secret_key: [u8; 32], kind: CipherKind) -> lang::Result<()>;
    fn encrypt(&mut self, plaintext: &[u8]) -> lang::Result<(Vec<u8>, [u8; 16])>;
    fn encrypt_unauth(&mut self, plaintext: &[u8]) -> lang::Result<Vec<u8>>;
    fn decrypt(&mut self, ciphertext: &[u8], mac: &[u8; 16]) -> lang::Result<Vec<u8>>;
    fn decrypt_unauth(&mut self, ciphertext: &[u8]) -> lang::Result<Vec<u8>>;
    async fn read(&mut self, len: usize) -> lang::Result<Bytes>;
    fn try_read(&mut self, len: usize) -> lang::Result<Option<Bytes>>;
    async fn read_exact(&mut self, len: usize) -> lang::Result<Bytes>;
    async fn send(&mut self, bytes: Bytes) -> lang::Result<usize>;
    async fn flush(&mut self) -> lang::Result<()>;
    #[allow(dead_code)]
    async fn shutdown(&mut self) -> lang::Result<()>;
}

trait Execute {
    async fn execute(&self, runtime: &mut impl Runtime) -> lang::Result<()>;
}

// TODO: remove when the compiler implements this trait.
#[allow(dead_code)]
trait Compile {
    fn compile(content: &str, role: Role) -> lang::Result<Vec<impl Execute>>;
}

// TODO, does this belong somewhere more relevant than in this mod?
pub fn padding_nbytes(payload_nbytes: usize, block_nbytes: usize) -> usize {
    let rem_nbytes = payload_nbytes % block_nbytes;
    (block_nbytes - rem_nbytes) % block_nbytes
}
