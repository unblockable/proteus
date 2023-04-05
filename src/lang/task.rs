#![allow(dead_code)]

use crate::lang::types::{AbstractFormat, Identifier};
use std::ops::Range;

use std::convert::From;

pub struct TaskID {}

impl TaskID {
    pub fn default() -> TaskID {
        TaskID {}
    }
}

pub struct Task {
    pub ins: Vec<Instruction>,
    pub id: TaskID,
}

pub enum Instruction {
    ReadApp(ReadAppArgs),
    ConcretizeFormat(ConcretizeFormatArgs),
    /// Not cryptographically secure.
    GenRandomBytes(GenRandomBytesArgs),
    CreateMessage(CreateMessageArgs),
    WriteNet(WriteNetArgs),
    ReadNet(ReadNetArgs),
    ComputeLength(ComputeLengthArgs),
    WriteApp(WriteAppArgs),
}

pub struct ReadAppArgs {
    pub name: Identifier,
    pub len: Range<usize>,
}

impl From<ReadAppArgs> for Instruction {
    fn from(value: ReadAppArgs) -> Self {
        Instruction::ReadApp(value)
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

pub struct GenRandomBytesArgs {
    pub name: Identifier,
    pub len: Range<usize>,
}

impl From<GenRandomBytesArgs> for Instruction {
    fn from(value: GenRandomBytesArgs) -> Self {
        Instruction::GenRandomBytes(value)
    }
}

pub struct CreateMessageArgs {
    pub name: Identifier,
    /// Specifies the location of the format object on the heap.
    pub fmt_name: Identifier,
    /// Specifies the locations of heap data and field names into which to copy
    /// the data.
    pub field_names: Vec<Identifier>,
}

impl From<CreateMessageArgs> for Instruction {
    fn from(value: CreateMessageArgs) -> Self {
        Instruction::CreateMessage(value)
    }
}

pub struct WriteNetArgs {
    pub msg_name: Identifier,
}

impl From<WriteNetArgs> for Instruction {
    fn from(value: WriteNetArgs) -> Self {
        Instruction::WriteNet(value)
    }
}

pub enum ReadNetLength {
    /// Amount to read specified in this heap variable.
    Identifier(Identifier),
    /// Amount to read specified by this range.
    Range(Range<usize>),
}

pub struct ReadNetArgs {
    pub name: Identifier,
    pub len: ReadNetLength,
}

impl From<ReadNetArgs> for Instruction {
    fn from(value: ReadNetArgs) -> Self {
        Instruction::ReadNet(value)
    }
}

pub struct ComputeLengthArgs {
    pub name: Identifier,
    pub msg_name: Identifier,
}

impl From<ComputeLengthArgs> for Instruction {
    fn from(value: ComputeLengthArgs) -> Self {
        Instruction::ComputeLength(value)
    }
}
pub struct WriteAppArgs {
    pub msg_name: Identifier,
}

impl From<WriteAppArgs> for Instruction {
    fn from(value: WriteAppArgs) -> Self {
        Instruction::WriteApp(value)
    }
}
