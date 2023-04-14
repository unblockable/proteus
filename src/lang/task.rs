#![allow(dead_code)]

use crate::lang::types::{AbstractFormat, Identifier};
use std::ops::Range;

use std::convert::From;

pub trait TaskProvider {
    fn get_next_tasks(&self, last_task: &TaskID) -> TaskSet;
}

#[derive(Debug)]
pub struct TaskPair {
    pub in_task: Task,
    pub out_task: Task,
}

#[derive(Debug)]
pub enum TaskSet {
    InTask(Task),
    OutTask(Task),
    InAndOutTasks(TaskPair),
}

#[derive(Eq, PartialEq, Clone, Copy, Debug)]
pub struct TaskID {
    id: usize,
}

impl TaskID {
    pub fn into_inner(&self) -> usize {
        self.id
    }
}

impl From<TaskID> for usize {
    fn from(value: TaskID) -> Self {
        value.id
    }
}

impl From<usize> for TaskID {
    fn from(value: usize) -> Self {
        TaskID { id: value }
    }
}

impl std::default::Default for TaskID {
    fn default() -> TaskID {
        TaskID { id: 0 }
    }
}

#[derive(Debug)]
pub struct Task {
    pub ins: Vec<Instruction>,
    pub id: TaskID,
}

// Auto-generates from implementations like
//   `impl From<WriteAppArgs> for Instruction`
// so we can upcast from args to the instruction variant.
#[enum_from::enum_from]
#[derive(Debug)]
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
#[derive(Debug)]
pub struct ComputeLengthArgs {
    pub name: Identifier,
    pub msg_name: Identifier,
    pub field_name: Identifier,
}


#[derive(Debug)]
pub struct ReadAppArgs {
    pub name: Identifier,
    pub len: Range<usize>,
}

#[derive(Debug)]
pub struct ConcretizeFormatArgs {
    pub name: Identifier,
    pub aformat: AbstractFormat,
}


#[derive(Debug)]
pub struct GenRandomBytesArgs {
    pub name: Identifier,
    pub len: Range<usize>,
}

#[derive(Debug)]
pub struct CreateMessageArgs {
    pub name: Identifier,
    /// Specifies the location of the format object on the heap.
    pub fmt_name: Identifier,
    /// Specifies the locations of heap data and field names into which to copy
    /// the data.
    pub field_names: Vec<Identifier>,
}

#[derive(Debug)]
pub struct WriteNetArgs {
    pub msg_name: Identifier,
}

#[derive(Debug)]
pub enum ReadNetLength {
    /// Amount to read specified in this heap variable.
    Identifier(Identifier),
    /// Amount to read specified by this range.
    Range(Range<usize>),
}

#[derive(Debug)]
pub struct ReadNetArgs {
    pub name: Identifier,
    pub len: ReadNetLength,
}

// Get the numeric value from msg->field and store it in name.
#[derive(Debug)]
pub struct GetNumericValueArgs {
    pub name: Identifier,
    pub msg_name: Identifier,
    pub field_name: Identifier,
}

#[derive(Debug)]
pub struct SetNumericValueArgs {
    pub msg_name: Identifier,
    pub field_name: Identifier,
    pub name: Identifier,
}

#[derive(Debug)]
pub struct WriteAppArgs {
    pub msg_name: Identifier,
    pub field_name: Identifier, // usually payload field
}
