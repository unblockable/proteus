use std::io::{self, Cursor};

use bytes::{Buf, BufMut, Bytes, BytesMut};
use tokio_util::codec::{Decoder, Encoder};
use xxhash_rust::xxh3::xxh3_64;

use crate::net::proto::socks;
use crate::net::proto::socks::address::Socks5Target;
use crate::net::proto::tunnel::message::{TunnelMessage, TunnelMessageKind};

pub struct TunnelCodec;

impl Encoder<TunnelMessage> for TunnelCodec {
    type Error = io::Error;

    fn encode(&mut self, msg: TunnelMessage, dst: &mut BytesMut) -> io::Result<()> {
        // If we return early, we want the dst buffer to be unmodified.
        let mut buf = BytesMut::new();
        self.encode_id(msg.id, &mut buf)?;
        self.encode_kind(msg.kind, &mut buf)?;
        let data = buf.freeze();

        let len = 2 + 8 + data.len() + 8;

        if len > u16::MAX as usize {
            return Err(io::Error::from(io::ErrorKind::FileTooLarge));
        }

        // Success, store the encoded bytes in dst.
        // The encoded buf will be: len, len_crc, data, data_crc
        dst.reserve(len);

        let len = len as u16;

        dst.put_u16(len);
        dst.put_u64(xxh3_64(&len.to_be_bytes()));
        dst.extend_from_slice(&data);
        dst.put_u64(xxh3_64(&data));

        Ok(())
    }
}

impl Decoder for TunnelCodec {
    type Item = TunnelMessage;

    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> io::Result<Option<Self::Item>> {
        // If we return Ok(None), we want the src buffer to be unmodified.
        let mut reader = Cursor::new(src as &BytesMut);

        // We first need the len (2 bytes) and len_crc (8 bytes)
        if reader.remaining() < 10 {
            return Ok(None);
        }

        let len = reader.get_u16();
        let len_crc_src = reader.get_u64();

        // Ensure the len checksum matches.
        let len_crc_dst = xxh3_64(&len.to_be_bytes());
        if len_crc_src != len_crc_dst {
            return Err(std::io::ErrorKind::InvalidData.into());
        }

        // Validate that len is in the correct range. We expect at least 18 bytes
        // (2 for len, 8 for len_crc, 8 for data_crc) and at most u16::MAX.
        if len < 18 {
            return Err(std::io::ErrorKind::InvalidInput.into());
        }

        // No need to decode the rest if we don't have everything yet.
        let msg_len = len as usize;
        if src.len() < msg_len {
            return Ok(None);
        }

        // OK, we have the entire message. Let's take it from src and avoid copying.
        let mut msg_bytes = src.split_to(msg_len);

        // Cut the len and len_crc from the front, we already processed those.
        msg_bytes.advance(10);

        // The remaining is the data and data_crc.
        let data = msg_bytes.split_to(msg_bytes.len() - 8).freeze();
        let data_crc_src = msg_bytes.get_u64();
        
        let data_crc_dst = xxh3_64(&data);
        if data_crc_src != data_crc_dst {
            return Err(std::io::ErrorKind::InvalidData.into());
        }

        // Now that we know the data is valid, decode it.
        let mut reader = Cursor::new(&data);
        let Some(id) = self.decode_id(&mut reader)? else {
            return Err(std::io::ErrorKind::InvalidInput.into());
        };
        let Some(kind) = self.decode_kind(&mut reader)? else {
            return Err(std::io::ErrorKind::InvalidInput.into());
        };

        // Success.
        Ok(Some(TunnelMessage { id, kind }))
    }
}

impl TunnelCodec {
    pub const fn encapsulated_bytes_max_len() -> usize {
        // This *must* be kept synchronized with our encoding scheme.
        // We want the total length of an encoded TunnelMessage to fit in a u16.
        // overhead = len (2) + len_crc (8) + [id (8) + kind (1) + bytes_len (2)] + bytes_crc (8)
        (u16::MAX as usize) - 29
    }

