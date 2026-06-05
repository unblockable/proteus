use std::process;

use control::PtLogLevel;

use super::args::PtArgs;
use crate::cli::pt::config::{Config, ConfigError, Mode};

mod client;
mod config;
mod control;
mod server;

pub async fn run(_args: PtArgs) -> anyhow::Result<()> {
    log::info!("Running in pt mode");

    control::send_to_parent(control::Message::Log((
        PtLogLevel::Notice,
        "All future log messages are directed to proteus stderr",
    )));

    let config = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            match e {
                ConfigError::Version(_) => control::send_to_parent(control::Message::VersionError),
                ConfigError::Proxy(msg) => {
                    control::send_to_parent(control::Message::ProxyError(msg.as_str()))
                }
                ConfigError::Env(msg) => {
                    control::send_to_parent(control::Message::EnvError(msg.as_str()))
                }
            };
            process::exit(1);
        }
    };

    log::info!("Finished parsing configuration.");
    log::debug!("{:?}", config);

    // Tell parent that we support the PT version.
    control::send_to_parent(control::Message::Version);

    match config.mode {
        Mode::Client(client_conf) => {
            client::run(config.common, client_conf).await?;
        }
        Mode::Server(server_conf) => {
            server::run(config.common, server_conf).await?;
        }
    }

    log::info!("Proteus completed, exiting now.");
    Ok(())
}
