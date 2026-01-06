use std::io::Cursor;

use anyhow::{anyhow, bail};
use bytes::BytesMut;
use tokio::io::{AsyncReadExt, Chain};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

use crate::cli::args::{ClientArgs, ClientMode};
use crate::lang::Role;
use crate::net::proto::socks::address::Socks5Target;
use crate::net::proto::{BytesSession, TunnelMessage, TurboSession, socks};
use crate::net::{
    Channel, SessionBuilder, TcpConnector, TunnelClient, TunnelEofMethod, fmt_stream_name,
};

pub async fn run(args: ClientArgs) -> anyhow::Result<()> {
    log::info!("Proteus is running in Client mode.");

    let psf_path = args
        .protocol
        .to_str()
        .ok_or(anyhow!("Path is not valid UTF-8"))?
        .to_string();

    match args.mode {
        ClientMode::Stream => {
            if args.session.turbo {
                run_stream_client::<
                    TurboSession<Chain<Cursor<BytesMut>, OwnedReadHalf>, OwnedWriteHalf>,
                >(args, psf_path)
                .await?
            } else {
                run_stream_client::<
                    BytesSession<Chain<Cursor<BytesMut>, OwnedReadHalf>, OwnedWriteHalf>,
                >(args, psf_path)
                .await?
            }
        }
        ClientMode::Tunnel => {
            if args.session.turbo {
                run_tunnel_client::<
                    TurboSession<Chain<Cursor<BytesMut>, OwnedReadHalf>, OwnedWriteHalf>,
                >(args, psf_path)
                .await?
            } else {
                run_tunnel_client::<
                    BytesSession<Chain<Cursor<BytesMut>, OwnedReadHalf>, OwnedWriteHalf>,
                >(args, psf_path)
                .await?
            }
        }
    }

    log::info!("Proteus completed, exiting now.");
    Ok(())
}

async fn run_stream_client<S>(args: ClientArgs, psf_path: String) -> anyhow::Result<()>
where
    S: SessionBuilder<
            Message = TunnelMessage,
            ReadHalf = Chain<Cursor<BytesMut>, OwnedReadHalf>,
            WriteHalf = OwnedWriteHalf,
        >,
{
    log::info!(
        "Running in stream mode, will create a new Proteus tunnel for each application stream."
    );

    // Clients listen for app connections and create proteus tunnels to a server.
    let protocol_spec = super::parse_protocol_spec(psf_path.clone(), Role::Client)?;
    let server_addr = super::parse_connect_address(&args.connect)?;
    let listener = super::bind_listener(&args.listen, Role::Client).await?;

    // Main loop waiting for app connections, each of which gets its own tunnel and channel.
    loop {
        let (app_stream, _) = listener.accept().await?;
        let protocol_spec = protocol_spec.clone();

        // A single connection result should not stop the listener.
        tokio::spawn(async move {
            let channel =
                Channel::disconnected(Socks5Target::from(server_addr), TcpConnector::default());
            let tunnel = TunnelClient::<S>::new(TunnelEofMethod::OnStreamCount(1));

            match add_stream_to_tunnel(app_stream, tunnel.clone()).await {
                Ok(_) => {
                    super::run_interpreter(
                        channel.clone(),
                        channel,
                        tunnel.clone(),
                        tunnel,
                        protocol_spec,
                    )
                    .await;
                }
                Err(e) => log::debug!("Error adding stream to tunnel: {e}"),
            }
        });
    }
}

async fn run_tunnel_client<S>(args: ClientArgs, psf_path: String) -> anyhow::Result<()>
where
    S: SessionBuilder<
            Message = TunnelMessage,
            ReadHalf = Chain<Cursor<BytesMut>, OwnedReadHalf>,
            WriteHalf = OwnedWriteHalf,
        > + 'static,
{
    log::info!(
        "Running in tunnel mode, will multiplex all application streams over a single Proteus tunnel."
    );

    // Clients listen for app connections and create proteus tunnels to a server.
    let protocol_spec = super::parse_protocol_spec(psf_path.clone(), Role::Client)?;
    let server_addr = super::parse_connect_address(&args.connect)?;
    let listener = super::bind_listener(&args.listen, Role::Client).await?;

    // We use a channel to manage a single connection to the proxy server.
    let channel = Channel::disconnected(Socks5Target::from(server_addr), TcpConnector::default());
    // We use a tunnel to manage the incoming virtual application stream sessions.
    let tunnel = TunnelClient::<S>::new(TunnelEofMethod::OnClose);

    // TODO: gracefully handle channel/tunnel close. (On timeout? On ctrl-c?)

    // Run a proteus protocol interpreter in the background to handle one server connection.
    {
        let (net_src, net_dst) = (channel.clone(), channel.clone());
        let (app_src, app_dst) = (tunnel.clone(), tunnel.clone());
        tokio::spawn(async move {
            super::run_interpreter(net_src, net_dst, app_src, app_dst, protocol_spec).await
        });
    }

    // Main loop waiting for app connections. The interpreter is already running, so the task
    // here is just to add new sessions to the tunnel.
    loop {
        let (app_stream, _) = listener.accept().await?;
        let tunnel = tunnel.clone();
        // A single connection result should not stop the listener.
        tokio::spawn(async move {
            if let Err(e) = add_stream_to_tunnel(app_stream, tunnel).await {
                log::debug!("Error adding stream to tunnel: {e}");
            }
        });
    }
}

async fn add_stream_to_tunnel<S>(
    app_stream: TcpStream,
    mut tunnel: TunnelClient<S>,
) -> anyhow::Result<()>
where
    S: SessionBuilder<
            Message = TunnelMessage,
            ReadHalf = Chain<Cursor<BytesMut>, OwnedReadHalf>,
            WriteHalf = OwnedWriteHalf,
        >,
{
    let peer_name = fmt_stream_name(&app_stream);
    log::debug!("Accepted new connection from client application {peer_name}");

    let (mut app_src, mut app_dst) = app_stream.into_split();

    match socks::run_socks5_server(&mut app_src, &mut app_dst).await {
        Ok(info) => {
            log::debug!("Socks5 with peer {peer_name} succeeded");

            // Do not discard bytes remaining from the socks interaction.
            let app_src = Cursor::new(info.remaining_read_buf).chain(app_src);

            tunnel
                .add_session(app_src, app_dst, Some(info.target))
                .await;

            Ok(())
        }
        Err(e) => bail!("Stream from peer {peer_name} failed during Socks5 protocol: {e}"),
    }
}
