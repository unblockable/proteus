mod net;
mod pt;

use log;
use std::process;
use std::io::Result;
use std::net::{TcpListener, TcpStream};

use crate::net::Connection;
use crate::net::socks;
use crate::pt::config::{Config, CommonConfig, ClientConfig, ServerConfig, ConfigError, Mode};
use crate::pt::control;

fn main() -> Result<()> {
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
            run_client(config.common, client_conf)?;
        },
        Mode::Server(server_conf) => {
            run_server(config.common, server_conf)?;
        }
    }

    log::info!("UPGen completed, exiting now.");
    Ok(())
}

fn run_client(_common_conf: CommonConfig, client_conf: ClientConfig) -> Result<()> {
    log::info!("UPGen is running in client mode.");

    if client_conf.proxy.is_some() {
        // TODO: normally we send the ProxyDone message, but since we don't yet
        // handle this case we send an error instead.
        // control::send_to_parent(control::Message::ProxyDone);
        control::send_to_parent(control::Message::ProxyError("proxy connections not implemented"));
        unimplemented!("outgoing connections through a SOCKS proxy");
    }

    // We run our socks5 forward proxy here; let the OS choose the port.
    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => {
            control::send_to_parent(control::Message::ClientReady(listener.local_addr()?));
            listener
        },
        Err(e) => {
            control::send_to_parent(control::Message::ClientError("unable to start forward proxy server"));
            return Err(e);
        }
    };
    
    // Main loop waiting for connections from socks client.
    for stream in listener.incoming() {
        // Failure on a specific connection does not close the listener.
        if let Err(e) = handle_forward_proxy_connection(stream) {
            log::debug!("Incoming connection attempt to forward proxy server failed: {}", e);
        }
    }

    log::info!("UPGen client is done running.");
    Ok(())
}

fn handle_forward_proxy_connection(incoming: Result<TcpStream>) -> Result<()> {
    let stream = incoming?;
    log::debug!("got stream {:?}", stream);

    let conn = Connection::new(stream)?;

    socks::server::run_protocol(conn);

    Ok(())
}

fn run_server(_common_conf: CommonConfig, server_conf: ServerConfig) -> Result<()> {
    log::info!("UPGen is running in server mode.");

    // We run our upgen reverse proxy server here; let the OS choose the port.
    let listener = match TcpListener::bind(server_conf.listen_bind_addr) {
        Ok(listener) => {
            control::send_to_parent(control::Message::ServerReady(server_conf.listen_bind_addr));
            listener
        },
        Err(e) => {
            control::send_to_parent(control::Message::ServerError("unable to start reverse proxy server"));
            return Err(e);
        }
    };
    
    // Main loop waiting for connections from upgen client.
    for stream in listener.incoming() {
        // Failure on a specific connection does not close the listener.
        if let Err(e) = handle_reverse_proxy_connection(stream) {
            log::debug!("Incoming connection attempt to reverse proxy server failed: {}", e);
        }
    }

    log::info!("UPGen server is done running.");
    Ok(())
}

fn handle_reverse_proxy_connection(incoming: Result<TcpStream>) -> Result<()> {
    let stream = incoming?;
    log::debug!("got stream {:?}", stream);
    Ok(())
}
