use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use log::LevelFilter;

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum EnumerableLevelFilter {
    Off,
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl From<EnumerableLevelFilter> for LevelFilter {
    fn from(value: EnumerableLevelFilter) -> Self {
        match value {
            EnumerableLevelFilter::Off => LevelFilter::Off,
            EnumerableLevelFilter::Error => LevelFilter::Error,
            EnumerableLevelFilter::Warn => LevelFilter::Warn,
            EnumerableLevelFilter::Info => LevelFilter::Info,
            EnumerableLevelFilter::Debug => LevelFilter::Debug,
            EnumerableLevelFilter::Trace => LevelFilter::Trace,
        }
    }
}

/// Proteus: establish network communication tunnels using programmable protocols.
#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct CliArgs {
    /// Filter log messages more verbose than the given level.
    #[arg(
        short = 'v',
        long,
        global = true,
        value_name = "LEVEL",
        default_value = "info",
        display_order = 10
    )]
    pub log_level: EnumerableLevelFilter,
    /// Override log filters using RUST_LOG directives supported by the env_logger crate.
    #[arg(
        short = 'f',
        long,
        global = true,
        value_name = "FILTERS",
        display_order = 11
    )]
    pub log_filter: Option<String>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Args)]
pub struct SessionArgs {
    /// Use a session-management protocol to recover from broken tunnels.
    #[arg(short, long, value_name = "BOOL", default_value_t = false)]
    // Using the full bool path to stop clap from treating this as a simple flag.
    pub persist: std::primitive::bool,
}

/// Holds the supported subcommands and their args.
#[derive(Subcommand)]
pub enum Command {
    /// Relay network traffic between applications and proteus proxy servers.
    Client(ClientArgs),
    /// Relay network traffic between proteus clients and Internet destinations.
    Server(ServerArgs),
    /// Relay network traffic through proteus tunnels using the pluggable transport v1 API.
    Pt(PtArgs),
    /// Locally compile and check a protocol specification file for correctness.
    Check(CheckArgs),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum ClientMode {
    /// Use multiple proteus tunnels to the server (~shadowsocks).
    Stream,
    /// Use a single proteus tunnel to the server (~vpn).
    Tunnel,
}

#[derive(Args)]
pub struct ClientArgs {
    /// The proteus protocol specification to use for our tunnels.
    #[arg(required = true)]
    pub protocol: PathBuf,
    /// The address of the proteus proxy server to which we connect our tunnels.
    #[arg(short, long, value_name = "ADDR:PORT", required = true)]
    pub connect: String,
    /// The address to listen for client application connections (use port 0 to auto-select).
    #[arg(short, long, value_name = "ADDR:PORT", default_value = "127.0.0.1:0")]
    pub listen: String,
    /// The mode for connecting to the proteus proxy server.
    #[arg(short, long, default_value = "tunnel")]
    pub mode: ClientMode,
    #[command(flatten)]
    pub session: SessionArgs,
}

#[derive(Args)]
pub struct ServerArgs {
    /// The proteus protocol specification to use for our tunnels.
    #[arg(required = true)]
    pub protocol: PathBuf,
    /// The address to listen for proteus client connections (use port 0 to auto-select).
    #[arg(short, long, value_name = "ADDR:PORT", default_value = "127.0.0.1:0")]
    pub listen: String,
    #[command(flatten)]
    pub session: SessionArgs,
}

// Args for PT mode are configured via env variables.
#[derive(Args)]
pub struct PtArgs {}

#[derive(Args)]
pub struct CheckArgs {
    /// The path to a specification file that defines the protocol to use
    #[arg(required = true)]
    pub protocol: PathBuf,
    /// Number of bytes to transfer using the protocol.
    #[arg(
        short,
        long,
        value_name = "N",
        default_value = "1024",
        display_order = 0
    )]
    pub num_bytes: usize,
}

pub fn parse_cli_args() -> CliArgs {
    CliArgs::parse()
}
