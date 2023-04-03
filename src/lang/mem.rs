use std::collections::HashMap;

use bytes::Bytes;

use crate::lang::types::DataType;

pub struct Data {
    pub kind: DataType,
    // pub data: Vec<u8>,
    pub data: Bytes, // TODO we don't want this, just using it to start
}

#[derive(Eq, Hash, PartialEq, Clone)]
pub struct HeapAddr {
    addr: String,
}

impl From<&str> for HeapAddr {
    fn from(s: &str) -> Self {
        HeapAddr {
            addr: s.to_string(),
        }
    }
}

pub struct Heap {
    addr_counter: u64,
    mem: HashMap<HeapAddr, Data>,
}

impl Heap {
    pub fn new() -> Self {
        Self {
            addr_counter: 0,
            mem: HashMap::new(),
        }
    }

    pub fn alloc(&mut self) -> HeapAddr {
        let addr = self.addr_counter.to_string();
        self.addr_counter += 1;
        HeapAddr { addr }
    }

    pub fn write(&mut self, addr: HeapAddr, data: Data) -> Option<Data> {
        self.mem.insert(addr, data)
    }

    pub fn read(&self, addr: &HeapAddr) -> Option<&Data> {
        self.mem.get(addr)
    }

    pub fn free(&mut self, addr: &HeapAddr) -> Option<Data> {
        self.mem.remove(addr)
    }
}
