use std::collections::HashMap;
use std::{io, process};

use control::PtLogLevel;
use tokio::net::{TcpListener, TcpStream};

use super::args::PtArgs;
use crate::cli::pt::config::{
    ClientConfig, CommonConfig, Config, ConfigError, ForwardProtocol, Mode, ServerConfig,
};
use crate::lang::Role;
use crate::lang::compiler::Compiler;
use crate::lang::interpreter::Interpreter;
use crate::lang::ir::bridge::{OldCompile, TaskProvider};
use crate::net::proto::socks;
use crate::net::{Connection, TcpConnector};

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
        let (rvs_stream, _) = listener.accept().await?;
        let conf = client_conf.clone();
        // A failure in a connection does not stop the server.
        tokio::spawn(async move { handle_client_connection(rvs_stream, conf).await });
    }
}

async fn handle_client_connection(rvs_stream: TcpStream, _conf: ClientConfig) -> io::Result<()> {
    let rvs_addr = rvs_stream.peer_addr()?;
    log::debug!("Accepted new stream from client {}", rvs_addr);

    match socks::run_socks5_server(Connection::from(rvs_stream), TcpConnector::new()).await {
        Ok((rvs_conn, pt_conn, username_opt)) => {
            log::debug!("Socks5 with peer {} succeeded", rvs_addr);

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
                "Running Proteus client protocol to forward data from {}",
                rvs_addr,
            );

            // Run the proteus protocol with the interpreter.
            match Interpreter::run(pt_conn, rvs_conn, client_spec, options).await {
                Ok(_) => log::debug!("Stream from peer {} succeeded Proteus protocol", rvs_addr),
                Err(e) => log::debug!(
                    "Stream from peer {} failed during Proteus protocol: {}",
                    rvs_addr,
                    e
                ),
            }
        }
        Err(e) => {
            log::debug!(
                "Stream from peer {} failed during Socks5 protocol: {}",
                rvs_addr,
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
        let (pt_stream, _) = listener.accept().await?;
        let conf = server_conf.clone();
        let spec = server_spec.clone();
        // A failure in a connection does not stop the server.
        tokio::spawn(async move { handle_server_connection(pt_stream, conf, spec).await });
    }
}

async fn handle_server_connection<T>(
    pt_stream: TcpStream,
    conf: ServerConfig,
    spec: T,
) -> io::Result<()>
where
    T: TaskProvider + Clone + Send,
{
    let pt_addr = pt_stream.peer_addr()?;
    log::debug!("Accepted new stream from Proteus client {}", pt_addr);

    let fwd_stream = tokio::net::TcpStream::connect(conf.forward_addr).await?;
    let fwd_addr = fwd_stream.peer_addr()?;
    log::debug!("Connected to forward server {}", fwd_addr);

    let pt_conn = Connection::from(pt_stream);
    let fwd_conn = Connection::from(fwd_stream);

    match conf.forward_proto {
        ForwardProtocol::Basic => {
            // No special OR handshake required.
            log::debug!(
                "Using basic 'data only' protocol with forward server {}",
                fwd_addr
            );
        }
        ForwardProtocol::Extended(_cookie_path) => {
            log::debug!(
                "Using extended OR protocol with forward server {}",
                fwd_addr
            );
            unimplemented!("Extended OR protocol is not yet supported.")
            // or::run_extor_client(fwd_conn).await
        }
    }

    log::debug!(
        "Running Proteus server protocol to forward data between {} and {}",
        pt_addr,
        fwd_addr
    );

    // Run the proteus protocol with the interpreter.
    match Interpreter::run(pt_conn, fwd_conn, spec, conf.options).await {
        Ok(_) => log::debug!("Stream from peer {} succeeded Proteus protocol", pt_addr),
        Err(e) => log::debug!(
            "Stream from peer {} failed during Proteus protocol: {}",
            pt_addr,
            e
        ),
    }

    Ok(())
}
