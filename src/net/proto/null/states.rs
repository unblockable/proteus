use async_trait::async_trait;

use crate::net::{
    proto::null::{
        self,
        spec::{self, null::*},
    },
    Connection,
};

impl InitState for NullProtocol<Init> {
    fn new(
        client_conn: Connection,
        server_conn: Connection,
    ) -> NullProtocol<Init> {
        Init {
            client_conn,
            server_conn,
        }
        .into()
    }

    fn start_client(self) -> NullProtocol<Data> {
        Data {
            tor_conn: self.state.client_conn,
            pt_conn: self.state.server_conn,
        }
        .into()
    }

    fn start_server(self) -> NullProtocol<Data> {
        Data {
            pt_conn: self.state.client_conn,
            tor_conn: self.state.server_conn,
        }
        .into()
    }
}

#[async_trait]
impl DataState for NullProtocol<Data> {
    async fn forward_data(mut self) -> DataResult {
        // read from tor -> write to pt; read from pt -> write to tor
        match self.state.tor_conn.splice_until_eof(&mut self.state.pt_conn).await {
            Ok(()) => {
                let next = Success {};
                DataResult::Success(next.into())
            }
            Err(net_err) => {
                let error = null::Error::from(net_err);
                let next = Error { error };
                DataResult::Error(next.into())
            }
        }
    }
}

impl SuccessState for NullProtocol<Success> {
    fn finish(self) {
        log::debug!("Null protocol completed successfully");
    }
}

impl ErrorState for NullProtocol<spec::null::Error> {
    fn finish(self) -> null::Error {
        self.state.error
    }
}
