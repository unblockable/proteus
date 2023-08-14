use std::{
    collections::HashMap,
    fmt,
    ops::Range,
    sync::{Arc, Mutex},
    task::Poll,
};

use anyhow::anyhow;
use bytes::{BufMut, Bytes, BytesMut};

use crate::crypto::{
    chacha::{Cipher, CipherKind},
    kdf,
};
use crate::net::{self, NetSource, NetSink};
use crate::lang::{
    common::Role,
    message::Message,
    spec::proteus::ProteusSpec,
    task::{Instruction, ReadNetLength, Task, TaskID, TaskProvider, TaskSet},
    types::{ConcreteFormat, Identifier},
};
use crate::net::Connection;

mod mem;
// #[cfg(test)]
// mod test;

pub struct Interpreter {
    spec: Box<dyn TaskProvider + Send + 'static>,
    options: HashMap<String, String>,
    /// Read buffer for data we are proxying. This is unobfuscated data
    /// typically read from a local process over a localhost connection.
    app_src: NetSource,
    /// Write buffer for data we are proxying. This is unobfuscated data
    /// typically written to a local process over a localhost connection.
    app_snk: NetSink,
    /// Read buffer for proteus protocol data. This is read from a proteus
    /// process typically running on a remote host, meaning that the data read
    /// here was observable by the censor.
    net_src: NetSource,
    /// Write buffer for proteus protocol data. This is written to a proteus
    /// process typically running on a remote host, meaning that the data
    /// written here will be observable to a censor.
    net_snk: NetSink,
    n_recv_app: usize,
    n_sent_app: usize,
    n_recv_net: usize,
    n_sent_net: usize,
}

impl Interpreter {
    pub fn new(
        proteus_conn: Connection,
        other_conn: Connection,
        spec: Box<dyn TaskProvider + Send + 'static>,
        options: HashMap<String, String>,
    ) -> Self {
        // Get the source and sink ends so that we can forward data in both
        // directions concurrently.
        let (net_src, net_snk) = proteus_conn.into_split();
        let (app_src, app_snk) = other_conn.into_split();

        Self {
            spec,
            options,
            app_src,
            app_snk,
            net_src,
            net_snk,
            n_recv_app: 0,
            n_sent_app: 0,
            n_recv_net: 0,
            n_sent_net: 0,
        }
    }

    /// Run the configured proteus protocol instance to completion. This returns
    /// when the proteus protocol terminates and all connections can be closed.
    pub async fn run(&mut self) -> anyhow::Result<()> {
        // let (net_src, net_snk) = self.proteus_conn.into_split();
        // let (app_src, app_snk) = self.other_conn.into_split();

        // let mut shared_int1 = SharedAsyncInterpreter::new(self.state.spec);
        // if let Err(e) = shared_int1.init().await {
        //     return RunResult::Error(proteus::Error::Protocol(e.to_string()).into());
        // }
        // let mut shared_int2 = shared_int1.clone();

        // match tokio::try_join!(
        //     obfuscate(app_source, net_sink, &mut shared_int1),
        //     deobfuscate(net_source, app_sink, &mut shared_int2),
        // ) {
        //     Ok(_) => RunResult::Success(Success {}.into()),
        //     Err(e) => RunResult::Error(e.into()),
        // }

        let start = self.spec.get_init_task();

        todo!()
    }

    async fn execute_task(&mut self, task: Task) -> anyhow::Result<()> {

        todo!()
    }

    async fn recv_app(&mut self, len: Range<usize>) -> anyhow::Result<Bytes> {
        log::trace!(
            "trying to receive {:?} bytes from app",
            len
        );

        let data = match self.app_src.read_bytes(len).await {
            Ok(data) => data,
            Err(e) => return Err(anyhow!("Error receiving from app: {e}"))
        };

        let n_bytes = data.len();
        self.n_recv_app += n_bytes;
        log::trace!("received {n_bytes} bytes from app");

        Ok(data.into())
    }

    async fn send_app(&mut self, bytes: Bytes,) -> anyhow::Result<usize> {
        log::trace!("trying to send {} bytes to app", bytes.len());

        let num_written = match self.app_snk.write_bytes(&bytes).await {
            Ok(num) => num,
            Err(e) => return  Err(anyhow!("Error sending to app: {e}"))
        };

        self.n_sent_app += num_written;
        log::trace!("sent {num_written} bytes to app");

        Ok(num_written)
    }

    async fn recv_net(&mut self, len: Range<usize>) -> anyhow::Result<Bytes> {
        log::trace!(
            "trying to receive {:?} bytes from net",
            len
        );

        let data = match self.net_src.read_bytes(len).await {
            Ok(data) => data,
            Err(e) => return Err(anyhow!("Error receiving from net: {e}"))
        };

        let n_bytes = data.len();
        self.n_recv_net += n_bytes;
        log::trace!("received {n_bytes} bytes from net");

        Ok(data.into())
    }

    async fn send_net(&mut self, bytes: Bytes,) -> anyhow::Result<usize> {
        log::trace!("trying to send {} bytes to net", bytes.len());

        let num_written = match self.app_snk.write_bytes(&bytes).await {
            Ok(num) => num,
            Err(e) => return  Err(anyhow!("Error sending to net: {e}"))
        };

        self.n_sent_net += num_written;
        log::trace!("sent {num_written} bytes to net");

        Ok(num_written)
    }
    
}
