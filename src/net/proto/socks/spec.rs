use typestate::typestate;

#[typestate]
pub mod socks5 {
    use crate::net::{
        proto::socks::{self, formatter::Formatter, frames::*},
        Connection,
    };

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
    pub struct Init {
        pub conn: Connection,
        pub fmt: Formatter,
    }
    pub trait Init {
        fn new(conn: Connection) -> Init;
        fn start_server(self) -> ServerHandshake1;
    }

    #[state]
    pub struct ServerHandshake1 {
        pub conn: Connection,
        pub fmt: Formatter,
    }
    #[async_trait]
    pub trait ServerHandshake1 {
        async fn recv_greeting(self) -> ServerHandshake1Result;
    }
    pub enum ServerHandshake1Result {
        ServerHandshake2,
        Error,
    }

    #[state]
    pub struct ServerHandshake2 {
        pub conn: Connection,
        pub fmt: Formatter,
        pub greeting: Greeting,
    }
    #[async_trait]
    pub trait ServerHandshake2 {
        async fn send_choice(self) -> ServerHandshake2Result;
    }
    pub enum ServerHandshake2Result {
        ServerAuth1,
        ServerCommand1,
        Error,
    }

    #[state]
    pub struct ServerAuth1 {
        pub conn: Connection,
        pub fmt: Formatter,
    }
    #[async_trait]
    pub trait ServerAuth1 {
        async fn recv_auth_request(self) -> ServerAuth1Result;
    }
    pub enum ServerAuth1Result {
        ServerAuth2,
        Error,
    }

    #[state]
    pub struct ServerAuth2 {
        pub conn: Connection,
        pub fmt: Formatter,
        pub auth_request: UserPassAuthRequest,
    }
    #[async_trait]
    pub trait ServerAuth2 {
        async fn send_auth_response(self) -> ServerAuth2Result;
    }
    pub enum ServerAuth2Result {
        ServerCommand1,
        Error,
    }

    #[state]
    pub struct ServerCommand1 {
        pub conn: Connection,
        pub fmt: Formatter,
        pub username: Option<String>,
    }
    #[async_trait]
    pub trait ServerCommand1 {
        async fn recv_connect_request(self) -> ServerCommand1Result;
    }
    pub enum ServerCommand1Result {
        ServerCommand2,
        Error,
    }

    #[state]
    pub struct ServerCommand2 {
        pub conn: Connection,
        pub fmt: Formatter,
        pub username: Option<String>,
        pub request: ConnectRequest,
    }
    #[async_trait]
    pub trait ServerCommand2 {
        async fn send_connect_response(self) -> ServerCommand2Result;
    }
    pub enum ServerCommand2Result {
        Success,
        Error,
    }

    #[state]
    pub struct Success {
        pub conn: Connection,
        pub dest: Connection,
        pub username: Option<String>,
    }
    pub trait Success {
        fn finish(self) -> (Connection, Connection, Option<String>);
    }

    #[state]
    pub struct Error {
        pub error: socks::Error,
    }
    pub trait Error {
        fn finish(self) -> socks::Error;
    }

    impl From<Init> for Socks5Protocol<Init> {
        fn from(state: Init) -> Self {
            Socks5Protocol::<Init> { state: state }
        }
    }

    impl From<ServerHandshake1> for Socks5Protocol<ServerHandshake1> {
        fn from(state: ServerHandshake1) -> Self {
            Socks5Protocol::<ServerHandshake1> { state: state }
        }
    }

    impl From<ServerHandshake2> for Socks5Protocol<ServerHandshake2> {
        fn from(state: ServerHandshake2) -> Self {
            Socks5Protocol::<ServerHandshake2> { state: state }
        }
    }

    impl From<ServerAuth1> for Socks5Protocol<ServerAuth1> {
        fn from(state: ServerAuth1) -> Self {
            Socks5Protocol::<ServerAuth1> { state: state }
        }
    }

    impl From<ServerAuth2> for Socks5Protocol<ServerAuth2> {
        fn from(state: ServerAuth2) -> Self {
            Socks5Protocol::<ServerAuth2> { state: state }
        }
    }

    impl From<ServerCommand1> for Socks5Protocol<ServerCommand1> {
        fn from(state: ServerCommand1) -> Self {
            Socks5Protocol::<ServerCommand1> { state: state }
        }
    }

    impl From<ServerCommand2> for Socks5Protocol<ServerCommand2> {
        fn from(state: ServerCommand2) -> Self {
            Socks5Protocol::<ServerCommand2> { state: state }
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
