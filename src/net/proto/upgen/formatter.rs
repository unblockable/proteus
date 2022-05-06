use std::cmp;
use std::io::Cursor;
use std::sync::{Arc, Mutex};

use bytes::Buf;
use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;

use crate::net::proto::upgen::{
    crypto::{null, CryptoProtocol},
    frames::{CovertPayload, FieldKind, FrameField, OvertFrameSpec},
};
use crate::net::{Deserializer, Serializer};

/// Wraps a formatter allowing us to share it across threads.
#[derive(Clone)]
pub struct SharedFormatter {
    inner: Arc<Mutex<Formatter>>,
}

impl SharedFormatter {
    pub fn new(fmt: Formatter) -> SharedFormatter {
        SharedFormatter {
            inner: Arc::new(Mutex::new(fmt)),
        }
    }
}

impl Serializer<CovertPayload> for SharedFormatter {
    fn serialize_frame(&mut self, src: CovertPayload) -> Bytes {
        // FIXME: we can create a much smaller critical section by only locking
        // the parts where we are modifying state rather than the entire
        // serialize function.
        self.inner.lock().unwrap().serialize_frame(src)
    }
}

impl Deserializer<CovertPayload> for SharedFormatter {
    fn deserialize_frame(&mut self, src: &mut std::io::Cursor<&BytesMut>) -> Option<CovertPayload> {
        // FIXME: we can create a much smaller critical section by only locking
        // the parts where we are modifying state rather than the entire
        // serialize function.
        self.inner.lock().unwrap().deserialize_frame(src)
    }
}

pub struct Formatter {
    frame_spec: OvertFrameSpec,
    crypt_spec: Box<dyn CryptoProtocol + Send + Sync>,
}

impl Formatter {
    /// Creates a formatter with a default frame spec with two fields:
    ///   1. variable-value length field (fixed at 2 bytes)
    ///   2. variable-value payload field
    pub fn new() -> Formatter {
        let mut default_spec = OvertFrameSpec::new();
        default_spec.push_field(FrameField::new(FieldKind::Length(2)));
        default_spec.push_field(FrameField::new(FieldKind::Payload));

        let default_crypto = Box::new(null::CryptoModule::new());

        Formatter {
            frame_spec: default_spec,
            crypt_spec: default_crypto,
        }
    }

    /// Sets the frame spec that we'll continue to follow when serializing or
    /// deserializing frames until a new frame spec is set.
    pub fn set_frame_spec(&mut self, frame_spec: OvertFrameSpec) {
        self.frame_spec = frame_spec;
    }

    fn serialize_single_frame(&mut self, payload: &mut Cursor<Bytes>) -> Bytes {
        // Write a frame with as much payload as we can.
        log::trace!("Trying to serialize covert frame");

        let fields = self.frame_spec.get_fields();

        log::trace!("We have {} fields", fields.len());

        // Payload len is variable, so compute that first.
        let mut payload_len = 0;
        for field in fields {
            if let FieldKind::Length(num_bytes) = field.kind {
                let base: u32 = 2;
                let num_bits = 8 * u32::from(num_bytes);
                let max_len = base.pow(num_bits) - 1;
                log::trace!(
                    "Found {}-byte length field which can encode payload length <= {} bytes",
                    num_bytes,
                    max_len
                );
                payload_len = cmp::min(max_len as usize, payload.remaining());
                log::trace!(
                    "We have {} available payload bytes, so we can write {} of it",
                    payload.remaining(),
                    payload_len
                );
                break;
            }
        }

        let fixed_len = self.frame_spec.get_fixed_len();
        let total_len = fixed_len + payload_len;
        let mut buf = BytesMut::with_capacity(total_len);

        log::trace!(
            "Computed lengths: fixed={}, payload={}, total={}",
            fixed_len,
            payload_len,
            total_len
        );

        // Now start writing.
        for field in fields {
            match &field.kind {
                FieldKind::Fixed(b) => {
                    log::trace!("Writing {} fixed-length bytes", b.len());
                    buf.put_slice(&b)
                }
                FieldKind::Length(num_bytes) => {
                    log::trace!(
                        "Writing payload length {} into a {}-byte length field",
                        payload_len,
                        num_bytes
                    );
                    match num_bytes {
                        1 => buf.put_u8(u8::try_from(payload_len).unwrap_or(u8::MAX)),
                        2 => buf.put_u16(u16::try_from(payload_len).unwrap_or(u16::MAX)),
                        3 => buf.put_u32(u32::try_from(payload_len).unwrap_or(u32::MAX)),
                        4 => buf.put_u64(u64::try_from(payload_len).unwrap_or(u64::MAX)),
                        _ => buf.put_u128(u128::try_from(payload_len).unwrap_or(u128::MAX)),
                    }
                }
                FieldKind::CryptoMaterial(material) => {
                    todo!()
                }
                FieldKind::Payload => {
                    if payload_len > 0 {
                        log::trace!(
                            "Writing {} payload bytes out of {} available bytes",
                            payload_len,
                            payload.remaining()
                        );

                        let plaintext = payload.copy_to_bytes(payload_len);
                        // FIXME don't unwrap this, instead bubble the error.
                        let ciphertext = self.crypt_spec.encrypt(&plaintext).unwrap();

                        buf.put_slice(&ciphertext);
                    }
                }
            }
        }

        log::trace!("Done serializing a frame.");
        buf.freeze()
    }

