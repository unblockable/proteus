use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use anyhow::bail;
use fast_socks5::server::Socks5ServerProtocol;
use fast_socks5::util::target_addr::TargetAddr;
use fast_socks5::{ReplyError, Socks5Command};
use tokio::net::{TcpListener, TcpStream};

use crate::cli::client::socks_reply_error;
use crate::cli::pt::config::{ClientConfig, CommonConfig};
use crate::cli::pt::control;
use crate::lang::Role;
use crate::lang::compiler::Compiler;

use crate::lang::ir::bridge::OldCompile;
use crate::net::client;
use crate::net;

pub async fn run(_: CommonConfig, conf: ClientConfig) -> anyhow::Result<()> {
    log::info!("Proteus is running in PT Client mode.");

    if conf.proxy.is_some() {
        // TODO: normally we send the ProxyDone message, but since we don't yet
        // handle this case we send an error instead.
        // control::send_to_parent(control::Message::ProxyDone);
        control::send_to_parent(control::Message::ProxyError(
            "proxy connections not implemented",
        ));
        bail!("Outgoing connections through a SOCKS proxy is unimplemented");
    }

    // We run our socks5 forward proxy here; let the OS choose the port.
    let listener = match TcpListener::bind("127.0.0.1:0").await {
        Ok(listener) => {
            // Tell our parent the address info so it knows where to connect.
            control::send_to_parent(control::Message::ClientReady(listener.local_addr()?));
            listener
        }
        Err(e) => {
            control::send_to_parent(control::Message::ClientError(
                "unable to start forward proxy server",
            ));
            bail!(client::Error::BindFailed(e));
        }
    };

    control::send_to_parent(control::Message::Status("BOOTSTRAPPED=Success"));

    log::info!(
        "Listening for SOCKS5 app connections on {:?}.",
        net::fmt_listener_name(&listener)
    );

    // Run a main loop waiting for connections from socks5 clients. Handle
    // incoming connections in background tasks. The listener stays active even
    // if any individual connection fails.
    loop {
        let (inbound, _) = listener.accept().await?;
        tokio::spawn(async move {
            match socks_then_transfer(inbound).await {
                Ok(target) => log::info!("Connection to bridge finished: {target}"),
                Err(e) => log::debug!("Connection to bridge failed: {e:?}"),
            };
        });
    }
}

async fn socks_then_transfer(inbound: TcpStream) -> Result<TargetAddr, client::Error> {
    log::debug!(
        "Accepted new stream from client {}",
        net::fmt_stream_name(&inbound)
    );

    let (socks, auth_result) =
        Socks5ServerProtocol::accept_password_auth(inbound, parse_socks_username).await?;

    // The options were validated in the socks authentication step.
    let options = auth_result.unwrap();
    let psf_path = options.get("psf").unwrap();

    // Read command before compiling the psf, so we can propagate compile errors
    // in the reply.
    let (socks, cmd, target) = socks.read_command().await?;

    if cmd != Socks5Command::TCPConnect {
        socks.reply_error(&ReplyError::CommandNotSupported).await?;
        return Err(client::Error::SocksCommandNotSupported);
    };

    // Compile the chosen PSF for the given target bridge.
    let Ok(proto) = Compiler::parse_path(psf_path, Role::Client) else {
        socks.reply_error(&ReplyError::GeneralFailure).await?;
        return Err(client::Error::PsfCompileFailed);
    };

    let (host, port) = target.clone().into_string_and_port();

    let outbound = match client::connect((host, port)).await {
        Ok(stream) => stream,
        Err(e) => {
            log::warn!("Connection to {target} failed: {e:?}.");
            socks.reply_error(&socks_reply_error(&e)).await?;
            return Err(e);
        }
    };

    let bind_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 0);
    let inbound = socks.reply_success(bind_addr).await?;

    log::info!("Connection to bridge succeeded: {target}");

    if options
        .get("turbo")
        .map_or(false, |v| v.to_ascii_lowercase().eq("true"))
    {
        client::drive_io_resumable(inbound, outbound, proto, target.clone()).await?;
        log::info!("Super transfer succeeded");
    } else {
        client::drive_io_direct(inbound, outbound, proto).await?;
        log::info!("Direct transfer succeeded");
    }

    Ok(target.into())
}

fn parse_socks_username(username: String, _password: String) -> Option<HashMap<String, String>> {
    let mut map = HashMap::new();

    // Parse the config options stored in the username.
    for entry in username.split(';').collect::<Vec<&str>>() {
        let parts: Vec<&str> = entry.split('=').filter(|tok| !tok.is_empty()).collect();
        if parts.len() == 2 {
            let k = parts.first().unwrap().to_string().to_lowercase();
            let v = parts.get(1).unwrap().to_string();
            map.insert(k, v);
        }
    }

    if map.contains_key("psf") {
        Some(map)
    } else {
        None
    }
}
