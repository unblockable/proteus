use std::collections::HashMap;

use crate::lang::data::{Data, DataKind};
use crate::lang::types::Identifier;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Heap address occupied: {addr:?}")]
    AddressOccupied { addr: Identifier },
    #[error("Heap address vacant: {addr:?}")]
    AddressVacant { addr: Identifier },
    #[error("Invalid heap data kind: {kind:?}")]
    InvalidDataKind { kind: DataKind },
}

pub struct Heap {
    mem: HashMap<Identifier, Data>,
}

impl Heap {
    pub fn new() -> Self {
        Self {
            mem: HashMap::new(),
        }
    }

    pub fn insert<T: Into<Data>>(&mut self, addr: Identifier, data: T) -> Result<(), self::Error> {
        self.mem
            .insert(addr.clone(), data.into())
            .map_or(Ok(()), |_| Err(self::Error::AddressOccupied { addr }))
    }

    pub fn get<'a, T: TryFrom<&'a Data>>(&'a self, addr: &Identifier) -> Result<T, self::Error> {
        let data = self
            .mem
            .get(addr)
            .ok_or(self::Error::AddressVacant { addr: addr.clone() })?;
        T::try_from(data).map_err(|_| self::Error::InvalidDataKind { kind: data.kind() })
    }

    pub fn remove<T: TryFrom<Data>>(&mut self, addr: &Identifier) -> Result<T, self::Error> {
        let data = self
            .mem
            .remove(addr)
            .ok_or(self::Error::AddressVacant { addr: addr.clone() })?;
        let kind = data.kind();
        T::try_from(data).map_err(|_| self::Error::InvalidDataKind { kind })
    }

    pub fn clear(&mut self) {
        self.mem.clear();
    }
}
