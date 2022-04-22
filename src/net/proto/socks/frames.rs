use bytes::{Buf, BufMut, BytesMut};
use std::io::Cursor;

use crate::net::{self, frame::Frame, proto::socks::address::Socks5Address};

#[derive(Debug, PartialEq)]
pub struct Greeting {
    pub version: u8,
    pub num_auth_methods: u8,
    pub supported_auth_methods: Vec<u8>,
}

#[derive(Debug, PartialEq)]
pub struct Choice {
    pub version: u8,
    pub auth_method: u8,
}

#[derive(Debug, PartialEq)]
pub struct UserPassAuthRequest {
    pub version: u8,
    pub username: String,
    pub password: String,
}

#[derive(Debug, PartialEq)]
pub struct UserPassAuthResponse {
    pub version: u8,
    pub status: u8,
}

#[derive(Debug, PartialEq)]
pub struct ConnectRequest {
    pub version: u8,
    pub command: u8,
    pub reserved: u8,
    pub dest_addr: Socks5Address,
    pub dest_port: u16,
}

#[derive(Debug, PartialEq)]
pub struct ConnectResponse {
    pub version: u8,
    pub status: u8,
    pub reserved: u8,
    pub bind_addr: Socks5Address,
    pub bind_port: u16,
}

impl Frame<Greeting> for Greeting {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<Greeting> {
        let version = buf.has_remaining().then(|| buf.get_u8())?;
        let num_auth_methods = buf.has_remaining().then(|| buf.get_u8())?;
        Some(Greeting {
            version,
            num_auth_methods,
            supported_auth_methods: net::get_bytes_vec(buf, num_auth_methods as usize)?,
        })
    }

    fn serialize(&self) -> BytesMut {
        let mut buf = BytesMut::with_capacity(8);

        buf.put_u8(self.version);
        buf.put_u8(self.supported_auth_methods.len() as u8);
        for method in self.supported_auth_methods.iter() {
            buf.put_u8(*method);
        }

        buf
    }
}

impl Frame<Choice> for Choice {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<Choice> {
        Some(Choice {
            version: buf.has_remaining().then(|| buf.get_u8())?,
            auth_method: buf.has_remaining().then(|| buf.get_u8())?,
        })
    }

    fn serialize(&self) -> BytesMut {
        let mut buf = BytesMut::with_capacity(2);
        buf.put_u8(self.version);
        buf.put_u8(self.auth_method);
        buf
    }
}

impl Frame<UserPassAuthRequest> for UserPassAuthRequest {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<UserPassAuthRequest> {
        let version = buf.has_remaining().then(|| buf.get_u8())?;

        let username_len = buf.has_remaining().then(|| buf.get_u8())?;
        let username_bytes = net::get_bytes_vec(buf, username_len as usize)?;

        let password_len = buf.has_remaining().then(|| buf.get_u8())?;
        let password_bytes = net::get_bytes_vec(buf, password_len as usize)?;

        Some(UserPassAuthRequest {
            version,
            username: String::from_utf8_lossy(&username_bytes).to_string(),
            password: String::from_utf8_lossy(&password_bytes).to_string(),
        })
    }

    fn serialize(&self) -> BytesMut {
        let capacity: usize = 3 + self.username.len() + self.password.len();
        let mut buf = BytesMut::with_capacity(capacity);

        buf.put_u8(self.version);
        buf.put_u8(self.username.len() as u8);
        buf.put_slice(self.username.as_bytes());
        buf.put_u8(self.password.len() as u8);
        buf.put_slice(self.password.as_bytes());

        buf
    }
}

impl Frame<UserPassAuthResponse> for UserPassAuthResponse {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<UserPassAuthResponse> {
        Some(UserPassAuthResponse {
            version: buf.has_remaining().then(|| buf.get_u8())?,
            status: buf.has_remaining().then(|| buf.get_u8())?,
        })
    }

    fn serialize(&self) -> BytesMut {
        let mut buf = BytesMut::with_capacity(2);
        buf.put_u8(self.version);
        buf.put_u8(self.status);
        buf
    }
}

impl Frame<ConnectRequest> for ConnectRequest {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<ConnectRequest> {
        Some(ConnectRequest {
            version: buf.has_remaining().then(|| buf.get_u8())?,
            command: buf.has_remaining().then(|| buf.get_u8())?,
            reserved: buf.has_remaining().then(|| buf.get_u8())?,
            dest_addr: Socks5Address::from_bytes(buf)?,
            dest_port: buf.has_remaining().then(|| buf.get_u16())?,
        })
    }

    fn serialize(&self) -> BytesMut {
        let mut buf = BytesMut::with_capacity(5 + self.dest_addr.len());
        buf.put_u8(self.version);
        buf.put_u8(self.command);
        buf.put_u8(self.reserved);
        self.dest_addr.to_bytes(&mut buf);
        buf.put_u16(self.dest_port);
        buf
    }
}

impl Frame<ConnectResponse> for ConnectResponse {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<ConnectResponse> {
        Some(ConnectResponse {
            version: buf.has_remaining().then(|| buf.get_u8())?,
            status: buf.has_remaining().then(|| buf.get_u8())?,
            reserved: buf.has_remaining().then(|| buf.get_u8())?,
            bind_addr: Socks5Address::from_bytes(buf)?,
            bind_port: buf.has_remaining().then(|| buf.get_u16())?,
        })
    }

    fn serialize(&self) -> BytesMut {
        let mut buf = BytesMut::with_capacity(5 + self.bind_addr.len());
        buf.put_u8(self.version);
        buf.put_u8(self.status);
        buf.put_u8(self.reserved);
        self.bind_addr.to_bytes(&mut buf);
        buf.put_u16(self.bind_port);
        buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    #[test]
    fn greeting() {
        let frame = Greeting {
            version: 5,
            num_auth_methods: 1,
            supported_auth_methods: vec![0; 1],
        };
        assert_eq!(
            frame,
            Greeting::deserialize(&mut Cursor::new(&frame.serialize())).unwrap()
        );
    }

    #[test]
    fn choice() {
        let frame = Choice {
            version: 5,
            auth_method: 0,
        };
        assert_eq!(
            frame,
            Choice::deserialize(&mut Cursor::new(&frame.serialize())).unwrap()
        );
    }

    #[test]
    fn user_pass_auth_request() {
        let frame = UserPassAuthRequest {
            version: 1,
            username: String::from("someuser"),
            password: String::from("somepassword"),
        };
        assert_eq!(
            frame,
            UserPassAuthRequest::deserialize(&mut Cursor::new(&frame.serialize())).unwrap()
        );
    }

    #[test]
    fn user_pass_auth_response() {
        let frame = UserPassAuthResponse {
            version: 1,
            status: 0,
        };
        assert_eq!(
            frame,
            UserPassAuthResponse::deserialize(&mut Cursor::new(&frame.serialize())).unwrap()
        );
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
            assert_eq!(
                frame,
                ConnectRequest::deserialize(&mut Cursor::new(&frame.serialize())).unwrap()
            );
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
            assert_eq!(
                frame,
                ConnectResponse::deserialize(&mut Cursor::new(&frame.serialize())).unwrap()
            );
        }
    }
}
