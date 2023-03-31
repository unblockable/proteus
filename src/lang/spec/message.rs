use crate::lang::field::Field;

pub struct MessageSpec {
    fields: Vec<Field>,
}

impl MessageSpec {
    pub fn get_field(&self, index: usize) -> &Field {
        &self.fields.get(index).unwrap()
    }
}

pub struct MessageSpecBuilder {
    fields: Vec<Field>,
}

impl MessageSpecBuilder {
    pub fn new() -> Self {
        Self { fields: vec![] }
    }

    pub fn add_field(&mut self, field: Field) {
        self.fields.push(field)
    }
}

impl From<MessageSpecBuilder> for MessageSpec {
    fn from(builder: MessageSpecBuilder) -> Self {
        MessageSpec {
            fields: builder.fields,
        }
    }
}
