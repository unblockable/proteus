use anyhow::bail;
use supertunnel::proto::ResumptionMap;
use tokio::net::TcpListener;

use crate::cli::pt::config::{CommonConfig, ForwardProtocol, ServerConfig};
use crate::cli::pt::control;
use crate::lang::Role;
use crate::lang::compiler::Compiler;
use crate::lang::ir::bridge::OldCompile;
use crate::net::server;
use crate::net;

pub async fn run(_: CommonConfig, conf: ServerConfig) -> anyhow::Result<()> {
    log::info!("Proteus is running in PT Server mode.");

    let Some(psf_path) = conf.options.get("psf") else {
        bail!(server::Error::PsfOptionMissing);
    };

    let Ok(proto) = Compiler::parse_path(psf_path, Role::Server) else {
        bail!(server::Error::PsfCompileFailed);
    };

    if conf.forward_proto != ForwardProtocol::Basic {
        bail!(server::Error::ExtOrNotSupported);
    };

    // We run our proteus reverse proxy server here; let the OS choose the port.
    let listener = match TcpListener::bind(conf.listen_bind_addr).await {
        Ok(listener) => {
            control::send_to_parent(control::Message::ServerReady(conf.listen_bind_addr));
            listener
        }
        Err(e) => {
            control::send_to_parent(control::Message::ServerError(
                "unable to start reverse proxy server",
            ));
            bail!(server::Error::BindFailed(e));
        }
    };

    control::send_to_parent(control::Message::Status("BOOTSTRAPPED=Success"));
    let target = conf.forward_addr;

    log::info!(
        "Listening for Proteus client connections on {:?}.",
        net::fmt_listener_name(&listener)
    );

    // Run a main loop waiting for connections from proteus proxy clients.
    // Handle incoming connections in background tasks. The listener stays
    // active even if any individual connection fails.
    if conf
        .options
        .get("turbo")
        .map_or(false, |v| v.to_ascii_lowercase().eq("true"))
    {
        // Shared global state for session resumption. This allows a client to
        // reconnect when a connection fails and resume a previously established
        // tunnel. The map uses an Arc internally, so cloning it is cheap: it
        // just increments a reference count.
        let map = ResumptionMap::new();
        loop {
            let (inbound, _) = listener.accept().await?;
            let (proto, map) = (proto.clone(), map.clone());
            tokio::spawn(async move {
                let outbound = server::connect(target).await?;
                server::drive_io_resumable(inbound, outbound, proto, map).await
            });
        }
    } else {
        // Here we just forward between inbound and outbound without any added
        // session resumption or other subprotocol extensions.
        loop {
            let (inbound, _) = listener.accept().await?;
            let proto = proto.clone();
            tokio::spawn(async move {
                let outbound = server::connect(target).await?;
                server::drive_io_direct(inbound, outbound, proto).await
            });
        }
    }
}
