use bytes::{Buf, BufMut, BytesMut};
use std::{fmt, net::{IpAddr, Ipv4Addr, Ipv6Addr}, io::Cursor};
use typestate::typestate;

use crate::net::{self, Frame};

pub mod server;

#[typestate]
mod socks5_protocol {
    use super::*;
    use crate::net::proto::socks;
    use crate::net::Connection;

    use async_trait::async_trait;

    pub const SOCKS_NULL: u8 = 0x00;
    pub const SOCKS_VERSION_5: u8 = 0x05;
    pub const SOCKS_AUTH_NONE: u8 = 0x00;
    pub const SOCKS_AUTH_USERPASS: u8 = 0x02;
    pub const SOCKS_AUTH_UNSUPPORTED: u8 = 0xff;
    pub const SOCKS_AUTH_USERPASS_VERSION: u8 = 0x01;
    pub const SOCKS_AUTH_STATUS_SUCCESS: u8 = 0x00;
    pub const SOCKS_AUTH_STATUS_FAILURE: u8 = 0x01;
    pub const SOCKS_COMMAND_CONNECT: u8 = 0x01;
    pub const SOCKS_STATUS_REQ_GRANTED: u8 = 0x00;
    pub const SOCKS_STATUS_GEN_FAILURE: u8 = 0x01;
    pub const SOCKS_STATUS_PROTO_ERR: u8 = 0x07;
    pub const SOCKS_STATUS_ADDR_ERR: u8 = 0x08;

    #[automaton]
    pub struct Socks5Protocol;

    #[state]
    pub struct Initialization {
        pub conn: Connection,
    }
    pub trait Initialization {
        fn new(conn: Connection) -> Initialization;
        fn start(self) -> ClientHandshake;
    }

    #[state]
    pub struct ClientHandshake {
        pub conn: Connection,
    }
    #[async_trait]
    pub trait ClientHandshake {
        async fn greeting(self) -> ClientHandshakeResult;
    }
    pub enum ClientHandshakeResult {
        ServerHandshake,
        Error,
    }

    #[state]
    pub struct ServerHandshake {
        pub conn: Connection,
        pub greeting: Greeting,
    }
    #[async_trait]
    pub trait ServerHandshake {
        async fn choice(self) -> ServerHandshakeResult;
    }
    pub enum ServerHandshakeResult {
        ClientAuthentication,
        ClientCommand,
        Error,
    }

    #[state]
    pub struct ClientAuthentication {
        pub conn: Connection,
    }
    #[async_trait]
    pub trait ClientAuthentication {
        async fn auth_request(self) -> ClientAuthenticationResult;
    }
    pub enum ClientAuthenticationResult {
        ServerAuthentication,
        Error,
    }

    #[state]
    pub struct ServerAuthentication {
        pub conn: Connection,
        pub auth_request: UserPassAuthRequest,
    }
    #[async_trait]
    pub trait ServerAuthentication {
        async fn auth_response(self) -> ServerAuthenticationResult;
    }
    pub enum ServerAuthenticationResult {
        ClientCommand,
        Error,
    }

    #[state]
    pub struct ClientCommand {
        pub conn: Connection,
    }
    #[async_trait]
    pub trait ClientCommand {
        async fn connect_request(self) -> ClientCommandResult;
    }
    pub enum ClientCommandResult {
        ServerCommand,
        Error,
    }

    #[state]
    pub struct ServerCommand {
        pub conn: Connection,
        pub request: ConnectRequest,
    }
    #[async_trait]
    pub trait ServerCommand {
        async fn connect_response(self) -> ServerCommandResult;
    }
    pub enum ServerCommandResult {
        Success,
        Error,
    }

    #[state]
    pub struct Success {
        pub conn: Connection,
        pub dest: Connection,
    }
    pub trait Success {
        fn finish(self) -> (Connection, Connection);
    }

    #[state]
    pub struct Error {
        pub error: socks::Error,
    }
    pub trait Error {
        fn finish(self) -> socks::Error;
    }

    impl From<Initialization> for Socks5Protocol<Initialization> {
        fn from(state: Initialization) -> Self {
            Socks5Protocol::<Initialization> { state: state }
        }
    }
    
