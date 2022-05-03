use async_trait::async_trait;
use bytes::{Buf, Bytes};

use crate::net::{
    self,
    proto::upgen::{
        self,
        formatter::Formatter,
        frames::CovertPayload,
        protocols::*,
        spec::{self, upgen::*},
    },
    Connection, NetSink, NetSource,
};

impl InitState for UpgenProtocol<Init> {
    fn new(
        upgen_conn: Connection,
        other_conn: Connection,
        spec: OvertProtocol,
    ) -> UpgenProtocol<Init> {
        UpgenProtocol::<Init> {
            state: Init {
                upgen_conn,
                other_conn,
                fmt: Formatter::new(),
                spec,
            },
        }
    }

    fn start_client(self) -> UpgenProtocol<ClientHandshake1> {
        self.into()
    }

    fn start_server(self) -> UpgenProtocol<ServerHandshake1> {
        self.into()
    }
}

#[async_trait]
impl ClientHandshake1State for UpgenProtocol<ClientHandshake1> {
    async fn send_handshake1(mut self) -> ClientHandshake1Result {
        // Get frame spec we want to send at this phase.
        let frame_spec = match &self.state.spec {
            OvertProtocol::OneRtt(p) => p.get_frame_spec(onertt::ProtocolPhase::Handshake1),
        };

        // Tell the serializer the frame spec it should follow.
        self.state.fmt.set_frame_spec(frame_spec);

        // We don't send any payload yet, but we want a handshake message.
        let msg = CovertPayload { data: Bytes::new() };

        match self
            .state
            .upgen_conn
            .write_frame(&self.state.fmt, msg)
            .await
        {
            Ok(_) => ClientHandshake1Result::ClientHandshake2(self.into()),
            Err(net_err) => ClientHandshake1Result::Error(upgen::Error::from(net_err).into()),
        }
    }
}

#[async_trait]
impl ClientHandshake2State for UpgenProtocol<ClientHandshake2> {
    async fn recv_handshake2(mut self) -> ClientHandshake2Result {
        // Get frame spec we expect to receive at this phase.
        let frame_spec = match &self.state.spec {
            OvertProtocol::OneRtt(p) => p.get_frame_spec(onertt::ProtocolPhase::Handshake2),
        };

        // Tell the deserializer the frame spec it should expect.
        self.state.fmt.set_frame_spec(frame_spec);

        match self.state.upgen_conn.read_frame(&self.state.fmt).await {
            Ok(_) => ClientHandshake2Result::Data(self.into()),
            Err(net_err) => ClientHandshake2Result::Error(upgen::Error::from(net_err).into()),
        }
    }
}

#[async_trait]
impl ServerHandshake1State for UpgenProtocol<ServerHandshake1> {
    async fn recv_handshake1(mut self) -> ServerHandshake1Result {
        // Get frame spec we expect to receive at this phase.
        let frame_spec = match &self.state.spec {
            OvertProtocol::OneRtt(p) => p.get_frame_spec(onertt::ProtocolPhase::Handshake1),
        };

        // Tell the deserializer the frame spec it should expect.
        self.state.fmt.set_frame_spec(frame_spec);

        match self.state.upgen_conn.read_frame(&self.state.fmt).await {
            Ok(_) => ServerHandshake1Result::ServerHandshake2(self.into()),
            Err(net_err) => ServerHandshake1Result::Error(upgen::Error::from(net_err).into()),
        }
    }
}

#[async_trait]
impl ServerHandshake2State for UpgenProtocol<ServerHandshake2> {
    async fn send_handshake2(mut self) -> ServerHandshake2Result {
        // Get frame spec we expect to receive at this phase.
        let frame_spec = match &self.state.spec {
            OvertProtocol::OneRtt(p) => p.get_frame_spec(onertt::ProtocolPhase::Handshake2),
        };

        // Tell the deserializer the frame spec it should expect.
        self.state.fmt.set_frame_spec(frame_spec);

        // We don't send any payload yet, but we want a handshake message.
        let msg = CovertPayload { data: Bytes::new() };

        match self
            .state
            .upgen_conn
            .write_frame(&self.state.fmt, msg)
            .await
        {
            Ok(_) => ServerHandshake2Result::Data(self.into()),
            Err(net_err) => ServerHandshake2Result::Error(upgen::Error::from(net_err).into()),
        }
    }
}

