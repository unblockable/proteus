use bytes::Bytes;

#[derive(Clone)]
pub struct Width {
    bytes: u8
}

#[derive(Clone)]
pub enum FrameField {
    FixedValue(Bytes),
    VariableLength(Width),
    VariableRandom(Width),
    VariablePayload(Width)
}

#[derive(Clone)]
pub struct OvertFrameSpec {
    fields: Vec<FrameField>
}

impl OvertFrameSpec {
    pub fn new() -> OvertFrameSpec {
        OvertFrameSpec {
            fields: Vec::new()
        }
    }

    pub fn push_field(&mut self, field: FrameField) {
        self.fields.push(field)
    }

    pub fn insert_field(&mut self, index: usize, field: FrameField) {
        match index > self.fields.len() {
            true => self.fields.push(field),
            false => self.fields.insert(index, field)
        }
    }
}
