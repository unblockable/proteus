mod pt;
mod socks;

use log;
use std::process;

use crate::pt::config::{Config, ConfigError, Mode};
use crate::pt::control;

fn main() {
    control::init_logger();

    log::info!("UPGen started and initialized logger.");

    let config = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            match e {
                ConfigError::VersionError(_) => control::send_to_parent(control::Message::VersionError),
                ConfigError::ProxyError(msg) => control::send_to_parent(control::Message::ProxyError(msg.as_str())),
                ConfigError::EnvError(msg) => control::send_to_parent(control::Message::EnvError(msg.as_str())),
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
            log::info!("UPGen is running in client mode.");

            if client_conf.proxy.is_some() {
                control::send_to_parent(control::Message::ProxyDone);
            }

            // TODO:
            // start client (socks server)
            // control::send_to_parent(control::Message::ClientMethod(sockaddr that the client's socks server is listening on));
            // wait for socks connections
        },
        Mode::Server(_server_conf) => {
            log::info!("UPGen is running in server mode.");

            // TODO:
            // start server (upgen reverse proxy)
            // wait for connections from upgen clients
        }
    }

    log::info!("UPGen completed, exiting now.");
}
