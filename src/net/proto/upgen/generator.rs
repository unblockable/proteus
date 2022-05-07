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

    fn compute_handshake_type_val(&mut self) -> u8 {
        let start_val_choices = [(0x00u8, 0.1), (0x01u8, 0.4), (0x0Au8, 0.2), (0x14u8, 0.3)];
        let start_val = self.choose_weighted(&start_val_choices);
        
        let num_encodable_types = self.choose(&[4, 5, 6]);
        let num_normal = 4;
        
        let norm_first = self.rng.gen_bool(0.5);
        
        if num_encodable_types == 4 || norm_first {
            // Normal messages first, control second
            // Handshake type is the start val
            start_val
        } else {
            // Control messages first, normal second
            // Handshake type comes after the control types
            let offset = num_encodable_types - num_normal;
            start_val + offset
        }
    }

    pub fn generate_overt_protocol(&mut self) -> OvertProtocol {
        let mut unenc_fields_hs = Vec::new();
        let mut num_enc_header_bytes_hs: usize = 0;

        let mut unenc_fields_data = Vec::new();
        let mut num_enc_header_bytes_data: usize = 0;

        // Type
        // Maybe in handshake, maybe in data
        {
            // FIXME: probs for in hs or data phase not specified in v1 doc, using uniform choices
            let in_handshake = self.rng.gen_bool(0.5);
            let in_data = self.rng.gen_bool(0.5);
            let is_enc = self.rng.gen_bool(0.5);
            
            if is_enc {
                // Encrypted, so we don't care about the values.
                let field_size = 1; // encode type in 1 byte
                if in_handshake {
                    num_enc_header_bytes_hs += field_size;
                }
                if in_data {
                    num_enc_header_bytes_data += field_size;
                }
            } else {
                // Unencrypted, use that computed type values
                let hs_val = self.compute_handshake_type_val();
                let data_val = hs_val + 1;
                if in_handshake {
                    let b = Bytes::copy_from_slice(&[hs_val]);
                    let field = FrameField::new(FieldKind::Fixed(b));
                    unenc_fields_hs.push(field);
                }
                if in_data {
                    let b = Bytes::copy_from_slice(&[data_val]);
                    let field = FrameField::new(FieldKind::Fixed(b));
                    unenc_fields_data.push(field);
                }
            }
        }

        // Length
        // Maybe in handshake, always in data
        // Should cover everything that is NOT fixed length (i.e., total - fixed)
        {
            // Always unencrypted
            let size = self.choose_weighted(&[(2u8, 0.75), (4u8, 0.25)]);
            let field = FrameField::new(FieldKind::Length(size));
            unenc_fields_hs.push(field.clone());
            unenc_fields_data.push(field);
        }

        // Version
        // Maybe in handshake, never in data
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
                    num_enc_header_bytes_hs += field_size;
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
                    unenc_fields_hs.push(field);
                }
            }
        }

        // Type, length, version come first in random order
        unenc_fields_hs.shuffle(&mut self.rng);

        // FIXME data field only has length and maybe type
        // but these two should be in the same order as in handshake
        unenc_fields_data.shuffle(&mut self.rng);

        // Observable randomness
        // Gets added here if included and unencrypted.
        // Add to num_enc_header_bytes if encrypted.
        {
            // prototype crypto module has no nonce
        }

        // Reserved bytes
        // Maybe in handshake, never in data
        // Always unencrypted, init to zeros
        {
            if self.rng.gen_bool(0.2) {
                // Included in handshake
                let size = self.choose_weighted(&[(1, 0.4), (2, 0.4), (3, 0.1), (4, 0.1)]);
                
                let mut buf = BytesMut::with_capacity(size);
                buf.put_bytes(0, size);
                let b = buf.freeze();

                let field = FrameField::new(FieldKind::Fixed(b));
                unenc_fields_hs.push(field);
            }
        }

        // Protocol-specific fields
        // Maybe in handhsake, maybe in data
        // Always encrypted
        {
            if num_enc_header_bytes_hs > 0 {
                let size = self.choose_weighted(&[(0, 0.5), (1, 0.25), (2, 0.25)]);
                num_enc_header_bytes_hs += size as usize;
            }
            if num_enc_header_bytes_data > 0 {
                let size = self.choose_weighted(&[(0, 0.8), (1, 0.1), (2, 0.1)]);
                num_enc_header_bytes_data += size as usize;
            }
        }

        // OK construct the protocol now
        let mut unenc_fields_h2 = unenc_fields_hs.clone();

        // TODO eventually we should choose among available crypto modules. The
        // decisions about key material will eventually depend on the chosen
        // crypto module. For now we default to the prototype module.
        // We also only build a one-rtt protocol and that should change.
        let (hs1, hs2) = {
            // Both handshake messages are mostly the same, except the key material.
            let mut h1_spec = OvertFrameSpec::new();
            let mut h2_spec = OvertFrameSpec::new();

            for field in unenc_fields_hs {
                h1_spec.push_field(field.clone());
                h2_spec.push_field(field);
            }

            // Ephemeral key exchange: place at end of unencrypted fields.
            {
                let f = FrameField::new(FieldKind::CryptoMaterial(CryptoMaterialKind::KeyMaterialSent));
                h1_spec.push_field(f);
            }
            {
                let f = FrameField::new(FieldKind::CryptoMaterial(CryptoMaterialKind::KeyMaterialReceived));
                h2_spec.push_field(f);
            }

            if num_enc_header_bytes_hs > 0 {
                let concat = create_encrypted_header_field(num_enc_header_bytes_hs);
                h1_spec.push_field(concat.clone());
                h2_spec.push_field(concat);

                let mac = FrameField::new(FieldKind::CryptoMaterial(CryptoMaterialKind::MAC));
                h1_spec.push_field(mac.clone());
                h2_spec.push_field(mac);
            }

            (h1_spec, h2_spec)
        };

        let data = {
            let mut data_spec = OvertFrameSpec::new();

            for field in unenc_fields_data {
                data_spec.push_field(field);
            }

            if num_enc_header_bytes_data > 0 {
                let concat = create_encrypted_header_field(num_enc_header_bytes_data);
                data_spec.push_field(concat);

                let mac = FrameField::new(FieldKind::CryptoMaterial(CryptoMaterialKind::MAC));
                data_spec.push_field(mac);
            }

            // Crypto module handles adding MAC after the payload.
            data_spec.push_field(FrameField::new(FieldKind::Payload));
            data_spec
        };

        let proto_spec = onertt::ProtocolSpec::new(hs1, hs2, data);
        OvertProtocol::OneRtt(proto_spec)
    }
}
