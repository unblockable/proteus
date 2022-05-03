use bytes::Bytes;

/// Covert application data that we are attempting to transfer across our
/// pluggable transport. The formatter will obfuscate this by wrapping it in
/// frames with various header fields, encrypting/decrypting, etc.
pub struct CovertPayload {
    pub data: Bytes,
}

#[derive(Clone)]
pub enum FieldKind {
    Fixed(Bytes),
    Length(u8), // num bytes
    Random(u8), // num bytes
    Payload,
}

#[derive(Clone)]
pub struct FrameField {
    pub kind: FieldKind
}

impl FrameField {
    pub fn new(kind: FieldKind) -> FrameField {
        FrameField { kind }
    }

    /// Returns the number of bytes this field will consume on the wire, or 0 if
    /// it's a variable length field.
    pub fn len(&self) -> usize {
        match &self.kind {
            FieldKind::Fixed(b) => b.len(),
            FieldKind::Length(l) => usize::from(*l),
            FieldKind::Random(l) => usize::from(*l),
            FieldKind::Payload => 0,
        }
    }
}

#[derive(Clone)]
pub struct OvertFrameSpec {
    fields: Vec<FrameField>,
}

impl OvertFrameSpec {
    pub fn new() -> OvertFrameSpec {
        OvertFrameSpec { fields: Vec::new() }
    }

    pub fn push_field(&mut self, field: FrameField) {
        self.fields.push(field)
    }

    pub fn get_fields(&self) -> &Vec<FrameField> {
        &self.fields
    }

    pub fn get_fixed_len(&self) -> usize {
        self.fields
            .iter()
            .map(|f| f.len())
            .collect::<Vec<usize>>()
            .iter()
            .sum()
    }
}
