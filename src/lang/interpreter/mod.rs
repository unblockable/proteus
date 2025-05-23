use std::collections::HashMap;

use forwarder::Forwarder;
use loader::Loader;

use crate::lang::ir::bridge::TaskProvider;
use crate::net::{Connection, Reader, Writer};

mod forwarder;
mod loader;
mod memory;
pub mod vm;

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
        T: TaskProvider + Clone + Send,
    {
        // Get the source and sink ends so that we can forward data in both
        // directions concurrently.
        let (net_src, net_dst) = net_conn.into_split();
        let (app_src, app_dst) = app_conn.into_split();

        // Buffers for data we are proxying. The inner src is unobfuscated data
        // maybe read from a local process over a localhost connection, while
        // the inner dst is to a proteus process typically running on a remote
        // host. The data written to the dst will be network-observable.
        let app_to_net = Forwarder::new(app_src, net_dst, None);

        // Buffers for data we are proxying. The inner src is from a proteus
        // process typically running on a remote host, while the inner dst is
        // is unobfuscated data maybe written to a local process over a localhost
        // connection. The data read from the src was network-observable.
        let net_to_app = Forwarder::new(net_src, app_dst, Some(app_to_net.share()));

        // Creates programs out of tasks from the protocol specification.
        let loader = Loader::new(protospec);

        // Execute both forwarding directions concurrently.
        let (_, _) = tokio::join!(
            Interpreter::execute(loader.clone(), app_to_net, ForwardingDirection::AppToNet),
            Interpreter::execute(loader, net_to_app, ForwardingDirection::NetToApp),
        );
        Ok(())
    }

    async fn execute<R, W, T>(
        mut loader: Loader<T>,
        mut forwarder: Forwarder<R, W>,
        direction: ForwardingDirection,
    ) -> anyhow::Result<()>
    where
        R: Reader,
        W: Writer,
        T: TaskProvider + Clone + Send,
    {
        loop {
            // Load a program for our direction, once one becomes available.
            let mut program = loader.load(direction).await?;
            // Runs the program by executing its sequence of instructions.
            let exe_result = program.execute(&mut forwarder).await;
            // The loader needs to know that this program finished, even on error.
            let unload_result = loader.unload(program);

            if exe_result.is_err() {
                return exe_result;
            } else if unload_result.is_err() {
                return unload_result;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::common::mock;
    use crate::lang::ir::test::basic::LengthPayloadSpec;
    use crate::lang::ir::test::basic_enc::EncryptedLengthPayloadSpec;
    use crate::lang::Role;

    #[tokio::test]
    async fn length_payload_unencrypted() {
        mock::tests::test_protocol_interpretability(
            LengthPayloadSpec::new(Role::Client),
            LengthPayloadSpec::new(Role::Server),
        )
        .await
    }

    #[tokio::test]
    async fn length_payload_encrypted() {
        mock::tests::test_protocol_interpretability(
            EncryptedLengthPayloadSpec::new(Role::Client),
            EncryptedLengthPayloadSpec::new(Role::Server),
        )
        .await
    }
}