    fn encode_id(&mut self, id: u64, dst: &mut BytesMut) -> io::Result<()> {
        dst.reserve(8);
        dst.put_u64(id);
        Ok(())
    }

    fn decode_id(&mut self, src: &mut Cursor<&Bytes>) -> io::Result<Option<u64>> {
        if src.remaining() >= 8 {
            Ok(Some(src.get_u64()))
        } else {
            Ok(None)
        }
    }

    fn encode_kind(&mut self, kind: TunnelMessageKind, dst: &mut BytesMut) -> io::Result<()> {
        let encoded_kind = match kind {
            TunnelMessageKind::Open => 0,
            TunnelMessageKind::Close => 1,
            TunnelMessageKind::Connect(_) => 2,
            TunnelMessageKind::Encapsulate(_) => 3,
        };
        dst.reserve(1);
        dst.put_u8(encoded_kind);

        match kind {
            TunnelMessageKind::Open => Ok(()),
            TunnelMessageKind::Close => Ok(()),
            TunnelMessageKind::Connect(target) => self.encode_target(target, dst),
            TunnelMessageKind::Encapsulate(bytes) => self.encode_bytes(bytes, dst),
        }
    }

    fn decode_kind(
        &mut self,
        src: &mut Cursor<&Bytes>,
    ) -> io::Result<Option<TunnelMessageKind>> {
        if src.remaining() >= 1 {
            let encoded_kind = src.get_u8();
            let command = match encoded_kind {
                0 => TunnelMessageKind::Open,
                1 => TunnelMessageKind::Close,
                2 => {
                    let Some(target) = self.decode_target(src)? else {
                        return Ok(None);
                    };
                    TunnelMessageKind::Connect(target)
                }
                3 => {
                    let Some(bytes) = self.decode_bytes(src)? else {
                        return Ok(None);
                    };
                    TunnelMessageKind::Encapsulate(bytes)
                }
                _ => return Err(io::Error::from(io::ErrorKind::InvalidData)),
            };
            Ok(Some(command))
        } else {
            Ok(None)
        }
    }

    fn encode_target(&mut self, target: Socks5Target, dst: &mut BytesMut) -> io::Result<()> {
        socks::codec::encode_target(target, dst)
    }

    fn decode_target(&mut self, src: &mut Cursor<&Bytes>) -> io::Result<Option<Socks5Target>> {
        let target_data = BytesMut::from(src.get_mut().clone());
        let mut target_cursor = Cursor::new(&target_data as &BytesMut);
        target_cursor.set_position(src.position());

        let result = socks::codec::decode_target(&mut target_cursor);
        
        src.set_position(target_cursor.position());
        result
    }

    fn encode_bytes(&mut self, bytes: Bytes, dst: &mut BytesMut) -> io::Result<()> {
        if bytes.len() > TunnelCodec::encapsulated_bytes_max_len() {
            return Err(io::Error::from(io::ErrorKind::FileTooLarge));
        }

        dst.reserve(2 + bytes.len());
        dst.put_u16(bytes.len() as u16);
        dst.extend_from_slice(&bytes);

        Ok(())
    }

