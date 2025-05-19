use std::fs;

use anyhow::{bail, Context};

use super::args::CheckArgs;
use crate::common::mock;
use crate::lang::parse::proteus::ProteusParser;
use crate::lang::parse::Parse;
use crate::lang::Role;

pub async fn run(args: CheckArgs) -> anyhow::Result<()> {
    log::info!("Running in check mode");

    log::info!("Reading file at {:?}...", args.protocol);

    if !args.protocol.exists() {
        bail!("Cannot check {:?}: file not found", args.protocol);
    } else if !args.protocol.is_file() {
        bail!("Cannot check {:?}: not a regular file", args.protocol);
    }

    let contents = fs::read_to_string(&args.protocol)?;

    log::info!(
        "Read {} bytes of content from {:?}",
        contents.len(),
        args.protocol
    );
    log::info!("Compiling protocol specification contents...");

    let client_spec = ProteusParser::parse_content(&contents, Role::Client)?;
    let server_spec = ProteusParser::parse_content(&contents, Role::Server)?;

    log::info!("✓ Compilation successful in both client and server roles!");
    log::info!(
        "Checking protocol interpretability while transferring {} bytes...",
        args.num_bytes
    );

    let res = mock::check_protocol_interpretability(client_spec, server_spec, args.num_bytes).await;

    log::info!("Protocol check complete, inspecting results...");

    let (c_recv, c_sent) = res.client_app.context("inspecting client app result")?;
    let _ = res.client_proxy.context("inspecting client proxy result")?;
    let _ = res.server_proxy.context("inspecting server proxy result")?;
    let (s_recv, s_sent) = res.server_app.context("inspecting server app result")?;

    log::info!("All processes returned OK, checking payloads now...");

    if c_sent.len() < args.num_bytes {
        bail!("Client sent {}/{} bytes", c_sent.len(), args.num_bytes);
    } else if s_sent.len() < args.num_bytes {
        bail!("Server sent {}/{} bytes", s_sent.len(), args.num_bytes);
    } else if c_recv.len() < args.num_bytes {
        bail!("Client received {}/{} bytes", c_recv.len(), args.num_bytes);
    } else if s_recv.len() < args.num_bytes {
        bail!("Server received {}/{} bytes", s_recv.len(), args.num_bytes);
    } else if s_sent.len() != c_recv.len() {
        bail!(
            "Server sent {} bytes but client received {} bytes",
            s_sent.len(),
            c_recv.len()
        );
    } else if c_sent.len() != s_recv.len() {
        bail!(
            "Client sent {} bytes but server received {} bytes",
            c_sent.len(),
            s_recv.len()
        );
    } else if c_sent[..] != s_recv[..] {
        bail!("Bytes sent by client do not equal bytes received by server");
    } else if s_sent[..] != c_recv[..] {
        bail!("Bytes sent by server do not equal bytes received by client");
    }

    log::info!("✓ Interpreter was successful in both client and server roles!");
    Ok(())
}
