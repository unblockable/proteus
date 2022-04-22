use typestate::typestate;

#[typestate]
pub mod socks5 {
    use crate::net::{
        proto::socks::{
            self,
            frames::{ConnectRequest, Greeting, UserPassAuthRequest},
        },
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
