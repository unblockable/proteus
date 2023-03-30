use bytes::Bytes;

use crate::net::Serialize;

// Data that we read from a connection.
pub struct NetworkData {
    bytes: Bytes,
}

impl NetworkData {
    pub fn len(&self) -> usize {
        self.bytes.len()
    }
}

impl From<Bytes> for NetworkData {
    fn from(bytes: Bytes) -> Self {
        NetworkData { bytes }
    }
}

impl From<NetworkData> for Bytes {
    fn from(data: NetworkData) -> Self {
        data.bytes
    }
}

// We can serialize a `NetworkData` directly, but we can't deserialize in
// isolation because we need to know how many bytes to read, so we leave that to
// the formatter.
impl Serialize<NetworkData> for NetworkData {
    fn serialize(&self) -> Bytes {
        self.bytes.clone()
    }
}
