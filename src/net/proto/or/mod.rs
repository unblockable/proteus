use bytes::{Buf, BufMut, BytesMut};
use std::{
    fmt,
    io::{Cursor},
};
use typestate::typestate;

use crate::net::{self, Frame};

pub mod client;

#[typestate]
mod ext_or_protocol {
    use super::*;
    use crate::net::or;
    use crate::net::Connection;

    use async_trait::async_trait;

    pub const EXTOR_AUTH_TYPE_SAFE_COOKIE: u8 = 0x01;
    pub const EXTOR_AUTH_TYPE_END: u8 = 0x00;
    pub const EXTOR_AUTH_STATUS_SUCCESS: u8 = 0x01;
    pub const EXTOR_AUTH_STATUS_FAILURE: u8 = 0x00;
    pub const EXTOR_COMMAND_DONE: u16 = 0x0000;
    pub const EXTOR_COMMAND_USERADDR: u16 = 0x0001;
    pub const EXTOR_COMMAND_TRANSPORT: u16 = 0x0002;
    pub const EXTOR_REPLY_OK: u16 = 0x1000;
    pub const EXTOR_REPLY_DENY: u16 = 0x1001;

    #[automaton]
    pub struct ExtOrProtocol;

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
        ClientAuthNonce,
        Error,
    }

    #[state]
    pub struct ClientAuthNonce {
        pub conn: Connection,
    }
    #[async_trait]
    pub trait ClientAuthNonce {
        async fn auth_nonce(self) -> ClientAuthNonceResult;
    }
    pub enum ClientAuthNonceResult {
        ServerAuthNonceHash,
        Error,
    }

    #[state]
    pub struct ServerAuthNonceHash {
        pub conn: Connection,
        pub client_auth: ClientNonce,
    }
    #[async_trait]
    pub trait ServerAuthNonceHash {
        async fn auth_nonce_hash(self) -> ServerAuthNonceHashResult;
    }
    pub enum ServerAuthNonceHashResult {
        ClientAuthHash,
        Error,
    }

    #[state]
    pub struct ClientAuthHash {
        pub conn: Connection,
        pub client_auth: ClientNonce,
        pub server_auth: ServerHashNonce,
    }
    #[async_trait]
    pub trait ClientAuthHash {
        async fn auth_hash(self) -> ClientAuthHashResult;
    }
    pub enum ClientAuthHashResult {
        ServerAuthStatus,
        Error,
    }

    #[state]
    pub struct ServerAuthStatus {
        pub conn: Connection,
    }
    #[async_trait]
    pub trait ServerAuthStatus {
        async fn auth_status(self) -> ServerAuthStatusResult;
    }
    pub enum ServerAuthStatusResult {
        ClientCommand,
        Error,
    }

    #[state]
    pub struct ClientCommand {
        pub conn: Connection,
    }
    #[async_trait]
    pub trait ClientCommand {
        async fn command(self) -> ClientCommandResult;
    }
    pub enum ClientCommandResult {
        ServerCommand,
        Error,
    }

    #[state]
    pub struct ServerCommand {
        pub conn: Connection,
    }
    #[async_trait]
    pub trait ServerCommand {
        async fn reply(self) -> ServerCommandResult;
    }
    pub enum ServerCommandResult {
        ClientCommand,
        Success,
        Error,
    }

    #[state]
    pub struct Success {
        pub conn: Connection,
    }
    pub trait Success {
        fn finish(self) -> Connection;
    }

    #[state]
    pub struct Error {
        pub error: or::Error,
    }
    pub trait Error {
        fn finish(self) -> or::Error;
    }

    impl From<Initialization> for ExtOrProtocol<Initialization> {
        fn from(state: Initialization) -> Self {
            ExtOrProtocol::<Initialization> { state: state }
        }
    }

    impl From<ClientHandshake> for ExtOrProtocol<ClientHandshake> {
        fn from(state: ClientHandshake) -> Self {
            ExtOrProtocol::<ClientHandshake> { state: state }
        }
    }

    impl From<ServerHandshake> for ExtOrProtocol<ServerHandshake> {
        fn from(state: ServerHandshake) -> Self {
            ExtOrProtocol::<ServerHandshake> { state: state }
        }
    }

    impl From<ClientAuthNonce> for ExtOrProtocol<ClientAuthNonce> {
        fn from(state: ClientAuthNonce) -> Self {
            ExtOrProtocol::<ClientAuthNonce> { state: state }
        }
    }

    impl From<ServerAuthNonceHash> for ExtOrProtocol<ServerAuthNonceHash> {
        fn from(state: ServerAuthNonceHash) -> Self {
            ExtOrProtocol::<ServerAuthNonceHash> { state: state }
        }
    }

    impl From<ClientAuthHash> for ExtOrProtocol<ClientAuthHash> {
        fn from(state: ClientAuthHash) -> Self {
            ExtOrProtocol::<ClientAuthHash> { state: state }
        }
    }

    impl From<ServerAuthStatus> for ExtOrProtocol<ServerAuthStatus> {
        fn from(state: ServerAuthStatus) -> Self {
            ExtOrProtocol::<ServerAuthStatus> { state: state }
        }
    }

    impl From<ClientCommand> for ExtOrProtocol<ClientCommand> {
        fn from(state: ClientCommand) -> Self {
            ExtOrProtocol::<ClientCommand> { state: state }
        }
    }

    impl From<ServerCommand> for ExtOrProtocol<ServerCommand> {
        fn from(state: ServerCommand) -> Self {
            ExtOrProtocol::<ServerCommand> { state: state }
        }
    }

    impl From<Success> for ExtOrProtocol<Success> {
        fn from(state: Success) -> Self {
            ExtOrProtocol::<Success> { state: state }
        }
    }

    impl From<Error> for ExtOrProtocol<Error> {
        fn from(state: Error) -> Self {
            ExtOrProtocol::<Error> { state: state }
        }
    }
}

