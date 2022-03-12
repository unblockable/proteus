mod pt;
mod socks;

use log;
use std::process;

use crate::pt::config::{Config, ConfigError};
use crate::pt::control;

fn main() {
    control::init_logger();

    log::info!("Hello, world!");

    let _config = match Config::from_env() {
        Ok(c) => {
            control::send_to_parent(control::Message::Version);
            control::send_to_parent(control::Message::ProxyDone);
            c
        }
        Err(e) => {
            match e {
                ConfigError::VersionError(_) => control::send_to_parent(control::Message::VersionError),
                ConfigError::ProxyError(msg) => control::send_to_parent(control::Message::ProxyError(msg.as_str())),
                ConfigError::EnvError(msg) => control::send_to_parent(control::Message::EnvError(msg.as_str())),
            };
            process::exit(1);
        }
    };

    
    // TODO
    // if config.mode == Config::Mode::Client{
    // start client
    // control::send_to_parent(control::Message::ClientMethod(sockaddr that the client's socks server is listening on));
    // }
    // if config.mode == Config::Mode::Server{
    // start server
    // }
}
