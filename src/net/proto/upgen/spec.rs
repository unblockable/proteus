use typestate::typestate;

#[typestate]
pub mod upgen {
    use crate::net::proto::upgen::generator::Generator;
    use crate::net::proto::upgen;
    use crate::net::Connection;

    use async_trait::async_trait;

    #[automaton]
    pub struct UpgenProtocol;

    #[state]
    pub struct Init {
        pub client_conn: Connection,
        pub server_conn: Connection,
        pub spec: Generator,
    }
    pub trait Init {
        fn new(client_conn: Connection, server_conn: Connection, seed: u64) -> Init;
        fn start_client(self) -> ClientHandshake1;
        fn start_server(self) -> ServerHandshake1;
    }

    #[state]
    pub struct ClientHandshake1 {
        pub client_conn: Connection,
        pub server_conn: Connection,
        pub spec: Generator,
    }
    #[async_trait]
    pub trait ClientHandshake1 {
        async fn send_handshake1(self) -> ClientHandshake1Result;
    }
    pub enum ClientHandshake1Result {
        ClientHandshake2,
        Error,
    }

    #[state]
    pub struct ClientHandshake2 {
        pub client_conn: Connection,
        pub server_conn: Connection,
        pub spec: Generator,
    }
    #[async_trait]
    pub trait ClientHandshake2 {
        async fn recv_handshake2(self) -> ClientHandshake2Result;
    }
    pub enum ClientHandshake2Result {
        Data,
        Error,
    }

    #[state]
    pub struct ServerHandshake1 {
        pub client_conn: Connection,
        pub server_conn: Connection,
        pub spec: Generator,
    }
    #[async_trait]
    pub trait ServerHandshake1 {
        async fn recv_handshake1(self) -> ServerHandshake1Result;
    }
    pub enum ServerHandshake1Result {
        ServerHandshake2,
        Error,
    }

    #[state]
    pub struct ServerHandshake2 {
        pub client_conn: Connection,
        pub server_conn: Connection,
        pub spec: Generator,
    }
    #[async_trait]
    pub trait ServerHandshake2 {
        async fn send_handshake2(self) -> ServerHandshake2Result;
    }
    pub enum ServerHandshake2Result {
        Data,
        Error,
    }

    #[state]
    pub struct Data {
        pub upgen_conn: Connection,
        pub tor_conn: Connection,
        pub spec: Generator,
    }
    #[async_trait]
    pub trait Data {
        async fn forward_data(self) -> DataResult;
    }
    pub enum DataResult {
        Data,
        Success,
        Error,
    }

    #[state]
    pub struct Success {
        pub client_conn: Connection,
        pub server_conn: Connection,
        pub spec: Generator,
    }
    pub trait Success {
        fn finish(self);
    }

    #[state]
    pub struct Error {
        pub error: upgen::Error,
    }
    pub trait Error {
        fn finish(self) -> upgen::Error;
    }

    impl From<Init> for UpgenProtocol<Init> {
        fn from(state: Init) -> Self {
            UpgenProtocol::<Init> { state: state }
        }
    }

    impl From<ClientHandshake1> for UpgenProtocol<ClientHandshake1> {
        fn from(state: ClientHandshake1) -> Self {
            UpgenProtocol::<ClientHandshake1> { state: state }
        }
    }

    impl From<ClientHandshake2> for UpgenProtocol<ClientHandshake2> {
        fn from(state: ClientHandshake2) -> Self {
            UpgenProtocol::<ClientHandshake2> { state: state }
        }
    }

    impl From<ServerHandshake1> for UpgenProtocol<ServerHandshake1> {
        fn from(state: ServerHandshake1) -> Self {
            UpgenProtocol::<ServerHandshake1> { state: state }
        }
    }

    impl From<ServerHandshake2> for UpgenProtocol<ServerHandshake2> {
        fn from(state: ServerHandshake2) -> Self {
            UpgenProtocol::<ServerHandshake2> { state: state }
        }
    }

    impl From<Data> for UpgenProtocol<Data> {
        fn from(state: Data) -> Self {
            UpgenProtocol::<Data> { state: state }
        }
    }

    impl From<Success> for UpgenProtocol<Success> {
        fn from(state: Success) -> Self {
            UpgenProtocol::<Success> { state: state }
        }
    }

    impl From<Error> for UpgenProtocol<Error> {
        fn from(state: Error) -> Self {
            UpgenProtocol::<Error> { state: state }
        }
    }
}
