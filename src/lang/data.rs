use bytes::Bytes;

use crate::lang::message::Message;
use crate::lang::types::ConcreteFormat;

// Generates `From` impls so we can go from the inner value to an enum instance
// of the associated variant.
#[enum_from::enum_from]
// Generates `TryFrom` impls so we can go from an instance of `HeapData` to the
// inner value, or an error if the inner value has an unexpected type.
#[enum_from::enum_try_from]
/// Data that can be stored by the VM.
pub enum Data {
    Bytes(Bytes),
    Format(ConcreteFormat),
    Message(Message),
    Number(u128),
}

#[derive(Debug)]
pub enum DataKind {
    Bytes,
    Format,
    Message,
    Number,
}

impl Data {
    pub fn kind(&self) -> DataKind {
        match self {
            Data::Bytes(_) => DataKind::Bytes,
            Data::Format(_) => DataKind::Format,
            Data::Message(_) => DataKind::Message,
            Data::Number(_) => DataKind::Number,
        }
    }
}