    fn decode_bytes(&mut self, src: &mut Cursor<&Bytes>) -> io::Result<Option<Bytes>> {
        if src.remaining() >= 2 {
            let len = src.get_u16() as usize;
            if src.remaining() >= len {
                // `copy_to_bytes()` is a deep copy on a `BytesMut`, but a shallow copy on a `Bytes`.
                let bytes = src.copy_to_bytes(len);
                Ok(Some(bytes))
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    use bytes::Bytes;

    use super::*;
    use crate::common::mock;
    use crate::net::proto::socks::address::Socks5Address;

    fn assert_encode_decode(msg: TunnelMessage) {
        let mut buf = BytesMut::new();

        let encode_result = TunnelCodec.encode(msg.clone(), &mut buf);
        assert!(encode_result.is_ok());

        let decode_result = TunnelCodec.decode(&mut buf);
        assert!(matches!(decode_result, Ok(Some(_))));

        let encoded_decoded_msg = decode_result.unwrap().unwrap();
        assert_eq!(msg, encoded_decoded_msg);
    }

    fn message(kind: TunnelMessageKind) -> TunnelMessage {
        TunnelMessage {
            id: 123456789,
            kind,
        }
    }

    fn test_valid_command(kind: TunnelMessageKind) {
        assert_encode_decode(message(kind));
    }

    #[test]
    fn valid_connect() {
        let addresses = vec![
            Socks5Address::from_name(String::from("test.com")),
            Socks5Address::from_addr(IpAddr::V4(Ipv4Addr::new(4, 3, 2, 1))),
            Socks5Address::from_addr(IpAddr::V6(Ipv6Addr::new(8, 7, 6, 5, 4, 3, 2, 1))),
            Socks5Address::Unknown,
        ];

        for addr in addresses {
            test_valid_command(TunnelMessageKind::Connect(Socks5Target::new(addr, 12345)));
        }
    }

    #[test]
    fn valid_bytes() {
        test_valid_command(TunnelMessageKind::Encapsulate(Bytes::from(
            "This is the payload.",
        )));
    }

    #[test]
    fn valid_close() {
        test_valid_command(TunnelMessageKind::Close);
    }

    #[test]
    fn valid_open() {
        test_valid_command(TunnelMessageKind::Open);
    }

    #[test]
    fn invalid_payload_length() {
        // Max supported payload length.
        let max_len = TunnelCodec::encapsulated_bytes_max_len();
        let bytes = mock::payload(max_len);
        test_valid_command(TunnelMessageKind::Encapsulate(bytes));

        // This payload is larger than supported.
        let bytes = mock::payload(max_len + 1);
        let msg = message(TunnelMessageKind::Encapsulate(bytes));

        // Should get encode error.
        let mut buf = BytesMut::new();
        let encode_result = TunnelCodec.encode(msg.clone(), &mut buf);
        assert!(encode_result.is_err());
    }

    fn test_invalid(kind: TunnelMessageKind, byte_to_stomp: usize) {
        let mut buf = BytesMut::new();

        let msg = message(kind);
        assert!(TunnelCodec.encode(msg, &mut buf).is_ok());

        buf[byte_to_stomp] = 99; // stomp a byte

        assert!(TunnelCodec.decode(&mut buf).is_err());
    }

    #[test]
    fn invalid_kind() {
        // The kind is the 9th byte (index 8).
        test_invalid(
            TunnelMessageKind::Connect(Socks5Target::new(Socks5Address::Unknown, 0)),
            8,
        );
    }

    #[test]
    fn invalid_payload_bytes() {
        // Max supported payload length.
        let max_len = TunnelCodec::encapsulated_bytes_max_len();
        let bytes = BytesMut::zeroed(max_len).freeze();

        // If we stomp a payload byte, the checksum should fail.
        test_invalid(TunnelMessageKind::Encapsulate(bytes), 10_000);
    }

    #[test]
    fn valid_checksum() {
        let max_len = TunnelCodec::encapsulated_bytes_max_len();
        let bytes = mock::payload(max_len);
        let checksum_src = xxh3_64(&bytes);
        let checksum_dst = xxh3_64(&bytes);
        assert_eq!(checksum_src, checksum_dst);
    }

    #[test]
    fn invalid_checksum() {
        let max_len = TunnelCodec::encapsulated_bytes_max_len();
        let bytes = mock::payload(max_len);
        let mut buf = BytesMut::from(bytes);
        let n = buf.len();

        buf[n - 1] = 1;
        let checksum_src = xxh3_64(&buf);

        buf[n - 1] = 0;
        let checksum_dst = xxh3_64(&buf);

        assert_ne!(checksum_src, checksum_dst);
    }
}
