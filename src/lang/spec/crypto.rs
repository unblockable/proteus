use std::ops::Range;

use super::message::MessageSpec;

pub enum CryptoOperationKind {
    Encrypt,
    Decrypt,
}

pub struct CryptoOperation {
    pub kind: CryptoOperationKind,
    pub input_fmt_range: Range<usize>,
    pub output_fmt_range: Range<usize>,
}

pub struct CryptoSpec {
    // input_fmt: MessageSpec,
    // output_fmt: MessageSpec,
    operations: Vec<CryptoOperation>,
}

impl CryptoSpec {
    // pub fn new(input_fmt: MessageSpec, output_fmt: MessageSpec) -> Self {
    //     Self {
    //         input_fmt,
    //         output_fmt,
    //         operations: Vec::new(),
    //     }
    // }

    pub fn add_operation(&mut self, op: CryptoOperation) {
        // TODO: do range checks and panic if out of range based on input and output formats
        self.operations.push(op)
    }

    pub fn get_operations(&self) -> &Vec<CryptoOperation> {
        &self.operations
    }
}

pub struct CryptoSpecBuilder {
    operations: Vec<CryptoOperation>,
}

impl CryptoSpecBuilder {
    pub fn new() -> Self {
        Self { operations: vec![] }
    }

    pub fn add_operation(&mut self, op: CryptoOperation) {
        // TODO: do range checks and panic if out of range based on input and output formats
        self.operations.push(op)
    }
}

impl From<CryptoSpecBuilder> for CryptoSpec {
    fn from(builder: CryptoSpecBuilder) -> Self {
        CryptoSpec {
            operations: builder.operations,
        }
    }
}
