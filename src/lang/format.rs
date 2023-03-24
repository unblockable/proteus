use bytes::Bytes;

pub enum FieldKind {
    // The value of the fixed enum is the actual value that will occur in the packet.
    Fixed(Bytes), // Fixed size, fixed value
    // The length enum holds the length of the variable-length fields of the packet in bytes
    Length(u8), // Fixed size, variable value
    // Unstructured bytes of unknown length.
    Blob,
    // Payload bytes that we forward through our tunnel are supplied by the caller.
    Payload, // Variable size, variable value
}

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
            FieldKind::Length(l) => usize::from(*l),
            FieldKind::Blob => 0,
            FieldKind::Payload => 0,
        }
    }
}

pub struct Format {
    fields: Vec<Field>,
}

impl Format {
    pub fn new() -> Format {
        Format { fields: Vec::new() }
    }

    pub fn push_field(&mut self, field: Field) {
        self.fields.push(field)
    }

    pub fn get_fields(&self) -> &Vec<Field> {
        &self.fields
    }

    pub fn get_min_len(&self) -> usize {
        self.fields
            .iter()
            .map(|f| f.len())
            .collect::<Vec<usize>>()
            .iter()
            .sum()
    }
}
