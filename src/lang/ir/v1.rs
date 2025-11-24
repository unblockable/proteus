use crate::lang::Role;
use crate::lang::types::{AbstractFormat, Identifier, PubkeyEncoding};

// Auto-generates `From` implementations like
//   `impl From<WriteAppArgs> for Instruction`
// so we can upcast from args to the instruction variant.
#[enum_from::enum_from]
#[derive(Debug)]
pub enum InstructionV1 {
    ComputeLength(ComputeLengthArgs),
    ConcretizeFormat(ConcretizeFormatArgs),
    CreateMessage(CreateMessageArgs),
    DecryptField(DecryptFieldArgs),
    EncryptField(EncryptFieldArgs),
    GetArrayBytes(GetArrayBytesArgs),
    GetNumericValue(GetNumericValueArgs),
    InitFixedSharedKey(InitFixedSharedKeyArgs),
    Read(ReadArgs),
    SetArrayBytes(SetArrayBytesArgs),
    SetNumericValue(SetNumericValueArgs),
    WriteApp(WriteAppArgs),
    WriteNet(WriteNetArgs),
    WriteNetTwice(WriteNetTwiceArgs),
    SaveKey(SaveKeyArgs),
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
    pub padding_field: Option<Identifier>,
    pub block_size_nbytes: Option<usize>,
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
    pub from_mac_field_id: Option<Identifier>,
    pub to_plaintext_heap_id: Identifier,
}

/// TODO
#[derive(Debug)]
pub struct EncryptFieldArgs {
    pub from_msg_heap_id: Identifier,
    pub from_field_id: Identifier,
    pub to_ciphertext_heap_id: Identifier,
    pub to_mac_heap_id: Option<Identifier>,
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
    pub role: Role,
}

/// Read a number of bytes given by the `from_len` range and store the result on
/// the heap in `to_heap_id`.
#[derive(Debug)]
pub struct ReadArgs {
    pub which: ReadWhich,
    pub how: ReadHow,
    pub from_len: ReadLength,
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

/// Write the bytes from the message stored on the heap at `from_msg_heap_id` to
/// the network in two write operations. We will write len_first_write first,
/// and then the remaining bytes second.
#[derive(Debug)]
pub struct WriteNetTwiceArgs {
    pub from_msg_heap_id: Identifier,
    pub len_first_write: usize,
}

#[derive(Debug)]
pub struct SaveKeyArgs {
    pub from_msg_heap_id: Identifier,
    pub from_field_id: Identifier, // usually payload field
    pub pubkey_encoding: PubkeyEncoding,
}

#[derive(Debug)]
pub enum ReadLength {
    /// Amount to read specified in this heap variable.
    Identifier(Identifier),
    /// Amount to read specified in this heap variable minus the usize value.
    IdentifierMinus((Identifier, usize)),
    /// Amount to read specified in this heap variable minus the second heap
    /// variable minus the usize value.
    IdentifierMinusMinus((Identifier, Identifier, usize)),
    /// Amount to read specified by this fixed usize value.
    Fixed(usize),
}

/// The read operation to perform.
#[derive(Debug)]
pub enum ReadHow {
    /// Async read will return between 1 and the specified len bytes, depending
    /// on how much is available in the underlying stream.
    Read,
    /// Like `Read`, but will immediately return if no bytes are available
    /// instead of blocking awaiting for bytes to arrive.
    TryRead,
    /// Read that will block awaiting for exactly the specified len bytes
    /// to be available in the underlying stream.
    ReadExact,
}

/// The buffer on which to execute the read.
#[derive(Debug)]
pub enum ReadWhich {
    App,
    Net,
}
