use typestate::typestate;

#[typestate]
pub mod proteus {
    use crate::lang::interpreter::Interpreter;
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
        pub int: Interpreter,
    }
    pub trait Init {
        fn new(app_conn: Connection, net_conn: Connection, spec: ProteusSpec) -> Init;
        fn start(self) -> Run;
    }

    #[state]
    pub struct Run {
        pub app_conn: Connection,
        pub net_conn: Connection,
        pub int: Interpreter,
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

    impl From<proteus::Error> for ProteusProtocol<Error> {
        fn from(error: proteus::Error) -> Self {
            ProteusProtocol::<Error> {
                state: Error { error },
            }
        }
    }
}
