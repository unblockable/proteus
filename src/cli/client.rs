use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::str::FromStr;

use anyhow::{anyhow, bail};
use fast_socks5::server::Socks5ServerProtocol;
use fast_socks5::util::target_addr::TargetAddr;
use fast_socks5::{ReplyError, Socks5Command};
use tokio::net::{TcpListener, TcpStream};

use crate::cli::args::{ClientArgs, ClientMode};
use crate::lang::Role;
use crate::lang::compiler::Compiler;
use crate::lang::ir::bridge::{OldCompile, TaskProvider};
use crate::net;
use crate::net::client::{self, Client, ConnectionHandler};

pub async fn run(args: ClientArgs) -> anyhow::Result<()> {
    let is_simplex = match args.mode {
        ClientMode::Stream => true,
        ClientMode::Tunnel => false,
    };
    let is_resumable = args.session.turbo;
    run_inner(args, Client::new(is_simplex, is_resumable)).await
}

async fn run_inner<T>(args: ClientArgs, client: T) -> anyhow::Result<()>
where
    T: ConnectionHandler + Clone + Send + 'static,
{
    let msg = match args.mode {
        ClientMode::Stream => {
            "Proteus is running in Client::Stream mode, will create a new Proteus connection for each SOCKS5 stream."
        }
        ClientMode::Tunnel => {
            "Proteus is running in Client::Tunnel mode, will multiplex all SOCKS5 streams over a single Proteus connection."
        }
    };
    log::info!("{msg}");

    let psf_path = args
        .protocol
        .to_str()
        .ok_or(anyhow!("Path is not valid UTF-8"))?
        .to_string();

    let Ok(proto) = Compiler::parse_path(&psf_path, Role::Client) else {
        bail!(client::Error::PsfCompileFailed);
    };

    let Ok(connect_addr) = SocketAddr::from_str(args.connect.as_str()) else {
        bail!(client::Error::ConnectParseFailed);
    };

    let listener = TcpListener::bind(&args.listen)
        .await
        .map_err(client::Error::BindFailed)?;

    log::info!(
        "Listening for SOCKS5 app connections on {:?}.",
        net::fmt_listener_name(&listener)
    );

    loop {
        let (inbound, _) = listener.accept().await?;
        let (client, proto) = (client.clone(), proto.clone());
        tokio::spawn(async move {
            match handle_connection(inbound, client, connect_addr, proto).await {
                Ok(target) => log::info!("Tunneled new stream to {target}"),
                Err(e) => log::debug!("New stream failed: {e:?}"),
            };
        });
    }

    // log::info!("Proteus Client completed, exiting now.");
    // Ok(())
}

async fn handle_connection<T, U>(
    inbound: TcpStream,
    mut client: T,
    server: SocketAddr,
    proto: U,
) -> Result<TargetAddr, client::Error>
where
    T: ConnectionHandler + Clone + Send + 'static,
    U: TaskProvider + Clone + Send + 'static,
{
    log::debug!(
        "Accepted new stream from client {}",
        net::fmt_stream_name(&inbound)
    );

    let socks = Socks5ServerProtocol::accept_no_auth(inbound).await?;
    let (socks, cmd, target) = socks.read_command().await?;

    if cmd != Socks5Command::TCPConnect {
        socks.reply_error(&ReplyError::CommandNotSupported).await?;
        return Err(client::Error::SocksCommandNotSupported);
    };

    match client.connect(server, proto, target.clone()).await {
        Ok(id) => {
            let bind_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 0);
            let inbound = socks.reply_success(bind_addr).await?;
            client.add(inbound, id).await?;
            Ok(target)
        }
        Err(e) => {
            socks.reply_error(&socks_reply_error(&e)).await?;
            Err(e)
        }
    }
}

pub fn socks_reply_error(err: &client::Error) -> ReplyError {
    match err {
        client::Error::ConnectFailed(std_io_e) => match std_io_e.kind() {
            std::io::ErrorKind::ConnectionRefused => ReplyError::ConnectionRefused,
            std::io::ErrorKind::HostUnreachable => ReplyError::HostUnreachable,
            std::io::ErrorKind::NetworkUnreachable => ReplyError::NetworkUnreachable,
            std::io::ErrorKind::TimedOut => ReplyError::ConnectionTimeout,
            _ => ReplyError::GeneralFailure,
        },
        _ => ReplyError::GeneralFailure,
    }
}
