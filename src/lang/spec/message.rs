struct MessageField {
    size: Option<u64>,
}

pub struct MessageSpec {
    fields: Vec<MessageField>,
}

impl MessageSpec {
    // pub fn add_operation(&mut self, op: MessageOperation) { // TODO: do range
    //     checks and panic if out of range based on input and output formats
    //     self.operations.push(op) }

    // pub fn get_operations(&self) -> &Vec<MessageOperation> {
    //     &self.operations
    // }
}

pub struct MessageSpecBuilder {
    fields: Vec<MessageField>,
}

impl MessageSpecBuilder {
    pub fn new() -> Self {
        Self { fields: vec![] }
    }

    pub fn add_field(&mut self, size: Option<u64>) {
        self.fields.push(MessageField { size })
    }
}

impl From<MessageSpecBuilder> for MessageSpec {
    fn from(builder: MessageSpecBuilder) -> Self {
        MessageSpec {
            fields: builder.fields,
        }
    }
}
