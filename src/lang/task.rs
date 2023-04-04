#![allow(dead_code)]

use std::ops::Range;
use crate::lang::types::{Identifier, AbstractFormat};

use std::convert::From;

struct TaskID {}

struct Task {
    ins: Vec<Instruction>,
    id: TaskID,
}

enum Instruction {
    ReadApp(ReadArgs),
    ReadNet(ReadArgs),
    ConcretizeFormat(ConcretizeFormatArgs),
    GenUniformRandom(GenUniformRandomArgs),
}

pub struct ReadArgs {
    pub name: Identifier,
    pub range: Range<usize>,
}

impl From<ReadArgs> for Instruction {
    fn from(value: ReadArgs) -> Self {
        Instruction::ReadApp(value)
    }
}

pub struct ConcretizeFormatArgs {
    pub name: Identifier,
    pub aformat: AbstractFormat
}

impl From<ConcretizeFormatArgs> for Instruction {
    fn from(value: ConcretizeFormatArgs) -> Self {
        Instruction::ConcretizeFormat(value)
    }
}

pub struct GenUniformRandomArgs {
    pub name: Identifier,
    pub range: Range<usize>,
}

impl From<GenUniformRandomArgs> for Instruction {
    fn from(value: GenUniformRandomArgs) -> Self {
        Instruction::GenUniformRandom(value)
    }
}
