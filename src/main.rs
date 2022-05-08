mod net;
mod pt;

use log;
use std::{io, process};
use tokio::net::{TcpListener, TcpStream};

use crate::net::proto::{null, socks, upgen};
use crate::net::Connection;
use crate::pt::config::{ClientConfig, CommonConfig, Config, ConfigError, Mode, ServerConfig, ForwardProtocol};
use crate::pt::control;

const FIXED_SEED: u64 = 123321;

#[tokio::main]
async fn main() -> io::Result<()> {
    control::init_logger();

    log::info!("UPGen started and initialized logger.");

    let config = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            match e {
                ConfigError::VersionError(_) => {
                    control::send_to_parent(control::Message::VersionError)
                }
                ConfigError::ProxyError(msg) => {
                    control::send_to_parent(control::Message::ProxyError(msg.as_str()))
                }
                ConfigError::EnvError(msg) => {
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

    log::info!("UPGen completed, exiting now.");
    Ok(())
}

async fn run_client(_common_conf: CommonConfig, client_conf: ClientConfig) -> io::Result<()> {
    log::info!("UPGen is running in client mode.");

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

    match socks::run_socks5_server(Connection::new(rvs_stream)).await {
        Ok((rvs_conn, pt_conn)) => {
            log::debug!("Socks5 with peer {} succeeded", rvs_addr);

            log::debug!(
                "Running UPGen client protocol to forward data from {}",
                rvs_addr,
            );

            // match null::run_null_client(rvs_conn, pt_conn).await {
            match upgen::run_upgen_client(pt_conn, rvs_conn, FIXED_SEED).await {
                Ok(_) => log::debug!("Stream from peer {} succeeded UPGen protocol", rvs_addr),
                Err(e) => log::debug!(
                    "Stream from peer {} failed during UPGen protocol: {}",
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
    log::info!("UPGen is running in server mode.");

    // We run our upgen reverse proxy server here; let the OS choose the port.
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

    // Main loop waiting for connections from upgen proxy clients.
    loop {
        let (pt_stream, _) = listener.accept().await?;
        let conf = server_conf.clone();
        // A failure in a connection does not stop the server.
        tokio::spawn(async move { handle_server_connection(pt_stream, conf).await });
    }
}

async fn handle_server_connection(pt_stream: TcpStream, conf: ServerConfig) -> io::Result<()> {
    let pt_addr = pt_stream.peer_addr()?;
    log::debug!("Accepted new stream from pt client {}", pt_addr);

    let fwd_stream = tokio::net::TcpStream::connect(conf.forward_addr).await?;
    let fwd_addr = fwd_stream.peer_addr()?;
    log::debug!("Connected to forward server {}", fwd_addr);

    let pt_conn = Connection::new(pt_stream);
    let fwd_conn = Connection::new(fwd_stream);

    match conf.forward_proto {
        ForwardProtocol::Basic => {
            // No special OR handshake required.
            log::debug!("Using basic 'data only' protocol with forward server {}", fwd_addr);
        },
        ForwardProtocol::Extended(_cookie_path) => {
            log::debug!("Using extended OR protocol with forward server {}", fwd_addr);
            unimplemented!("Extended OR protocol is not yet supported.")
            // or::run_extor_client(fwd_conn).await
        }
    }

    log::debug!(
        "Running UPGen server protocol to forward data between {} and {}",
        pt_addr,
        fwd_addr
    );

    // match null::run_null_server(pt_conn, fwd_conn).await {
    match upgen::run_upgen_server(pt_conn, fwd_conn, FIXED_SEED).await {
        Ok(_) => log::debug!("Stream from peer {} succeeded UPGen protocol", pt_addr),
        Err(e) => log::debug!(
            "Stream from peer {} failed during UPGen protocol: {}",
            pt_addr,
            e
        ),
    }

    Ok(())
}
