use std::marker::PhantomData;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use anyhow::bail;

use crate::lang::interpreter::Interpreter;
use crate::lang::ir::bridge::TaskProvider;
use crate::net::{Connection, Connector, Reader, Reconnector, Writer};

mod formatter;
mod frames;

#[derive(Clone)]
pub struct SharedTunnelState {
    _inner: Arc<Mutex<Option<TunnelState>>>,
}

impl Default for SharedTunnelState {
    fn default() -> Self {
        Self {
            _inner: Arc::new(Mutex::new(None)),
        }
    }
}

pub struct TunnelState {
    _peer: SocketAddr,
}

#[derive(Clone)]
pub struct TunnelClient<R: Reader, W: Writer, C: Reconnector<R, W>> {
    connector: C,
    _shared: SharedTunnelState,
    _phantom_r: PhantomData<R>,
    _phantom_w: PhantomData<W>,
}

impl<R: Reader, W: Writer, C: Reconnector<R, W>> TunnelClient<R, W, C> {
    pub fn new(connector: C) -> Self {
        Self {
            connector,
            _shared: SharedTunnelState::default(),
            _phantom_r: PhantomData,
            _phantom_w: PhantomData,
        }
    }

    pub async fn run_session<R2, W2, T>(
        self,
        app_conn: Connection<R2, W2>,
        protospec: T,
    ) -> anyhow::Result<()>
    where
        R2: Reader,
        W2: Writer,
        T: TaskProvider + Clone + Send,
    {
        let (net_conn, local_addr) = self.connector.reconnect().await?;
        log::debug!("Connected to tunnel server {}", local_addr);
        Interpreter::run(net_conn, app_conn, protospec).await
    }
}

#[derive(Clone)]
pub struct TunnelServer<R: Reader, W: Writer, C: Connector<R, W>> {
    connector: C,
    target: Option<SocketAddr>,
    _shared: SharedTunnelState,
    _phantom_r: PhantomData<R>,
    _phantom_w: PhantomData<W>,
}

impl<R: Reader, W: Writer, C: Connector<R, W>> TunnelServer<R, W, C> {
    pub fn new(connector: C) -> Self {
        Self {
            connector,
            target: None,
            _shared: SharedTunnelState::default(),
            _phantom_r: PhantomData,
            _phantom_w: PhantomData,
        }
    }

    pub fn replace_target(&mut self, target: SocketAddr) -> Option<SocketAddr> {
        self.target.replace(target)
    }

    pub async fn run_session<R2, W2, T>(
        self,
        net_conn: Connection<R2, W2>,
        protospec: T,
    ) -> anyhow::Result<()>
    where
        R2: Reader,
        W2: Writer,
        T: TaskProvider + Clone + Send,
    {
        let Some(target_addr) = self.target else {
            bail!("No target address specified")
        };

        let (app_conn, local_addr) = self.connector.connect(target_addr).await?;
        log::debug!("Connected to target server {}", local_addr);
        Interpreter::run(net_conn, app_conn, protospec).await
    }
}
