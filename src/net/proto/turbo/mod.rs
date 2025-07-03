use std::marker::PhantomData;
use std::net::SocketAddr;

use anyhow::bail;

use crate::lang::interpreter::Interpreter;
use crate::lang::ir::bridge::TaskProvider;
use crate::net::proto::turbo::session::Session;
use crate::net::{Connection, Connector, Reader, Writer};

mod formatter;
mod frames;
pub mod session;

#[derive(Clone)]
pub struct TurboClient<R, W, C>
where
    R: Reader + Send,
    W: Writer + Send,
    C: Connector<R, W>,
{
    connector: C,
    _phantom_r: PhantomData<R>,
    _phantom_w: PhantomData<W>,
}

impl<R, W, C> TurboClient<R, W, C>
where
    R: Reader + Send,
    W: Writer + Send,
    C: Connector<R, W>,
{
    pub fn new(connector: C) -> Self {
        Self {
            connector,
            _phantom_r: PhantomData,
            _phantom_w: PhantomData,
        }
    }

    pub async fn run_session<T>(
        self,
        app_conn: Connection<R, W>,
        protospec: T,
    ) -> anyhow::Result<()>
    where
        T: TaskProvider + Clone + Send,
    {
        let (net_conn, local_addr) = self.connector.connect().await?;
        log::debug!("Connected to tunnel server {}", local_addr);

        let (net_src, net_dst) = net_conn.into_split();
        let (app_src, app_dst) = Session::new(app_conn).into_split();

        Interpreter::run_split(net_src, net_dst, app_src, app_dst, protospec).await
    }
}

#[derive(Clone)]
pub struct TurboServer<R, W, C>
where
    R: Reader + Send,
    W: Writer + Send,
    C: Connector<R, W>,
{
    connector: C,
    target: Option<SocketAddr>,
    _phantom_r: PhantomData<R>,
    _phantom_w: PhantomData<W>,
}

impl<R, W, C> TurboServer<R, W, C>
where
    R: Reader + Send,
    W: Writer + Send,
    C: Connector<R, W>,
{
    pub fn new(connector: C) -> Self {
        Self {
            connector,
            target: None,
            _phantom_r: PhantomData,
            _phantom_w: PhantomData,
        }
    }

    pub fn replace_target(&mut self, target: SocketAddr) -> Option<SocketAddr> {
        self.target.replace(target)
    }

    pub async fn run_session<T>(
        mut self,
        net_conn: Connection<R, W>,
        protospec: T,
    ) -> anyhow::Result<()>
    where
        T: TaskProvider + Clone + Send,
    {
        let Some(target_addr) = self.target else {
            bail!("No target address specified")
        };
        self.connector = self.connector.into_self(target_addr);

        let (app_conn, local_addr) = self.connector.connect().await?;
        log::debug!("Connected to target server {}", local_addr);

        let (net_src, net_dst) = net_conn.into_split();
        let (app_src, app_dst) = Session::new(app_conn).into_split();

        Interpreter::run_split(net_src, net_dst, app_src, app_dst, protospec).await
    }
}

#[cfg(test)]
mod tests {
    // TODO
}
