use crate::lang::message::Message;
use crate::lang::types::{ConcreteFormat, Identifier};
use anyhow::{anyhow, bail};
use bytes::Bytes;
use std::collections::HashMap;

#[derive(Debug)]
pub enum HeapDataKind {
    Bytes,
    Format,
    Message,
    Number,
}

// Generates `From` impls so we can go from the inner value to an enum instance
// of the associated variant.
#[enum_from::enum_from]
// Generates `TryFrom` impls so we can go from an instance of `HeapData` to the
// inner value, or an error if the inner value has an unexpected type.
#[enum_from::enum_try_from]
pub enum HeapData {
    Bytes(Bytes),
    Format(ConcreteFormat),
    Message(Message),
    Number(u128),
}

impl HeapData {
    pub fn kind(&self) -> HeapDataKind {
        match self {
            HeapData::Bytes(_) => HeapDataKind::Bytes,
            HeapData::Format(_) => HeapDataKind::Format,
            HeapData::Message(_) => HeapDataKind::Message,
            HeapData::Number(_) => HeapDataKind::Number,
        }
    }
}

pub struct Heap {
    mem: HashMap<Identifier, HeapData>,
}

impl Heap {
    pub fn new() -> Self {
        Self {
            mem: HashMap::new(),
        }
    }

    pub fn insert<T>(&mut self, addr: Identifier, data: T) -> anyhow::Result<()>
    where
        T: Into<HeapData>,
    {
        self.mem
            .insert(addr.clone(), data.into())
            .map_or(Ok(()), |_| {
                bail!("Overwrote heap data at address '{addr:?}'")
            })
    }

    pub fn get<'a, T>(&'a self, addr: &Identifier) -> anyhow::Result<T>
    where
        T: TryFrom<&'a HeapData>,
    {
        let data = self
            .mem
            .get(addr)
            .ok_or(anyhow!("No heap data at address '{addr:?}'"))?;
        T::try_from(data)
            .map_err(|_| anyhow!("Heap data is not of the requested type '{:?}'", data.kind()))
    }

    pub fn remove<T>(&mut self, addr: &Identifier) -> anyhow::Result<T>
    where
        T: TryFrom<HeapData>,
    {
        let data = self
            .mem
            .remove(addr)
            .ok_or(anyhow!("No heap data at address '{addr:?}'"))?;
        let kind = data.kind();
        T::try_from(data)
            .map_err(|_| anyhow!("Heap data is not of the requested type '{:?}'", kind))
    }
}
