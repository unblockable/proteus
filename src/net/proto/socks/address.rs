use std::fmt::Display;
use std::net::{IpAddr, SocketAddr};

#[derive(Debug, PartialEq, Clone)]
pub struct Socks5Target {
    addr: Socks5Address,
    port: u16,
}

impl Socks5Target {
    pub fn new(addr: Socks5Address, port: u16) -> Self {
        Self { addr, port }
    }

    pub fn addr(&self) -> Socks5Address {
        self.addr.clone()
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

impl From<SocketAddr> for Socks5Target {
    fn from(addr: SocketAddr) -> Self {
        match addr {
            SocketAddr::V4(socket_addr_v4) => Socks5Target {
                addr: Socks5Address::IpAddr(IpAddr::V4(*socket_addr_v4.ip())),
                port: socket_addr_v4.port(),
            },
            SocketAddr::V6(socket_addr_v6) => Socks5Target {
                addr: Socks5Address::IpAddr(IpAddr::V6(*socket_addr_v6.ip())),
                port: socket_addr_v6.port(),
            },
        }
    }
}

impl Display for Socks5Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.addr, self.port)
    }
}

#[derive(Debug, Default, PartialEq, Clone)]
pub enum Socks5Address {
    IpAddr(IpAddr),
    Name(String),
    #[default]
    Unknown,
}

impl Socks5Address {
    #[cfg(test)]
    pub fn from_name(mut name: String) -> Socks5Address {
        name.truncate(u8::MAX as usize);
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
            Socks5Address::Name(name) => 1 + 1 + name.len().min(u8::MAX as usize), // type + len + name
            Socks5Address::Unknown => 1,                                           // type
        }
    }
}

impl Display for Socks5Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Socks5Address::IpAddr(ip_addr) => write!(f, "{ip_addr}"),
            Socks5Address::Name(s) => write!(f, "{s}"),
            Socks5Address::Unknown => write!(f, "<unknown>"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    use super::*;

    #[test]
    fn check_from() {
        let addresses = vec![
            Socks5Address::from_name(String::from("test.com")),
            Socks5Address::from_addr(IpAddr::V4(Ipv4Addr::new(4, 3, 2, 1))),
            Socks5Address::from_addr(IpAddr::V6(Ipv6Addr::new(8, 7, 6, 5, 4, 3, 2, 1))),
        ];

        for addr in addresses {
            let _target = Socks5Target { addr, port: 12345 };
        }
    }

    fn check_name_len(addr: &Socks5Address, expected_len: usize) {
        let addr_len = addr.len();

        // includes 1 byte for type, 1 byte for len, expected_len bytes for name
        assert_eq!(addr_len, 2 + expected_len);

        let Socks5Address::Name(name) = addr else {
            panic!()
        };

        assert_eq!(name.len(), expected_len);
    }

    #[test]
    fn max_name_length() {
        let addr = Socks5Address::from_name("a".repeat(255));
        check_name_len(&addr, 255);
    }

    #[test]
    fn name_too_long() {
        // Long names are truncated to 255 bytes
        for len in [256, 257, 258, 300, 1000] {
            let addr = Socks5Address::from_name("a".repeat(len));
            check_name_len(&addr, 255);
        }
    }
}
