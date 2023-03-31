use std::{
    ops::Range,
    sync::{Arc, Mutex},
};

use bytes::Bytes;

use crate::lang::{
    command::*,
    mem::{Heap, HeapAddr},
    spec::proteus::ProteusSpec,
};

pub struct Interpreter {
    spec: ProteusSpec,
    heap: Heap,
}

impl Interpreter {
    pub fn new(spec: ProteusSpec) -> Self {
        Self {
            spec,
            heap: Heap::new(),
        }
    }

    pub async fn next_net_cmd_out(&mut self) -> NetCmdOut {
        todo!()
    }

    pub async fn next_net_cmd_in(&mut self) -> NetCmdIn {
        todo!()
    }
}

/// Wraps the ionterpreter allowing us to share it across threads.
#[derive(Clone)]
pub struct SharedInterpreter {
    inner: Arc<Mutex<Interpreter>>,
}

impl SharedInterpreter {
    pub fn new(int: Interpreter) -> SharedInterpreter {
        SharedInterpreter {
            inner: Arc::new(Mutex::new(int)),
        }
    }

    pub async fn next_net_cmd_out(&mut self) -> NetCmdOut {
        todo!()
    }

    pub async fn next_net_cmd_in(&mut self) -> NetCmdIn {
        todo!()
    }

    pub fn store(&mut self, addr: HeapAddr, data: Bytes) {
        // TODO need to build a Data object to write below.
        // self.inner.lock().unwrap().heap.write(addr, data);
        todo!()
    }
}
