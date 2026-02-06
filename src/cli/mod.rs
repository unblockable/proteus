use std::net::SocketAddr;
use std::str::FromStr;
use std::time::Duration;

use anyhow::anyhow;
use args::CliArgs;
use env_logger::{Builder, Target};
use tokio::net::TcpListener;

use crate::common::sync::AsyncMap;
use crate::lang::Role;
use crate::lang::compiler::Compiler;
use crate::lang::ir::bridge::{OldCompile, TaskProvider};

mod args;
mod check;
mod client;
mod pt;
mod server;

pub async fn run() -> anyhow::Result<()> {
    let args = args::parse_cli_args();
    setup_logging(&args);

    log::info!("Parsed CLI args and initialized logger!");

    let result = match args.command {
        args::Command::Client(args) => client::run(args).await,
        args::Command::Server(args) => server::run(args).await,
        args::Command::Pt(args) => pt::run(args).await,
        args::Command::Check(args) => check::run(args).await,
    };

    if let Err(e) = &result {
        log::error!("Error occurred: {}", e);
        log::error!("Caused by: {}", e.root_cause());
        log::info!("Exiting cleanly with error :/");
    } else {
        log::info!("Exiting cleanly with success :)");
    }

    result
}

fn setup_logging(args: &CliArgs) {
    // Set up logger.
    let mut logger: Builder = Builder::new();
    logger.format_timestamp_micros();

    if let Some(filters) = &args.log_filter {
        // Configure with the RUST_LOG directives string from the cli arg.
        logger.parse_filters(filters.as_str())
    } else {
        // Configure just the log level.
        logger.filter_level(args.log_level.into())
    };

    // Log to stderr because PT mode uses stdout for comms with parent.
    logger.target(Target::Stderr).init();
}

fn parse_connect_address(connect: &String) -> anyhow::Result<SocketAddr> {
    log::info!("Client configured to connect network tunnel to proxy server {connect}");
    let server_addr = SocketAddr::from_str(connect.as_str())
        .map_err(|e| anyhow!("Error parsing server connect info: {e}"))?;
    Ok(server_addr)
}

fn parse_protocol_spec(
    psf_path: String,
    role: Role,
) -> anyhow::Result<impl TaskProvider + Clone + Send> {
    log::info!("{role:?} configured with protocol specification file {psf_path}");
    let protocol_spec = Compiler::parse_path(&psf_path, role)
        .map_err(|e| anyhow!("Error parsing protocol specification file: {e}"))?;
    Ok(protocol_spec)
}

/// Open a TcpListener using the address encoded in `listen`.
/// Note, listen port could be 0, in which case the OS will choose the port.
async fn bind_listener(listen: &String, role: Role) -> anyhow::Result<TcpListener> {
    let listener = TcpListener::bind(listen)
        .await
        .map_err(|e| anyhow!("Error listening on {listen}: {e}"))?;

    let addr = listener.local_addr()?;

    match role {
        Role::Client => log::info!("Client listening for app connections on {addr:?}."),
        Role::Server => log::info!("Server listening for proteus connections on {addr:?}."),
    };

    Ok(listener)
}

async fn remove_after_countdown<T>(mut map: AsyncMap<T>, id: u64) {
    for i in (0..60).rev() {
        if map.contains(&id).await {
            tokio::time::sleep(Duration::from_secs(1)).await;
            log::trace!("Tunnel {id} resumption countdown: {i}");
        } else {
            break;
        }
    }

    if map.remove(id).await.is_some() {
        log::info!("Resumption timeout expired for tunnel {id}");
    } else {
        log::debug!("Tunnel {id} might have been resumed as it is no longer mapped");
    }
}
