use std::net::SocketAddr;
use std::time::Duration;

use supertunnel::proto::*;
use supertunnel::util::{DecodedSinkWriter, EncodedStreamReader};
use supertunnel::{FrameSize, Protocol};

use crate::lang::interpreter::Interpreter;
use crate::lang::ir::bridge::TaskProvider;
use crate::net::{NetPayload, NetPayloadFactory, NetStream};
use crate::{lang, net};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("PSF file path is unspecified")]
    PsfOptionMissing,
    #[error("Compilation of the PSF file failed")]
    PsfCompileFailed,
    #[error("Extended OR protocol was configured but is not supported")]
    ExtOrNotSupported,
    #[error("Bind failed with I/O error: {0}")]
    BindFailed(std::io::Error),
    #[error("Connect failed with I/O error: {0}")]
    ConnectFailed(std::io::Error),
    #[error("Interpreter failed: {0}")]
    InterpreterFailed(#[from] lang::interpreter::Error),
}

pub async fn connect<S: NetStream + Send>(target: SocketAddr) -> Result<S, Error> {
    net::connect_timeout(target, Duration::from_secs(15))
        .await
        .map_err(Error::ConnectFailed)
}

pub async fn drive_io_direct<S, T>(
    inbound: S,
    outbound: S,
    proto: T,
) -> Result<(usize, usize), Error>
where
    S: NetStream,
    T: TaskProvider + Clone + Send,
{
    log::debug!("Forwarding without extensions");

    let (net_src, net_dst) = inbound.into_split();
    let (app_src, app_dst) = outbound.into_split();

    let mut interpreter = Interpreter::new(app_src, app_dst, net_src, net_dst, proto);
    let result = interpreter.run_try_join().await;
    result.map_err(Error::InterpreterFailed)
}

pub async fn drive_io_resumable<S, T>(
    inbound: S,
    outbound: S,
    proto: T,
    map: ResumptionMap<Reliability<NetPayload<S>>>,
) -> Result<(usize, usize), Error>
where
    S: NetStream,
    T: TaskProvider + Clone + Send,
{
    log::debug!("Forwarding with extensions: framing, resumption, reliability");

    // The inbound is the net connection to the client, the outbound is the
    // connection to Tor or other forwarding proxy.
    let (net_src, net_dst) = inbound.into_split();
    let (app_src, app_dst) = outbound.into_split();

    // Use a super tunnel stack of extensions.
    type Stack<S> = Framing<ResumptionServer<Reliability<NetPayload<S>>>>;

    // Compute the max payload chunk size for our stack.
    let stack_mss = PayloadCodec::mss(ReliabilityCodec::mss(ResumptionCodec::mss(
        FramingCodec::mss(FramingCodec::mtu()),
    )));

    let payload = Payload::new(0, app_src, app_dst, stack_mss);
    let rely = Reliability::new(payload);
    let handle = rely.handle().clone();
    let resume = ResumptionServer::new_with(rely, map);
    let stack = Framing::new(resume);

    let (stream, sink) = stack.into_split();
    let stack_reader = EncodedStreamReader::<Stack<S>>::new(stream, Stack::<S>::codec());
    let stack_writer = DecodedSinkWriter::<Stack<S>>::new(sink, Stack::<S>::codec());

    // Run the interpreter to forward data.
    let mut interpreter = Interpreter::new(stack_reader, stack_writer, net_src, net_dst, proto);

    // In case of failure, the client might want to resume on a new connection.
    // Super Tunnel handles this through the ResumptionMap, so we can safely
    // drop the io components and return to clean up this spawned task.
    let result = interpreter.run_try_join_with(handle).await;
    result.map_err(Error::InterpreterFailed)
}

pub async fn drive_io_routable<S, T, F>(inbound: S, proto: T) -> Result<(usize, usize), Error>
where
    S: NetStream,
    T: TaskProvider + Clone + Send,
    F: NetPayloadFactory<Stream = S>,
{
    log::debug!("Forwarding with extensions: framing, routing");

    // The inbound is the net connection to the client. The client will request
    // us to make outgoing connections, which is handled by the RoutingServer.
    let (net_src, net_dst) = inbound.into_split();

    // Use a super tunnel stack of extensions.
    type Stack<S, F> = Framing<RoutingServer<NetPayload<S>, F>>;

    // Compute the max payload chunk size for our stack.
    let stack_mss = PayloadCodec::mss(RoutingCodec::mss(FramingCodec::mss(FramingCodec::mtu())));

    // The router server connects new applications using tcp.
    let connector = NetPayloadFactory::new(stack_mss);
    let router = RoutingServer::new(connector).await;
    let stack = Framing::new(router);

    let (stream, sink) = stack.into_split();
    let stack_reader = EncodedStreamReader::<Stack<S, F>>::new(stream, Stack::<S, F>::codec());
    let stack_writer = DecodedSinkWriter::<Stack<S, F>>::new(sink, Stack::<S, F>::codec());

    let mut interpreter = Interpreter::new(stack_reader, stack_writer, net_src, net_dst, proto);
    let result = interpreter.run_try_join().await;
    result.map_err(Error::InterpreterFailed)
}

pub async fn drive_io_routable_resumable<S, T, F>(
    inbound: S,
    proto: T,
    map: ResumptionMap<Reliability<RoutingServer<NetPayload<S>, F>>>,
) -> Result<(usize, usize), Error>
where
    S: NetStream,
    T: TaskProvider + Clone + Send,
    F: NetPayloadFactory<Stream = S>,
{
    log::debug!("Forwarding with extensions: framing, resumption, reliability, routing");

    // The inbound is the net connection to the client. The client will request
    // us to make outgoing connections, which is handled by the RoutingServer.
    let (net_src, net_dst) = inbound.into_split();

    // Use a super tunnel stack of extensions.
    type Stack<S, F> = Framing<ResumptionServer<Reliability<RoutingServer<NetPayload<S>, F>>>>;

    // Compute the max payload chunk size for our stack.
    let stack_mss = PayloadCodec::mss(RoutingCodec::mss(ReliabilityCodec::mss(
        ResumptionCodec::mss(FramingCodec::mss(FramingCodec::mtu())),
    )));

    // The router server connects new applications using tcp.
    let connector = NetPayloadFactory::new(stack_mss);
    let router = RoutingServer::new(connector).await;
    let rely = Reliability::new(router);
    let rely_handle = rely.handle().clone();
    let resume = ResumptionServer::new_with(rely, map);
    let stack = Framing::new(resume);

    let (stream, sink) = stack.into_split();
    let stack_reader = EncodedStreamReader::<Stack<S, F>>::new(stream, Stack::<S, F>::codec());
    let stack_writer = DecodedSinkWriter::<Stack<S, F>>::new(sink, Stack::<S, F>::codec());

    let mut interpreter = Interpreter::new(stack_reader, stack_writer, net_src, net_dst, proto);
    let result = interpreter.run_try_join_with(rely_handle).await;
    result.map_err(Error::InterpreterFailed)
}
