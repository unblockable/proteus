use async_trait::async_trait;

use crate::{
    lang::{
        command::{NetCommandIn, NetCommandOut},
        interpreter::{Interpreter, SharedInterpreter},
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

impl InitState for ProteusProtocol<Init> {
    fn new(app_conn: Connection, net_conn: Connection, spec: ProteusSpec) -> ProteusProtocol<Init> {
        Init {
            app_conn,
            net_conn,
            int: Interpreter::new(spec),
        }
        .into()
    }

    fn start(self) -> ProteusProtocol<Run> {
        Run {
            app_conn: self.state.app_conn,
            net_conn: self.state.net_conn,
            int: self.state.int,
        }
        .into()
    }
}

#[async_trait]
impl RunState for ProteusProtocol<Run> {
    async fn run(mut self) -> RunResult {
        // Get the source and sink ends so we can forward data in both
        // directions concurrently.
        let (net_source, net_sink) = self.state.net_conn.into_split();
        let (app_source, app_sink) = self.state.app_conn.into_split();

        let mut shared_int1 = SharedInterpreter::new(self.state.int);
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
    shared_int: &mut SharedInterpreter,
) -> Result<(usize, usize), proteus::Error> {
    let mut total_num_read: usize = 0;
    let mut total_num_written: usize = 0;

    loop {
        // TODO: refactor the read/write here and in deobfuscate are identical.
        match shared_int.next_net_cmd_out().await {
            NetCommandOut::ReadApp(args) => {
                let mut fmt = Formatter::new(args.read_len);

                let net_data = match source.read_frame(&mut fmt).await {
                    Ok(data) => data,
                    Err(net_err) => match net_err {
                        net::Error::Eof => break,
                        _ => return Err(proteus::Error::from(net_err)),
                    },
                };

                total_num_read += net_data.len();
                log::trace!("obfuscate: read {} app bytes", net_data.len());

                shared_int.store(args.store_addr, net_data.into());
            }
            NetCommandOut::WriteNet(args) => {
                let num_written = match sink.write_bytes(&args.bytes).await {
                    Ok(num) => num,
                    Err(e) => return Err(proteus::Error::from(e)),
                };

                total_num_written += num_written;
                log::trace!("obfuscate: wrote {} net bytes", num_written);
            }
            NetCommandOut::Close => {
                break;
            }
        };
    }

    log::info!(
        "obfuscate: done! read {} total bytes, wrote {} total bytes",
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
    shared_int: &mut SharedInterpreter,
) -> Result<(usize, usize), proteus::Error> {
    let mut total_num_read: usize = 0;
    let mut total_num_written: usize = 0;

    loop {
        match shared_int.next_net_cmd_in().await {
            NetCommandIn::ReadNet(args) => {
                let mut fmt = Formatter::new(args.read_len);

                let net_data = match source.read_frame(&mut fmt).await {
                    Ok(data) => data,
                    Err(net_err) => match net_err {
                        net::Error::Eof => break,
                        _ => return Err(proteus::Error::from(net_err)),
                    },
                };

                total_num_read += net_data.len();
                log::trace!("deobfuscate: read {} net bytes", net_data.len());

                shared_int.store(args.store_addr, net_data.into());
            }
            NetCommandIn::WriteApp(args) => {
                let num_written = match sink.write_bytes(&args.bytes).await {
                    Ok(num) => num,
                    Err(e) => return Err(proteus::Error::from(e)),
                };

                total_num_written += num_written;
                log::trace!("deobfuscate: wrote {} app bytes", num_written);
            }
            NetCommandIn::Close => {
                break;
            }
        };
    }

    log::info!(
        "deobfuscate: done! read {} total bytes, wrote {} total bytes",
        total_num_read,
        total_num_written
    );
    Ok((total_num_read, total_num_written))
}
