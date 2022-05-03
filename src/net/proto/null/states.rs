use async_trait::async_trait;
use tokio::io::AsyncWriteExt;

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

        // Try to turn the connections back into streams.
        let (mut pt_stream, pt_bytes) = match self.state.pt_conn.into_stream() {
            Ok(s) => s,
            Err(_) => {
                let next = Error { error: null::Error::Copy };
                return DataResult::Error(next.into());
            }
        };
        let (mut tor_stream, tor_bytes) = match self.state.tor_conn.into_stream() {
            Ok(s) => s,
            Err(_) => {
                let next = Error { error: null::Error::Copy };
                return DataResult::Error(next.into());
            }
        };

        // Now write the leftover bytes.
        if pt_bytes.len() > 0 {
            if tor_stream.write_all(&pt_bytes).await.is_err() {
                let next = Error { error: null::Error::Copy };
                return DataResult::Error(next.into());
            }
        }
        if tor_bytes.len() > 0 {
            if pt_stream.write_all(&tor_bytes).await.is_err() {
                let next = Error { error: null::Error::Copy };
                return DataResult::Error(next.into());
            }
        }

        // Let tokio handle the remaining copying.
        match tokio::io::copy_bidirectional(&mut pt_stream, &mut tor_stream).await {
            Ok(_) => {
                let next = Success {};
                DataResult::Success(next.into())
            }
            Err(_) => {
                let error = null::Error::Copy;
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
