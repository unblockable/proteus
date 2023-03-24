pub enum Role {
    Client,
    Server,
}

pub struct LengthBounds {
    min: u64,
    max: u64,
}

pub struct VertexId {
    value: usize,
}
