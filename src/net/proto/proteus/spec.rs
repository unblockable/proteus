use typestate::typestate;

#[typestate]
pub mod proteus {
    use crate::lang::spec::ProteusSpecification;
    use crate::net::proto::proteus;
    use crate::net::proto::proteus::formatter::Formatter;
    use crate::net::Connection;

    use async_trait::async_trait;

    #[automaton]
    pub struct ProteusProtocol;

    #[state]
    pub struct Init {
        pub app_conn: Connection,
        pub net_conn: Connection,
        pub spec: ProteusSpecification,
        pub fmt: Formatter,
    }
    pub trait Init {
        fn new(app_conn: Connection, net_conn: Connection, spec: ProteusSpecification) -> Init;
        fn start(self) -> Run;
    }

    #[state]
    pub struct Run {
        pub app_conn: Connection,
        pub net_conn: Connection,
        pub spec: ProteusSpecification,
        pub fmt: Formatter,
    }
    #[async_trait]
    pub trait Run {
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

    impl From<Run> for ProteusProtocol<Run> {
        fn from(state: Run) -> Self {
            ProteusProtocol::<Run> { state: state }
        }
    }

    impl From<Success> for ProteusProtocol<Success> {
        fn from(state: Success) -> Self {
            ProteusProtocol::<Success> { state: state }
        }
    }

    impl From<Error> for ProteusProtocol<Error> {
        fn from(state: Error) -> Self {
            ProteusProtocol::<Error> { state: state }
        }
    }
}
