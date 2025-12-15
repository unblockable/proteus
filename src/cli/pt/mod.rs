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
use crate::lang::ir::bridge::{OldCompile, TaskProvider};
use crate::net::proto::socks::address::Socks5Target;
use crate::net::proto::{BytesSession, socks};
use crate::net::{
    AsyncConnectExt, Channel, FixedTargetTcpConnector, TcpConnector, TunnelClient, TunnelEofMethod,
    TunnelServer, fmt_stream_name,
};

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

async fn handle_client_connection(app_stream: TcpStream, _conf: ClientConfig) {
    let peer_name = fmt_stream_name(&app_stream);

    log::debug!("Accepted new stream from client {peer_name}");
    let (mut app_src, mut app_dst) = app_stream.into_split();

    match socks::run_socks5_server(&mut app_src, &mut app_dst).await {
        Ok(info) => {
            log::debug!("Socks5 with peer {peer_name} succeeded");

            if !info.remaining_read_buf.is_empty() {
                log::error!(
                    "Socks5 buffer has {} bytes remaining",
                    info.remaining_read_buf.len()
                );
                // TODO: can we maybe do something like this?
                // app_src = Cursor::new(info.remaining_read_buf).chain(app_src);
                return;
            }

            let target = info.target.clone();

            let options = match info.creds {
                Some(creds) => {
                    log::debug!("Obtained Socks5 username: {}", creds.username);
                    let mut map = HashMap::new();
                    for entry in creds.username.split(';').collect::<Vec<&str>>() {
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
                "Running Proteus client protocol to forward data from {peer_name} to proxy server at {target}",
            );

            // Normally this connection would have been done during the SOCKS handshake,
            // so that we could return a SOCKS error if the connection fails.
            // We currently removed that from our SOCKS impl to handle other modes.
            log::debug!("Will need to connect network tunnel to proxy server {target}",);

            if false {
                // TODO run a wrapped tunnel session.
                let net = Channel::disconnected(target, TcpConnector::default());
                let mut app: TunnelClient<BytesSession<_, _>> =
                    TunnelClient::new(TunnelEofMethod::OnStreamCount(1));
                app.add_session(app_src, app_dst, None).await;
                super::run_interpreter(net.clone(), net, app.clone(), app, client_spec).await;
            } else {
                // Use the TcpStream io directly without wrappers.
                match TcpConnector::default().connect(target.clone()).await {
                    Ok((net_src, net_dst, name)) => {
                        log::debug!("Successfully connected to proxy: {name}");
                        // To isolate testing the channel without a tunnel, uncomment this:
                        // let c = Channel::connected(net_src, net_dst, name);
                        // let (net_src, net_dst) = (c.clone(), c);
                        super::run_interpreter(net_src, net_dst, app_src, app_dst, client_spec)
                            .await;
                    }
                    Err(e) => {
                        log::warn!("Failed to connect to Socks5 proxy target {target}: {e}",);
                    }
                }
            }
        }
        Err(e) => {
            log::debug!("Stream from peer {peer_name} failed during Socks5 protocol: {e}");
        }
    }
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

async fn handle_server_connection<T>(net_stream: TcpStream, conf: ServerConfig, server_spec: T)
where
    T: TaskProvider + Clone + Send,
{
    let peer_name = fmt_stream_name(&net_stream);

    log::debug!("Accepted new network stream from Proteus client {peer_name}");

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

    let (net_src, net_dst) = net_stream.into_split();

    let target = Socks5Target::from(conf.forward_addr);

    if false {
        // TODO run a wrapped tunnel session.
        let connector = FixedTargetTcpConnector::new(target);
        let app: TunnelServer<BytesSession<_, _>, _> =
            TunnelServer::new(TunnelEofMethod::OnStreamCount(1), connector);
        let net = Channel::connected(net_src, net_dst, peer_name);
        super::run_interpreter(net.clone(), net, app.clone(), app, server_spec).await;
    } else {
        // Use the TcpStream io directly without wrappers.
        match TcpConnector::default().connect(target.clone()).await {
            Ok((app_src, app_dst, name)) => {
                log::debug!("Successfully connected to forward target: {name}");
                super::run_interpreter(net_src, net_dst, app_src, app_dst, server_spec).await;
            }
            Err(e) => {
                log::warn!("Failed to connect to configured forward target {target}: {e}",);
            }
        }
    }
}
