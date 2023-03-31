use bytes::Bytes;

#[derive(Clone)]
pub enum LengthFieldKind {
    // Length field covers payload data only.
    Payload,
    // Length field covers all variable-length fields.
    Variable,
    // Length field covers all fields.
    All,
}

#[derive(Clone)]
pub struct LengthField {
    // Tells us how to compute the value to write into the length field.
    kind: LengthFieldKind,
    // Tells us the number of bytes that are used to store the length,
    // which constrains the total message size.
    size: u8,
}

impl LengthField {
    pub fn new(kind: LengthFieldKind, size: u8) -> LengthField {
        LengthField { kind, size }
    }
}

#[derive(Clone)]
pub enum FieldKind {
    // The value of the fixed enum is the actual value that will occur in the packet.
    Fixed(Bytes), // Fixed size, fixed value
    // The length enum holds the length of the variable-length fields of the packet in bytes
    Length(LengthField), // Fixed size, variable value
    // Unstructured bytes of unknown length, e.g., encrypted data.
    Blob,
    // Payload bytes that we forward through our tunnel are supplied by the caller.
    Payload, // Variable size, variable value
}

#[derive(Clone)]
pub struct Field {
    pub kind: FieldKind,
}

impl Field {
    pub fn new(kind: FieldKind) -> Field {
        Field { kind }
    }

    /// Returns the number of bytes this field will consume on the wire, or 0 if
    /// it's a variable length field.
    pub fn len(&self) -> usize {
        match &self.kind {
            FieldKind::Fixed(b) => b.len(),
            FieldKind::Length(l) => 0, //usize::from(*l),
            FieldKind::Blob => 0,
            FieldKind::Payload => 0,
        }
    }
}
