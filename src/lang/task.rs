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

#[derive(Debug)]
pub enum ReadNetLength {
    /// Amount to read specified in this heap variable.
    Identifier(Identifier),
    /// Amount to read specified in this heap variable minus the given value.
    IdentifierMinus((Identifier, usize)),
    /// Amount to read specified by this range.
    Range(Range<usize>),
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
    DecryptField(DecryptFieldArgs),
    EncryptField(EncryptFieldArgs),
    GenRandomBytes(GenRandomBytesArgs),
    GetArrayBytes(GetArrayBytesArgs),
    GetNumericValue(GetNumericValueArgs),
    InitFixedSharedKey(InitFixedSharedKeyArgs),
    ReadApp(ReadAppArgs),
    ReadNet(ReadNetArgs),
    SetArrayBytes(SetArrayBytesArgs),
    SetNumericValue(SetNumericValueArgs),
    WriteApp(WriteAppArgs),
    WriteNet(WriteNetArgs),
}

/// Compute the length of all `from_msg_id` fields that are ordered after
/// `from_field_id`, and store the length in `to_heap_id`.
#[derive(Debug)]
pub struct ComputeLengthArgs {
    pub from_msg_heap_id: Identifier,
    pub from_field_id: Identifier,
    pub to_heap_id: Identifier,
}

/// Instantiates a `ConcreteFormat` from the given `from_format` and stores the
/// result in `to_heap_id`. All fields of type `DynamicArray` must already
/// contain a bytes object with an identical id on the heap when using this
/// instruction, or else it will fail.
#[derive(Debug)]
pub struct ConcretizeFormatArgs {
    pub from_format: AbstractFormat,
    pub to_heap_id: Identifier,
}

/// Creates an allocated message from the `ConcreteFormat` on the heap given by
/// `from_format_heap_id` and stores the message on the heap in `to_heap_id`.
#[derive(Debug)]
pub struct CreateMessageArgs {
    pub from_format_heap_id: Identifier,
    pub to_heap_id: Identifier,
}

/// TODO
#[derive(Debug)]
pub struct DecryptFieldArgs {
    pub from_msg_heap_id: Identifier,
    pub from_ciphertext_field_id: Identifier,
    pub from_mac_field_id: Identifier,
    pub to_plaintext_heap_id: Identifier,
}

/// TODO
#[derive(Debug)]
pub struct EncryptFieldArgs {
    pub from_msg_heap_id: Identifier,
    pub from_field_id: Identifier,
    pub to_ciphertext_heap_id: Identifier,
    pub to_mac_heap_id: Identifier,
}

/// TODO. Generate cryptographically insecure random bytes.
#[derive(Debug)]
pub struct GenRandomBytesArgs {
    pub from_len: Range<usize>,
    pub to_heap_id: Identifier,
}

/// Get the bytes data from the field given by `from_field_id` inside of the
/// message stored on the heap at `from_msg_heap_id`, and store the bytes on the
/// heap in `to_heap_id`.
#[derive(Debug)]
pub struct GetArrayBytesArgs {
    pub from_msg_heap_id: Identifier,
    pub from_field_id: Identifier,
    pub to_heap_id: Identifier,
}

/// Get the numeric value from the field given by `from_field_id` inside of the
/// message stored on the heap at `from_msg_heap_id`, and store the value on the
/// heap in `to_heap_id`.
#[derive(Debug)]
pub struct GetNumericValueArgs {
    pub from_msg_heap_id: Identifier,
    pub from_field_id: Identifier,
    pub to_heap_id: Identifier,
}

/// TODO
#[derive(Debug)]
pub struct InitFixedSharedKeyArgs {
    pub password: String,
}

/// Read a number of bytes given by the `from_len` range from the application
/// and store the result on the heap in `to_heap_id`.
#[derive(Debug)]
pub struct ReadAppArgs {
    pub from_len: Range<usize>,
    pub to_heap_id: Identifier,
}

/// Read a number of bytes given by `from_len` from the network and store the
/// result on the heap in `to_heap_id`.
#[derive(Debug)]
pub struct ReadNetArgs {
    pub from_len: ReadNetLength,
    pub to_heap_id: Identifier,
}

/// Set the bytes stored on the heap at `from_heap_id` in the field
/// `to_field_id` inside the message stored on the heap at `to_msg_heap_id`.
#[derive(Debug)]
pub struct SetArrayBytesArgs {
    pub from_heap_id: Identifier,
    pub to_msg_heap_id: Identifier,
    pub to_field_id: Identifier,
}

/// Set the numeric value stored on the heap at `from_heap_id` in the field
/// `to_field_id` inside the message stored on the heap at `to_msg_heap_id`.
#[derive(Debug)]
pub struct SetNumericValueArgs {
    pub from_heap_id: Identifier,
    pub to_msg_heap_id: Identifier,
    pub to_field_id: Identifier,
}

/// Write the bytes from the field `from_field_id` inside of the message stored
/// at `from_msg_heap_id` on the heap to the application.
#[derive(Debug)]
pub struct WriteAppArgs {
    pub from_msg_heap_id: Identifier,
    pub from_field_id: Identifier, // usually payload field
}

/// Write the bytes from the message stored on the heap at `from_msg_heap_id` to
/// the network.
#[derive(Debug)]
pub struct WriteNetArgs {
    pub from_msg_heap_id: Identifier,
}
