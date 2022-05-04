use bytes::Bytes;

/// Covert application data that we are attempting to transfer across our
/// pluggable transport. The formatter will obfuscate this by wrapping it in
/// frames with various header fields, encrypting/decrypting, etc.
pub struct CovertPayload {
    pub data: Bytes,
}

pub enum EncryptionMaterialKind {
    Handshake,
    MAC
}

#[derive(Clone)]
pub enum FieldKind {
    // The value of the fixed enum is the actual value that will occur in the packet.
    Fixed(Bytes), // Fixed size, fixed value
    // The length enum holds the length of the variable-length fields of the packet in bytes
    Length(u8), // Fixed size, variable value
    // The random enum holds the length of the random bytes.
    Random(u8), // Fixed size, variable value
    // The encrypted enum holds the length of the encrypted header field.
    // Will be filled with randomness, for now.
    Encrypted(u8), // Fixed size, variable value
    // Payload bytes are supplied by the caller that needs to be transported.
    EncryptionMaterial(EncryptionMaterialKind),
    Payload, //  Variable size, variable value
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
            FieldKind::Encrypted(l) => usize::from(*l),
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
