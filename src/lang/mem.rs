use std::collections::HashMap;

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

pub struct Heap<T> {
    addr_counter: u64,
    mem: HashMap<HeapAddr, T>,
}

impl<T> Heap<T> {
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

    pub fn write(&mut self, addr: HeapAddr, data: T) -> Option<T> {
        self.mem.insert(addr, data)
    }

    pub fn read(&self, addr: &HeapAddr) -> Option<&T> {
        self.mem.get(addr)
    }

    pub fn free(&mut self, addr: &HeapAddr) -> Option<T> {
        self.mem.remove(addr)
    }
}