    impl From<ClientHandshake> for Socks5Protocol<ClientHandshake> {
        fn from(state: ClientHandshake) -> Self {
            Socks5Protocol::<ClientHandshake> { state: state }
        }
    }
    
    impl From<ServerHandshake> for Socks5Protocol<ServerHandshake> {
        fn from(state: ServerHandshake) -> Self {
            Socks5Protocol::<ServerHandshake> { state: state }
        }
    }

    impl From<ClientAuthentication> for Socks5Protocol<ClientAuthentication> {
        fn from(state: ClientAuthentication) -> Self {
            Socks5Protocol::<ClientAuthentication> { state: state }
        }
    }

    impl From<ServerAuthentication> for Socks5Protocol<ServerAuthentication> {
        fn from(state: ServerAuthentication) -> Self {
            Socks5Protocol::<ServerAuthentication> { state: state }
        }
    }

    impl From<ClientCommand> for Socks5Protocol<ClientCommand> {
        fn from(state: ClientCommand) -> Self {
            Socks5Protocol::<ClientCommand> { state: state }
        }
    }

    impl From<ServerCommand> for Socks5Protocol<ServerCommand> {
        fn from(state: ServerCommand) -> Self {
            Socks5Protocol::<ServerCommand> { state: state }
        }
    }

    impl From<Success> for Socks5Protocol<Success> {
        fn from(state: Success) -> Self {
            Socks5Protocol::<Success> { state: state }
        }
    }
    
    impl From<Error> for Socks5Protocol<Error> {
        fn from(state: Error) -> Self {
            Socks5Protocol::<Error> { state: state }
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct Greeting {
    version: u8,
    num_auth_methods: u8,
    supported_auth_methods: Vec<u8>,
}

#[derive(Debug, PartialEq)]
pub struct Choice {
    version: u8,
    auth_method: u8,
}

#[derive(Debug, PartialEq)]
pub struct UserPassAuthRequest {
    version: u8,
    username: String,
    password: String,
}

#[derive(Debug, PartialEq)]
pub struct UserPassAuthResponse {
    version: u8,
    status: u8,
}

#[derive(Debug, PartialEq)]
pub struct ConnectRequest {
    version: u8,
    command: u8,
    reserved: u8,
    dest_addr: Socks5Address,
    dest_port: u16,
}

#[derive(Debug, PartialEq)]
pub struct ConnectResponse {
    version: u8,
    status: u8,
    reserved: u8,
    bind_addr: Socks5Address,
    bind_port: u16,
}

pub enum Error {
    Version,
    Reserved,
    AuthMethod,
    Auth(String),
    ConnectMethod,
    Connect(String),
    Network(net::Error),
}

impl From<net::Error> for Error {
    fn from(e: net::Error) -> Self {
        Error::Network(e)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Version => write!(f, "Socks version mismatch"),
            Error::Reserved => write!(f, "Socks non-zero reserved field"),
            Error::AuthMethod => write!(f, "No supported authentication methods"),
            Error::Auth(s) => write!(f, "Chosen authentication method failed: {}", s),
            Error::ConnectMethod => write!(f, "No supported connect methods"),
            Error::Connect(s) => write!(f, "Chosen connect method failed: {}", s),
            Error::Network(e) => write!(f, "Network error: {}", e),
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum Socks5Address {
    IpAddr(IpAddr),
    Name(String),
    Unknown,
}

impl Socks5Address {
    #[cfg(test)]
    fn from_name(name: String) -> Socks5Address {
        Socks5Address::Name(name)
    }

    #[cfg(test)]
    fn from_addr(addr: IpAddr) -> Socks5Address {
        Socks5Address::IpAddr(addr)
    }

    fn from_bytes(src_buf: &mut Cursor<&BytesMut>) -> Option<Socks5Address> {
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

    fn to_bytes(&self, dst_buf: &mut BytesMut) {
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

    fn len(&self) -> usize {
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
        assert_eq!(frame, Choice::deserialize(&mut Cursor::new(&frame.serialize())).unwrap());
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
