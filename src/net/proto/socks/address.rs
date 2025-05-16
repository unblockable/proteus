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
    pub fn from_name(name: String) -> Socks5Address {
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
            Socks5Address::Name(name) => 1 + 1 + name.len(), // type + len + name
            Socks5Address::Unknown => 1,                     // type
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
                buf.put_u8(0x03);
                buf.put_u8(name.len() as u8);
                buf.put_slice(name.as_bytes());
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
