use typestate::typestate;

#[typestate]
pub mod proteus {
    use crate::lang::{ProteusSpecification, Role};
    use crate::net::proto::proteus;
    use crate::net::proto::proteus::formatter::Formatter;
    use crate::net::Connection;

    use async_trait::async_trait;

    #[automaton]
    pub struct ProteusProtocol;

    #[state]
    pub struct Init {
        pub app_conn: Connection,
        pub proteus_conn: Connection,
        pub spec: ProteusSpecification,
        pub fmt: Formatter,
    }
    pub trait Init {
        fn new(
            app_conn: Connection,
            proteus_conn: Connection,
            spec: ProteusSpecification,
            fmt: Formatter,
        ) -> Init;
        fn start(self, role: Role) -> Action;
    }

    #[state]
    pub struct Action {
        pub app_conn: Connection,
        pub proteus_conn: Connection,
        pub spec: ProteusSpecification,
        pub fmt: Formatter,
        pub role: Role,
    }
    #[async_trait]
    pub trait Action {
        async fn run(self) -> ActionResult;
    }
    pub enum ActionResult {
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

    impl From<Action> for ProteusProtocol<Action> {
        fn from(state: Action) -> Self {
            ProteusProtocol::<Action> { state: state }
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
