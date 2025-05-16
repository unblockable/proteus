use std::io::Cursor;

use bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::net::proto::socks::address::Socks5Address;
use crate::net::{Deserialize, Serialize};

#[derive(Debug, PartialEq)]
pub struct Greeting {
    pub version: u8,
    pub num_auth_methods: u8,
    pub supported_auth_methods: Bytes,
}

impl Serialize<Greeting> for Greeting {
    fn serialize(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(8);

        buf.put_u8(self.version);
        buf.put_u8(self.supported_auth_methods.len() as u8);
        for method in self.supported_auth_methods.iter() {
            buf.put_u8(*method);
        }

        buf.freeze()
    }
}

impl Deserialize<Greeting> for Greeting {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<Greeting> {
        let version = (buf.remaining() >= 1).then(|| buf.get_u8())?;
        let num_auth_methods = (buf.remaining() >= 1).then(|| buf.get_u8())?;
        let size = num_auth_methods as usize;
        Some(Greeting {
            version,
            num_auth_methods,
            supported_auth_methods: (buf.remaining() >= size).then(|| buf.copy_to_bytes(size))?,
        })
    }
}

#[derive(Debug, PartialEq)]
pub struct Choice {
    pub version: u8,
    pub auth_method: u8,
}

impl Serialize<Choice> for Choice {
    fn serialize(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(2);
        buf.put_u8(self.version);
        buf.put_u8(self.auth_method);
        buf.freeze()
    }
}

impl Deserialize<Choice> for Choice {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<Choice> {
        Some(Choice {
            version: (buf.remaining() >= 1).then(|| buf.get_u8())?,
            auth_method: (buf.remaining() >= 1).then(|| buf.get_u8())?,
        })
    }
}

#[derive(Debug, PartialEq)]
pub struct UserPassAuthRequest {
    pub version: u8,
    pub username: String,
    pub password: String,
}

impl Serialize<UserPassAuthRequest> for UserPassAuthRequest {
    fn serialize(&self) -> Bytes {
        let capacity: usize = 3 + self.username.len() + self.password.len();
        let mut buf = BytesMut::with_capacity(capacity);

        buf.put_u8(self.version);
        buf.put_u8(self.username.len() as u8);
        buf.put_slice(self.username.as_bytes());
        buf.put_u8(self.password.len() as u8);
        buf.put_slice(self.password.as_bytes());

        buf.freeze()
    }
}

impl Deserialize<UserPassAuthRequest> for UserPassAuthRequest {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<UserPassAuthRequest> {
        let version = (buf.remaining() >= 1).then(|| buf.get_u8())?;

        let username_len = (buf.remaining() >= 1).then(|| buf.get_u8() as usize)?;
        let username_bytes =
            (buf.remaining() >= username_len).then(|| buf.copy_to_bytes(username_len))?;

        let password_len = (buf.remaining() >= 1).then(|| buf.get_u8() as usize)?;
        let password_bytes =
            (buf.remaining() >= password_len).then(|| buf.copy_to_bytes(password_len))?;

        Some(UserPassAuthRequest {
            version,
            username: String::from_utf8_lossy(&username_bytes).to_string(),
            password: String::from_utf8_lossy(&password_bytes).to_string(),
        })
    }
}

#[derive(Debug, PartialEq)]
pub struct UserPassAuthResponse {
    pub version: u8,
    pub status: u8,
}

impl Serialize<UserPassAuthResponse> for UserPassAuthResponse {
    fn serialize(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(2);
        buf.put_u8(self.version);
        buf.put_u8(self.status);
        buf.freeze()
    }
}

impl Deserialize<UserPassAuthResponse> for UserPassAuthResponse {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<UserPassAuthResponse> {
        Some(UserPassAuthResponse {
            version: (buf.remaining() >= 1).then(|| buf.get_u8())?,
            status: (buf.remaining() >= 1).then(|| buf.get_u8())?,
        })
    }
}

