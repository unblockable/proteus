use loader::Loader;
use tokio::io::{AsyncRead, AsyncWrite};
use vm::VirtualMachine;

use crate::lang::ir::bridge::TaskProvider;
use crate::lang::{self, Runtime};

pub mod crypto;
pub mod io;
mod loader;
pub mod mem;
pub mod program;
mod vm;

#[derive(Clone, Copy, Debug)]
pub enum ForwardingDirection {
    AppToNet,
    NetToApp,
}

pub type Result<T> = std::result::Result<T, self::Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Error in app to net direction: {0}")]
    AppToNet(lang::Error),
    #[error("Error in net to app direction: {0}")]
    NetToApp(lang::Error),
}

impl Error {
    fn new(err: lang::Error, dir: ForwardingDirection) -> Self {
        match dir {
            ForwardingDirection::AppToNet => Self::AppToNet(err),
            ForwardingDirection::NetToApp => Self::NetToApp(err),
        }
    }
}

impl From<Error> for lang::Error {
    fn from(value: Error) -> Self {
        match value {
            Error::AppToNet(e) => e,
            Error::NetToApp(e) => e,
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ErrorHandler {
    pub raise_err_on_app_eof: bool,
    pub raise_err_on_net_eof: bool,
    pub shutdown_net_on_app_eof: bool,
    pub shutdown_app_on_net_eof: bool,
}

impl ErrorHandler {
    /// Initialize a builder with all flags set to false by default.
    pub fn builder() -> Self {
        Self::default()
    }

    /// Configures the handler to raise an error when it observes an EOF on the
    /// application reader.
    pub fn raise_err_on_app_eof(mut self) -> Self {
        self.raise_err_on_app_eof = true;
        self
    }

    /// Configures the handler to raise an error when it observes an EOF on the
    /// network reader.
    pub fn raise_err_on_net_eof(mut self) -> Self {
        self.raise_err_on_net_eof = true;
        self
    }

    /// Configures the handler to shut down the network writer when it observes
    /// an application EOF.
    pub fn shutdown_net_on_app_eof(mut self) -> Self {
        self.shutdown_net_on_app_eof = true;
        self
    }

    /// Configures the handler to shut down the application writer when it
    /// observes a network EOF.
    pub fn shutdown_app_on_net_eof(mut self) -> Self {
        self.shutdown_app_on_net_eof = true;
        self
    }
}

pub struct Interpreter<NetR, AppR, NetW, AppW, T>
where
    NetR: AsyncRead + Unpin,
    AppR: AsyncRead + Unpin,
    NetW: AsyncWrite + Unpin,
    AppW: AsyncWrite + Unpin,
    T: TaskProvider + Clone + Send,
{
    fwd_app_to_net: Forwarder<AppR, NetW, T>,
    fwd_net_to_app: Forwarder<NetR, AppW, T>,
}

impl<NetR, AppR, NetW, AppW, T> Interpreter<NetR, AppR, NetW, AppW, T>
where
    NetR: AsyncRead + Unpin,
    AppR: AsyncRead + Unpin,
    NetW: AsyncWrite + Unpin,
    AppW: AsyncWrite + Unpin,
    T: TaskProvider + Clone + Send,
{
    pub fn new(app_src: AppR, app_dst: AppW, net_src: NetR, net_dst: NetW, protospec: T) -> Self {
        // Buffers for data we are proxying. The inner src is unobfuscated data
        // maybe read from a local process over a localhost connection, while
        // the inner dst is to a proteus process typically running on a remote
        // host. The data written to the dst will be network-observable.
        let app_to_net_vm = VirtualMachine::new(app_src, net_dst, None);

        // Buffers for data we are proxying. The inner src is from a proteus
        // process typically running on a remote host, while the inner dst is
        // is unobfuscated data maybe written to a local process over a localhost
        // connection. The data read from the src was network-observable.
        let net_to_app_vm = VirtualMachine::new(net_src, app_dst, Some(app_to_net_vm.share()));

        // Creates programs out of tasks from the protocol specification.
        let app_to_net_loader = Loader::new(protospec);
        let net_to_app_loader = app_to_net_loader.clone();

        Self {
            fwd_app_to_net: Forwarder::new(
                app_to_net_loader,
                app_to_net_vm,
                ForwardingDirection::AppToNet,
            ),
            fwd_net_to_app: Forwarder::new(
                net_to_app_loader,
                net_to_app_vm,
                ForwardingDirection::NetToApp,
            ),
        }
    }

    /// Runs the interpreter in both forwarding directions concurrently until
    /// **both** forwarding operations complete. Returns the result of each
    /// operation as `(app_to_net_result, net_to_app_result)`.
    pub async fn run_join(&mut self, handler: ErrorHandler) -> (Result<usize>, Result<usize>) {
        let result = tokio::join!(
            self.fwd_app_to_net.run(
                handler.raise_err_on_app_eof,
                handler.shutdown_net_on_app_eof
            ),
            self.fwd_net_to_app.run(
                handler.raise_err_on_net_eof,
                handler.shutdown_app_on_net_eof
            )
        );

        log::info!(
            "Interpreter result: app_to_net: {:?}, net_to_app: {:?}",
            result.0,
            result.1
        );

        result
    }

    /// Runs the interpreter in both forwarding directions concurrently until
    /// **both** forwarding operation completes, or one returns an error. On
    /// error, it returns the error of the first operation that failed after
    /// cancelling the other operation.
    pub async fn run_try_join(&mut self, handler: ErrorHandler) -> Result<(usize, usize)> {
        let result = tokio::try_join!(
            self.fwd_app_to_net.run(
                handler.raise_err_on_app_eof,
                handler.shutdown_net_on_app_eof
            ),
            self.fwd_net_to_app.run(
                handler.raise_err_on_net_eof,
                handler.shutdown_app_on_net_eof
            )
        );

        log::info!("Interpreter result: {:?}", result);

        result
    }

    pub fn num_bytes_sent(&self) -> (usize, usize) {
        (
            self.fwd_app_to_net.vm.num_bytes_sent(),
            self.fwd_net_to_app.vm.num_bytes_sent(),
        )
    }

    pub fn into_inner(self) -> (AppR, AppW, NetR, NetW) {
        let (app_reader, net_writer) = self.fwd_app_to_net.vm.into_io();
        let (net_reader, app_writer) = self.fwd_net_to_app.vm.into_io();
        (app_reader, app_writer, net_reader, net_writer)
    }
}

struct Forwarder<R, W, T>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
    T: TaskProvider + Clone + Send,
{
    loader: Loader<T>,
    vm: VirtualMachine<R, W>,
    direction: ForwardingDirection,
}

impl<R, W, T> Forwarder<R, W, T>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
    T: TaskProvider + Clone + Send,
{
    fn new(loader: Loader<T>, vm: VirtualMachine<R, W>, direction: ForwardingDirection) -> Self {
        Self {
            loader,
            vm,
            direction,
        }
    }

    async fn run(&mut self, err_on_eof: bool, shutdown_on_eof: bool) -> Result<usize> {
        loop {
            // Load a program for our direction, once one becomes available.
            let mut program = match self.loader.load(self.direction).await {
                Ok(prog) => prog,
                Err(e) => return Err(Error::new(lang::Error::Anyhow(e), self.direction)),
            };

            // Runs the program by executing its sequence of instructions.
            let exe_result = program.execute(&mut self.vm).await;

            // The loader needs to know that this program finished, even on error.
            let unload_result = self.loader.unload(program);

            if let Err(e) = exe_result {
                return self.handle_error(e, err_on_eof, shutdown_on_eof).await;
            } else if let Err(e) = unload_result {
                return self
                    .handle_error(lang::Error::Anyhow(e), err_on_eof, shutdown_on_eof)
                    .await;
            }
        }
    }

    async fn handle_error(
        &mut self,
        error: lang::Error,
        err_on_eof: bool,
        shutdown_on_eof: bool,
    ) -> Result<usize> {
        // Note: we might get an EOF as a result of network interference. The
        // caller should configure the shutdown mode if it wants to recover.
        match error {
            lang::Error::Io(io::Error::Eof) => {
                if shutdown_on_eof {
                    let _ = self.vm.shutdown().await;
                }
                if err_on_eof {
                    Err(Error::new(error, self.direction))
                } else {
                    Ok(self.vm.num_bytes_sent())
                }
            }
            _ => Err(Error::new(error, self.direction)),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::lang::Role;
    use crate::lang::ir::test::basic::LengthPayloadSpec;
    use crate::lang::ir::test::basic_enc::EncryptedLengthPayloadSpec;
    use crate::util;

    #[tokio::test]
    async fn length_payload_unencrypted() {
        // let _ = env_logger::try_init();
        util::test_protocol_interpretability(
            LengthPayloadSpec::new(Role::Client),
            LengthPayloadSpec::new(Role::Server),
        )
        .await
    }

    #[tokio::test]
    async fn length_payload_encrypted() {
        // let _ = env_logger::try_init();
        util::test_protocol_interpretability(
            EncryptedLengthPayloadSpec::new(Role::Client),
            EncryptedLengthPayloadSpec::new(Role::Server),
        )
        .await
    }
}
