use std::collections::HashMap;

use crate::lang::types::Identifier;

pub struct Heap<T> {
    mem: HashMap<Identifier, T>,
}

impl<T> Heap<T> {
    pub fn new() -> Self {
        Self {
            mem: HashMap::new(),
        }
    }

    pub fn insert(&mut self, addr: Identifier, data: T) -> Option<T> {
        self.mem.insert(addr, data)
    }

    pub fn get(&self, addr: &Identifier) -> Option<&T> {
        self.mem.get(addr)
    }

    pub fn remove(&mut self, addr: &Identifier) -> Option<T> {
        self.mem.remove(addr)
    }
}
