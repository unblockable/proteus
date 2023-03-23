use async_trait::async_trait;
use tokio::io::AsyncWriteExt;

use crate::{
    lang::spec::ProteusSpecification,
    net::{
        proto::proteus::{
            self,
            formatter::Formatter,
            spec::{self, proteus::*},
        },
        Connection,
    },
};

impl InitState for ProteusProtocol<Init> {
    fn new(
        app_conn: Connection,
        net_conn: Connection,
        spec: ProteusSpecification,
    ) -> ProteusProtocol<Init> {
        Init {
            app_conn,
            net_conn,
            spec,
            fmt: Formatter::new(),
        }
        .into()
    }

    fn start(self) -> ProteusProtocol<Run> {
        Run {
            app_conn: self.state.app_conn,
            net_conn: self.state.net_conn,
            spec: self.state.spec,
            fmt: self.state.fmt,
        }
        .into()
    }
}

#[async_trait]
impl RunState for ProteusProtocol<Run> {
    async fn run(mut self) -> RunResult {
        // Read from self.state.app_conn -> write to self.state.proteus_conn.
        // Read from self.state.proteus_conn -> write to self.state.app_conn.

        // // Get the source and sink ends so we can forward data concurrently.
        // let (mut proteus_source, mut proteus_sink) = proteus_conn.into_split();
        // let (mut other_source, mut other_sink) = other_conn.into_split();

        // loop {
        //     let Run = proto.get_next_Run();

        //     match Run.get_kind(role.clone()) {
        //         RunKind::Send => {
        //             // Read the raw covert data stream.
        //             let bytes = match other_source.read_bytes().await {
        //                 Ok(b) => b,
        //                 Err(net_err) => match net_err {
        //                     net::Error::Eof => break,
        //                     _ => return Err(proteus::Error::from(net_err)),
        //                 },
        //             };

        //             log::trace!("obfuscate: read {} app bytes", bytes.len());

        //             if bytes.has_remaining() {
        //                 let payload = CovertPayload { data: bytes };
        //                 let message = proto.pack_message(payload);

        //                 let num_written = match proteus_sink.write_frame(&mut formatter, message).await {
        //                     Ok(num) => num,
        //                     Err(e) => return Err(proteus::Error::from(e)),
        //                 };

        //                 log::trace!("obfuscate: wrote {} covert bytes", num_written);
        //             }
        //         },
        //         RunKind::Receive => {
        //             todo!()
        //         }
        //     }
        // }

        todo!()
    }
}

impl SuccessState for ProteusProtocol<Success> {
    fn finish(self) {
        log::debug!("Proteus protocol completed successfully");
    }
}

impl ErrorState for ProteusProtocol<spec::proteus::Error> {
    fn finish(self) -> proteus::Error {
        self.state.error
    }
}
