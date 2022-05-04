use std::cmp;

use bytes::Buf;
use bytes::BufMut;
use bytes::Bytes;
use bytes::BytesMut;

use crate::net::proto::upgen::frames::{CovertPayload, FieldKind, FrameField, OvertFrameSpec};
use crate::net::{Deserializer, Serializer};

pub struct Formatter {
    frame_spec: OvertFrameSpec,
    // encryption_module: 
}

impl Formatter {
    /// Creates a formatter with a default frame spec with two fields:
    ///   1. variable-value length field (fixed at 2 bytes)
    ///   2. variable-value payload field
    pub fn new() -> Formatter {
        let mut default = OvertFrameSpec::new();
        default.push_field(FrameField::new(FieldKind::Length(2)));
        default.push_field(FrameField::new(FieldKind::Payload));
        Formatter {
            frame_spec: default,
        }
    }

    /// Sets the frame spec that we'll continue to follow when serializing or
    /// deserializing frames until a new frame spec is set.
    pub fn set_frame_spec(&mut self, frame_spec: OvertFrameSpec) {
        self.frame_spec = frame_spec;
    }
}

impl Serializer<CovertPayload> for Formatter {
    fn serialize_frame(&self, src: CovertPayload) -> Bytes {
        // Write as many frames as needed until we write all of the payload.
        log::trace!("Trying to serialize covert frame");

        let fields = self.frame_spec.get_fields();

        log::trace!("We have {} fields", fields.len());

        // Payload len is variable, so compute that first.
        let mut payload_len = 0;
        for field in fields {
            if let FieldKind::Length(num_bytes) = field.kind {
                let base: u32 = 2;
                let num_bits = 8 * u32::from(num_bytes);
                let max_len = base.pow(num_bits);
                log::trace!(
                    "Found {}-byte length field which can encode payload length up to {} bytes",
                    num_bytes,
                    max_len
                );
                payload_len = cmp::min(max_len as usize, src.data.len());
                log::trace!(
                    "We have {} available payload bytes, so we can write {} of it",
                    src.data.len(),
                    payload_len
                );
                break;
            }
        }

        let fixed_len = self.frame_spec.get_fixed_len();
        let total_len = fixed_len + payload_len;
        let mut buf = BytesMut::with_capacity(total_len * 2);

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
                FieldKind::Random(num_bytes) => {
                    let count = usize::from(*num_bytes);
                    log::trace!("Writing {} random bytes", count);
                    buf.put_bytes(0, count); // XXX make random
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
                FieldKind::Payload => {
                    if payload_len > 0 {
                        log::trace!(
                            "Writing {} payload bytes out of {} available bytes",
                            payload_len,
                            src.data.len()
                        );
                        buf.put_slice(&src.data[0..payload_len]);
                    }
                }
            }
        }

        log::trace!(
            "Done serializing. We lost {} bytes",
            src.data.len() - payload_len
        );
        buf.freeze()
    }
}

impl Deserializer<CovertPayload> for Formatter {
    fn deserialize_frame(&self, src: &mut std::io::Cursor<&BytesMut>) -> Option<CovertPayload> {
        // Read as many frames as are available and return payload.
        // Return None if we don't yet have enough data to extract any payload.
        log::trace!("Trying to Deserialize covert frame");

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
                FieldKind::Random(num_bytes) => {
                    let len = *num_bytes as usize;
                    (src.remaining() >= len).then(|| {
                        log::trace!("Ignoring {} bytes from random field", len);
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
                FieldKind::Payload => {
                    let len = payload_len;
                    if len > 0 {
                        let data = (src.remaining() >= len).then(|| {
                            log::trace!("Copying {} bytes from payload field", len);
                            src.copy_to_bytes(len)
                        })?;
                        payload = Some(data);
                    }
                }
            }
        }

        log::trace!("Done deserializing. We lost {} bytes.", src.remaining());
        match payload {
            None => Some(CovertPayload { data: Bytes::new() }),
            Some(data) => Some(CovertPayload { data }),
        }
    }
}
