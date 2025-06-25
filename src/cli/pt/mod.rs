use std::collections::HashMap;
use std::{io, process};

use anyhow::bail;
use control::PtLogLevel;
use tokio::net::{TcpListener, TcpStream};

use super::args::PtArgs;
use crate::cli::pt::config::{
    ClientConfig, CommonConfig, Config, ConfigError, ForwardProtocol, Mode, ServerConfig,
};
use crate::lang::Role;
use crate::lang::compiler::Compiler;
use crate::lang::ir::bridge::{OldCompile, TaskProvider};
use crate::net::proto::socks;
use crate::net::proto::turbo::{TunnelClient, TunnelServer};
use crate::net::{Connection, TcpConnector, TcpReconnector};

pub mod config;
pub mod control;

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
            run_client(config.common, client_conf).await?;
        }
        Mode::Server(server_conf) => {
            run_server(config.common, server_conf).await?;
        }
    }

    log::info!("Proteus completed, exiting now.");
    Ok(())
}

async fn run_client(_common_conf: CommonConfig, client_conf: ClientConfig) -> io::Result<()> {
    log::info!("Proteus is running in client mode.");

    if client_conf.proxy.is_some() {
        // TODO: normally we send the ProxyDone message, but since we don't yet
        // handle this case we send an error instead.
        // control::send_to_parent(control::Message::ProxyDone);
        control::send_to_parent(control::Message::ProxyError(
            "proxy connections not implemented",
        ));
        unimplemented!("outgoing connections through a SOCKS proxy");
    }

    // We run our socks5 forward proxy here; let the OS choose the port.
    let listener = match TcpListener::bind("127.0.0.1:0").await {
        Ok(listener) => {
            // Tell our parent the address info so it knows where to connect.
            control::send_to_parent(control::Message::ClientReady(listener.local_addr()?));
            listener
        }
        Err(e) => {
            control::send_to_parent(control::Message::ClientError(
                "unable to start forward proxy server",
            ));
            return Err(e);
        }
    };

    log::info!(
        "Proteus client listening for SOCKS5 app connections on {:?}.",
        listener.local_addr()?
    );
    control::send_to_parent(control::Message::Status("BOOTSTRAPPED=Success"));

    // Main loop waiting for connections from reverse socks5 clients.
    loop {
        let (app_stream, _) = listener.accept().await?;
        let conf = client_conf.clone();
        // A failure in a connection does not stop the server.
        tokio::spawn(async move { handle_client_connection(app_stream, conf).await });
    }
}

async fn handle_client_connection(app_stream: TcpStream, _conf: ClientConfig) -> io::Result<()> {
    let app_addr = app_stream.peer_addr()?;
    log::debug!("Accepted new stream from client {}", app_addr);

    match socks::run_socks5_server(Connection::from(app_stream)).await {
        Ok((app_conn, username_opt, _, target_addr)) => {
            log::debug!("Socks5 with peer {} succeeded", app_addr);

            let options = match username_opt {
                Some(username) => {
                    log::debug!("Obtained Socks5 username: {}", username);
                    let mut map = HashMap::new();
                    for entry in username.split(';').collect::<Vec<&str>>() {
                        let parts: Vec<&str> =
                            entry.split('=').filter(|tok| !tok.is_empty()).collect();
                        if parts.len() == 2 {
                            let k = parts.first().unwrap().to_string();
                            let v = parts.get(1).unwrap().to_string();
                            map.insert(k, v);
                        }
                    }
                    map
                }
                None => HashMap::new(),
            };

            // TODO double check, I think the PSF path can change for every Tor
            // Browser connection, so we have to parse the PSF here on every connection.
            let filepath = options.get("psf").unwrap();
            let client_spec = Compiler::parse_path(filepath, Role::Client).unwrap();

            log::debug!(
                "Running Proteus client protocol to forward data from {} to {}",
                app_addr,
                target_addr
            );

            let tunnel = TunnelClient::new(TcpReconnector::from(target_addr));

            // Run a new client session over the tunnel.
            match tunnel.run_session(app_conn, client_spec).await {
                Ok(_) => log::debug!(
                    "Stream from peer {} succeeded over tunnel {}",
                    app_addr,
                    target_addr
                ),
                Err(e) => log::debug!(
                    "Stream from peer {} failed over tunnel {}: {}",
                    app_addr,
                    target_addr,
                    e
                ),
            }
        }
        Err(e) => {
            log::debug!(
                "Stream from peer {} failed during Socks5 protocol: {}",
                app_addr,
                e
            );
        }
    }

    Ok(())
}

async fn run_server(_common_conf: CommonConfig, server_conf: ServerConfig) -> io::Result<()> {
    log::info!("Proteus is running in server mode.");

    // We run our proteus reverse proxy server here; let the OS choose the port.
    let listener = match TcpListener::bind(server_conf.listen_bind_addr).await {
        Ok(listener) => {
            control::send_to_parent(control::Message::ServerReady(server_conf.listen_bind_addr));
            listener
        }
        Err(e) => {
            control::send_to_parent(control::Message::ServerError(
                "unable to start reverse proxy server",
            ));
            return Err(e);
        }
    };

    let filepath = server_conf.options.get("psf").unwrap();
    let server_spec = Compiler::parse_path(filepath, Role::Server).unwrap();

    log::info!(
        "Proteus server listening for Proteus client connections on {:?}.",
        listener.local_addr()?
    );
    control::send_to_parent(control::Message::Status("BOOTSTRAPPED=Success"));

    // Main loop waiting for connections from proteus proxy clients.
    loop {
        let (net_stream, _) = listener.accept().await?;
        let conf = server_conf.clone();
        let spec = server_spec.clone();
        // A failure in a connection does not stop the server.
        tokio::spawn(async move { handle_server_connection(net_stream, conf, spec).await });
    }
}

async fn handle_server_connection<T>(
    net_stream: TcpStream,
    conf: ServerConfig,
    server_spec: T,
) -> anyhow::Result<()>
where
    T: TaskProvider + Clone + Send,
{
    let net_addr = net_stream.peer_addr()?;
    log::debug!("Accepted new stream from Proteus client {}", net_addr);

    match conf.forward_proto {
        ForwardProtocol::Basic => {
            // No special OR handshake required.
            log::debug!(
                "Using basic 'data only' protocol with forward server {}",
                conf.forward_addr
            );
        }
        ForwardProtocol::Extended(_cookie_path) => {
            log::debug!(
                "Using extended OR protocol with forward server {}",
                conf.forward_addr
            );
            unimplemented!("Extended OR protocol is not yet supported.")
            // or::run_extor_client(fwd_conn).await
        }
    }

    log::debug!(
        "Running Proteus server protocol to forward data between {} and {}",
        net_addr,
        conf.forward_addr
    );

    let mut tunnel = TunnelServer::new(TcpConnector::default());
    if let Some(addr) = tunnel.replace_target(conf.forward_addr) {
        bail!("Expected an empty forwarding address, found {addr}")
    }
    let net_conn = Connection::from(net_stream);

    // Run a new server session over the tunnel.
    match tunnel.run_session(net_conn, server_spec).await {
        Ok(_) => log::debug!("Stream from peer {} succeeded Proteus protocol", net_addr),
        Err(e) => log::debug!(
            "Stream from peer {} failed during Proteus protocol: {}",
            net_addr,
            e
        ),
    }

    Ok(())
}
