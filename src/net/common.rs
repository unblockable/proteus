use std::fmt::Debug;
use std::time::Duration;

use tokio::net::{TcpListener, TcpStream, ToSocketAddrs};

pub async fn connect_timeout<A>(target: A, timeout: Duration) -> std::io::Result<TcpStream>
where
    A: ToSocketAddrs + Clone + Debug,
{
    match tokio::time::timeout(timeout, TcpStream::connect(target.clone())).await {
        Ok(result) => match result {
            Ok(outbound) => {
                log::debug!(
                    "Successfully connected to {target:?}: {}",
                    fmt_stream_name(&outbound)
                );
                Ok(outbound)
            }
            Err(e) => {
                log::warn!("Connection to {target:?} failed with error {e}.");
                Err(e)
            }
        },
        Err(_) => {
            log::warn!("Connection to {target:?} timed out.");
            Err(std::io::ErrorKind::TimedOut.into())
        }
    }
}

pub fn fmt_stream_name(stream: &TcpStream) -> String {
    let peer = match stream.peer_addr() {
        Ok(addr) => format!("{addr}"),
        Err(_) => "unknown".to_string(),
    };
    let local = match stream.local_addr() {
        Ok(addr) => format!("{addr}"),
        Err(_) => "unknown".to_string(),
    };
    format!("[{local}]->[{peer}]")
}

pub fn fmt_listener_name(listener: &TcpListener) -> String {
    let local = match listener.local_addr() {
        Ok(addr) => format!("{addr}"),
        Err(_) => "unknown".to_string(),
    };
    format!("[{local}]")
}
