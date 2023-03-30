use std::ops::Range;

use bytes::Bytes;

use crate::lang::mem::HeapAddr;

pub struct WriteCmdArgs {
    // Write these bytes.
    pub bytes: Bytes,
}

pub struct ReadCmdArgs {
    // Read this many bytes.
    pub read_len: Range<usize>,
    // Store the bytes at this addr on the heap.
    pub store_addr: HeapAddr,
}

pub enum NetCommandOut {
    ReadApp(ReadCmdArgs),
    WriteNet(WriteCmdArgs),
    Close,
}

pub enum NetCommandIn {
    ReadNet(ReadCmdArgs),
    WriteApp(WriteCmdArgs),
    Close,
}
