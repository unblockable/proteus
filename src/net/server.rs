use std::net::SocketAddr;
use std::time::Duration;

use crate::lang;
use crate::lang::interpreter::{ErrorHandler, Interpreter};
use crate::net::common as net;

use supertunnel::{FrameSize, Protocol};
use supertunnel::proto::*;
use supertunnel::util::{DecodedSinkWriter, EncodedStreamReader};
use tokio::net::TcpStream;

use crate::lang::ir::bridge::TaskProvider;

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

pub async fn connect(target: SocketAddr) -> Result<TcpStream, Error> {
    net::connect_timeout(target, Duration::from_secs(15))
        .await
        .map_err(Error::ConnectFailed)
}

pub async fn drive_io_direct<T>(
    inbound: TcpStream,
    outbound: TcpStream,
    proto: T,
) -> Result<(usize, usize), Error>
where
    T: TaskProvider + Clone + Send,
{
    log::debug!("Forwarding without extensions");

    let (net_src, net_dst) = inbound.into_split();
    let (app_src, app_dst) = outbound.into_split();

    let mut interpreter = Interpreter::new(app_src, app_dst, net_src, net_dst, proto);
    let handler = ErrorHandler::builder()
        .shutdown_app_on_net_eof()
        .shutdown_net_on_app_eof();

    let result = interpreter.run_try_join(handler).await;
    result.map_err(Error::InterpreterFailed)
}

pub async fn drive_io_resumable<T>(
    inbound: TcpStream,
    outbound: TcpStream,
    proto: T,
    map: ResumptionMap<Reliability<TcpPayload>>,
) -> Result<(usize, usize), Error>
where
    T: TaskProvider + Clone + Send,
{
    log::debug!("Forwarding with extensions: framing, resumption, reliability");

    // The inbound is the net connection to the client, the outbound is the
    // connection to Tor or other forwarding proxy.
    let (net_src, net_dst) = inbound.into_split();
    let (app_src, app_dst) = outbound.into_split();

    // Use a super tunnel stack of extensions.
    type Stack = Framing<ResumptionServer<Reliability<TcpPayload>>>;

    // Compute the max payload chunk size for our stack.
    let stack_mss = PayloadCodec::mss(ReliabilityCodec::mss(ResumptionCodec::mss(
        FramingCodec::mss(FramingCodec::mtu()),
    )));

    let payload = Payload::new(0, app_src, app_dst, stack_mss);
    let rely = Reliability::new(payload);
    let mut rely_handle = rely.handle().clone();
    let resume = ResumptionServer::new_with(rely, map);
    let stack = Framing::new(resume);

    let (stream, sink) = stack.into_split();
    let stack_reader = EncodedStreamReader::<Stack>::new(stream, Stack::codec());
    let stack_writer = DecodedSinkWriter::<Stack>::new(sink, Stack::codec());

    // Run the interpreter to forward data.
    let mut interpreter = Interpreter::new(stack_reader, stack_writer, net_src, net_dst, proto);
    let handler = ErrorHandler::builder()
        .raise_err_on_net_eof()
        .raise_err_on_app_eof();

    // In case of failure, the client might want to resume on a new connection.
    // Super Tunnel handles this through the ResumptionMap, so we can safely
    // drop the io components and return to clean up this spawned task.
    match interpreter.run_try_join(handler).await {
        Ok(r) => Ok(r),
        Err(e) => {
            if rely_handle.is_shutdown_complete().await {
                Ok(interpreter.num_bytes_sent())
            } else {
                Err(Error::InterpreterFailed(e))
            }
        }
    }
}

pub async fn drive_io_routable<T>(inbound: TcpStream, proto: T) -> Result<(usize, usize), Error>
where
    T: TaskProvider + Clone + Send,
{
    log::debug!("Forwarding with extensions: framing, routing");

    // The inbound is the net connection to the client. The client will request
    // us to make outgoing connections, which is handled by the RoutingServer.
    let (net_src, net_dst) = inbound.into_split();

    // Use a super tunnel stack of extensions.
    type Stack = Framing<RoutingServer<TcpPayload, TcpPayloadFactory>>;

    // Compute the max payload chunk size for our stack.
    let stack_mss = PayloadCodec::mss(RoutingCodec::mss(FramingCodec::mss(FramingCodec::mtu())));

    // The router server connects new applications using tcp.
    let connector = TcpPayloadFactory::new(stack_mss);
    let router = RoutingServer::new(connector).await;
    let stack = Framing::new(router);

    let (stream, sink) = stack.into_split();
    let stack_reader = EncodedStreamReader::<Stack>::new(stream, Stack::codec());
    let stack_writer = DecodedSinkWriter::<Stack>::new(sink, Stack::codec());

    let mut interpreter = Interpreter::new(stack_reader, stack_writer, net_src, net_dst, proto);

    let handler = ErrorHandler::builder()
        .shutdown_app_on_net_eof()
        .shutdown_net_on_app_eof();

    let result = interpreter.run_try_join(handler).await;
    result.map_err(Error::InterpreterFailed)
}

pub async fn drive_io_routable_resumable<T>(
    inbound: TcpStream,
    proto: T,
    map: ResumptionMap<Reliability<RoutingServer<TcpPayload, TcpPayloadFactory>>>,
) -> Result<(usize, usize), Error>
where
    T: TaskProvider + Clone + Send,
{
    log::debug!("Forwarding with extensions: framing, resumption, reliability, routing");

    // The inbound is the net connection to the client. The client will request
    // us to make outgoing connections, which is handled by the RoutingServer.
    let (net_src, net_dst) = inbound.into_split();

    // Use a super tunnel stack of extensions.
    type Stack =
        Framing<ResumptionServer<Reliability<RoutingServer<TcpPayload, TcpPayloadFactory>>>>;

    // Compute the max payload chunk size for our stack.
    let stack_mss = PayloadCodec::mss(RoutingCodec::mss(ReliabilityCodec::mss(
        ResumptionCodec::mss(FramingCodec::mss(FramingCodec::mtu())),
    )));

    // The router server connects new applications using tcp.
    let connector = TcpPayloadFactory::new(stack_mss);
    let router = RoutingServer::new(connector).await;
    let rely = Reliability::new(router);
    let mut rely_handle = rely.handle().clone();
    let resume = ResumptionServer::new_with(rely, map);
    let stack = Framing::new(resume);

    let (stream, sink) = stack.into_split();
    let stack_reader = EncodedStreamReader::<Stack>::new(stream, Stack::codec());
    let stack_writer = DecodedSinkWriter::<Stack>::new(sink, Stack::codec());

    let mut interpreter = Interpreter::new(stack_reader, stack_writer, net_src, net_dst, proto);

    let handler = ErrorHandler::builder()
        .raise_err_on_net_eof()
        .raise_err_on_app_eof();

    match interpreter.run_try_join(handler).await {
        Ok(r) => Ok(r),
        Err(e) => {
            if rely_handle.is_shutdown_complete().await {
                Ok(interpreter.num_bytes_sent())
            } else {
                Err(Error::InterpreterFailed(e))
            }
        }
    }
}
