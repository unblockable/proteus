use std::io::{self, Cursor};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use bytes::{Buf, BufMut, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use crate::net::proto::socks::address::{Socks5Address, Socks5Target};
use crate::net::proto::socks::message::{
    Choice, ConnectRequest, ConnectResponse, Greeting, Message, MessageKind, UserPassAuthRequest,
    UserPassAuthResponse,
};

pub struct Socks5Codec {
    next_decode: Option<MessageKind>,
}

impl Socks5Codec {
    pub fn new() -> Self {
        Self { next_decode: None }
    }

    pub fn set_next_decode(&mut self, kind: MessageKind) {
        self.next_decode = Some(kind);
    }
}

impl Encoder<Message> for Socks5Codec {
    type Error = io::Error;

    fn encode(&mut self, msg: Message, dst: &mut BytesMut) -> io::Result<()> {
        // Use a buffer so dst is unmodified on error.
        let mut buf = BytesMut::new();

        match msg {
            Message::Greeting(greeting) => Socks5Codec::encode_greeting(greeting, &mut buf)?,
            Message::Choice(choice) => Socks5Codec::encode_choice(choice, &mut buf)?,
            Message::UserPassAuthRequest(req) => {
                Socks5Codec::encode_user_pass_auth_request(req, &mut buf)?
            }
            Message::UserPassAuthResponse(resp) => {
                Socks5Codec::encode_user_pass_auth_response(resp, &mut buf)?
            }
            Message::ConnectRequest(req) => Socks5Codec::encode_connect_request(req, &mut buf)?,
            Message::ConnectResponse(resp) => Socks5Codec::encode_connect_response(resp, &mut buf)?,
        }

        // Success, store the encoded bytes in dst.
        let bytes = buf.freeze();
        dst.reserve(bytes.len());
        dst.extend_from_slice(&bytes);

        Ok(())
    }
}

impl Decoder for Socks5Codec {
    type Item = Message;

    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> io::Result<Option<Self::Item>> {
        // Must call set_next_decode() so we know what message to expect next.
        let Some(expected_msg_kind) = self.next_decode else {
            return Err(io::Error::from(io::ErrorKind::Other));
        };

        let mut reader = Cursor::new(src as &BytesMut);

        let maybe_msg = match expected_msg_kind {
            MessageKind::Greeting => Socks5Codec::decode_greeting(&mut reader)?.map(Message::from),
            MessageKind::Choice => Socks5Codec::decode_choice(&mut reader)?.map(Message::from),
            MessageKind::UserPassAuthRequest => {
                Socks5Codec::decode_user_pass_auth_request(&mut reader)?.map(Message::from)
            }
            MessageKind::UserPassAuthResponse => {
                Socks5Codec::decode_user_pass_auth_response(&mut reader)?.map(Message::from)
            }
            MessageKind::ConnectRequest => {
                Socks5Codec::decode_connect_request(&mut reader)?.map(Message::from)
            }
            MessageKind::ConnectResponse => {
                Socks5Codec::decode_connect_response(&mut reader)?.map(Message::from)
            }
        };

        if maybe_msg.is_some() {
            // Success, mark the src bytes as consumed.
            let num_consumed = reader.position() as usize;
            src.advance(num_consumed);
        }

        Ok(maybe_msg)
    }
}

/// Stateless encoding and decoding logic.
impl Socks5Codec {
    fn encode_greeting(greeting: Greeting, dst: &mut BytesMut) -> io::Result<()> {
        let num_auth_methods = greeting.supported_auth_methods.len().min(u8::MAX as usize);

        dst.reserve(2 + num_auth_methods);

        dst.put_u8(greeting.version);
        dst.put_u8(num_auth_methods as u8);
        dst.put_slice(&greeting.supported_auth_methods[0..num_auth_methods]);

        Ok(())
    }

    fn decode_greeting(src: &mut Cursor<&BytesMut>) -> io::Result<Option<Greeting>> {
        if src.remaining() < 2 {
            return Ok(None);
        }

        let version = src.get_u8();
        let num_auth_methods = src.get_u8();

        if src.remaining() < num_auth_methods as usize {
            return Ok(None);
        }

        let supported_auth_methods = src.copy_to_bytes(num_auth_methods as usize);

        Ok(Some(Greeting {
            version,
            num_auth_methods,
            supported_auth_methods,
        }))
    }

    fn encode_choice(choice: Choice, dst: &mut BytesMut) -> io::Result<()> {
        dst.reserve(2);
        dst.put_u8(choice.version);
        dst.put_u8(choice.auth_method);
        Ok(())
    }

    fn decode_choice(src: &mut Cursor<&BytesMut>) -> io::Result<Option<Choice>> {
        if src.remaining() < 2 {
            return Ok(None);
        }

        let version = src.get_u8();
        let auth_method = src.get_u8();

        Ok(Some(Choice {
            version,
            auth_method,
        }))
    }

    fn encode_user_pass_auth_request(
        req: UserPassAuthRequest,
        dst: &mut BytesMut,
    ) -> io::Result<()> {
        let len = 3 + req.username.len() + req.password.len();
        dst.reserve(len);

        dst.put_u8(req.version);
        dst.put_u8(req.username.len() as u8);
        dst.put_slice(req.username.as_bytes());
        dst.put_u8(req.password.len() as u8);
        dst.put_slice(req.password.as_bytes());

        Ok(())
    }

    fn decode_user_pass_auth_request(
        src: &mut Cursor<&BytesMut>,
    ) -> io::Result<Option<UserPassAuthRequest>> {
        if src.remaining() < 2 {
            return Ok(None);
        }

        let version = src.get_u8();
        let username_len = src.get_u8() as usize;

        if src.remaining() < username_len {
            return Ok(None);
        }
        let username_bytes = src.copy_to_bytes(username_len);

        if src.remaining() < 1 {
            return Ok(None);
        }
        let password_len = src.get_u8() as usize;

        if src.remaining() < password_len {
            return Ok(None);
        }
        let password_bytes = src.copy_to_bytes(password_len);

        Ok(Some(UserPassAuthRequest {
            version,
            username: String::from_utf8_lossy(&username_bytes).to_string(),
            password: String::from_utf8_lossy(&password_bytes).to_string(),
        }))
    }

    fn encode_user_pass_auth_response(
        req: UserPassAuthResponse,
        dst: &mut BytesMut,
    ) -> io::Result<()> {
        dst.reserve(2);
        dst.put_u8(req.version);
        dst.put_u8(req.status);
        Ok(())
    }

    fn decode_user_pass_auth_response(
        src: &mut Cursor<&BytesMut>,
    ) -> io::Result<Option<UserPassAuthResponse>> {
        if src.remaining() < 2 {
            return Ok(None);
        }

        let version = src.get_u8();
        let status = src.get_u8();

        Ok(Some(UserPassAuthResponse { version, status }))
    }

    fn encode_connect_request(req: ConnectRequest, dst: &mut BytesMut) -> io::Result<()> {
        dst.reserve(3);
        dst.put_u8(req.version);
        dst.put_u8(req.command);
        dst.put_u8(req.reserved);

        Socks5Codec::encode_address(req.dest_addr, dst)?;

        dst.reserve(2);
        dst.put_u16(req.dest_port);

        Ok(())
    }

    fn decode_connect_request(src: &mut Cursor<&BytesMut>) -> io::Result<Option<ConnectRequest>> {
        if src.remaining() < 3 {
            return Ok(None);
        }

        let version = src.get_u8();
        let command = src.get_u8();
        let reserved = src.get_u8();

        let Some(dest_addr) = Socks5Codec::decode_address(src)? else {
            return Ok(None);
        };

        if src.remaining() < 2 {
            return Ok(None);
        }
        let dest_port = src.get_u16();

        Ok(Some(ConnectRequest {
            version,
            command,
            reserved,
            dest_addr,
            dest_port,
        }))
    }

    fn encode_connect_response(resp: ConnectResponse, dst: &mut BytesMut) -> io::Result<()> {
        dst.reserve(3);
        dst.put_u8(resp.version);
        dst.put_u8(resp.status);
        dst.put_u8(resp.reserved);

        Socks5Codec::encode_address(resp.bind_addr, dst)?;

        dst.reserve(2);
        dst.put_u16(resp.bind_port);

        Ok(())
    }

    fn decode_connect_response(src: &mut Cursor<&BytesMut>) -> io::Result<Option<ConnectResponse>> {
        if src.remaining() < 3 {
            return Ok(None);
        }

        let version = src.get_u8();
        let status = src.get_u8();
        let reserved = src.get_u8();

        let Some(bind_addr) = Socks5Codec::decode_address(src)? else {
            return Ok(None);
        };

        if src.remaining() < 2 {
            return Ok(None);
        }
        let bind_port = src.get_u16();

        Ok(Some(ConnectResponse {
            version,
            status,
            reserved,
            bind_addr,
            bind_port,
        }))
    }

    fn encode_target(target: Socks5Target, dst: &mut BytesMut) -> io::Result<()> {
        Socks5Codec::encode_address(target.addr(), dst)?;
        dst.reserve(2);
        dst.put_u16(target.port());
        Ok(())
    }

    fn decode_target(src: &mut Cursor<&BytesMut>) -> io::Result<Option<Socks5Target>> {
        let Some(addr) = Socks5Codec::decode_address(src)? else {
            return Ok(None);
        };

        if src.remaining() < 2 {
            return Ok(None);
        }
        let port = src.get_u16();

        Ok(Some(Socks5Target::new(addr, port)))
    }

    fn encode_address(addr: Socks5Address, dst: &mut BytesMut) -> io::Result<()> {
        dst.reserve(addr.len());

        match addr {
            Socks5Address::IpAddr(addr) => match addr {
                IpAddr::V4(a) => {
                    dst.put_u8(0x01);
                    dst.put_slice(&a.octets());
                }
                IpAddr::V6(a) => {
                    dst.put_u8(0x04);
                    dst.put_slice(&a.octets());
                }
            },
            Socks5Address::Name(name) => {
                let len = name.len().min(u8::MAX as usize);
                dst.put_u8(0x03);
                dst.put_u8(len as u8);
                dst.put_slice(&name.as_bytes()[0..len]);
            }
            Socks5Address::Unknown => {
                dst.put_u8(0x0);
            }
        }

        Ok(())
    }

    fn decode_address(src: &mut Cursor<&BytesMut>) -> io::Result<Option<Socks5Address>> {
        if src.remaining() < 1 {
            return Ok(None);
        }
        let addr_type = src.get_u8();

        match addr_type {
            0x01 => {
                if src.remaining() < 4 {
                    return Ok(None);
                }

                let mut octets = [0u8; 4];
                src.copy_to_slice(&mut octets);

                let addr = Ipv4Addr::new(octets[0], octets[1], octets[2], octets[3]);
                Ok(Some(Socks5Address::IpAddr(IpAddr::from(addr))))
            }
            0x03 => {
                if src.remaining() < 1 {
                    return Ok(None);
                }
                let name_len = src.get_u8() as usize;

                if src.remaining() < name_len {
                    return Ok(None);
                }
                let name_bytes = src.copy_to_bytes(name_len);
                let name = String::from_utf8_lossy(&name_bytes).to_string();
                Ok(Some(Socks5Address::Name(name)))
            }
            0x04 => {
                if src.remaining() < 16 {
                    return Ok(None);
                }

                let mut seg = [0u16; 8];
                for item in &mut seg {
                    *item = src.get_u16();
                }

                let addr = Ipv6Addr::new(
                    seg[0], seg[1], seg[2], seg[3], seg[4], seg[5], seg[6], seg[7],
                );
                Ok(Some(Socks5Address::IpAddr(IpAddr::from(addr))))
            }
            _ => Ok(Some(Socks5Address::Unknown)),
        }
    }
}

pub fn encode_target(target: Socks5Target, dst: &mut BytesMut) -> io::Result<()> {
    // Use a buffer so dst is unmodified on error.
    let mut buf = BytesMut::new();

    Socks5Codec::encode_target(target, &mut buf)?;

    // Success, store the encoded bytes in dst.
    let bytes = buf.freeze();
    dst.reserve(bytes.len());
    dst.extend_from_slice(&bytes);

    Ok(())
}

pub fn decode_target(src: &mut Cursor<&BytesMut>) -> io::Result<Option<Socks5Target>> {
    Socks5Codec::decode_target(src)
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    use super::*;

    #[test]
    fn greeting() {
        let frame = Greeting {
            version: 5,
            num_auth_methods: 1,
            supported_auth_methods: vec![0; 1].into(),
        };

        let mut buf = BytesMut::new();
        Socks5Codec::encode_greeting(frame.clone(), &mut buf).unwrap();
        let result = Socks5Codec::decode_greeting(&mut Cursor::new(&buf));

        assert!(matches!(result, Ok(Some(_))));
        assert_eq!(frame, result.unwrap().unwrap());
    }

    #[test]
    fn choice() {
        let frame = Choice {
            version: 5,
            auth_method: 0,
        };

        let mut buf = BytesMut::new();
        Socks5Codec::encode_choice(frame.clone(), &mut buf).unwrap();
        let result = Socks5Codec::decode_choice(&mut Cursor::new(&buf));

        assert!(matches!(result, Ok(Some(_))));
        assert_eq!(frame, result.unwrap().unwrap());
    }

    #[test]
    fn auth_request() {
        let frame = UserPassAuthRequest {
            version: 1,
            username: String::from("someuser"),
            password: String::from("somepassword"),
        };

        let mut buf = BytesMut::new();
        Socks5Codec::encode_user_pass_auth_request(frame.clone(), &mut buf).unwrap();
        let result = Socks5Codec::decode_user_pass_auth_request(&mut Cursor::new(&buf));

        assert!(matches!(result, Ok(Some(_))));
        assert_eq!(frame, result.unwrap().unwrap());
    }

    #[test]
    fn auth_response() {
        let frame = UserPassAuthResponse {
            version: 1,
            status: 0,
        };

        let mut buf = BytesMut::new();
        Socks5Codec::encode_user_pass_auth_response(frame.clone(), &mut buf).unwrap();
        let result = Socks5Codec::decode_user_pass_auth_response(&mut Cursor::new(&buf));

        assert!(matches!(result, Ok(Some(_))));
        assert_eq!(frame, result.unwrap().unwrap());
    }

    #[test]
    fn connect_request() {
        let addresses = vec![
            Socks5Address::from_name(String::from("test.com")),
            Socks5Address::from_addr(IpAddr::V4(Ipv4Addr::new(4, 3, 2, 1))),
            Socks5Address::from_addr(IpAddr::V6(Ipv6Addr::new(8, 7, 6, 5, 4, 3, 2, 1))),
        ];

        for addr in addresses {
            let frame = ConnectRequest {
                version: 5,
                command: 1,
                reserved: 0,
                dest_addr: addr,
                dest_port: 9000,
            };

            let mut buf = BytesMut::new();
            Socks5Codec::encode_connect_request(frame.clone(), &mut buf).unwrap();
            let result = Socks5Codec::decode_connect_request(&mut Cursor::new(&buf));

            assert!(matches!(result, Ok(Some(_))));
            assert_eq!(frame, result.unwrap().unwrap());
        }
    }

    #[test]
    fn connect_response() {
        let addresses = vec![
            Socks5Address::from_name(String::from("test.com")),
            Socks5Address::from_addr(IpAddr::V4(Ipv4Addr::new(4, 3, 2, 1))),
            Socks5Address::from_addr(IpAddr::V6(Ipv6Addr::new(8, 7, 6, 5, 4, 3, 2, 1))),
        ];

        for addr in addresses {
            let frame = ConnectResponse {
                version: 5,
                status: 1,
                reserved: 0,
                bind_addr: addr,
                bind_port: 9000,
            };

            let mut buf = BytesMut::new();
            Socks5Codec::encode_connect_response(frame.clone(), &mut buf).unwrap();
            let result = Socks5Codec::decode_connect_response(&mut Cursor::new(&buf));

            assert!(matches!(result, Ok(Some(_))));
            assert_eq!(frame, result.unwrap().unwrap());
        }
    }

    #[test]
    fn target() {
        let addresses = vec![
            Socks5Address::from_name(String::from("test.com")),
            Socks5Address::from_addr(IpAddr::V4(Ipv4Addr::new(4, 3, 2, 1))),
            Socks5Address::from_addr(IpAddr::V6(Ipv6Addr::new(8, 7, 6, 5, 4, 3, 2, 1))),
        ];

        for addr in addresses {
            let target = Socks5Target::new(addr, 12345);

            let mut buf = BytesMut::new();
            Socks5Codec::encode_target(target.clone(), &mut buf).unwrap();
            let result = Socks5Codec::decode_target(&mut Cursor::new(&buf));

            assert!(matches!(result, Ok(Some(_))));
            assert_eq!(target, result.unwrap().unwrap());
        }
    }

    #[test]
    fn address() {
        // Long names are truncated to 255 bytes
        for len in [254, 255, 256, 257, 258, 300, 1000] {
            let addr = Socks5Address::from_name("a".repeat(len));

            let mut buf = BytesMut::new();
            Socks5Codec::encode_address(addr.clone(), &mut buf).unwrap();
            let result = Socks5Codec::decode_address(&mut Cursor::new(&buf));

            assert!(matches!(result, Ok(Some(_))));
            assert_eq!(addr, result.unwrap().unwrap());
        }
    }
}
