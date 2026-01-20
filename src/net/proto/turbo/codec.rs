use std::io::{self, Cursor};

use bytes::{Buf, BufMut, Bytes, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use crate::net::proto::tunnel::codec::TunnelCodec;
use crate::net::proto::turbo::message::{Command, DataCursor, Payload, TurboMessage};

pub struct TurboCodec;

impl Encoder<TurboMessage> for TurboCodec {
    type Error = io::Error;

    fn encode(&mut self, msg: TurboMessage, dst: &mut BytesMut) -> io::Result<()> {
        let mut buf = BytesMut::new();

        // If we return early, the dst buffer is unmodified.
        self.encode_cursor(msg.write, &mut buf)?;
        self.encode_cursor(msg.read, &mut buf)?;
        self.encode_command(msg.command, &mut buf)?;

        // Success, store the encoded bytes in dst.
        let bytes = buf.freeze();
        dst.reserve(bytes.len());
        dst.extend_from_slice(&bytes);

        Ok(())
    }
}

impl Decoder for TurboCodec {
    type Item = TurboMessage;

    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> io::Result<Option<Self::Item>> {
        self.decode_nocopy(&mut src.clone().freeze())
    }
}

impl TurboCodec {
    pub fn decode_nocopy(&mut self, src: &mut Bytes) -> io::Result<Option<TurboMessage>> {
        let mut reader = Cursor::new(src as &Bytes);

        // If we return early, the src buffer is unmodified.
        let Some(write) = self.decode_cursor(&mut reader)? else {
            return Ok(None);
        };
        let Some(read) = self.decode_cursor(&mut reader)? else {
            return Ok(None);
        };
        let Some(command) = self.decode_command(&mut reader)? else {
            return Ok(None);
        };

        // Success, mark the src bytes as consumed.
        let num_consumed = reader.position() as usize;
        src.advance(num_consumed);

        Ok(Some(TurboMessage {
            write,
            read,
            command,
        }))
    }

    pub const fn payload_max_len() -> usize {
        // This *must* be kept synchronized with our encoding scheme.
        // overhead = write (8) + read (8) + cmd (1) + payload_len (2)
        TunnelCodec::encapsulated_bytes_max_len() - 19
    }

    fn encode_cursor(&mut self, cursor: DataCursor, dst: &mut BytesMut) -> io::Result<()> {
        dst.reserve(8);
        dst.put_u64(cursor);
        Ok(())
    }

    fn decode_cursor(&mut self, src: &mut Cursor<&Bytes>) -> io::Result<Option<DataCursor>> {
        if src.remaining() >= 8 {
            Ok(Some(src.get_u64()))
        } else {
            Ok(None)
        }
    }

    fn encode_command(&mut self, command: Command, dst: &mut BytesMut) -> io::Result<()> {
        let command_type = match &command {
            Command::Forward(_) => 0,
            Command::ForwardAck => 1,
            Command::Rewind => 2,
            Command::RewindAck => 3,
            Command::Shut => 4,
            Command::ShutAck => 5,
            Command::Reset => 6,
        };
        dst.reserve(1);
        dst.put_u8(command_type);

        match command {
            Command::Forward(payload) => self.encode_payload(payload, dst),
            _ => Ok(()),
        }
    }

    fn decode_command(&mut self, src: &mut Cursor<&Bytes>) -> io::Result<Option<Command>> {
        if src.remaining() >= 1 {
            let command_type = src.get_u8();
            let command = match command_type {
                0 => {
                    let Some(payload) = self.decode_payload(src)? else {
                        return Ok(None);
                    };
                    Command::Forward(payload)
                }
                1 => Command::ForwardAck,
                2 => Command::Rewind,
                3 => Command::RewindAck,
                4 => Command::Shut,
                5 => Command::ShutAck,
                6 => Command::Reset,
                _ => return Err(io::Error::from(io::ErrorKind::InvalidData)),
            };
            Ok(Some(command))
        } else {
            Ok(None)
        }
    }

    fn encode_payload(&mut self, payload: Payload, dst: &mut BytesMut) -> io::Result<()> {
        if payload.data.len() > u16::MAX as usize {
            return Err(io::Error::from(io::ErrorKind::FileTooLarge));
        }

        dst.reserve(2 + payload.data.len());
        dst.put_u16(payload.data.len() as u16);
        dst.extend_from_slice(&payload.data);

        Ok(())
    }

    fn decode_payload(&mut self, src: &mut Cursor<&Bytes>) -> io::Result<Option<Payload>> {
        if src.remaining() >= 2 {
            let data_len = src.get_u16() as usize;
            if src.remaining() >= data_len {
                // `copy_to_bytes()` is a deep copy on a `BytesMut`, but a shallow copy on a `Bytes`.
                let data = src.copy_to_bytes(data_len);
                Ok(Some(Payload { data }))
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
    use bytes::Bytes;

    use crate::common::mock;

    use super::*;

    fn assert_encode_decode(msg: TurboMessage) {
        let mut buf = BytesMut::new();

        let encode_result = TurboCodec.encode(msg.clone(), &mut buf);
        assert!(encode_result.is_ok());

        let decode_result = TurboCodec.decode(&mut buf);
        assert!(matches!(decode_result, Ok(Some(_))));

        let encoded_decoded_msg = decode_result.unwrap().unwrap();
        assert_eq!(msg, encoded_decoded_msg);
    }

    fn message(command: Command) -> TurboMessage {
        TurboMessage {
            write: 123,
            read: 321,
            command,
        }
    }

    fn test_valid_command(command: Command) {
        assert_encode_decode(message(command));
    }

    #[test]
    fn command_forward() {
        test_valid_command(Command::Forward(Payload::from(Bytes::from(
            "This is the payload.",
        ))));
    }

    #[test]
    fn command_forward_ack() {
        test_valid_command(Command::ForwardAck);
    }

    #[test]
    fn command_rewind() {
        test_valid_command(Command::Rewind);
    }

    #[test]
    fn command_rewind_ack() {
        test_valid_command(Command::RewindAck);
    }

    #[test]
    fn command_shut() {
        test_valid_command(Command::Shut);
    }

    #[test]
    fn command_shut_ack() {
        test_valid_command(Command::ShutAck);
    }

    #[test]
    fn reset() {
        test_valid_command(Command::Reset);
    }

    #[test]
    fn invalid_payload_length() {
        // Max supported payload length.
        let payload = Payload::from(mock::payload(u16::MAX as usize));
        test_valid_command(Command::Forward(payload));

        // This payload is larger than supported.
        let payload = Payload::from(mock::payload(u16::MAX as usize + 1));
        let msg = message(Command::Forward(payload));

        // Should get encode error.
        let mut buf = BytesMut::new();
        let encode_result = TurboCodec.encode(msg.clone(), &mut buf);
        assert!(encode_result.is_err());
    }

    fn test_invalid(command: Command, byte_to_stomp: usize) {
        let mut buf = BytesMut::new();

        let msg = message(command);
        assert!(TurboCodec.encode(msg, &mut buf).is_ok());

        buf[byte_to_stomp] = 99; // stomp a byte

        assert!(TurboCodec.decode(&mut buf).is_err());
    }

    #[test]
    fn invalid_command() {
        // The command type is the 17th byte (index 16).
        test_invalid(Command::Shut, 16);
    }
}
