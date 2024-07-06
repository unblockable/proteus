mod compiler;
pub mod interpreter;
mod memory;
mod message;
pub mod parse;
pub mod spec;
mod task;
mod types;

// #[cfg(test)]
// mod test;

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Role {
    Client,
    Server,
}
