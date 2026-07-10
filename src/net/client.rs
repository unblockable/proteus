use std::fmt::Debug;
use std::net::SocketAddr;
use std::time::Duration;

use fast_socks5::server::SocksServerError;
use fast_socks5::util::target_addr::TargetAddr;
use supertunnel::net::NetworkTarget;
use supertunnel::proto::*;
use supertunnel::util::{DecodedSinkWriter, EncodedStreamReader, EndMethod};
use supertunnel::{FrameSize, Protocol};
use tokio::net::{TcpStream, ToSocketAddrs};
use tokio_util::sync::CancellationToken;

use crate::lang;
use crate::lang::interpreter::{self, ErrorHandler, Interpreter};
use crate::lang::ir::bridge::TaskProvider;
use crate::net;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Compilation of the PSF file failed")]
    PsfCompileFailed,
    #[error("Parsing the connect address failed")]
    ConnectParseFailed,
    #[error("Internal socks server error: {0}")]
    Socks(#[from] SocksServerError),
    #[error("Socks command not supported")]
    SocksCommandNotSupported,
    #[error("Bind failed with I/O error: {0}")]
    BindFailed(std::io::Error),
    #[error("Connect failed with I/O error: {0}")]
    ConnectFailed(std::io::Error),
    #[error("Error during resumption request")]
    ResumeRequest,
    #[error("Error during resumption response")]
    ResumeResponse,
    #[error("Interpreter failed: {0}")]
    InterpreterFailed(#[from] interpreter::Error),
    #[error("Routing failed: {0:?}")]
    RoutingFailed(RoutingError),
    #[error("Router exists")]
    RouterExists,
    #[error("Router does not exist")]
    RouterDoesNotExist,
}

pub async fn connect<A>(target: A) -> Result<TcpStream, Error>
where
    A: ToSocketAddrs + Clone + Debug,
{
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
    log::debug!(
        "Transferring between {} and {} using direct i/o.",
        net::fmt_stream_name(&inbound),
        net::fmt_stream_name(&outbound)
    );

    let (app_src, app_dst) = inbound.into_split();
    let (net_src, net_dst) = outbound.into_split();

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
    reconnect_addr: TargetAddr,
) -> Result<(usize, usize), Error>
where
    T: TaskProvider + Clone + Send,
{
    log::debug!(
        "Transferring between {} and {} using resumable super tunnel.",
        net::fmt_stream_name(&inbound),
        net::fmt_stream_name(&outbound)
    );

    let (app_src, app_dst) = inbound.into_split();

    // Compute the max payload chunk size for our stack.
    let stack_mss = PayloadCodec::mss(ReliabilityCodec::mss(ResumptionCodec::mss(
        FramingCodec::mss(FramingCodec::mtu()),
    )));

    let payload = Payload::new(0, app_src, app_dst, stack_mss);
    let rely = Reliability::new(payload);
    let rely_handle = rely.handle().clone();
    let resume = ResumptionClient::new(rely);
    let resume_handle = resume.handle().clone();
    let stack = Framing::new(resume);

    reconnecting_client(
        stack,
        outbound,
        proto,
        rely_handle,
        resume_handle,
        reconnect_addr,
    )
    .await
}

async fn reconnecting_client<S, T>(
    stack: S,
    tunnel: TcpStream,
    proto: T,
    mut rely_handle: ReliabilityHandle,
    resume_handle: ResumptionHandle,
    target: TargetAddr,
) -> Result<(usize, usize), Error>
where
    S: Protocol,
    <S as Protocol>::Codec: Send,
    <S as Protocol>::Message: Send,
    <S as Protocol>::StreamHalf: Unpin + Send,
    <S as Protocol>::SinkHalf: Unpin + Send,
    T: TaskProvider + Clone + Send,
{
    let (stream, sink) = stack.into_split();
    let mut stack_reader = EncodedStreamReader::<S>::new(stream, S::codec());
    let mut stack_writer = DecodedSinkWriter::<S>::new(sink, S::codec());

    let (mut net_reader, mut net_writer) = tunnel.into_split();

    let cancel_token = CancellationToken::new();

    loop {
        let mut interpreter = Interpreter::new(
            stack_reader,
            stack_writer,
            net_reader,
            net_writer,
            proto.clone(),
        );
        let handler = ErrorHandler::builder()
            .raise_err_on_net_eof()
            .raise_err_on_app_eof();

        let result = tokio::select! {
            result = interpreter.run_try_join(handler) => {
                result
            }
            _ = cancel_token.cancelled() => {
                return Err(Error::ResumeResponse);
            }
        };

        match result {
            Ok(n) => return Ok(n),
            Err(e) => {
                if !should_try_to_recover(&e, &mut rely_handle).await {
                    return Err(Error::InterpreterFailed(e));
                }
            }
        }

        (stack_reader, stack_writer, _, _) = interpreter.into_inner();

        let reconnected_stream = TcpStream::connect(target.to_string())
            .await
            .map_err(Error::ConnectFailed)?;
        (net_reader, net_writer) = reconnected_stream.into_split();

        stack_reader.clear();
        stack_writer.clear();

        let mut resuming = resume_handle.clone();

        if let Err(e) = resuming.resume_request(Some(Duration::from_secs(1))).await {
            log::info!("Failed to request a resume: {e}");
            return Err(Error::ResumeRequest);
        };

        let child_token = cancel_token.clone();
        tokio::spawn(async move {
            match resuming
                .resume_response(Some(Duration::from_secs(30)))
                .await
            {
                Ok(_new_id) => {
                    log::info!("Successfully resumed tunnel!");
                }
                Err(e) => {
                    log::info!("Failed to resume tunnel: {e}");
                    child_token.cancel();
                }
            }
        });
    }
}

async fn should_try_to_recover(err: &interpreter::Error, handle: &mut ReliabilityHandle) -> bool {
    // Check if this error is caused by the network-side connection.
    let is_caused_by_net = match err {
        interpreter::Error::NetToApp(lang::Error::Io(interpreter::io::Error::Eof)) => true,
        interpreter::Error::NetToApp(lang::Error::Io(interpreter::io::Error::Read(_))) => true,
        interpreter::Error::AppToNet(lang::Error::Io(interpreter::io::Error::Write(_))) => true,
        _ => false,
    };

    // If we already performed a graceful shutdown, no need to recover.
    is_caused_by_net && !handle.is_shutdown_complete().await
}

fn network_target(target: TargetAddr) -> NetworkTarget {
    match target {
        TargetAddr::Ip(addr) => addr.into(),
        TargetAddr::Domain(host, port) => (host, port).into(),
    }
}

async fn oneshot_client<S, T>(
    stack: S,
    outbound: TcpStream,
    proto: T,
) -> Result<(usize, usize), Error>
where
    S: Protocol + 'static,
    <S as Protocol>::Codec: Send,
    <S as Protocol>::Message: Send,
    <S as Protocol>::StreamHalf: Unpin + Send,
    <S as Protocol>::SinkHalf: Unpin + Send,
    T: TaskProvider + Clone + Send + 'static,
{
    let (net_reader, net_writer) = outbound.into_split();

    // Prepare the stack adapters.
    let (stream, sink) = stack.into_split();
    let stack_reader = EncodedStreamReader::<S>::new(stream, S::codec());
    let stack_writer = DecodedSinkWriter::<S>::new(sink, S::codec());

    // Prepare the interpreter to transfer between the stack and network connection.
    let mut interpreter =
        Interpreter::new(stack_reader, stack_writer, net_reader, net_writer, proto);

    let handler = ErrorHandler::builder()
        .shutdown_app_on_net_eof()
        .shutdown_net_on_app_eof();

    interpreter
        .run_try_join(handler)
        .await
        .map_err(Error::InterpreterFailed)
}

pub trait ConnectionHandler {
    fn connect<T>(
        &mut self,
        server: SocketAddr,
        proto: T,
        target: TargetAddr,
    ) -> impl Future<Output = Result<u64, Error>> + Send
    where
        T: TaskProvider + Clone + Send + 'static;

    fn add(
        &mut self,
        inbound: TcpStream,
        id: u64,
    ) -> impl Future<Output = Result<(), Error>> + Send;
}

pub struct Client {
    router: Option<RoutingHandle<TcpPayload>>,
    simplex: bool,
    resume: bool,
}

impl Client {
    pub fn new(simplex: bool, resume: bool) -> Self {
        Self {
            router: None,
            simplex,
            resume,
        }
    }
}

impl ConnectionHandler for Client {
    async fn connect<T>(
        &mut self,
        server: SocketAddr,
        proto: T,
        target: TargetAddr,
    ) -> Result<u64, Error>
    where
        T: TaskProvider + Clone + Send + 'static,
    {
        if self.simplex && self.router.is_some() {
            return Err(Error::RouterExists);
        } else if self.router.is_none() {
            // Connect to the Proteus server.
            let outbound = connect(server).await?;

            // Setup the stack for this connection.
            let router_handle = if self.resume {
                type _Stack = Framing<ResumptionClient<Reliability<RoutingClient<TcpPayload>>>>;

                let router = RoutingClient::<TcpPayload>::new();
                let router_handle = router.handle().clone();
                let rely = Reliability::new(router);
                let rely_handle = rely.handle().clone();
                let resume = ResumptionClient::new(rely);
                let resume_handle = resume.handle().clone();
                let stack = Framing::new(resume);

                tokio::spawn(reconnecting_client(
                    stack,
                    outbound,
                    proto,
                    rely_handle,
                    resume_handle,
                    target.clone(),
                ));

                router_handle
            } else {
                type _Stack = Framing<RoutingClient<TcpPayload>>;

                let router = RoutingClient::<TcpPayload>::new();
                let router_handle = router.handle().clone();
                let stack = Framing::new(router);

                tokio::spawn(oneshot_client(stack, outbound, proto));

                router_handle
            };

            self.router = Some(router_handle);
        }

        let Some(router) = self.router.as_mut() else {
            return Err(Error::RouterDoesNotExist);
        };

        // Now ask the server to connect to the target.
        router
            .connect_session(network_target(target), Some(Duration::from_secs(30)))
            .await
            .map_err(Error::RoutingFailed)
    }

    async fn add(&mut self, inbound: TcpStream, id: u64) -> Result<(), Error> {
        let Some(handle) = self.router.as_mut() else {
            return Err(Error::RouterDoesNotExist);
        };

        // Create the new session for the tunnel using the stack mss.
        let stack_mss = if self.resume {
            PayloadCodec::mss(RoutingCodec::mss(ReliabilityCodec::mss(
                ResumptionCodec::mss(FramingCodec::mss(FramingCodec::mtu())),
            )))
        } else {
            PayloadCodec::mss(RoutingCodec::mss(FramingCodec::mss(FramingCodec::mtu())))
        };

        let (app_src, app_dst) = inbound.into_split();
        let payload = Payload::new(id, app_src, app_dst, stack_mss);

        // Add the session.
        handle
            .add_session(id, payload)
            .await
            .map_err(Error::RoutingFailed)?;

        // In simplex mode, the tunnel should stop after this session ends.
        if self.simplex {
            handle
                .set_end_method(EndMethod::Empty)
                .await
                .map_err(Error::RoutingFailed)?;
        }

        Ok(())
    }
}

impl Clone for Client {
    fn clone(&self) -> Self {
        // In multiplex mode, persist the tunnel. In simplex mode, drop the
        // router so we create a new tunnel on the next connection attempt.
        let router = if self.simplex {
            None
        } else {
            self.router.clone()
        };

        Self {
            router,
            simplex: self.simplex,
            resume: self.resume,
        }
    }
}
