#![allow(dead_code)]

use crate::lang::types::{AbstractFormat, Identifier};
use std::ops::Range;

use std::convert::From;

pub struct TaskID {}

impl TaskID {
    fn default() -> TaskID {
        TaskID {}
    }
}

pub struct Task {
    ins: Vec<Instruction>,
    id: TaskID,
}

enum Instruction {
    ReadApp(ReadAppArgs),
    ReadNet(ReadNetArgs),
    ConcretizeFormat(ConcretizeFormatArgs),
    GenUniformRandom(GenUniformRandomArgs),
}

pub struct ReadAppArgs {
    pub name: Identifier,
    pub range: Range<usize>,
}

impl From<ReadAppArgs> for Instruction {
    fn from(value: ReadAppArgs) -> Self {
        Instruction::ReadApp(value)
    }
}

pub struct ReadNetArgs {
    pub name: Identifier,
    pub range: Range<usize>,
}

impl From<ReadNetArgs> for Instruction {
    fn from(value: ReadNetArgs) -> Self {
        Instruction::ReadNet(value)
    }
}

pub struct ConcretizeFormatArgs {
    pub name: Identifier,
    pub aformat: AbstractFormat,
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
