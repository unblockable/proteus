use crate::lang::ir::v1::InstructionV1;

pub mod bridge;
pub mod v1;

pub enum Instruction {
    // TODO: remove when the compiler constructs `Instruction::V1`s.
    #[allow(dead_code)]
    V1(InstructionV1),
}

#[cfg(test)]
pub mod test;
