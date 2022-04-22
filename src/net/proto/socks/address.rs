use bytes::{Buf, BufMut, BytesMut};
use std::{
    io::Cursor,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
};

use crate::net;

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

    pub fn from_bytes(src_buf: &mut Cursor<&BytesMut>) -> Option<Socks5Address> {
        let addr_type = src_buf.has_remaining().then(|| src_buf.get_u8())?;

        match addr_type {
            0x01 => Some(Socks5Address::IpAddr(IpAddr::from(Ipv4Addr::new(
                src_buf.has_remaining().then(|| src_buf.get_u8())?,
                src_buf.has_remaining().then(|| src_buf.get_u8())?,
                src_buf.has_remaining().then(|| src_buf.get_u8())?,
                src_buf.has_remaining().then(|| src_buf.get_u8())?,
            )))),
            0x03 => {
                let name_len = src_buf.has_remaining().then(|| src_buf.get_u8())?;
                let name_bytes = net::get_bytes_vec(src_buf, name_len as usize)?;
                Some(Socks5Address::Name(
                    String::from_utf8_lossy(&name_bytes).to_string(),
                ))
            }
            0x04 => Some(Socks5Address::IpAddr(IpAddr::from(Ipv6Addr::new(
                src_buf.has_remaining().then(|| src_buf.get_u16())?,
                src_buf.has_remaining().then(|| src_buf.get_u16())?,
                src_buf.has_remaining().then(|| src_buf.get_u16())?,
                src_buf.has_remaining().then(|| src_buf.get_u16())?,
                src_buf.has_remaining().then(|| src_buf.get_u16())?,
                src_buf.has_remaining().then(|| src_buf.get_u16())?,
                src_buf.has_remaining().then(|| src_buf.get_u16())?,
                src_buf.has_remaining().then(|| src_buf.get_u16())?,
            )))),
            _ => Some(Socks5Address::Unknown),
        }
    }

    pub fn to_bytes(&self, dst_buf: &mut BytesMut) {
        match self {
            Socks5Address::IpAddr(addr) => match addr {
                IpAddr::V4(a) => {
                    dst_buf.put_u8(0x01);
                    for octet in a.octets().iter() {
                        dst_buf.put_u8(*octet);
                    }
                }
                IpAddr::V6(a) => {
                    dst_buf.put_u8(0x04);
                    for segment in a.segments().iter() {
                        dst_buf.put_u16(*segment);
                    }
                }
            },
            Socks5Address::Name(name) => {
                dst_buf.put_u8(0x03);
                dst_buf.put_u8(name.len() as u8);
                dst_buf.put_slice(name.as_bytes());
            }
            Socks5Address::Unknown => {
                dst_buf.put_u8(0x0);
            }
        }
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
