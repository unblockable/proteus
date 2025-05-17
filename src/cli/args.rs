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

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
/// Proteus: establish network communication tunnels using programmable protocols.
pub struct CliArgs {
    /// Filter log messages more verbose than the given level.
    #[arg(
        short,
        long,
        global = true,
        value_name = "LEVEL",
        default_value = "info",
        display_order = 0
    )]
    pub log_level: EnumerableLevelFilter,
    /// Override log filters using RUST_LOG directives supported by the env_logger crate.
    #[arg(
        short = 'f',
        long,
        global = true,
        value_name = "FILTERS",
        display_order = 1
    )]
    pub log_filter: Option<String>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
/// Holds the supported subcommands and their args.
pub enum Command {
    /// Proxy network traffic through proteus tunnels using a SOCKS API.
    Socks(SocksArgs),
    /// Proxy network traffic through proteus tunnels using the pluggable transport v1 API.
    Pt(PtArgs),
    /// Locally compile and check a protocol specification file for correctness.
    Check(CheckArgs),
}

#[derive(Args)]
pub struct SocksArgs {}

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
        display_order = 2
    )]
    pub num_bytes: usize,
}

pub fn parse_cli_args() -> CliArgs {
    CliArgs::parse()
}
