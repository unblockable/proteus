use std::collections::HashMap;

use anyhow::{anyhow, bail};

use crate::lang::Data;
use crate::lang::types::Identifier;

pub struct Heap {
    mem: HashMap<Identifier, Data>,
}

impl Heap {
    pub fn new() -> Self {
        Self {
            mem: HashMap::new(),
        }
    }

    pub fn insert<T: Into<Data>>(&mut self, addr: Identifier, data: T) -> anyhow::Result<()> {
        self.mem
            .insert(addr.clone(), data.into())
            .map_or(Ok(()), |_| {
                bail!("Overwrote heap data at address '{addr:?}'")
            })
    }

    pub fn get<'a, T: TryFrom<&'a Data>>(&'a self, addr: &Identifier) -> anyhow::Result<T> {
        let data = self
            .mem
            .get(addr)
            .ok_or(anyhow!("No heap data at address '{addr:?}'"))?;
        T::try_from(data)
            .map_err(|_| anyhow!("Heap data is not of the requested type '{:?}'", data.kind()))
    }

    pub fn remove<T: TryFrom<Data>>(&mut self, addr: &Identifier) -> anyhow::Result<T> {
        let data = self
            .mem
            .remove(addr)
            .ok_or(anyhow!("No heap data at address '{addr:?}'"))?;
        let kind = data.kind();
        T::try_from(data)
            .map_err(|_| anyhow!("Heap data is not of the requested type '{:?}'", kind))
    }

    pub fn clear(&mut self) {
        self.mem.clear();
    }
}
