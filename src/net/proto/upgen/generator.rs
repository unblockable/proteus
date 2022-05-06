use bytes::{Bytes, BytesMut, BufMut};
use rand::{rngs::StdRng, seq::SliceRandom, Rng, SeedableRng};

use crate::net::proto::upgen::crypto::*;
use crate::net::proto::upgen::frames::{FieldKind, FrameField, OvertFrameSpec};
use crate::net::proto::upgen::protocols::*;

fn create_encrypted_header_field(size: usize) -> FrameField {
    FrameField::new(FieldKind::CryptoMaterial(
        CryptoMaterialKind::EncryptedHeader(size),
    ))
}

pub struct Generator {
    rng: StdRng,
}

impl Generator {
    pub fn new(seed: u64) -> Generator {
        // Use the seed to generate an overt protocol which specifies the format
        // of all frames that are transferred over the network.
        Generator {
            rng: StdRng::seed_from_u64(seed),
        }
    }

    fn create_frame_spec(&self) -> OvertFrameSpec {
        let mut frame_spec = OvertFrameSpec::new();
        frame_spec.push_field(FrameField::new(FieldKind::Fixed(Bytes::from("UPGen v1"))));
        frame_spec.push_field(FrameField::new(FieldKind::Length(2)));
        frame_spec.push_field(FrameField::new(FieldKind::Payload));
        frame_spec
    }

    fn choose_weighted<T>(&mut self, choices: &[(T, f64)]) -> T
    where
        T: Copy,
    {
        choices
            .choose_weighted(&mut self.rng, |item| item.1)
            .unwrap()
            .0
    }

    fn choose<T>(&mut self, choices: &[T]) -> T
    where
        T: Copy,
    {
        *choices.choose(&mut self.rng).unwrap()
    }

    pub fn generate_overt_protocol(&mut self) -> OvertProtocol {
        let mut unenc_fields_h1 = Vec::new();
        let mut num_enc_header_bytes: usize = 0;

        // Type
        {
            let field_size = 1; // encode type in 1 byte
            if self.rng.gen_bool(0.5) {
                // Encrypted, so we don't care about the values.
                num_enc_header_bytes += field_size;
            } else {
                // Unencrypted, need to compute values
                let start_val_choices = [(0x00u8, 0.1), (0x01u8, 0.4), (0x0Au8, 0.2), (0x14u8, 0.3)];
                let start_val = self.choose_weighted(&start_val_choices);

                let num_encodable_types = self.choose(&[4, 5, 6]);
                let num_normal = 4;

                let offset = if num_encodable_types > 4 && self.rng.gen_bool(0.5) {
                    // Normal messages first, control second
                    // Handshake type is the start val
                    start_val
                } else {
                    // Control messages first, normal second
                    num_encodable_types - num_normal
                };

                let val = start_val + offset;
                let b = Bytes::copy_from_slice(&[val]);
                let field = FrameField::new(FieldKind::Fixed(b));
                unenc_fields_h1.push(field);
            }
        }

        // Length
        // Should cover everything that is NOT fixed length (i.e., total - fixed)
        {
            // Always unencrypted
            let size = self.choose_weighted(&[(2u8, 0.75), (4u8, 0.25)]);
            let field = FrameField::new(FieldKind::Length(size));
            unenc_fields_h1.push(field);
        }

        // Version
        // Never in data phase
        {
            if self.rng.gen_bool(0.5) {
                // Included in handshake
                let field_size = if self.rng.gen_bool(0.5) {
                    1
                } else {
                    2
                };

                if self.rng.gen_bool(0.5) {
                    // Encrypted
                    num_enc_header_bytes += field_size;
                } else {
                    // Unencrypted
                    let major = self.choose(&[0u8, 1u8, 2u8, 3u8]);
                    let b = match field_size {
                        1 => Bytes::copy_from_slice(&[major]),
                        _ => {
                            let minor = self.choose_weighted(&[(0u8, 0.5), (1u8, 0.4), (2u8, 0.1)]);
                            Bytes::copy_from_slice(&[major, minor])
                        }
                    };
                    let field = FrameField::new(FieldKind::Fixed(b));
                    unenc_fields_h1.push(field);
                }
            }
        }

        // Type, length, version come first in random order
        unenc_fields_h1.shuffle(&mut self.rng);

        // Reserved bytes
        // Always unencrypted, init to zeros
        // Not in data
        {
            if self.rng.gen_bool(0.2) {
                let size = self.choose_weighted(&[(1, 0.4), (2, 0.4), (3, 0.1), (4, 0.1)]);
                let mut buf = BytesMut::with_capacity(size);
                buf.put_bytes(0, size);
                let b = buf.freeze();
                let field = FrameField::new(FieldKind::Fixed(b));
                unenc_fields_h1.push(field);
            }
        }

        // Protocol-specific fields
        // Always encrypted
        {
            if num_enc_header_bytes > 0 {
                let size = self.choose_weighted(&[(0, 0.5), (1, 0.25), (2, 0.25)]);
                if size > 0 {
                    num_enc_header_bytes += size as usize;
                }
            }
        }

        // OK construct the protocol now
        let mut unenc_fields_h2 = unenc_fields_h1.clone();
        let mut data_fields = unenc_fields_h2.clone();

        let handshake1 = {
            let mut spec = OvertFrameSpec::new();
            for field in unenc_fields_h1 {
                spec.push_field(field);
            }
            if num_enc_header_bytes > 0 {
                spec.push_field(create_encrypted_header_field(num_enc_header_bytes));
            }
            spec
        };

        let handshake2 = {
            // FIXME remove/adjust fields
            let mut spec = OvertFrameSpec::new();
            for field in unenc_fields_h2 {
                spec.push_field(field);
            }
            if num_enc_header_bytes > 0 {
                spec.push_field(create_encrypted_header_field(num_enc_header_bytes));
            }
            spec
        };

        let data = {
            // FIXME need to remove handshake-only fields
            let mut spec = OvertFrameSpec::new();
            for field in data_fields {
                spec.push_field(field);
            }
            if num_enc_header_bytes > 0 {
                spec.push_field(create_encrypted_header_field(num_enc_header_bytes));
            }
            spec.push_field(FrameField::new(FieldKind::Payload));
            spec
        };

        let proto_spec = onertt::ProtocolSpec::new(handshake1, handshake2, data);
        OvertProtocol::OneRtt(proto_spec)
    }
}