#[derive(Debug, PartialEq)]
pub struct ConnectRequest {
    pub version: u8,
    pub command: u8,
    pub reserved: u8,
    pub dest_addr: Socks5Address,
    pub dest_port: u16,
}

impl Serialize<ConnectRequest> for ConnectRequest {
    fn serialize(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(5 + self.dest_addr.len());
        buf.put_u8(self.version);
        buf.put_u8(self.command);
        buf.put_u8(self.reserved);
        buf.put(self.dest_addr.serialize());
        buf.put_u16(self.dest_port);
        buf.freeze()
    }
}

impl Deserialize<ConnectRequest> for ConnectRequest {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<ConnectRequest> {
        Some(ConnectRequest {
            version: (buf.remaining() >= 1).then(|| buf.get_u8())?,
            command: (buf.remaining() >= 1).then(|| buf.get_u8())?,
            reserved: (buf.remaining() >= 1).then(|| buf.get_u8())?,
            dest_addr: Socks5Address::deserialize(buf)?,
            dest_port: (buf.remaining() >= 2).then(|| buf.get_u16())?,
        })
    }
}

#[derive(Debug, PartialEq)]
pub struct ConnectResponse {
    pub version: u8,
    pub status: u8,
    pub reserved: u8,
    pub bind_addr: Socks5Address,
    pub bind_port: u16,
}

impl Serialize<ConnectResponse> for ConnectResponse {
    fn serialize(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(5 + self.bind_addr.len());
        buf.put_u8(self.version);
        buf.put_u8(self.status);
        buf.put_u8(self.reserved);
        buf.put(self.bind_addr.serialize());
        buf.put_u16(self.bind_port);
        buf.freeze()
    }
}

impl Deserialize<ConnectResponse> for ConnectResponse {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<ConnectResponse> {
        Some(ConnectResponse {
            version: (buf.remaining() >= 1).then(|| buf.get_u8())?,
            status: (buf.remaining() >= 1).then(|| buf.get_u8())?,
            reserved: (buf.remaining() >= 1).then(|| buf.get_u8())?,
            bind_addr: Socks5Address::deserialize(buf)?,
            bind_port: (buf.remaining() >= 2).then(|| buf.get_u16())?,
        })
    }
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
        buf.put(frame.serialize());

        assert_eq!(
            frame,
            Greeting::deserialize(&mut Cursor::new(&buf)).unwrap()
        );
    }

    #[test]
    fn choice() {
        let frame = Choice {
            version: 5,
            auth_method: 0,
        };

        let mut buf = BytesMut::new();
        buf.put(frame.serialize());

        assert_eq!(frame, Choice::deserialize(&mut Cursor::new(&buf)).unwrap());
    }

    #[test]
    fn auth_request() {
        let frame = UserPassAuthRequest {
            version: 1,
            username: String::from("someuser"),
            password: String::from("somepassword"),
        };

        let mut buf = BytesMut::new();
        buf.put(frame.serialize());

        assert_eq!(
            frame,
            UserPassAuthRequest::deserialize(&mut Cursor::new(&buf)).unwrap()
        );
    }

    #[test]
    fn auth_response() {
        let frame = UserPassAuthResponse {
            version: 1,
            status: 0,
        };

        let mut buf = BytesMut::new();
        buf.put(frame.serialize());

        assert_eq!(
            frame,
            UserPassAuthResponse::deserialize(&mut Cursor::new(&buf)).unwrap()
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

            let mut buf = BytesMut::new();
            buf.put(frame.serialize());

            assert_eq!(
                frame,
                ConnectRequest::deserialize(&mut Cursor::new(&buf)).unwrap()
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

            let mut buf = BytesMut::new();
            buf.put(frame.serialize());

            assert_eq!(
                frame,
                ConnectResponse::deserialize(&mut Cursor::new(&buf)).unwrap()
            );
        }
    }
}
