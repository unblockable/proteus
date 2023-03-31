use typestate::typestate;

#[typestate]
pub mod proteus {
    use crate::lang::spec::proteus::ProteusSpec;
    use crate::net::proto::proteus;
    use crate::net::Connection;

    use async_trait::async_trait;

    #[automaton]
    pub struct ProteusProtocol;

    #[state]
    pub struct Init {
        pub app_conn: Connection,
        pub net_conn: Connection,
        pub spec: ProteusSpec,
    }
    #[async_trait]
    pub trait Init {
        fn new(app_conn: Connection, net_conn: Connection, spec: ProteusSpec) -> Init;
        async fn run(self) -> RunResult;
    }
    pub enum RunResult {
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
        pub error: proteus::Error,
    }
    pub trait Error {
        fn finish(self) -> proteus::Error;
    }

    impl From<Init> for ProteusProtocol<Init> {
        fn from(state: Init) -> Self {
            ProteusProtocol::<Init> { state: state }
        }
    }

    impl From<Success> for ProteusProtocol<Success> {
        fn from(state: Success) -> Self {
            ProteusProtocol::<Success> { state: state }
        }
    }

    impl From<proteus::Error> for ProteusProtocol<Error> {
        fn from(error: proteus::Error) -> Self {
            ProteusProtocol::<Error> {
                state: Error { error },
            }
        }
    }
}
