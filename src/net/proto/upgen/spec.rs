use typestate::typestate;

#[typestate]
pub mod upgen {
    use crate::net::{
        proto::upgen::{self, formatter::Formatter, protocols::OvertProtocol},
        Connection,
    };

    use async_trait::async_trait;

    #[automaton]
    pub struct UpgenProtocol;

    #[state]
    pub struct Init {
        pub upgen_conn: Connection,
        pub other_conn: Connection,
        pub fmt: Formatter,
        pub spec: OvertProtocol,
    }
    pub trait Init {
        fn new(upgen_conn: Connection, other_conn: Connection, spec: OvertProtocol) -> Init;
        fn start_client(self) -> ClientHandshake1;
        fn start_server(self) -> ServerHandshake1;
    }

    #[state]
    pub struct ClientHandshake1 {
        pub upgen_conn: Connection,
        pub other_conn: Connection,
        pub fmt: Formatter,
        pub spec: OvertProtocol,
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
        pub upgen_conn: Connection,
        pub other_conn: Connection,
        pub fmt: Formatter,
        pub spec: OvertProtocol,
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
        pub upgen_conn: Connection,
        pub other_conn: Connection,
        pub fmt: Formatter,
        pub spec: OvertProtocol,
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
        pub upgen_conn: Connection,
        pub other_conn: Connection,
        pub fmt: Formatter,
        pub spec: OvertProtocol,
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
        pub other_conn: Connection,
        pub fmt: Formatter,
        pub spec: OvertProtocol,
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
        pub upgen_conn: Connection,
        pub other_conn: Connection,
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

    impl From<upgen::Error> for UpgenProtocol<Error> {
        fn from(error: upgen::Error) -> Self {
            UpgenProtocol::<Error> { state: Error { error } }
        }
    }

    impl From<Init> for ClientHandshake1 {
        fn from(prev: Init) -> Self {
            ClientHandshake1 {
                upgen_conn: prev.upgen_conn,
                other_conn: prev.other_conn,
                fmt: prev.fmt,
                spec: prev.spec,
            }
        }
    }

    impl From<UpgenProtocol<Init>> for UpgenProtocol<ClientHandshake1> {
        fn from(prev: UpgenProtocol<Init>) -> Self {
            UpgenProtocol::<ClientHandshake1> {
                state: prev.state.into()
            }
        }
    }

    impl From<ClientHandshake1> for ClientHandshake2 {
        fn from(prev: ClientHandshake1) -> Self {
            ClientHandshake2 {
                upgen_conn: prev.upgen_conn,
                other_conn: prev.other_conn,
                fmt: prev.fmt,
                spec: prev.spec,
            }
        }
    }

    impl From<UpgenProtocol<ClientHandshake1>> for UpgenProtocol<ClientHandshake2> {
        fn from(prev: UpgenProtocol<ClientHandshake1>) -> Self {
            UpgenProtocol::<ClientHandshake2> {
                state: prev.state.into()
            }
        }
    }

    impl From<ClientHandshake2> for Data {
        fn from(prev: ClientHandshake2) -> Self {
            Data {
                upgen_conn: prev.upgen_conn,
                other_conn: prev.other_conn,
                fmt: prev.fmt,
                spec: prev.spec,
            }
        }
    }

    impl From<UpgenProtocol<ClientHandshake2>> for UpgenProtocol<Data> {
        fn from(prev: UpgenProtocol<ClientHandshake2>) -> Self {
            UpgenProtocol::<Data> {
                state: prev.state.into()
            }
        }
    }

    impl From<Init> for ServerHandshake1 {
        fn from(prev: Init) -> Self {
            ServerHandshake1 {
                upgen_conn: prev.upgen_conn,
                other_conn: prev.other_conn,
                fmt: prev.fmt,
                spec: prev.spec,
            }
        }
    }

    impl From<UpgenProtocol<Init>> for UpgenProtocol<ServerHandshake1> {
        fn from(prev: UpgenProtocol<Init>) -> Self {
            UpgenProtocol::<ServerHandshake1> {
                state: prev.state.into()
            }
        }
    }

    impl From<ServerHandshake1> for ServerHandshake2 {
        fn from(prev: ServerHandshake1) -> Self {
            ServerHandshake2 {
                upgen_conn: prev.upgen_conn,
                other_conn: prev.other_conn,
                fmt: prev.fmt,
                spec: prev.spec,
            }
        }
    }

    impl From<UpgenProtocol<ServerHandshake1>> for UpgenProtocol<ServerHandshake2> {
        fn from(prev: UpgenProtocol<ServerHandshake1>) -> Self {
            UpgenProtocol::<ServerHandshake2> {
                state: prev.state.into()
            }
        }
    }

    impl From<ServerHandshake2> for Data {
        fn from(prev: ServerHandshake2) -> Self {
            Data {
                upgen_conn: prev.upgen_conn,
                other_conn: prev.other_conn,
                fmt: prev.fmt,
                spec: prev.spec,
            }
        }
    }

    impl From<UpgenProtocol<ServerHandshake2>> for UpgenProtocol<Data> {
        fn from(prev: UpgenProtocol<ServerHandshake2>) -> Self {
            UpgenProtocol::<Data> {
                state: prev.state.into()
            }
        }
    }

    impl From<Data> for Success {
        fn from(prev: Data) -> Self {
            Success {
                upgen_conn: prev.upgen_conn,
                other_conn: prev.other_conn,
            }
        }
    }

    impl From<UpgenProtocol<Data>> for UpgenProtocol<Success> {
        fn from(prev: UpgenProtocol<Data>) -> Self {
            UpgenProtocol::<Success> {
                state: prev.state.into()
            }
        }
    }
}
