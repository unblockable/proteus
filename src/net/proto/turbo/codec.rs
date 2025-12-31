use std::io::{self, Cursor};

use bytes::{Buf, BufMut, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use crate::net::proto::turbo::message::{
    self, Command, DataCursor, Payload, Request, Response, TurboMessage,
};

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
        let mut reader = Cursor::new(src as &BytesMut);

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
}

impl TurboCodec {
    fn encode_cursor(&mut self, cursor: DataCursor, dst: &mut BytesMut) -> io::Result<()> {
        dst.reserve(8);
        dst.put_u64(cursor);
        Ok(())
    }

    fn decode_cursor(&mut self, src: &mut Cursor<&BytesMut>) -> io::Result<Option<DataCursor>> {
        if src.remaining() >= 8 {
            Ok(Some(src.get_u64()))
        } else {
            Ok(None)
        }
    }

    fn encode_command(&mut self, command: Command, dst: &mut BytesMut) -> io::Result<()> {
        let command_type = match command {
            Command::Request(_) => 0,
            Command::Response(_) => 1,
            Command::Reset => 2,
        };
        dst.reserve(1);
        dst.put_u8(command_type);

        match command {
            Command::Request(request) => self.encode_request(request, dst),
            Command::Response(response) => self.encode_response(response, dst),
            Command::Reset => Ok(())
        }
    }

    fn decode_command(&mut self, src: &mut Cursor<&BytesMut>) -> io::Result<Option<Command>> {
        if src.remaining() >= 1 {
            let command_type = src.get_u8();
            let command = match command_type {
                0 => {
                    let Some(request) = self.decode_request(src)? else {
                        return Ok(None);
                    };
                    Command::Request(request)
                }
                1 => {
                    let Some(response) = self.decode_response(src)? else {
                        return Ok(None);
                    };
                    Command::Response(response)
                }
                2 => Command::Reset,
                _ => return Err(io::Error::from(io::ErrorKind::InvalidData)),
            };
            Ok(Some(command))
        } else {
            Ok(None)
        }
    }

    fn encode_request(&mut self, request: Request, dst: &mut BytesMut) -> io::Result<()> {
        let request_type = match request {
            Request::Forward(_) => 0,
            Request::Rewind => 1,
            Request::Shut => 2,
        };
        dst.reserve(1);
        dst.put_u8(request_type);

        match request {
            Request::Forward(payload) => self.encode_payload(payload, dst),
            _ => Ok(()),
        }
    }

    fn decode_request(&mut self, src: &mut Cursor<&BytesMut>) -> io::Result<Option<Request>> {
        if src.remaining() >= 1 {
            let request_type = src.get_u8();
            let request = match request_type {
                0 => {
                    let Some(payload) = self.decode_payload(src)? else {
                        return Ok(None);
                    };
                    Request::Forward(payload)
                }
                1 => Request::Rewind,
                2 => Request::Shut,
                _ => return Err(io::Error::from(io::ErrorKind::InvalidData)),
            };
            Ok(Some(request))
        } else {
            Ok(None)
        }
    }

    fn encode_response(&mut self, response: Response, dst: &mut BytesMut) -> io::Result<()> {
        let (response_type, result) = match response {
            Response::Forward(result) => (0, result),
            Response::Rewind(result) => (1, result),
            Response::Shut(result) => (2, result),
        };
        dst.reserve(1);
        dst.put_u8(response_type);
        self.encode_result(result, dst)
    }

    fn decode_response(&mut self, src: &mut Cursor<&BytesMut>) -> io::Result<Option<Response>> {
        if src.remaining() >= 1 {
            let response_type = src.get_u8();

            let Some(result) = self.decode_result(src)? else {
                return Ok(None);
            };

            let response = match response_type {
                0 => Response::Forward(result),
                1 => Response::Rewind(result),
                2 => Response::Shut(result),
                _ => return Err(io::Error::from(io::ErrorKind::InvalidData)),
            };
            Ok(Some(response))
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

    fn decode_payload(&mut self, src: &mut Cursor<&BytesMut>) -> io::Result<Option<Payload>> {
        if src.remaining() >= 2 {
            let data_len = src.get_u16() as usize;
            if src.remaining() >= data_len {
                let data = src.copy_to_bytes(data_len);
                Ok(Some(Payload { data }))
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    fn encode_result(&mut self, result: message::Result, dst: &mut BytesMut) -> io::Result<()> {
        let value = match result {
            message::Result::Ok => 0,
            message::Result::Error => 1,
        };
        dst.reserve(1);
        dst.put_u8(value);
        Ok(())
    }

    fn decode_result(
        &mut self,
        src: &mut Cursor<&BytesMut>,
    ) -> io::Result<Option<message::Result>> {
        if src.remaining() >= 1 {
            let result = match src.get_u8() {
                0 => message::Result::Ok,
                1 => message::Result::Error,
                _ => return Err(io::Error::from(io::ErrorKind::InvalidData)),
            };
            Ok(Some(result))
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
    fn request_forward() {
        test_valid_command(Command::Request(Request::Forward(Payload::from(
            Bytes::from("This is the payload."),
        ))));
    }

    #[test]
    fn request_rewind() {
        test_valid_command(Command::Request(Request::Rewind));
    }

    #[test]
    fn request_shut() {
        test_valid_command(Command::Request(Request::Shut));
    }

    #[test]
    fn response_forward() {
        for result in [message::Result::Ok, message::Result::Error] {
            test_valid_command(Command::Response(Response::Forward(result)));
        }
    }

    #[test]
    fn response_rewind() {
        for result in [message::Result::Ok, message::Result::Error] {
            test_valid_command(Command::Response(Response::Rewind(result)));
        }
    }

    #[test]
    fn response_shut() {
        for result in [message::Result::Ok, message::Result::Error] {
            test_valid_command(Command::Response(Response::Shut(result)));
        }
    }

    #[test]
    fn reset() {
        test_valid_command(Command::Reset);
    }

    #[test]
    fn invalid_payload_length() {
        // Max supported payload length.
        let payload = Payload::from(mock::payload(u16::MAX as usize));
        test_valid_command(Command::Request(Request::Forward(payload)));

        // This payload is larger than supported.
        let payload = Payload::from(mock::payload(u16::MAX as usize + 1));
        let msg = message(Command::Request(Request::Forward(payload)));

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
        test_invalid(Command::Request(Request::Shut), 16);
    }

    #[test]
    fn invalid_request() {
        // The request type is the 18th byte (index 17).
        test_invalid(Command::Request(Request::Shut), 17);
    }

    #[test]
    fn invalid_response() {
        // The response type is the 18th byte (index 17).
        test_invalid(Command::Response(Response::Shut(message::Result::Ok)), 17);
    }

    #[test]
    fn invalid_response_result() {
        // The response result type is the 19th byte (index 18).
        test_invalid(Command::Response(Response::Shut(message::Result::Ok)), 18);
    }
}
