use std::ops::Range;

use crate::lang::format::Format;

pub enum MessageOperationKind {
    Add,
}

pub struct MessageOperation {
    kind: MessageOperationKind,
    output_fmt_range: Range<usize>,
}

pub struct MessageSpec {
    fmt: Format,
    operations: Vec<MessageOperation>,
}

impl MessageSpec {
    pub fn new(fmt: Format) -> Self {
        Self {
            fmt,
            operations: Vec::new(),
        }
    }

    pub fn add_operation(&mut self, op: MessageOperation) {
        // TODO: do range checks and panic if out of range based on input and output formats
        self.operations.push(op)
    }

    pub fn get_operations(&self) -> &Vec<MessageOperation> {
        &self.operations
    }
}
