use typestate::typestate;

#[typestate]
pub mod null {
    use crate::net::proto::null;
    use crate::net::Connection;

    use async_trait::async_trait;

    #[automaton]
    pub struct NullProtocol;

    #[state]
    pub struct Init {
        pub client_conn: Connection,
        pub server_conn: Connection,
    }
    pub trait Init {
        fn new(client_conn: Connection, server_conn: Connection) -> Init;
        fn start_client(self) -> Data;
        fn start_server(self) -> Data;
    }

    #[state]
    pub struct Data {
        pub pt_conn: Connection,
        pub tor_conn: Connection,
    }
    #[async_trait]
    pub trait Data {
        async fn forward_data(self) -> DataResult;
    }
    pub enum DataResult {
        Success,
        Error,
    }

    #[state]
    pub struct Success {}
    pub trait Success {
        fn finish(self);
    }

    #[state]
    pub struct Error {
        pub error: null::Error,
    }
    pub trait Error {
        fn finish(self) -> null::Error;
    }

    impl From<Init> for NullProtocol<Init> {
        fn from(state: Init) -> Self {
            NullProtocol::<Init> { state: state }
        }
    }

    impl From<Data> for NullProtocol<Data> {
        fn from(state: Data) -> Self {
            NullProtocol::<Data> { state: state }
        }
    }

    impl From<Success> for NullProtocol<Success> {
        fn from(state: Success) -> Self {
            NullProtocol::<Success> { state: state }
        }
    }

    impl From<Error> for NullProtocol<Error> {
        fn from(state: Error) -> Self {
            NullProtocol::<Error> { state: state }
        }
    }
}
