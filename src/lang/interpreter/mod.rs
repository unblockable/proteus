use loader::Loader;
use supertunnel::proto::ReliabilityHandle;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CleanupAction {
    /// Call shutdown to propagate valid EOF and let the other forwarder finish.
    ShutdownThenOk,
    /// Flush and shutdown the socket, but stop both forwarders.
    ShutdownThenErr,
    _Ok,
    /// Stops both forwarders, but don't shutdown so we can still re-use the socket.
    Err,
}

fn eof_shutdown_handler(error: &Error) -> CleanupAction {
    match error {
        Error::AppToNet(lang::Error::Io(io::Error::Eof)) => CleanupAction::ShutdownThenOk,
        Error::NetToApp(lang::Error::Io(io::Error::Eof)) => CleanupAction::ShutdownThenOk,
        _ => CleanupAction::ShutdownThenErr,
    }
}

async fn reliable_shutdown_handler(error: &Error, handle: &mut ReliabilityHandle) -> CleanupAction {
    // Check if this error is caused by the network-side connection.
    #[allow(clippy::match_like_matches_macro)]
    let action = match error {
        Error::NetToApp(lang::Error::Io(io::Error::Eof))
        | Error::AppToNet(lang::Error::Io(io::Error::Eof)) => {
            if handle.is_shutdown_complete().await {
                CleanupAction::ShutdownThenOk
            } else {
                CleanupAction::Err
            }
        }
        Error::NetToApp(lang::Error::Io(io::Error::Read(_)))
        | Error::AppToNet(lang::Error::Io(io::Error::Write(_))) => CleanupAction::Err,
        Error::AppToNet(lang::Error::Io(io::Error::Read(_)))
        | Error::NetToApp(lang::Error::Io(io::Error::Write(_))) => CleanupAction::ShutdownThenErr,
        _ => CleanupAction::ShutdownThenErr,
    };

    log::debug!("Taking action {action:?} in response to error {error:?}");

    action
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
    pub async fn run_join(&mut self) -> (Result<usize>, Result<usize>) {
        let result = tokio::join!(self.fwd_app_to_net.run(None), self.fwd_net_to_app.run(None));

        self.log_completion(&format!("{:?}", result.0), &format!("{:?}", result.1));

        result
    }

    /// Runs the interpreter in both forwarding directions concurrently until
    /// **both** forwarding operation completes, or one returns an error. On
    /// error, it returns the error of the first operation that failed after
    /// cancelling the other operation.
    pub async fn run_try_join(&mut self) -> Result<(usize, usize)> {
        self.run_try_join_internal(None).await
    }

    pub async fn run_try_join_with(&mut self, handle: ReliabilityHandle) -> Result<(usize, usize)> {
        self.run_try_join_internal(Some(handle)).await
    }

    async fn run_try_join_internal(
        &mut self,
        handle: Option<ReliabilityHandle>,
    ) -> Result<(usize, usize)> {
        let result = tokio::try_join!(
            self.fwd_app_to_net.run(handle.clone()),
            self.fwd_net_to_app.run(handle)
        );

        let (a2n_res, n2a_res) = match &result {
            Ok((a2n, n2a)) => (format!("Ok({a2n})"), format!("Ok({n2a})")),
            Err(Error::AppToNet(e)) => (format!("Err({e:?})"), "Cancelled".to_string()),
            Err(Error::NetToApp(e)) => ("Cancelled".to_string(), format!("Err({e:?})")),
        };

        self.log_completion(&a2n_res, &n2a_res);

        result
    }

    fn log_completion(&self, app_to_net_res: &str, net_to_app_res: &str) {
        let ((app_in, net_out), (net_in, app_out)) = self.num_bytes_forwarded();

        log::info!(
            "Interpreter done: \
            AppToNet({}➡{}, {}), \
            NetToApp({}➡{}, {})",
            app_in,
            net_out,
            app_to_net_res,
            net_in,
            app_out,
            net_to_app_res
        );
    }

    fn num_bytes_forwarded(&self) -> ((usize, usize), (usize, usize)) {
        (
            (
                self.fwd_app_to_net.vm.num_bytes_recv(),
                self.fwd_app_to_net.vm.num_bytes_sent(),
            ),
            (
                self.fwd_net_to_app.vm.num_bytes_recv(),
                self.fwd_net_to_app.vm.num_bytes_sent(),
            ),
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

    async fn run(&mut self, handle: Option<ReliabilityHandle>) -> Result<usize> {
        loop {
            // Load a program for our direction, once one becomes available.
            let mut program = match self.loader.load(self.direction).await {
                Ok(prog) => prog,
                Err(e) => return self.handle_error(lang::Error::Anyhow(e), handle).await,
            };

            // Runs the program by executing its sequence of instructions.
            let exe_result = program.execute(&mut self.vm).await;

            // The loader needs to know that this program finished, even on error.
            let unload_result = self.loader.unload(program);

            if let Err(e) = exe_result {
                return self.handle_error(e, handle).await;
            } else if let Err(e) = unload_result {
                return self.handle_error(lang::Error::Anyhow(e), handle).await;
            }
        }
    }

    async fn handle_error(
        &mut self,
        error: lang::Error,
        handle: Option<ReliabilityHandle>,
    ) -> Result<usize> {
        log::trace!("Forwarder direction {:?} error {error:?}", self.direction);
        let ierror = Error::new(error, self.direction);

        let result = match handle {
            Some(mut handle) => reliable_shutdown_handler(&ierror, &mut handle).await,
            None => eof_shutdown_handler(&ierror),
        };

        match result {
            CleanupAction::ShutdownThenOk => {
                let _ = self.vm.shutdown().await;
                Ok(self.vm.num_bytes_sent())
            }
            CleanupAction::ShutdownThenErr => {
                let _ = self.vm.shutdown().await;
                Err(ierror)
            }
            CleanupAction::_Ok => Ok(self.vm.num_bytes_sent()),
            CleanupAction::Err => Err(ierror),
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
