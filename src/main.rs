mod net;
mod pt;

use log;
use std::{io, process};
use tokio::net::TcpListener;

use crate::net::proto::{null, socks};
use crate::net::Connection;
use crate::pt::config::{
    ClientConfig, CommonConfig, Config, ConfigError, Mode, ServerConfig,
};
use crate::pt::control;

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

    match run_client_main_loop(listener).await {
        Ok(_) => {
            log::info!("UPGen client completed.");
            Ok(())
        }
        Err(e) => {
            log::info!("UPGen client completed with error: {}", e);
            Err(e)
        }
    }
}

async fn run_client_main_loop(listener: TcpListener) -> io::Result<()> {
    // Main loop waiting for connections from socks client.
    loop {
        let (stream, sock_addr) = listener.accept().await?;
        log::debug!("Accepted new stream from peer {}", sock_addr);
        // TODO: handle success
        // TODO: place into separate task
        match socks::run_socks5_server(Connection::new(stream)).await {
            Ok((client_conn, server_conn)) => {
                log::debug!("Socks5 with peer {} succeeded", sock_addr);
                match null::run_null_client(client_conn, server_conn).await {
                    Ok(_) => log::debug!("Stream from peer {} succeeded Null protocol", sock_addr),
                    Err(e) => log::debug!(
                        "Stream from peer {} failed during Null protocol: {}",
                        sock_addr,
                        e
                    ),
                }
            }
            Err(e) => {
                log::debug!(
                    "Stream from peer {} failed during Socks5 protocol: {}",
                    sock_addr,
                    e
                );
            }
        }
    }
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

    match run_server_main_loop(listener, server_conf).await {
        Ok(_) => {
            log::info!("UPGen client completed.");
            Ok(())
        }
        Err(e) => {
            log::info!("UPGen client completed with error: {}", e);
            Err(e)
        }
    }
}

async fn run_server_main_loop(listener: TcpListener, server_conf: ServerConfig) -> io::Result<()> {
    // Main loop waiting for connections from upgen proxy client.
    loop {
        let (pt_stream, sock_addr) = listener.accept().await?;

        log::debug!("Accepted new stream from peer {}", sock_addr);

        let tor_stream = tokio::net::TcpStream::connect(server_conf.forward_addr).await?;

        log::debug!("Connected to Tor at {}", tor_stream.peer_addr()?);

        let pt_conn = Connection::new(pt_stream);
        let tor_conn = Connection::new(tor_stream);

        match null::run_null_server(pt_conn, tor_conn).await {
            Ok(_) => log::debug!("Stream from peer {} succeeded Null protocol", sock_addr),
            Err(e) => log::debug!(
                "Stream from peer {} failed during Null protocol: {}",
                sock_addr,
                e
            ),
        }

        // TODO
        // match server_conf.forward_proto {
        //     ForwardProtocol::Basic => {
        //         // no special handshake required
        //     },
        //     ForwardProtocol::Extended(cookie_path) => {
        //         // or::run_extor_client(tor_conn).await
        //         todo!()
        //     }
        // }
    }
}
