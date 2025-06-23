use std::io::Cursor;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::net::{Deserialize, Serialize};

#[derive(Debug, PartialEq)]
pub enum Socks5Address {
    IpAddr(IpAddr),
    Name(String),
    Unknown,
}

impl Socks5Address {
    #[cfg(test)]
    pub fn from_name(mut name: String) -> Socks5Address {
        name.truncate(255);
        Socks5Address::Name(name)
    }

    #[cfg(test)]
    pub fn from_addr(addr: IpAddr) -> Socks5Address {
        Socks5Address::IpAddr(addr)
    }

    pub fn len(&self) -> usize {
        match self {
            Socks5Address::IpAddr(addr) => match addr {
                IpAddr::V4(_) => 1 + 4,  // type + addr
                IpAddr::V6(_) => 1 + 16, // type + addr
            },
            Socks5Address::Name(name) => 1 + 1 + name.len().min(255), // type + len + name
            Socks5Address::Unknown => 1,                              // type
        }
    }
}

impl Serialize<Socks5Address> for Socks5Address {
    fn serialize(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(8);

        match self {
            Socks5Address::IpAddr(addr) => match addr {
                IpAddr::V4(a) => {
                    buf.put_u8(0x01);
                    for octet in a.octets().iter() {
                        buf.put_u8(*octet);
                    }
                }
                IpAddr::V6(a) => {
                    buf.put_u8(0x04);
                    for segment in a.segments().iter() {
                        buf.put_u16(*segment);
                    }
                }
            },
            Socks5Address::Name(name) => {
                let len = name.len().min(255);
                buf.put_u8(0x03);
                buf.put_u8(len as u8);
                buf.put_slice(&name.as_bytes()[0..len]);
            }
            Socks5Address::Unknown => {
                buf.put_u8(0x0);
            }
        }

        buf.freeze()
    }
}

impl Deserialize<Socks5Address> for Socks5Address {
    fn deserialize(src: &mut Cursor<&BytesMut>) -> Option<Socks5Address> {
        let addr_type = (src.remaining() >= 1).then(|| src.get_u8())?;

        match addr_type {
            0x01 => Some(Socks5Address::IpAddr(IpAddr::from(Ipv4Addr::new(
                (src.remaining() >= 1).then(|| src.get_u8())?,
                (src.remaining() >= 1).then(|| src.get_u8())?,
                (src.remaining() >= 1).then(|| src.get_u8())?,
                (src.remaining() >= 1).then(|| src.get_u8())?,
            )))),
            0x03 => {
                let name_len = (src.remaining() >= 1).then(|| src.get_u8() as usize)?;
                let name_bytes =
                    (src.remaining() >= name_len).then(|| src.copy_to_bytes(name_len))?;
                Some(Socks5Address::Name(
                    String::from_utf8_lossy(&name_bytes).to_string(),
                ))
            }
            0x04 => Some(Socks5Address::IpAddr(IpAddr::from(Ipv6Addr::new(
                (src.remaining() >= 2).then(|| src.get_u16())?,
                (src.remaining() >= 2).then(|| src.get_u16())?,
                (src.remaining() >= 2).then(|| src.get_u16())?,
                (src.remaining() >= 2).then(|| src.get_u16())?,
                (src.remaining() >= 2).then(|| src.get_u16())?,
                (src.remaining() >= 2).then(|| src.get_u16())?,
                (src.remaining() >= 2).then(|| src.get_u16())?,
                (src.remaining() >= 2).then(|| src.get_u16())?,
            )))),
            _ => Some(Socks5Address::Unknown),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    use super::*;

    #[test]
    fn valid() {
        let addresses = vec![
            Socks5Address::from_name(String::from("test.com")),
            Socks5Address::from_addr(IpAddr::V4(Ipv4Addr::new(4, 3, 2, 1))),
            Socks5Address::from_addr(IpAddr::V6(Ipv6Addr::new(8, 7, 6, 5, 4, 3, 2, 1))),
        ];

        for addr in addresses {
            let mut buf = BytesMut::new();
            buf.put(addr.serialize());

            assert_eq!(
                addr,
                Socks5Address::deserialize(&mut Cursor::new(&buf)).unwrap()
            );
        }
    }

    #[test]
    fn max_length() {
        let addr = Socks5Address::from_name("a".repeat(255));
        assert_eq!(addr.len(), 257);

        let mut buf = BytesMut::new();
        buf.put(addr.serialize());

        assert_eq!(
            addr,
            Socks5Address::deserialize(&mut Cursor::new(&buf)).unwrap()
        );
    }

    #[test]
    fn too_long() {
        // Long names are truncated to 255 bytes
        for len in [256, 257, 258, 300, 1000] {
            let addr = Socks5Address::from_name("a".repeat(len));
            // includes 1 byte for type, 1 byte for len, 255 bytes for name
            assert_eq!(addr.len(), 257);

            let mut buf = BytesMut::new();
            buf.put(addr.serialize());

            assert_eq!(
                addr,
                Socks5Address::deserialize(&mut Cursor::new(&buf)).unwrap()
            );
        }
    }
}