/// Reads a covert data stream from `source`, obfuscates it, and writes the
/// upgen protocol frames to `sink`.
///
/// Returns a tuple of the total number of bytes read from the source and
/// written to the sink as `(read, written)`.
///
/// Upon return, the `source` and `sink` references will be dropped and shutdown
/// will be called on the `sink` indicating no more data will be written to it.
async fn obfuscate(
    mut source: NetSource,
    mut sink: NetSink,
    fmt: &Formatter,
) -> Result<(usize, usize), upgen::Error> {
    let mut total_num_read: usize = 0;
    let mut total_num_written: usize = 0;

    loop {
        // Read the raw covert data stream.
        let bytes = match source.read_bytes().await {
            Ok(b) => b,
            Err(net_err) => match net_err {
                net::Error::Eof => break,
                _ => return Err(upgen::Error::from(net_err)),
            },
        };

        total_num_read += bytes.len();
        log::trace!("obfuscate: read {} app bytes", bytes.len());

        // If we have data, write the overt frames using the upgen formatter.
        if bytes.has_remaining() {
            let payload = CovertPayload { data: bytes };

            let num_written = match sink.write_frame(fmt, payload).await {
                Ok(num) => num,
                Err(e) => return Err(upgen::Error::from(e)),
            };

            total_num_written += num_written;
            log::trace!("obfuscate: wrote {} covert bytes", num_written);
        }

    }

    log::info!(
        "obfuscate: done! read {} total bytes, wrote {} total bytes",
        total_num_read,
        total_num_written
    );
    Ok((total_num_read, total_num_written))
}

/// Reads upgen protocol frames from `source`, deobfuscates them, and writes
/// covert data to `sink`.
///
/// Returns a tuple of the total number of bytes read from the source and
/// written to the sink as `(read, written)`.
///
/// Upon return, the `source` and `sink` references will be dropped and shutdown
/// will be called on the `sink` indicating no more data will be written to it.
async fn deobfuscate(
    mut source: NetSource,
    mut sink: NetSink,
    fmt: &Formatter,
) -> Result<(usize, usize), upgen::Error> {
    let mut total_num_read: usize = 0;
    let mut total_num_written: usize = 0;

    loop {
        // Read overt frames using the upgen formatter.
        let payload = match source.read_frame(fmt).await {
            Ok(frame) => frame,
            Err(net_err) => match net_err {
                net::Error::Eof => break,
                _ => return Err(upgen::Error::from(net_err)),
            },
        };

        total_num_read += payload.data.len();
        log::trace!("deobfuscate: read {} covert bytes", payload.data.len());

        // If we got a covert payload, write the raw data.
        if payload.data.has_remaining() {
            let num_written = match sink.write_bytes(&payload.data).await {
                Ok(num) => num,
                Err(e) => return Err(upgen::Error::from(e)),
            };

            total_num_written += num_written;
            log::trace!("deobfuscate: wrote {} app bytes", num_written);
        }
    }

    log::info!(
        "deobfuscate: done! read {} total bytes, wrote {} total bytes",
        total_num_read,
        total_num_written
    );
    Ok((total_num_read, total_num_written))
}

#[async_trait]
impl DataState for UpgenProtocol<Data> {
    async fn forward_data(mut self) -> DataResult {
        // read from tor, encrypt, send to upgen
        // read from upgen, decrypt, send to tor

        // Get frame spec we expect to receive at this phase.
        let frame_spec = match &self.state.spec {
            OvertProtocol::OneRtt(p) => p.get_frame_spec(onertt::ProtocolPhase::Data),
        };

        // Tell the formatter the frame specs to use during the data phase.
        self.state.fmt.set_frame_spec(frame_spec);

        // Get the source and sink ends so we can forward data concurrently.
        let (upgen_source, upgen_sink) = self.state.upgen_conn.into_split();
        let (other_source, other_sink) = self.state.other_conn.into_split();

        // TODO: docs for try_join! shows how to do these concurrently AND in parallel,
        // but that requires that we can send the objects across threads.
        // let handle1 = tokio::spawn(obfuscate(other_source, upgen_sink));
        // let handle2 = tokio::spawn(deobfuscate(upgen_source, other_sink));

        match tokio::try_join!(
            obfuscate(other_source, upgen_sink, &self.state.fmt),
            deobfuscate(upgen_source, other_sink, &self.state.fmt),
        ) {
            Ok(_) => DataResult::Success(Success {}.into()),
            Err(e) => DataResult::Error(e.into()),
        }
    }
}

impl SuccessState for UpgenProtocol<Success> {
    fn finish(self) {
        log::debug!("UPGen protocol completed successfully");
    }
}

impl ErrorState for UpgenProtocol<spec::upgen::Error> {
    fn finish(self) -> upgen::Error {
        self.state.error
    }
}
