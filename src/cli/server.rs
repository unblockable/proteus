use anyhow::{anyhow, bail};
use supertunnel::proto::ResumptionMap;
use tokio::net::TcpListener;

use crate::cli::args::ServerArgs;
use crate::lang::Role;
use crate::lang::compiler::Compiler;
use crate::lang::ir::bridge::OldCompile;
use crate::net;
use crate::net::server;

pub async fn run(args: ServerArgs) -> anyhow::Result<()> {
    log::info!("Proteus is running in Server mode.");

    let psf_path = args
        .protocol
        .to_str()
        .ok_or(anyhow!("Path is not valid UTF-8"))?
        .to_string();

    let Ok(proto) = Compiler::parse_path(&psf_path, Role::Server) else {
        bail!(server::Error::PsfCompileFailed);
    };

    let listener = TcpListener::bind(&args.listen)
        .await
        .map_err(server::Error::BindFailed)?;

    log::info!(
        "Listening for Proteus client connections on {:?}.",
        net::fmt_listener_name(&listener)
    );

    if args.session.persist {
        let map = ResumptionMap::new();
        loop {
            let (inbound, _) = listener.accept().await?;
            let (proto, map) = (proto.clone(), map.clone());
            tokio::spawn(server::drive_io_routable_resumable(inbound, proto, map));
        }
    } else {
        loop {
            let (inbound, _) = listener.accept().await?;
            tokio::spawn(server::drive_io_routable(inbound, proto.clone()));
        }
    }
}