    fn deserialize_single_frame(&mut self, src: &mut Cursor<&BytesMut>) -> Option<Bytes> {
        // Read a single frame and return any payload.
        // Return None if we don't yet have enough data to extract any payload.
        log::trace!("Trying to deserialize covert frame");

        let fields = self.frame_spec.get_fields();
        let fixed_len = self.frame_spec.get_fixed_len();

        log::trace!(
            "We have {} fields with a fixed length of {} bytes",
            fields.len(),
            fixed_len
        );
        log::trace!("There are {} bytes in src", src.remaining());

        if src.remaining() < fixed_len {
            log::trace!("Not ready, a full frame is not yet available");
            return None;
        }

        let mut payload_len = 0;
        let mut payload: Option<Bytes> = None;

        for field in fields {
            match &field.kind {
                FieldKind::Fixed(b) => {
                    let len = b.len();
                    (src.remaining() >= len).then(|| {
                        log::trace!("Ignoring {} bytes from fixed field", len);
                        src.advance(len)
                    })?;
                }
                FieldKind::Length(num_bytes) => {
                    let len = *num_bytes as usize;

                    payload_len = (src.remaining() >= len).then(|| match len {
                        1 => src.get_u8() as usize,
                        2 => src.get_u16() as usize,
                        3 => src.get_u32() as usize,
                        4 => src.get_u64() as usize,
                        _ => src.get_u128() as usize,
                    })?;

                    log::trace!(
                        "Got payload length of {} bytes from a {}-byte length field",
                        payload_len,
                        len
                    );
                }
                FieldKind::CryptoMaterial(material) => {
                    todo!()
                }
                FieldKind::Payload => {
                    let len = payload_len;
                    if len > 0 {
                        let ciphertext = (src.remaining() >= len).then(|| {
                            log::trace!("Copying {} bytes from payload field", len);
                            src.copy_to_bytes(len)
                        })?;
                        // FIXME don't unwrap this, instead bubble the error.
                        let plaintext = self.crypt_spec.encrypt(&ciphertext).unwrap();
                        payload = Some(plaintext);
                    }
                }
            }
        }

        log::trace!("Done deserializing a frame.",);
        match payload {
            None => Some(Bytes::new()),
            Some(data) => Some(data),
        }
    }
}

impl Serializer<CovertPayload> for Formatter {
    fn serialize_frame(&mut self, src: CovertPayload) -> Bytes {
        // Write as many frames as needed until we write all of the payload.
        let mut payload_cursor = Cursor::new(src.data);

        // Always serialize at least one frame even if there is no payload.
        let mut overt_bytes = self.serialize_single_frame(&mut payload_cursor);

        while payload_cursor.has_remaining() {
            let frame = self.serialize_single_frame(&mut payload_cursor);
            let mut chain = overt_bytes.chain(frame);
            overt_bytes = chain.copy_to_bytes(chain.remaining());
        }

        // Return the bytes to be written to the network.
        overt_bytes
    }
}

