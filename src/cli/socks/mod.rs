use std::net::SocketAddr;
use std::str::FromStr;

use anyhow::anyhow;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};

use crate::cli::args::{ClientArgs, ServerArgs};
use crate::lang::Role;
use crate::lang::compiler::Compiler;
use crate::lang::interpreter::Interpreter;
use crate::lang::ir::bridge::{OldCompile, TaskProvider};
use crate::net::proto::socks;
use crate::net::proto::turbo::tunnel::Tunnel;
use crate::net::proto::turbo::broker::SessionBroker;
use crate::net::{Connection, TcpConnector, TcpReader, TcpWriter};

use super::args::SocksArgs;

pub async fn run(args: SocksArgs) -> anyhow::Result<()> {
    log::info!("Running in socks mode");

    let psf_path = args
        .protocol
        .to_str()
        .ok_or(anyhow!("Path is not valid UTF-8"))?
        .to_string();

    match args.role {
        crate::cli::args::Role::Client(client_args) => run_client(client_args, psf_path).await?,
        crate::cli::args::Role::Server(server_args) => run_server(server_args, psf_path).await?,
    }

    log::info!("Proteus completed, exiting now.");
    Ok(())
}

async fn run_client(client_args: ClientArgs, psf_path: String) -> anyhow::Result<()> {
    log::info!("Proteus is running in client mode.");

    // We need a proxy server to which we will connect.
    log::info!(
        "Client configured to connect network tunnel to proxy server {}",
        client_args.connect
    );
    let server_addr = SocketAddr::from_str(client_args.connect.as_str())
        .map_err(|e| anyhow!("Error parsing server connect info: {e}"))?;
    let server_connector = TcpConnector::from(server_addr);

    // We need a protocol that we should run over the tunnel to the proxy server.
    log::info!("Client configured with protocol specification file {psf_path}",);
    let protocol_spec = Compiler::parse_path(&psf_path, Role::Client)
        .map_err(|e| anyhow!("Error parsing protocol specification file: {e}"))?;

    // We multiplex many client application streams over the tunnel to the proxy server.
    // Note, listen port could be 0, in which case the OS will choose the port.
    let listener = TcpListener::bind(client_args.listen)
        .await
        .map_err(|e| anyhow!("Error listening for connections: {e}"))?;
    log::info!(
        "Client listening for SOCKS5 connections on {:?}.",
        listener.local_addr()?
    );

    // TODO: when do we close down the broker/controller and start new ones?

    // We use a controller to manage the tunnel to the proxy server, and a broker to
    // manage the incoming application stream sessions.
    let controller = Tunnel::new_client(server_connector);
    let broker = SessionBroker::new_socks_client();

    // Run a proteus protocol interpreter in the background. We only run one because we
    // only have a single connection to the proxy server.
    {
        let (controller, broker) = (controller.clone(), broker.clone());
        tokio::spawn(async move { run_interpreter(controller, broker, protocol_spec).await });
    }

    // Main loop waiting for SOCKS5 connections from applications.
    // Note that a failure in a connection does not stop the listener.
    loop {
        let (app_stream, _) = listener.accept().await?;
        let broker = broker.clone();
        tokio::spawn(async move { handle_client_connection(app_stream, broker).await });
    }
}

async fn handle_client_connection(
    app_stream: TcpStream,
    mut broker: SessionBroker<OwnedReadHalf, OwnedWriteHalf, TcpConnector>,
) {
    let peer_name = match app_stream.peer_addr() {
        Ok(addr) => format!("<{addr}>"),
        Err(_) => format!("<unknown>"),
    };

    log::debug!("Accepted new stream from client {peer_name}");

    match socks::run_socks5_server(Connection::from(app_stream)).await {
        Ok((app_conn, _, _, target)) => {
            log::debug!("Socks5 with peer {peer_name} succeeded");
            let (app_src, app_dst) = app_conn.into_split();
            let app_src = app_src.into_inner();
            broker.add_session_socks_client(app_src, app_dst, target).await;
        }
        Err(e) => {
            log::debug!("Stream from peer {peer_name} failed during Socks5 protocol: {e}");
        }
    }
}

async fn run_server(server_args: ServerArgs, psf_path: String) -> anyhow::Result<()> {
    log::info!("Proteus is running in server mode.");

    // We need a protocol that we should run over the tunnel to the client.
    log::info!("Server configured with protocol specification file {psf_path}",);
    let protocol_spec = Compiler::parse_path(&psf_path, Role::Server)
        .map_err(|e| anyhow!("Error parsing protocol specification file: {e}"))?;

    let listener = TcpListener::bind(server_args.listen).await?;
    log::info!(
        "Proteus server listening for Proteus client connections on {:?}.",
        listener.local_addr()?
    );

    // Main loop waiting for connections from proteus proxy clients.
    // A failure in a connection does not stop the listener.
    loop {
        let (net_stream, _) = listener.accept().await?;
        let protocol_spec = protocol_spec.clone();
        tokio::spawn(async move { handle_server_connection(net_stream, protocol_spec).await });
    }
}

async fn handle_server_connection(
    net_stream: TcpStream,
    protocol_spec: impl TaskProvider + Send + Clone,
) {
    let peer_name = match net_stream.peer_addr() {
        Ok(addr) => format!("<{addr}>"),
        Err(_) => format!("<unknown>"),
    };

    log::debug!("Accepted new network stream from Proteus client {peer_name}");

    let net_conn = Connection::from(net_stream);
    let (net_src, net_dst) = net_conn.into_split();

    // We use a controller to manage the tunnel to the client, and a broker to
    // manage the outgoing server stream sessions.
    let controller = Tunnel::new_server(net_src, net_dst);
    let broker = SessionBroker::new_socks_server();

    run_interpreter(controller, broker, protocol_spec).await;
}

async fn run_interpreter(
    controller: Tunnel<TcpReader, TcpWriter, TcpConnector>,
    broker: SessionBroker<OwnedReadHalf, OwnedWriteHalf, TcpConnector>,
    protocol_spec: impl TaskProvider + Send + Clone,
) {
    match Interpreter::run_split(
        controller.clone(),
        controller,
        broker.clone(),
        broker,
        protocol_spec,
    )
    .await
    {
        Ok(_) => log::debug!("Tunnel protocol succeeded",),
        Err(e) => log::debug!("Tunnel protocol failed: {e}",),
    }
}
