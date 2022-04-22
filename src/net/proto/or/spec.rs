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
    pub struct Init {
        pub conn: Connection,
    }
    pub trait Init {
        fn new(conn: Connection) -> Init;
        fn start_client(self) -> ClientHandshake1;
    }

    #[state]
    pub struct ClientHandshake1 {
        pub conn: Connection,
    }
    #[async_trait]
    pub trait ClientHandshake1 {
        async fn recv_greeting(self) -> ClientHandshake1Result;
    }
    pub enum ClientHandshake1Result {
        ClientHandshake2,
        Error,
    }

    #[state]
    pub struct ClientHandshake2 {
        pub conn: Connection,
        pub greeting: Greeting,
    }
    #[async_trait]
    pub trait ClientHandshake2 {
        async fn send_choice(self) -> ClientHandshake2Result;
    }
    pub enum ClientHandshake2Result {
        ClientAuth1,
        Error,
    }

    #[state]
    pub struct ClientAuth1 {
        pub conn: Connection,
    }
    #[async_trait]
    pub trait ClientAuth1 {
        async fn send_nonce(self) -> ClientAuth1Result;
    }
    pub enum ClientAuth1Result {
        ClientAuth2,
        Error,
    }

    #[state]
    pub struct ClientAuth2 {
        pub conn: Connection,
        pub client_auth: ClientNonce,
    }
    #[async_trait]
    pub trait ClientAuth2 {
        async fn recv_nonce_hash(self) -> ClientAuth2Result;
    }
    pub enum ClientAuth2Result {
        ClientAuth3,
        Error,
    }

    #[state]
    pub struct ClientAuth3 {
        pub conn: Connection,
        pub client_auth: ClientNonce,
        pub server_auth: ServerHashNonce,
    }
    #[async_trait]
    pub trait ClientAuth3 {
        async fn send_hash(self) -> ClientAuth3Result;
    }
    pub enum ClientAuth3Result {
        ClientAuth4,
        Error,
    }

    #[state]
    pub struct ClientAuth4 {
        pub conn: Connection,
    }
    #[async_trait]
    pub trait ClientAuth4 {
        async fn recv_status(self) -> ClientAuth4Result;
    }
    pub enum ClientAuth4Result {
        ClientCommand1,
        Error,
    }

    #[state]
    pub struct ClientCommand1 {
        pub conn: Connection,
    }
    #[async_trait]
    pub trait ClientCommand1 {
        async fn send_command(self) -> ClientCommand1Result;
    }
    pub enum ClientCommand1Result {
        ClientCommand2,
        Error,
    }

    #[state]
    pub struct ClientCommand2 {
        pub conn: Connection,
    }
    #[async_trait]
    pub trait ClientCommand2 {
        async fn recv_reply(self) -> ClientCommand2Result;
    }
    pub enum ClientCommand2Result {
        ClientCommand1,
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

    impl From<Init> for ExtOrProtocol<Init> {
        fn from(state: Init) -> Self {
            ExtOrProtocol::<Init> { state: state }
        }
    }

    impl From<ClientHandshake1> for ExtOrProtocol<ClientHandshake1> {
        fn from(state: ClientHandshake1) -> Self {
            ExtOrProtocol::<ClientHandshake1> { state: state }
        }
    }

    impl From<ClientHandshake2> for ExtOrProtocol<ClientHandshake2> {
        fn from(state: ClientHandshake2) -> Self {
            ExtOrProtocol::<ClientHandshake2> { state: state }
        }
    }

    impl From<ClientAuth1> for ExtOrProtocol<ClientAuth1> {
        fn from(state: ClientAuth1) -> Self {
            ExtOrProtocol::<ClientAuth1> { state: state }
        }
    }

    impl From<ClientAuth2> for ExtOrProtocol<ClientAuth2> {
        fn from(state: ClientAuth2) -> Self {
            ExtOrProtocol::<ClientAuth2> { state: state }
        }
    }

    impl From<ClientAuth3> for ExtOrProtocol<ClientAuth3> {
        fn from(state: ClientAuth3) -> Self {
            ExtOrProtocol::<ClientAuth3> { state: state }
        }
    }

    impl From<ClientAuth4> for ExtOrProtocol<ClientAuth4> {
        fn from(state: ClientAuth4) -> Self {
            ExtOrProtocol::<ClientAuth4> { state: state }
        }
    }

    impl From<ClientCommand1> for ExtOrProtocol<ClientCommand1> {
        fn from(state: ClientCommand1) -> Self {
            ExtOrProtocol::<ClientCommand1> { state: state }
        }
    }

    impl From<ClientCommand2> for ExtOrProtocol<ClientCommand2> {
        fn from(state: ClientCommand2) -> Self {
            ExtOrProtocol::<ClientCommand2> { state: state }
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