pub enum Error {
    AuthMethod,
    AuthStatusFailed,
    AuthStatusUnknown,
    Auth(String),
    Address(String),
    Transport(String),
    Command(String),
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
            Error::AuthMethod => write!(f, "Ext OR authentication method unsupported"),
            Error::AuthStatusFailed => write!(f, "Ext OR authentication failed"),
            Error::AuthStatusUnknown => write!(f, "Ext OR authentication status unknown"),
            Error::Auth(s) => write!(f, "Chosen Ext OR authentication method failed: {}", s),
            Error::Address(s) => write!(f, "User address denied: {}", s),
            Error::Transport(s) => write!(f, "Transport denied: {}", s),
            Error::Command(s) => write!(f, "Command denied: {}", s),
            Error::Network(e) => write!(f, "Network error: {}", e),
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct Greeting {
    auth_types: Vec<u8>,
}

#[derive(Debug, PartialEq)]
pub struct Choice {
    auth_type: u8,
}

#[derive(Debug, PartialEq)]
pub struct ClientNonce {
    nonce: [u8; 32],
}

#[derive(Debug, PartialEq)]
pub struct ServerHashNonce {
    hash: [u8; 32],
    nonce: [u8; 32],
}

#[derive(Debug, PartialEq)]
pub struct ClientHash {
    hash: [u8; 32],
}

#[derive(Debug, PartialEq)]
pub struct ServerStatus {
    status: u8,
}

#[derive(Debug, PartialEq)]
pub struct Command {
    command: u16,
    body: String,
}

#[derive(Debug, PartialEq)]
pub struct Reply {
    reply: u16,
}

impl Frame<Greeting> for Greeting {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<Greeting> {
        let mut auth_types = Vec::new();

        while buf.has_remaining() {
            match buf.get_u8() {
                ext_or_protocol::EXTOR_AUTH_TYPE_END => return Some(Greeting { auth_types }),
                t => auth_types.push(t),
            }
        }

        None
    }

    fn serialize(&self) -> BytesMut {
        let mut buf = BytesMut::with_capacity(4);

        for method in self.auth_types.iter() {
            buf.put_u8(*method);
        }
        buf.put_u8(ext_or_protocol::EXTOR_AUTH_TYPE_END);

        buf
    }
}

impl Frame<Choice> for Choice {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<Choice> {
        Some(Choice {
            auth_type: buf.has_remaining().then(|| buf.get_u8())?,
        })
    }

    fn serialize(&self) -> BytesMut {
        let mut buf = BytesMut::with_capacity(4);
        buf.put_u8(self.auth_type);
        buf
    }
}

impl Frame<ClientNonce> for ClientNonce {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<ClientNonce> {
        let mut nonce: [u8; 32] = [0; 32];
        for i in 0..32 {
            nonce[i] = buf.has_remaining().then(|| buf.get_u8())?;
        }
        Some(ClientNonce { nonce })
    }

    fn serialize(&self) -> BytesMut {
        let mut buf = BytesMut::with_capacity(32);
        for i in 0..32 {
            buf.put_u8(self.nonce[i]);
        }
        buf
    }
}

impl Frame<ServerHashNonce> for ServerHashNonce {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<ServerHashNonce> {
        let mut hash: [u8; 32] = [0; 32];
        for i in 0..32 {
            hash[i] = buf.has_remaining().then(|| buf.get_u8())?;
        }
        let mut nonce: [u8; 32] = [0; 32];
        for i in 0..32 {
            nonce[i] = buf.has_remaining().then(|| buf.get_u8())?;
        }
        Some(ServerHashNonce { hash, nonce })
    }

