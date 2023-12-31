use async_trait::async_trait;

use crate::{
    lang::{
        interpreter::{NetOpIn, NetOpOut, SharedAsyncInterpreter},
        spec::proteus::ProteusSpec,
    },
    net::{
        self,
        proto::proteus::{
            self,
            formatter::Formatter,
            spec::{self, proteus::*},
        },
        Connection, NetSink, NetSource,
    },
};

#[async_trait]
impl InitState for ProteusProtocol<Init> {
    fn new(app_conn: Connection, net_conn: Connection, spec: ProteusSpec) -> ProteusProtocol<Init> {
        Init {
            app_conn,
            net_conn,
            spec,
        }
        .into()
    }

    async fn run(mut self) -> RunResult {
        // Get the source and sink ends so we can forward data in both
        // directions concurrently.
        let (net_source, net_sink) = self.state.net_conn.into_split();
        let (app_source, app_sink) = self.state.app_conn.into_split();

        let mut shared_int1 = SharedAsyncInterpreter::new(self.state.spec);
        if let Err(e) = shared_int1.init().await {
            return RunResult::Error(proteus::Error::Protocol(e.to_string()).into());
        }
        let mut shared_int2 = shared_int1.clone();

        match tokio::try_join!(
            obfuscate(app_source, net_sink, &mut shared_int1),
            deobfuscate(net_source, app_sink, &mut shared_int2),
        ) {
            Ok(_) => RunResult::Success(Success {}.into()),
            Err(e) => RunResult::Error(e.into()),
        }
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

/// Reads a covert data stream from `source`, obfuscates it, and writes the
/// proteus data to `sink`.
///
/// Returns a tuple of the total number of bytes read from the source and
/// written to the sink as `(read, written)`.
///
/// Upon return, the `source` and `sink` references will be dropped and shutdown
/// will be called on the `sink` indicating no more data will be written to it.
async fn obfuscate(
    mut source: NetSource,
    mut sink: NetSink,
    shared_int: &mut SharedAsyncInterpreter,
) -> Result<(usize, usize), proteus::Error> {
    let mut total_num_read: usize = 0;
    let mut total_num_written: usize = 0;

    loop {
        // TODO: refactor the read/write here and in deobfuscate are identical.
        match shared_int.next_net_cmd_out().await {
            NetOpOut::RecvApp(args) => {
                log::trace!(
                    "obfuscate: trying to read frame of size {:?} from app",
                    args.len
                );
                let mut fmt = Formatter::new(args.len);

                let net_data = match source.read_frame(&mut fmt).await {
                    Ok(data) => data,
                    Err(net_err) => match net_err {
                        net::Error::Eof => break,
                        _ => return Err(proteus::Error::from(net_err)),
                    },
                };

                total_num_read += net_data.len();
                log::trace!("obfuscate: read {} app bytes", net_data.len());

                shared_int.store_out(args.addr, net_data.into()).await;
            }
            NetOpOut::SendNet(args) => {
                log::trace!("obfuscate: trying to write bytes to net");

                let num_written = match sink.write_bytes(&args.bytes).await {
                    Ok(num) => num,
                    Err(e) => return Err(proteus::Error::from(e)),
                };

                total_num_written += num_written;
                log::trace!("obfuscate: wrote {} net bytes", num_written);
            }
            NetOpOut::_Close => {
                break;
            }
            NetOpOut::Error(s) => return Err(proteus::Error::Protocol(s)),
        };
    }

    log::info!(
        "obfuscate: success! read {} total bytes, wrote {} total bytes",
        total_num_read,
        total_num_written
    );
    Ok((total_num_read, total_num_written))
}

/// Reads proteus data from `source`, deobfuscates it, and writes covert data to
/// `sink`.
///
/// Returns a tuple of the total number of bytes read from the source and
/// written to the sink as `(read, written)`.
///
/// Upon return, the `source` and `sink` references will be dropped and shutdown
/// will be called on the `sink` indicating no more data will be written to it.
async fn deobfuscate(
    mut source: NetSource,
    mut sink: NetSink,
    shared_int: &mut SharedAsyncInterpreter,
) -> Result<(usize, usize), proteus::Error> {
    let mut total_num_read: usize = 0;
    let mut total_num_written: usize = 0;

    loop {
        match shared_int.next_net_cmd_in().await {
            NetOpIn::RecvNet(args) => {
                log::trace!(
                    "deobfuscate: trying to read frame of size {:?} from app",
                    args.len
                );
                let mut fmt = Formatter::new(args.len);

                let net_data = match source.read_frame(&mut fmt).await {
                    Ok(data) => data,
                    Err(net_err) => match net_err {
                        net::Error::Eof => break,
                        _ => return Err(proteus::Error::from(net_err)),
                    },
                };

                total_num_read += net_data.len();
                log::trace!("deobfuscate: read {} net bytes", net_data.len());

                shared_int.store_in(args.addr, net_data.into()).await;
            }
            NetOpIn::SendApp(args) => {
                log::trace!("deobfuscate: trying to write bytes to app");
                let num_written = match sink.write_bytes(&args.bytes).await {
                    Ok(num) => num,
                    Err(e) => return Err(proteus::Error::from(e)),
                };

                total_num_written += num_written;
                log::trace!("deobfuscate: wrote {} app bytes", num_written);
            }
            NetOpIn::_Close => {
                break;
            }
            NetOpIn::Error(s) => return Err(proteus::Error::Protocol(s)),
        };
    }

    log::info!(
        "deobfuscate: success! read {} total bytes, wrote {} total bytes",
        total_num_read,
        total_num_written
    );
    Ok((total_num_read, total_num_written))
}
