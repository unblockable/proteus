use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::bail;
use forwarder::Forwarder;
use loader::{Loader, LoaderResult};

use crate::lang::task::TaskProvider;
use crate::net::{Connection, Reader, Writer};

mod forwarder;
mod loader;
mod program;

#[derive(Clone, Copy, Debug)]
pub enum ForwardingDirection {
    AppToNet,
    NetToApp,
}

pub struct Interpreter {}

impl Interpreter {
    /// Run the configured proteus protocol instance to completion. This returns
    /// when the proteus protocol terminates and all connections can be closed.
    pub async fn run<R, W, T>(
        net_conn: Connection<R, W>,
        app_conn: Connection<R, W>,
        protospec: T,
        _options: HashMap<String, String>,
    ) -> anyhow::Result<()>
    where
        R: Reader,
        W: Writer,
        T: TaskProvider + Send,
    {
        // Get the source and sink ends so that we can forward data in both
        // directions concurrently.
        let (net_src, net_dst) = net_conn.into_split();
        let (app_src, app_dst) = app_conn.into_split();

        // Buffers for data we are proxying. The inner src is unobfuscated data
        // typically read from a local process over a localhost connection, while
        // the inner dst is to a proteus process typically running on a remote
        // host. The data written to the dst will be network-observable.
        let app_to_net = Forwarder::new(app_src, net_dst, None);

        // Buffers for data we are proxying. The inner src is from a proteus
        // process typically running on a remote host, while the inner dst is
        // is unobfuscated data typically written to a local process over a localhost connection.
        // The data read from the src was network-observable.
        let net_to_app = Forwarder::new(net_src, app_dst, Some(app_to_net.share()));

        // Creates programs out of tasks from the protocol specification.
        let loader = Arc::new(Mutex::new(Loader::new(protospec)));

        // Execute both forwarding directions concurrently.
        match tokio::try_join!(
            Interpreter::execute(loader.clone(), app_to_net, ForwardingDirection::AppToNet),
            Interpreter::execute(loader, net_to_app, ForwardingDirection::NetToApp),
        ) {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        }
    }

    async fn execute<R, W, T>(
        loader: Arc<Mutex<Loader<T>>>,
        mut forwarder: Forwarder<R, W>,
        direction: ForwardingDirection,
    ) -> anyhow::Result<()>
    where
        R: Reader,
        W: Writer,
        T: TaskProvider + Send,
    {
        loop {
            let loader_result = {
                match loader.lock() {
                    Ok(mut l) => l.next(direction),
                    Err(e) => bail!("Loader mutex was poisoned: {}", e.to_string()),
                }
            };

            // TODO: the other side should be able to wake us up if our side is pending.
            // Not sure how to do that yet, so for now I'm using `tokio::time::sleep`.
            // We should remove that, and also remove the "time" feature from the
            // tokio crate in Cargo.toml.
            match loader_result {
                LoaderResult::Ready(mut program) => program.execute(&mut forwarder).await?,
                LoaderResult::Pending => tokio::time::sleep(Duration::from_millis(10)).await,
            }
        }
    }
}