    fn serialize(&self) -> BytesMut {
        let mut buf = BytesMut::with_capacity(64);
        for i in 0..32 {
            buf.put_u8(self.hash[i]);
        }
        for i in 0..32 {
            buf.put_u8(self.nonce[i]);
        }
        buf
    }
}

impl Frame<ClientHash> for ClientHash {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<ClientHash> {
        let mut hash: [u8; 32] = [0; 32];
        for i in 0..32 {
            hash[i] = buf.has_remaining().then(|| buf.get_u8())?
        }
        Some(ClientHash { hash })
    }

    fn serialize(&self) -> BytesMut {
        let mut buf = BytesMut::with_capacity(32);
        for i in 0..32 {
            buf.put_u8(self.hash[i]);
        }
        buf
    }
}

impl Frame<ServerStatus> for ServerStatus {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<ServerStatus> {
        Some(ServerStatus {
            status: buf.has_remaining().then(|| buf.get_u8())?,
        })
    }

    fn serialize(&self) -> BytesMut {
        let mut buf = BytesMut::with_capacity(4);
        buf.put_u8(self.status);
        buf
    }
}

impl Frame<Command> for Command {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<Command> {
        let command = buf.has_remaining().then(|| buf.get_u16())?;
        let body_len = buf.has_remaining().then(|| buf.get_u16())?;
        let body_bytes = net::get_bytes_vec(buf, body_len as usize)?;

        Some(Command {
            command,
            body: String::from_utf8_lossy(&body_bytes).to_string(),
        })
    }

    fn serialize(&self) -> BytesMut {
        let mut buf = BytesMut::with_capacity(4);
        buf.put_u16(self.command);
        buf.put_u16(self.body.len() as u16);
        buf.put_slice(self.body.as_bytes());
        buf
    }
}

impl Frame<Reply> for Reply {
    fn deserialize(buf: &mut Cursor<&BytesMut>) -> Option<Reply> {
        Some(Reply {
            reply: buf.has_remaining().then(|| buf.get_u16())?,
        })
    }

    fn serialize(&self) -> BytesMut {
        let mut buf = BytesMut::with_capacity(4);
        buf.put_u16(self.reply);
        buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greeting() {
        let frame = Greeting {
            auth_types: vec![1],
        };
        assert_eq!(
            frame,
            Greeting::deserialize(&mut Cursor::new(&frame.serialize())).unwrap()
        );
    }

    #[test]
    fn greeting_partial() {
        let mut bytes = BytesMut::new();
        bytes.put_u8(1);
        bytes.put_u8(2);

        assert!(Greeting::deserialize(&mut Cursor::new(&bytes)).is_none());

        bytes.put_u8(ext_or_protocol::EXTOR_AUTH_TYPE_END);

        let frame = Greeting {
            auth_types: vec![1, 2],
        };

        assert_eq!(
            frame,
            Greeting::deserialize(&mut Cursor::new(&bytes)).unwrap()
        );

        bytes.advance(1);

        let frame2 = Greeting {
            auth_types: vec![2],
        };

        assert_eq!(
            frame2,
            Greeting::deserialize(&mut Cursor::new(&bytes)).unwrap()
        );
    }

    #[test]
    fn choice() {
        let frame = Choice { auth_type: 1 };
        assert_eq!(
            frame,
            Choice::deserialize(&mut Cursor::new(&frame.serialize())).unwrap()
        );
    }

    #[test]
    fn client_nonce() {
        let nonce: [u8; 32] = [111; 32];
        let frame = ClientNonce { nonce };
        assert_eq!(
            frame,
            ClientNonce::deserialize(&mut Cursor::new(&frame.serialize())).unwrap()
        );
    }

    #[test]
    fn server_hashnonce() {
        let hash: [u8; 32] = [222; 32];
        let nonce: [u8; 32] = [111; 32];
        let frame = ServerHashNonce { hash, nonce };
        assert_eq!(
            frame,
            ServerHashNonce::deserialize(&mut Cursor::new(&frame.serialize())).unwrap()
        );
    }

    #[test]
    fn client_hash() {
        let hash: [u8; 32] = [222; 32];
        let frame = ClientHash { hash };
        assert_eq!(
            frame,
            ClientHash::deserialize(&mut Cursor::new(&frame.serialize())).unwrap()
        );
    }

    #[test]
    fn server_status() {
        let frame = ServerStatus { status: 1 };
        assert_eq!(
            frame,
            ServerStatus::deserialize(&mut Cursor::new(&frame.serialize())).unwrap()
        );
    }

    #[test]
    fn command() {
        let frame = Command {
            command: ext_or_protocol::EXTOR_COMMAND_TRANSPORT,
            body: String::from("upgen"),
        };
        assert_eq!(
            frame,
            Command::deserialize(&mut Cursor::new(&frame.serialize())).unwrap()
        );
    }

    #[test]
    fn reply() {
        let frame = Reply {
            reply: ext_or_protocol::EXTOR_REPLY_OK,
        };
        assert_eq!(
            frame,
            Reply::deserialize(&mut Cursor::new(&frame.serialize())).unwrap()
        );
    }
}
