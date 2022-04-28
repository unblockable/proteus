use bytes::Bytes;

/// A covert application message that we are attempting to transfer across our
/// pluggable transport. The formatter will obfuscate this by wrapping it in
/// frames with various header fields, encrypting/decrypting, etc.
pub struct CovertMessage {
    pub data: Bytes
}

/// An overt message that can be read and written from and to the network using
/// the UPGen formatter.
pub struct OvertMessage {
    pub payload: Option<CovertMessage>
}
