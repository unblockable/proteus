use std::fmt::Debug;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use fast_socks5::server::SocksServerError;
use fast_socks5::util::target_addr::TargetAddr;
use supertunnel::net::NetworkTarget;
use supertunnel::proto::*;
use supertunnel::util::{DecodedSinkWriter, EncodedStreamReader, EndMethod};
use supertunnel::{FrameSize, Protocol};
use tokio::net::ToSocketAddrs;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::lang::interpreter::{self, ErrorHandler, Interpreter};
use crate::lang::ir::bridge::TaskProvider;
use crate::net::{NetPayload, NetStream};
use crate::{lang, net};

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

pub async fn connect<A, S>(target: A) -> Result<S, Error>
where
    A: ToSocketAddrs + Clone + Send + Debug,
    S: NetStream + Send,
{
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
    log::debug!(
        "Transferring between {} and {} using direct i/o.",
        inbound.name(),
        outbound.name()
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

pub async fn drive_io_resumable<S, T>(
    inbound: S,
    outbound: S,
    proto: T,
    reconnect_addr: TargetAddr,
) -> Result<(usize, usize), Error>
where
    S: NetStream,
    T: TaskProvider + Clone + Send,
{
    log::debug!(
        "Transferring between {} and {} using resumable super tunnel.",
        inbound.name(),
        outbound.name()
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

async fn reconnecting_client<P, S, T>(
    stack: P,
    tunnel: S,
    proto: T,
    mut rely_handle: ReliabilityHandle,
    resume_handle: ResumptionHandle,
    target: TargetAddr,
) -> Result<(usize, usize), Error>
where
    P: Protocol,
    <P as Protocol>::Codec: Send,
    <P as Protocol>::Message: Send,
    <P as Protocol>::StreamHalf: Unpin + Send,
    <P as Protocol>::SinkHalf: Unpin + Send,
    S: NetStream,
    T: TaskProvider + Clone + Send,
{
    let (stream, sink) = stack.into_split();
    let mut stack_reader = EncodedStreamReader::<P>::new(stream, P::codec());
    let mut stack_writer = DecodedSinkWriter::<P>::new(sink, P::codec());

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

        let reconnected_stream = S::connect(target.to_string())
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
    #[allow(clippy::match_like_matches_macro)]
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

async fn oneshot_client<P, S, T>(stack: P, outbound: S, proto: T) -> Result<(usize, usize), Error>
where
    P: Protocol,
    <P as Protocol>::Codec: Send,
    <P as Protocol>::Message: Send,
    <P as Protocol>::StreamHalf: Unpin + Send,
    <P as Protocol>::SinkHalf: Unpin + Send,
    S: NetStream,
    T: TaskProvider + Clone + Send,
{
    let (net_reader, net_writer) = outbound.into_split();

    // Prepare the stack adapters.
    let (stream, sink) = stack.into_split();
    let stack_reader = EncodedStreamReader::<P>::new(stream, P::codec());
    let stack_writer = DecodedSinkWriter::<P>::new(sink, P::codec());

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

pub trait ConnectionHandler<S: NetStream> {
    fn connect<T>(
        &mut self,
        server: SocketAddr,
        proto: T,
        target: TargetAddr,
    ) -> impl Future<Output = Result<u64, Error>> + Send
    where
        T: TaskProvider + Clone + Send + 'static;

    fn add(&mut self, inbound: S, id: u64) -> impl Future<Output = Result<(), Error>> + Send
    where
        S: NetStream;
}

pub struct Client<S: NetStream> {
    state: ConnectionState<S>,
    simplex: bool,
    resume: bool,
}

enum ConnectionState<S: NetStream> {
    Simplex,
    Multiplex(Arc<Mutex<Option<RoutingHandle<NetPayload<S>>>>>),
    Connected(RoutingHandle<NetPayload<S>>),
}

impl<S: NetStream> Client<S> {
    pub fn new(simplex: bool, resume: bool) -> Self {
        let state = if simplex {
            ConnectionState::Simplex
        } else {
            ConnectionState::Multiplex(Arc::new(Mutex::new(None)))
        };
        Self {
            state,
            simplex,
            resume,
        }
    }
}

async fn start_tunnel<S, T>(
    server: SocketAddr,
    proto: T,
    target: TargetAddr,
    resume: bool,
) -> Result<RoutingHandle<NetPayload<S>>, Error>
where
    S: NetStream,
    T: TaskProvider + Clone + Send + 'static,
{
    // Connect to the Proteus server.
    let outbound = connect::<_, S>(server).await?;

    // Setup the stack for this connection.
    if resume {
        type _Stack<S> = Framing<ResumptionClient<Reliability<RoutingClient<NetPayload<S>>>>>;

        let router = RoutingClient::<NetPayload<S>>::new();
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
            target,
        ));

        Ok(router_handle)
    } else {
        type _Stack<S> = Framing<RoutingClient<NetPayload<S>>>;

        let router = RoutingClient::<NetPayload<S>>::new();
        let router_handle = router.handle().clone();
        let stack = Framing::new(router);

        tokio::spawn(oneshot_client(stack, outbound, proto));

        Ok(router_handle)
    }
}

impl<S: NetStream> ConnectionHandler<S> for Client<S> {
    async fn connect<T>(
        &mut self,
        server: SocketAddr,
        proto: T,
        target: TargetAddr,
    ) -> Result<u64, Error>
    where
        T: TaskProvider + Clone + Send + 'static,
    {
        let router = match &self.state {
            ConnectionState::Simplex => {
                start_tunnel::<S, T>(server, proto, target.clone(), self.resume).await?
            }
            ConnectionState::Multiplex(mutex) => {
                let mut guard = mutex.lock().await;

                if let Some(router) = guard.as_mut() {
                    router.clone()
                } else {
                    let router =
                        start_tunnel::<S, T>(server, proto, target.clone(), self.resume).await?;
                    guard.insert(router).clone()
                }
            }
            ConnectionState::Connected(router) => {
                if self.simplex {
                    return Err(Error::RouterExists);
                }
                router.clone()
            }
        };

        // In all cases, if we get here we have a connected tunnel.
        self.state = ConnectionState::Connected(router);

        // Get a mutable reference to the router.
        let ConnectionState::Connected(router) = &mut self.state else {
            return Err(Error::RouterDoesNotExist);
        };

        // Ask the server-side of the tunnel to connect to the target.
        router
            .connect_session(network_target(target), Some(Duration::from_secs(30)))
            .await
            .map_err(Error::RoutingFailed)
    }

    async fn add(&mut self, inbound: S, id: u64) -> Result<(), Error> {
        // Get a mutable reference to the router.
        let ConnectionState::Connected(router) = &mut self.state else {
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
        router
            .add_session(id, payload)
            .await
            .map_err(Error::RoutingFailed)?;

        // In simplex mode, the tunnel should stop after this session ends.
        if self.simplex {
            router
                .set_end_method(EndMethod::Empty)
                .await
                .map_err(Error::RoutingFailed)?;
        }

        Ok(())
    }
}

impl<S: NetStream> Clone for Client<S> {
    fn clone(&self) -> Self {
        let state = match &self.state {
            ConnectionState::Simplex => ConnectionState::Simplex,
            ConnectionState::Multiplex(mutex) => {
                if let Ok(mut guard) = mutex.try_lock()
                    && let Some(router) = guard.as_mut()
                {
                    // Transition the clone to the connected state so we can
                    // prevent unnecessary locking going forward.
                    ConnectionState::Connected(router.clone())
                } else {
                    // If the lock is held elsewhere, or we have not yet made
                    // the tunnel, persist the mutex. Note, we try to move to
                    // the connected state quickly, so lock contention should be
                    // low in practice.
                    ConnectionState::Multiplex(mutex.clone())
                }
            }
            ConnectionState::Connected(router) => {
                // In simplex mode, drop the router so we create a new tunnel on
                // the next socks stream. In multiplex mode, persist the tunnel.
                if self.simplex {
                    ConnectionState::Simplex
                } else {
                    ConnectionState::Connected(router.clone())
                }
            }
        };

        Self {
            state,
            simplex: self.simplex,
            resume: self.resume,
        }
    }
}