impl Deserializer<CovertPayload> for Formatter {
    fn deserialize_frame(&mut self, src: &mut Cursor<&BytesMut>) -> Option<CovertPayload> {
        // Read as many frames as are available and return combined payload.
        let mut payload = match self.deserialize_single_frame(src) {
            Some(b) => b,
            None => return None,
        };

        while src.has_remaining() {
            // Ensure we don't discard bytes from partial frames.
            let frame_start_pos = src.position();
            let data = match self.deserialize_single_frame(src) {
                Some(b) => b,
                None => {
                    // Handle the partial frame next time.
                    src.set_position(frame_start_pos);
                    break;
                }
            };
            let mut chain = payload.chain(data);
            payload = chain.copy_to_bytes(chain.remaining());
        }

        Some(CovertPayload { data: payload })
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn get_alpha() -> Bytes {
        Bytes::from_static(b"abcdefghijklmnopqrstuvwxyz")
    }

    fn get_payload(len: usize) -> Bytes {
        let mut buf = BytesMut::with_capacity(len);
        buf.put_bytes(b'x', len);
        buf.freeze()
    }

    fn get_simple_formatter() -> Formatter {
        let mut fmt = Formatter::new();
        let mut spec = OvertFrameSpec::new();
        spec.push_field(FrameField::new(FieldKind::Length(1)));
        spec.push_field(FrameField::new(FieldKind::Payload));
        fmt.set_frame_spec(spec);
        fmt
    }

    fn get_complex_formatter() -> Formatter {
        let mut fmt = Formatter::new();
        let mut spec = OvertFrameSpec::new();
        spec.push_field(FrameField::new(FieldKind::Fixed(Bytes::from(
            "Test Greeting v1.1.1.1",
        ))));
        spec.push_field(FrameField::new(FieldKind::Fixed(Bytes::from_static(&[20]))));
        spec.push_field(FrameField::new(FieldKind::Fixed(get_alpha())));
        spec.push_field(FrameField::new(FieldKind::Length(1)));
        spec.push_field(FrameField::new(FieldKind::Payload));
        fmt.set_frame_spec(spec);
        fmt
    }

    fn assert_serialize_deserialize_eq(bytes: Bytes, mut fmt: Formatter) {
        let mut bytes1 = BytesMut::with_capacity(bytes.len());
        bytes1.put_slice(&bytes);
        let msg1 = CovertPayload {
            data: bytes1.freeze(),
        };
        let bytes_serialized = fmt.serialize_frame(msg1);

        let mut bytes2 = BytesMut::with_capacity(bytes_serialized.len());
        bytes2.put_slice(&bytes_serialized);
        let msg2 = fmt.deserialize_frame(&mut Cursor::new(&bytes2)).unwrap();
        let bytes_deserialized = msg2.data;

        assert_eq!(bytes.len(), bytes_deserialized.len());
        assert_eq!(bytes, bytes_deserialized);
    }

    #[test]
    fn small_payload_simple_formatter() {
        assert_serialize_deserialize_eq(get_payload(10), get_simple_formatter());
        assert_serialize_deserialize_eq(get_alpha(), get_simple_formatter());
    }

    #[test]
    fn small_payload_complex_formatter() {
        assert_serialize_deserialize_eq(get_payload(20), get_complex_formatter());
        assert_serialize_deserialize_eq(get_alpha(), get_simple_formatter());
    }

    #[test]
    fn multiple_frames() {
        let mut fmt = Formatter::new();
        let mut spec = OvertFrameSpec::new();
        spec.push_field(FrameField::new(FieldKind::Length(1)));
        spec.push_field(FrameField::new(FieldKind::Payload));
        fmt.set_frame_spec(spec);
        // Payload fits in one frame
        assert_serialize_deserialize_eq(get_payload(255), get_simple_formatter());
        // Payload needs two frames
        assert_serialize_deserialize_eq(get_payload(256), get_simple_formatter());
    }

    #[test]
    fn large_payload_complex_formatter() {
        assert_serialize_deserialize_eq(get_payload(100000), get_complex_formatter());
    }
}
