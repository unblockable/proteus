use typestate::typestate;

#[typestate]
pub mod extor {
    use crate::net::{
        proto::or::{
            self,
            frames::{ClientNonce, Greeting, ServerHashNonce},
        },
        Connection,
    };

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
