use std::ops::Range;

use crate::lang::format::Format;

pub enum CryptoOperationKind {
    Encrypt,
    Decrypt,
}

pub struct CryptoOperation {
    kind: CryptoOperationKind,
    input_fmt_range: Range<usize>,
    output_fmt_range: Range<usize>,
}

pub struct CryptoSpec {
    input_fmt: Format,
    output_fmt: Format,
    operations: Vec<CryptoOperation>,
}

impl CryptoSpec {
    pub fn new(input_fmt: Format, output_fmt: Format) -> Self {
        Self {
            input_fmt,
            output_fmt,
            operations: Vec::new(),
        }
    }

    pub fn add_operation(&mut self, op: CryptoOperation) {
        // TODO: do range checks and panic if out of range based on input and output formats
        self.operations.push(op)
    }

    pub fn get_operations(&self) -> &Vec<CryptoOperation> {
        &self.operations
    }
}
