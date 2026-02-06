use loader::Loader;
use tokio::io::{AsyncRead, AsyncWrite};
use vm::VirtualMachine;

use crate::lang;
use crate::lang::ir::bridge::TaskProvider;

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

#[derive(Debug)]
pub struct RunResult<NetR, AppR, NetW, AppW> {
    pub app_to_net: ForwardResult<AppR, NetW>,
    pub net_to_app: ForwardResult<NetR, AppW>,
}

#[derive(Debug)]
pub struct ForwardResult<R, W> {
    pub result: lang::Result<()>,
    pub src: R,
    pub dst: W,
}

impl<NetR, AppR, NetW, AppW> std::fmt::Display for RunResult<NetR, AppR, NetW, AppW> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "RunComplete(app_to_net: {:?}, net_to_app: {:?})",
            self.app_to_net.result, self.net_to_app.result
        )
    }
}

pub struct Interpreter {}

impl Interpreter {
    /// Run the configured proteus protocol instance to completion. This returns
    /// when the proteus protocol terminates and all connections can be closed.
    pub async fn run<NetR, AppR, NetW, AppW, T>(
        net_src: NetR,
        net_dst: NetW,
        app_src: AppR,
        app_dst: AppW,
        protospec: T,
    ) -> RunResult<NetR, AppR, NetW, AppW>
    where
        NetR: AsyncRead + Unpin,
        AppR: AsyncRead + Unpin,
        NetW: AsyncWrite + Unpin,
        AppW: AsyncWrite + Unpin,
        T: TaskProvider + Clone + Send,
    {
        // Buffers for data we are proxying. The inner src is unobfuscated data
        // maybe read from a local process over a localhost connection, while
        // the inner dst is to a proteus process typically running on a remote
        // host. The data written to the dst will be network-observable.
        let app_to_net = VirtualMachine::new(app_src, net_dst, None);

        // Buffers for data we are proxying. The inner src is from a proteus
        // process typically running on a remote host, while the inner dst is
        // is unobfuscated data maybe written to a local process over a localhost
        // connection. The data read from the src was network-observable.
        let net_to_app = VirtualMachine::new(net_src, app_dst, Some(app_to_net.share()));

        // Creates programs out of tasks from the protocol specification.
        let loader = Loader::new(protospec);

        // Execute both forwarding directions concurrently.
        let (app_to_net_res, net_to_app_res) = tokio::join!(
            Interpreter::execute(loader.clone(), app_to_net, ForwardingDirection::AppToNet),
            Interpreter::execute(loader, net_to_app, ForwardingDirection::NetToApp),
        );

        // ROB:
        // If we can return the net_src and net_dst objects, then the caller
        // can decide to just drop everything, or if just the channel failed
        // but the tunnel is OK, it can recover when using turbo mode by
        // reconnecting the channel and clearing the tunnel read buffer to make
        // sure we don't have any partial pending reads and to guarantee
        // message alignment.
        let result = RunResult {
            app_to_net: app_to_net_res,
            net_to_app: net_to_app_res,
        };

        log::info!("Interpreter completed with result: {result}");

        result
    }

    async fn execute<R, W, T>(
        mut loader: Loader<T>,
        mut vm: VirtualMachine<R, W>,
        direction: ForwardingDirection,
    ) -> ForwardResult<R, W>
    where
        R: AsyncRead + Unpin,
        W: AsyncWrite + Unpin,
        T: TaskProvider + Clone + Send,
    {
        loop {
            // Load a program for our direction, once one becomes available.
            let mut program = match loader.load(direction).await {
                Ok(prog) => prog,
                Err(e) => return make_result(vm, lang::Error::Anyhow(e)),
            };

            // Runs the program by executing its sequence of instructions.
            let exe_result = program.execute(&mut vm).await;

            // The loader needs to know that this program finished, even on error.
            let unload_result = loader.unload(program);

            if let Err(e) = exe_result {
                return make_result(vm, e);
            } else if let Err(e) = unload_result {
                return make_result(vm, lang::Error::Anyhow(e));
            }
        }
    }
}

fn make_result<R, W>(vm: VirtualMachine<R, W>, e: lang::Error) -> ForwardResult<R, W>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let (src, dst) = vm.into_inner();
    ForwardResult {
        result: Err(e),
        src,
        dst,
    }
}

#[cfg(test)]
mod tests {
    use crate::common::mock;
    use crate::lang::Role;
    use crate::lang::ir::test::basic::LengthPayloadSpec;
    use crate::lang::ir::test::basic_enc::EncryptedLengthPayloadSpec;

    #[tokio::test]
    async fn length_payload_unencrypted() {
        // let _ = env_logger::try_init();
        mock::test_protocol_interpretability(
            LengthPayloadSpec::new(Role::Client),
            LengthPayloadSpec::new(Role::Server),
        )
        .await
    }

    #[tokio::test]
    async fn length_payload_encrypted() {
        // let _ = env_logger::try_init();
        mock::test_protocol_interpretability(
            EncryptedLengthPayloadSpec::new(Role::Client),
            EncryptedLengthPayloadSpec::new(Role::Server),
        )
        .await
    }
}
