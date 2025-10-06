use std::io;
use std::net::SocketAddr;
use std::str::FromStr;

use anyhow::anyhow;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};

use crate::cli::args::{ClientArgs, ServerArgs};
use crate::lang::Role;
use crate::lang::compiler::Compiler;
use crate::lang::interpreter::Interpreter;
use crate::lang::ir::bridge::OldCompile;
use crate::net::proto::socks;
use crate::net::proto::turbo::TurboSession;
use crate::net::{BufReader, Connection, Connector, TcpConnector};

use super::args::SocksArgs;

pub async fn run(args: SocksArgs) -> anyhow::Result<()> {
    log::info!("Running in socks mode");

    let psf_path = args
        .protocol
        .to_str()
        .ok_or(anyhow!("Path is not valid UTF-8"))?
        .to_string();

    match args.role {
        crate::cli::args::Role::Client(client_args) => run_client(client_args, psf_path).await?,
        crate::cli::args::Role::Server(server_args) => run_server(server_args, psf_path).await?,
    }

    log::info!("Proteus completed, exiting now.");
    Ok(())
}

async fn run_client(client_args: ClientArgs, psf_path: String) -> io::Result<()> {
    // We run our socks5 forward proxy here; let the OS choose the port.
    let listener = TcpListener::bind(client_args.listen).await?;

    log::info!(
        "Proteus client listening for SOCKS5 app connections on {:?}.",
        listener.local_addr()?
    );

    // Main loop waiting for connections from reverse socks5 clients.
    loop {
        let (app_stream, _) = listener.accept().await?;
        let connect = client_args.connect.clone();
        let path = psf_path.clone();

        // A failure in a connection does not stop the server.
        tokio::spawn(async move { handle_client_connection(app_stream, connect, path).await });
    }
}

async fn handle_client_connection(
    app_stream: TcpStream,
    connect: String,
    psf_path: String,
) -> io::Result<()> {
    let app_addr = app_stream.peer_addr()?;
    log::debug!("Accepted new stream from client {app_addr}");

    let spec = Compiler::parse_path(&psf_path, Role::Client).unwrap();

    match socks::run_socks5_server(Connection::from(app_stream)).await {
        Ok((app_conn, _, _, dest_addr)) => {
            log::debug!("Socks5 with peer {app_addr} succeeded");

            log::debug!("Connecting network tunnel to target {connect}");
            let target_addr = SocketAddr::from_str(connect.as_str()).unwrap();
            let net_connector = TcpConnector::from(target_addr);

            match net_connector.connect().await {
                Ok((net_conn, local_addr)) => {
                    log::debug!("Connection to {target_addr} succeeded (bound to {local_addr})");

                    let (net_src, net_dst) = net_conn.into_split();
                    let (app_src, app_dst) =
                        TurboSession::new_connected_client(app_conn, Some(dest_addr));

                    match Interpreter::run_split(net_src, net_dst, app_src, app_dst, spec).await {
                        Ok(_) => log::debug!(
                            "Stream from peer {app_addr} succeeded over tunnel {target_addr}",
                        ),
                        Err(e) => log::debug!(
                            "Stream from peer {app_addr} failed over tunnel {target_addr}: {e}",
                        ),
                    }
                }
                Err(e) => {
                    log::debug!("Connection to {target_addr} failed: {e}");
                }
            }
        }
        Err(e) => {
            log::debug!("Stream from peer {app_addr} failed during Socks5 protocol: {e}");
        }
    }

    Ok(())
}

async fn run_server(server_args: ServerArgs, psf_path: String) -> io::Result<()> {
    log::info!("Proteus is running in server mode.");

    let listener = TcpListener::bind(server_args.listen).await?;

    log::info!(
        "Proteus server listening for Proteus client connections on {:?}.",
        listener.local_addr()?
    );

    // Main loop waiting for connections from proteus proxy clients.
    loop {
        let (net_stream, _) = listener.accept().await?;
        let path = psf_path.clone();
        // A failure in a connection does not stop the server.
        tokio::spawn(async move { handle_server_connection(net_stream, path).await });
    }
}

async fn handle_server_connection(net_stream: TcpStream, psf_path: String) -> anyhow::Result<()> {
    let net_addr = net_stream.peer_addr()?;
    log::debug!("Accepted new stream from Proteus client {net_addr}");

    let spec = Compiler::parse_path(&psf_path, Role::Server).unwrap();

    let net_conn = Connection::from(net_stream);
    let (net_src, net_dst) = net_conn.into_split();
    let (app_src, app_dst) = TurboSession::new_server::<BufReader<OwnedReadHalf>, OwnedWriteHalf>();

    // Run a new server session over the tunnel.
    match Interpreter::run_split(net_src, net_dst, app_src, app_dst, spec).await {
        Ok(_) => {
            log::debug!("Stream from peer {net_addr} succeeded",)
        }
        Err(e) => {
            log::debug!("Stream from peer {net_addr} failed: {e}",)
        }
    }

    Ok(())
}
