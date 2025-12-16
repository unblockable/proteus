use std::io::{self, Cursor};

use bytes::{Buf, BufMut, Bytes, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use crate::net::proto::socks;
use crate::net::proto::socks::address::Socks5Target;
use crate::net::proto::tunnel::message::{TunnelMessage, TunnelMessageKind};

pub struct TunnelCodec;

impl Encoder<TunnelMessage> for TunnelCodec {
    type Error = io::Error;

    fn encode(&mut self, msg: TunnelMessage, dst: &mut BytesMut) -> io::Result<()> {
        let mut buf = BytesMut::new();

        // If we return early, the dst buffer is unmodified.
        self.encode_id(msg.session_id, &mut buf)?;
        self.encode_kind(msg.kind, &mut buf)?;

        // Success, store the encoded bytes in dst.
        let bytes = buf.freeze();
        dst.reserve(bytes.len());
        dst.extend_from_slice(&bytes);

        Ok(())
    }
}

impl Decoder for TunnelCodec {
    type Item = TunnelMessage;

    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> io::Result<Option<Self::Item>> {
        let mut reader = Cursor::new(src as &BytesMut);

        // If we return early, the src buffer is unmodified.
        let Some(session_id) = self.decode_id(&mut reader)? else {
            return Ok(None);
        };
        let Some(kind) = self.decode_kind(&mut reader)? else {
            return Ok(None);
        };

        // Success, mark the src bytes as consumed.
        let num_consumed = reader.position() as usize;
        src.advance(num_consumed);

        Ok(Some(TunnelMessage { session_id, kind }))
    }
}

impl TunnelCodec {
    fn encode_id(&mut self, id: u64, dst: &mut BytesMut) -> io::Result<()> {
        dst.reserve(8);
        dst.put_u64(id);
        Ok(())
    }

    fn decode_id(&mut self, src: &mut Cursor<&BytesMut>) -> io::Result<Option<u64>> {
        if src.remaining() >= 8 {
            Ok(Some(src.get_u64()))
        } else {
            Ok(None)
        }
    }

    fn encode_kind(&mut self, kind: TunnelMessageKind, dst: &mut BytesMut) -> io::Result<()> {
        let encoded_kind = match kind {
            TunnelMessageKind::Open(_) => 0,
            TunnelMessageKind::Encapsulated(_) => 1,
            TunnelMessageKind::Close => 2,
        };
        dst.reserve(1);
        dst.put_u8(encoded_kind);

        match kind {
            TunnelMessageKind::Open(target) => self.encode_target(target, dst),
            TunnelMessageKind::Encapsulated(bytes) => self.encode_bytes(bytes, dst),
            TunnelMessageKind::Close => Ok(()),
        }
    }

    fn decode_kind(
        &mut self,
        src: &mut Cursor<&BytesMut>,
    ) -> io::Result<Option<TunnelMessageKind>> {
        if src.remaining() >= 1 {
            let encoded_kind = src.get_u8();
            let command = match encoded_kind {
                0 => {
                    let Some(target) = self.decode_target(src)? else {
                        return Ok(None);
                    };
                    TunnelMessageKind::Open(target)
                }
                1 => {
                    let Some(bytes) = self.decode_bytes(src)? else {
                        return Ok(None);
                    };
                    TunnelMessageKind::Encapsulated(bytes)
                }
                2 => TunnelMessageKind::Close,
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

    fn decode_target(&mut self, src: &mut Cursor<&BytesMut>) -> io::Result<Option<Socks5Target>> {
        socks::codec::decode_target(src)
    }

    fn encode_bytes(&mut self, bytes: Bytes, dst: &mut BytesMut) -> io::Result<()> {
        if bytes.len() > u16::MAX as usize {
            return Err(io::Error::from(io::ErrorKind::FileTooLarge));
        }

        dst.reserve(2 + bytes.len());
        dst.put_u16(bytes.len() as u16);
        dst.extend_from_slice(&bytes);

        Ok(())
    }

    fn decode_bytes(&mut self, src: &mut Cursor<&BytesMut>) -> io::Result<Option<Bytes>> {
        if src.remaining() >= 2 {
            let len = src.get_u16() as usize;
            if src.remaining() >= len {
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
            session_id: 123456789,
            kind,
        }
    }

    fn test_valid_command(kind: TunnelMessageKind) {
        assert_encode_decode(message(kind));
    }

    #[test]
    fn valid_open() {
        let addresses = vec![
            Socks5Address::from_name(String::from("test.com")),
            Socks5Address::from_addr(IpAddr::V4(Ipv4Addr::new(4, 3, 2, 1))),
            Socks5Address::from_addr(IpAddr::V6(Ipv6Addr::new(8, 7, 6, 5, 4, 3, 2, 1))),
            Socks5Address::Unknown,
        ];

        for addr in addresses {
            test_valid_command(TunnelMessageKind::Open(Socks5Target::new(addr, 12345)));
        }
    }

    #[test]
    fn valid_bytes() {
        test_valid_command(TunnelMessageKind::Encapsulated(Bytes::from(
            "This is the payload.",
        )));
    }

    #[test]
    fn valid_close() {
        test_valid_command(TunnelMessageKind::Close);
    }

    #[test]
    fn invalid_payload_length() {
        // Max supported payload length.
        let bytes = mock::payload(u16::MAX as usize);
        test_valid_command(TunnelMessageKind::Encapsulated(bytes));

        // This payload is larger than supported.
        let bytes = mock::payload(u16::MAX as usize + 1);
        let msg = message(TunnelMessageKind::Encapsulated(bytes));

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
            TunnelMessageKind::Open(Socks5Target::new(Socks5Address::Unknown, 0)),
            8,
        );
    }
}
