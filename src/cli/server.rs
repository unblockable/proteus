use anyhow::anyhow;
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

use crate::cli::args::ServerArgs;
use crate::lang::Role;
use crate::lang::ir::bridge::TaskProvider;
use crate::net::proto::{BytesSession, TunnelMessage, TurboSession};
use crate::net::{
    Channel, SessionBuilder, TcpConnector, TunnelEofMethod, TunnelServer, fmt_stream_name,
};

pub async fn run(args: ServerArgs) -> anyhow::Result<()> {
    log::info!("Proteus is running in Server mode.");

    let psf_path = args
        .protocol
        .to_str()
        .ok_or(anyhow!("Path is not valid UTF-8"))?
        .to_string();

    if args.session.turbo {
        log::info!("Tunnels will use the TurboSession session manager.");
        run_server::<TurboSession<OwnedReadHalf, OwnedWriteHalf>>(args, psf_path).await?;
    } else {
        log::info!("Tunnels will use the BytesSession session manager.");
        run_server::<BytesSession<OwnedReadHalf, OwnedWriteHalf>>(args, psf_path).await?;
    }

    log::info!("Proteus completed, exiting now.");
    Ok(())
}

async fn run_server<S>(server_args: ServerArgs, psf_path: String) -> anyhow::Result<()>
where
    S: SessionBuilder<
            Message = TunnelMessage,
            ReadHalf = OwnedReadHalf,
            WriteHalf = OwnedWriteHalf,
        > + 'static,
{
    let protocol_spec = super::parse_protocol_spec(psf_path.clone(), Role::Server)?;
    let listener = super::bind_listener(&server_args.listen, Role::Server).await?;

    // Main loop waiting for connections from proteus proxy clients.
    loop {
        let (net_stream, _) = listener.accept().await?;
        let protocol_spec = protocol_spec.clone();
        // A failure in a connection should not stop the listener.
        tokio::spawn(async move { handle_connection::<S>(net_stream, protocol_spec).await });
    }
}

async fn handle_connection<S>(
    net_stream: TcpStream,
    protocol_spec: impl TaskProvider + Send + Clone,
) where
    S: SessionBuilder<
            Message = TunnelMessage,
            ReadHalf = OwnedReadHalf,
            WriteHalf = OwnedWriteHalf,
        > + 'static,
{
    let peer_name = fmt_stream_name(&net_stream);
    log::debug!("Accepted new connection from Proteus client {peer_name}");

    let (net_src, net_dst) = net_stream.into_split();

    // We use a channel to manage the network connection to the proteus client.
    let net = Channel::connected(net_src, net_dst, peer_name);

    // We use a tunnel to manage the application sessions with destination servers.
    let app: TunnelServer<S, _> =
        TunnelServer::new(TunnelEofMethod::OnClose, TcpConnector::default());

    super::run_interpreter(net.clone(), net, app.clone(), app, protocol_spec).await;
}
