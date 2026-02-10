use std::io::Cursor;
use std::net::SocketAddr;
use std::time::Duration;

use anyhow::{anyhow, bail};
use bytes::Bytes;
use tokio::io::{AsyncReadExt, Chain};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

use crate::cli::args::{ClientArgs, ClientMode};
use crate::lang::Role;
use crate::lang::interpreter::Interpreter;
use crate::lang::ir::bridge::TaskProvider;
use crate::net::proto::socks::address::Socks5Target;
use crate::net::proto::{BytesSession, TunnelMessage, TurboSession, socks};
use crate::net::{
    Channel, SessionBuilder, TcpConnector, TunnelClient, TunnelEofMethod, fmt_stream_name,
};

type TcpR = Chain<Cursor<Bytes>, OwnedReadHalf>;
type TcpW = OwnedWriteHalf;

pub async fn run(args: ClientArgs) -> anyhow::Result<()> {
    log::info!("Proteus is running in Client mode.");

    let psf_path = args
        .protocol
        .to_str()
        .ok_or(anyhow!("Path is not valid UTF-8"))?
        .to_string();

    match args.mode {
        ClientMode::Stream => {
            log::info!(
                "Running in stream mode, will create a new Proteus tunnel for each application stream."
            );

            if args.session.turbo {
                log::info!("Tunnels will use the TurboSession session manager.");
                run_stream_client::<TurboSession<TcpR, TcpW>>(args, psf_path).await?
            } else {
                log::info!("Tunnels will use the BytesSession session manager.");
                run_stream_client::<BytesSession<TcpR, TcpW>>(args, psf_path).await?
            }
        }
        ClientMode::Tunnel => {
            log::info!(
                "Running in tunnel mode, will multiplex all application streams over a single Proteus tunnel."
            );

            if args.session.turbo {
                log::info!("Tunnels will use the TurboSession session manager.");
                run_tunnel_client::<TurboSession<TcpR, TcpW>>(args, psf_path).await?
            } else {
                log::info!("Tunnels will use the BytesSession session manager.");
                run_tunnel_client::<BytesSession<TcpR, TcpW>>(args, psf_path).await?
            }
        }
    }

    log::info!("Proteus completed, exiting now.");
    Ok(())
}

async fn run_stream_client<S>(args: ClientArgs, psf_path: String) -> anyhow::Result<()>
where
    S: SessionBuilder<Message = TunnelMessage, ReadHalf = TcpR, WriteHalf = TcpW> + 'static,
{
    // Clients listen for app connections and create proteus tunnels to a server.
    let protocol_spec = super::parse_protocol_spec(psf_path.clone(), Role::Client)?;
    let server_addr = super::parse_connect_address(&args.connect)?;
    let listener = super::bind_listener(&args.listen, Role::Client).await?;

    // Main loop waiting for app connections.
    loop {
        match listener.accept().await {
            Ok((app_stream, _)) => {
                let protocol_spec = protocol_spec.clone();

                // A single connection result should not stop the listener.
                tokio::spawn(async move {
                    // Each stream gets its own tunnel. The tunnel ends when the stream ends.
                    let tunnel = TunnelClient::<S>::new(TunnelEofMethod::OnStreamCount(1));

                    match add_stream_to_tunnel(app_stream, tunnel.clone()).await {
                        Ok(_) => {
                            run_interpreter_loop(
                                tunnel,
                                server_addr,
                                protocol_spec,
                                args.session.turbo,
                            )
                            .await
                        }
                        Err(e) => log::debug!("Error adding stream to tunnel: {e}"),
                    }
                });
            }
            Err(e) => log::debug!("Error accepting connection: {e}"),
        }
    }
}

async fn run_tunnel_client<S>(args: ClientArgs, psf_path: String) -> anyhow::Result<()>
where
    S: SessionBuilder<Message = TunnelMessage, ReadHalf = TcpR, WriteHalf = TcpW> + 'static,
{
    // Clients listen for app connections and create proteus tunnels to a server.
    let protocol_spec = super::parse_protocol_spec(psf_path.clone(), Role::Client)?;
    let server_addr = super::parse_connect_address(&args.connect)?;
    let listener = super::bind_listener(&args.listen, Role::Client).await?;

    // We use a single tunnel to manage all incoming application streams.
    let tunnel = TunnelClient::<S>::new(TunnelEofMethod::OnClose);

    // Run a proteus interpreter in the background to drive the tunnel.
    let tunnel_clone = tunnel.clone();
    let mut bg_tunnel_task = tokio::spawn(async move {
        run_interpreter_loop(tunnel_clone, server_addr, protocol_spec, args.session.turbo).await
    });

    // Asynchronously add application streams into the already-running tunnel.
    loop {
        tokio::select! {
            connection = listener.accept() => {
                match connection {
                    Ok((app_stream, _)) => {
                        let tunnel = tunnel.clone();
                        tokio::spawn(async move {
                            if let Err(e) = add_stream_to_tunnel(app_stream, tunnel).await {
                                log::debug!("Error adding stream to tunnel: {e}");
                            }
                        });
                    },
                    Err(e) => log::debug!("Error accepting connection: {e}"),
                }
            }
            result = &mut bg_tunnel_task => {
                // The tunnel task completed, stop the client.
                return result.map_err(|e| anyhow!("{e}"));
            }
        };
    }
}

async fn add_stream_to_tunnel<S>(
    app_stream: TcpStream,
    mut tunnel: TunnelClient<S>,
) -> anyhow::Result<()>
where
    S: SessionBuilder<Message = TunnelMessage, ReadHalf = TcpR, WriteHalf = TcpW> + 'static,
{
    let peer_name = fmt_stream_name(&app_stream);
    log::debug!("Accepted new connection from client application {peer_name}");

    let (mut app_src, mut app_dst) = app_stream.into_split();

    match socks::run_socks5_server(&mut app_src, &mut app_dst).await {
        Ok(info) => {
            log::debug!("Socks5 with peer {peer_name} succeeded");

            // Do not discard bytes remaining from the socks interaction.
            let app_src = Cursor::new(info.remaining_read_buf.freeze()).chain(app_src);

            tunnel
                .add_session(app_src, app_dst, Some(info.target))
                .await;

            Ok(())
        }
        Err(e) => bail!("Stream from peer {peer_name} failed during Socks5 protocol: {e}"),
    }
}

async fn run_interpreter_loop<S, T>(
    mut tunnel: TunnelClient<S>,
    server: SocketAddr,
    protocol: T,
    is_turbo: bool,
) where
    S: SessionBuilder<Message = TunnelMessage, ReadHalf = TcpR, WriteHalf = TcpW> + 'static,
    T: TaskProvider + Clone + Send,
{
    // TODO: gracefully handle channel/tunnel close. (On timeout? On ctrl-c?)
    loop {
        let mut channel =
            Channel::disconnected(Socks5Target::from(server), TcpConnector::default());

        let (net_src, net_dst) = (channel.clone(), channel.clone());
        let (app_src, app_dst) = (tunnel.clone(), tunnel.clone());
        let protocol = protocol.clone();

        Interpreter::run(net_src, net_dst, app_src, app_dst, protocol).await;

        // If there is a channel error, we could try again on a new channel, but
        // we need to be running in turbo mode (for retransmission support) with
        // the tunnel in a resumable state.
        if is_turbo
            && channel.has_error().await
            && let Some(id) = tunnel.can_resume().await
        {
            log::info!("Trying to resume {id}");
            tokio::time::sleep(Duration::from_secs(1)).await;
            tunnel.initiate_resume(id).await;
            continue;
        } else {
            log::info!("No resume needed");
            break;
        }
    }
}
