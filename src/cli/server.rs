use anyhow::anyhow;
use tokio::net::TcpStream;

use crate::cli::args::ServerArgs;
use crate::lang::Role;
use crate::lang::ir::bridge::TaskProvider;
use crate::net::proto::BytesSession;
use crate::net::{Channel, TcpConnector, TunnelEofMethod, TunnelServer, fmt_stream_name};

pub async fn run(args: ServerArgs) -> anyhow::Result<()> {
    log::info!("Proteus is running in Server mode.");

    let psf_path = args
        .protocol
        .to_str()
        .ok_or(anyhow!("Path is not valid UTF-8"))?
        .to_string();

    run_server(args, psf_path).await?;

    log::info!("Proteus completed, exiting now.");
    Ok(())
}

async fn run_server(server_args: ServerArgs, psf_path: String) -> anyhow::Result<()> {
    let protocol_spec = super::parse_protocol_spec(psf_path.clone(), Role::Server)?;
    let listener = super::bind_listener(&server_args.listen, Role::Server).await?;

    // Main loop waiting for connections from proteus proxy clients.
    loop {
        let (net_stream, _) = listener.accept().await?;
        let protocol_spec = protocol_spec.clone();
        // A failure in a connection should not stop the listener.
        tokio::spawn(async move { handle_connection(net_stream, protocol_spec).await });
    }
}

async fn handle_connection(net_stream: TcpStream, protocol_spec: impl TaskProvider + Send + Clone) {
    let peer_name = fmt_stream_name(&net_stream);
    log::debug!("Accepted new connection from Proteus client {peer_name}");

    let (net_src, net_dst) = net_stream.into_split();

    // We use a channel to manage the network connection to the proteus client.
    let net = Channel::connected(net_src, net_dst, peer_name);

    // We use a tunnel to manage the application sessions with destination servers.
    let app: TunnelServer<BytesSession<_, _>, _> =
        TunnelServer::new(TunnelEofMethod::OnClose, TcpConnector::default());

    super::run_interpreter(net.clone(), net, app.clone(), app, protocol_spec).await;
}
