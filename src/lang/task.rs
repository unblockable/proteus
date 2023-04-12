#![allow(dead_code)]

use crate::lang::types::{AbstractFormat, Identifier};
use std::ops::Range;

use std::convert::From;

pub trait TaskProvider {
    fn get_next_tasks(&self, last_task: &TaskID) -> TaskSet;
}

pub struct TaskPair {
    pub in_task: Task,
    pub out_task: Task,
}

pub enum TaskSet {
    InTask(Task),
    OutTask(Task),
    InAndOutTasks(TaskPair),
}

#[derive(Eq, PartialEq)]
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
    ComputeLength(ComputeLengthArgs),
    ConcretizeFormat(ConcretizeFormatArgs),
    CreateMessage(CreateMessageArgs),
    /// Not cryptographically secure.
    GenRandomBytes(GenRandomBytesArgs),
    GetNumericValue(GetNumericValueArgs),
    ReadApp(ReadAppArgs),
    ReadNet(ReadNetArgs),
    SetNumericValue(SetNumericValueArgs),
    WriteApp(WriteAppArgs),
    WriteNet(WriteNetArgs),
}

/// Compute the length of all msg fields after field and store in name.
pub struct ComputeLengthArgs {
    pub name: Identifier,
    pub msg_name: Identifier,
    pub field_name: Identifier,
}

impl From<ComputeLengthArgs> for Instruction {
    fn from(value: ComputeLengthArgs) -> Self {
        Instruction::ComputeLength(value)
    }
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

// Get the numeric value from msg->field and store it in name.
pub struct GetNumericValueArgs {
    pub name: Identifier,
    pub msg_name: Identifier,
    pub field_name: Identifier,
}

impl From<GetNumericValueArgs> for Instruction {
    fn from(value: GetNumericValueArgs) -> Self {
        Instruction::GetNumericValue(value)
    }
}

pub struct SetNumericValueArgs {
    pub msg_name: Identifier,
    pub field_name: Identifier,
    pub name: Identifier,
}

impl From<SetNumericValueArgs> for Instruction {
    fn from(value: SetNumericValueArgs) -> Self {
        Instruction::SetNumericValue(value)
    }
}

pub struct WriteAppArgs {
    pub msg_name: Identifier,
    pub field_name: Identifier, // usually payload field
}

impl From<WriteAppArgs> for Instruction {
    fn from(value: WriteAppArgs) -> Self {
        Instruction::WriteApp(value)
    }
}
