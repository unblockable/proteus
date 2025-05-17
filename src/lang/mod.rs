mod compiler;
pub mod interpreter;
mod memory;
mod message;
pub mod parse;
pub mod spec;
pub mod task;
mod types;

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Role {
    Client,
    Server,
}

pub fn padding_nbytes(payload_nbytes: usize, block_nbytes: usize) -> usize {
    let rem_nbytes = payload_nbytes % block_nbytes;
    (block_nbytes - rem_nbytes) % block_nbytes
}
