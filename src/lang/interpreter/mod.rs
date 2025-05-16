use std::collections::HashMap;

use forwarder::Forwarder;
use loader::Loader;

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
    use std::collections::HashMap;
    use std::future::Future;

    use bytes::{Bytes, BytesMut};
    use tokio::io::{AsyncWriteExt, DuplexStream};

    use crate::lang::interpreter::Interpreter;
    use crate::lang::parse::proteus::ProteusParser;
    use crate::lang::parse::Parse;
    use crate::lang::spec::proteus::ProteusSpec;
    use crate::lang::spec::test::basic::LengthPayloadSpec;
    use crate::lang::spec::test::basic_enc::EncryptedLengthPayloadSpec;
    use crate::lang::task::TaskProvider;
    use crate::lang::Role;
    use crate::net::tests::{create_mock_connection_pair, generate_payload};
    use crate::net::{BufReader, Connection, Reader};

    async fn application_read(mut reader: BufReader<DuplexStream>) -> anyhow::Result<Bytes> {
        let mut payload = BytesMut::new();
        loop {
            match reader.read_bytes(1..2usize.pow(12u32)).await {
                Ok(bytes) => payload.extend(bytes),
                Err(_) => break,
            }
        }
        Ok(payload.freeze())
    }

    async fn application_write(mut writer: DuplexStream) -> anyhow::Result<Bytes> {
        let payload = generate_payload(100..100_000);
        writer.write_all(&payload[..]).await?;
        writer.shutdown().await?;
        Ok(payload)
    }

    async fn run_application(
        conn: Connection<BufReader<DuplexStream>, DuplexStream>,
    ) -> anyhow::Result<(Bytes, Bytes)> {
        let (reader, writer) = conn.into_split();
        let (r_result, w_result) =
            tokio::join!(application_read(reader), application_write(writer));
        Ok((r_result.unwrap(), w_result.unwrap()))
    }

    fn assert_payload_result(a: Bytes, b: Bytes) {
        assert!(a.len() > 0);
        assert_eq!(a.len(), b.len());
        assert_eq!(&a[..], &b[..]);
    }

    async fn run_proxy_network<T, F, Fut>(
        client_spec: Option<T>,
        server_spec: Option<T>,
        run_proxy: F,
    ) where
        T: TaskProvider + Send,
        F: Fn(
            Option<T>,
            Connection<BufReader<DuplexStream>, DuplexStream>,
            Connection<BufReader<DuplexStream>, DuplexStream>,
        ) -> Fut,
        Fut: Future<Output = anyhow::Result<()>>,
    {
        let (c_client, s_client) = create_mock_connection_pair();
        let (c_net, s_net) = create_mock_connection_pair();
        let (c_server, s_server) = create_mock_connection_pair();

        let (client_result, _, _, server_result) = tokio::join!(
            run_application(c_client),
            run_proxy(client_spec, c_net, s_client),
            run_proxy(server_spec, s_net, s_server),
            run_application(c_server),
        );

        let (c_recv_payload, c_sent_payload) = client_result.unwrap();
        let (s_recv_payload, s_sent_payload) = server_result.unwrap();

        assert_payload_result(c_sent_payload, s_recv_payload);
        assert_payload_result(s_sent_payload, c_recv_payload);
    }

    async fn forward(mut src: DuplexStream, mut dst: DuplexStream) -> anyhow::Result<u64> {
        Ok(tokio::io::copy(&mut src, &mut dst).await?)
    }

    async fn run_io_copier<T: TaskProvider + Send>(
        _protospec: Option<T>,
        net_conn: Connection<BufReader<DuplexStream>, DuplexStream>,
        app_conn: Connection<BufReader<DuplexStream>, DuplexStream>,
    ) -> anyhow::Result<()> {
        let (net_r, net_w) = net_conn.into_split();
        let (app_r, app_w) = app_conn.into_split();
        // Unwrap the BufReader too.
        let net_r = net_r.into_inner();
        let app_r = app_r.into_inner();
        // We need to move the streams into `forward()` so that the DuplexStreams close
        // when the tokio::io::copy function receives EOF and returns. Otherwise the EOF
        // does not properly propagate backward.
        let (_, _) = tokio::join!(forward(app_r, net_w), forward(net_r, app_w));
        Ok(())
    }

    async fn run_interpreter<T: TaskProvider + Clone + Send>(
        protospec: Option<T>,
        net_conn: Connection<BufReader<DuplexStream>, DuplexStream>,
        app_conn: Connection<BufReader<DuplexStream>, DuplexStream>,
    ) -> anyhow::Result<()> {
        Interpreter::run(
            net_conn,
            app_conn,
            protospec.expect("This test needs a valid protospec"),
            HashMap::<String, String>::new(),
        )
        .await
    }

    #[tokio::test]
    async fn test_mock_connection() {
        let (c, s) = create_mock_connection_pair();

        let (c_reader, c_writer) = c.into_split();
        let (s_reader, s_writer) = s.into_split();

        let c_writer_payload = application_write(c_writer).await.unwrap();
        let s_reader_payload = application_read(s_reader).await.unwrap();

        assert_payload_result(c_writer_payload, s_reader_payload);

        let s_writer_payload = application_write(s_writer).await.unwrap();
        let c_reader_payload = application_read(c_reader).await.unwrap();

        assert_payload_result(s_writer_payload, c_reader_payload);
    }

    #[tokio::test]
    async fn test_mock_processes() {
        let (c, s) = create_mock_connection_pair();

        let (c_result, s_result) = tokio::join!(run_application(c), run_application(s),);

        let (c_recv_payload, c_sent_payload) = c_result.unwrap();
        let (s_recv_payload, s_sent_payload) = s_result.unwrap();

        assert_payload_result(c_sent_payload, s_recv_payload);
        assert_payload_result(s_sent_payload, c_recv_payload);
    }

    #[tokio::test]
    async fn test_mock_proxies() {
        run_proxy_network(None::<ProteusSpec>, None::<ProteusSpec>, &run_io_copier).await;
    }

    async fn test_protocol<T: TaskProvider + Clone + Send>(client_spec: T, server_spec: T) {
        run_proxy_network(Some(client_spec), Some(server_spec), &run_interpreter).await;
    }

    #[tokio::test]
    async fn integration_static_basic() {
        test_protocol(
            LengthPayloadSpec::new(Role::Client),
            LengthPayloadSpec::new(Role::Server),
        )
        .await
    }

    #[tokio::test]
    async fn integration_static_basic_enc() {
        test_protocol(
            EncryptedLengthPayloadSpec::new(Role::Client),
            EncryptedLengthPayloadSpec::new(Role::Server),
        )
        .await
    }

    async fn integration_with_psf(psf_filepath: &str) {
        test_protocol(
            ProteusParser::parse_path(&psf_filepath, Role::Client).unwrap(),
            ProteusParser::parse_path(&psf_filepath, Role::Server).unwrap(),
        )
        .await
    }

    #[tokio::test]
    async fn integration_psf_basic() {
        integration_with_psf(&"tests/fixtures/simple.psf").await
    }

    #[tokio::test]
    async fn integration_psf_basic_enc() {
        integration_with_psf(&"tests/fixtures/shadowsocks.psf").await
    }

    #[tokio::test]
    async fn integration_psf_padded_enc() {
        integration_with_psf(&"tests/fixtures/shadowsocks_padded.psf").await
    }

    #[tokio::test]
    async fn integration_psf_handshake_no_payload() {
        integration_with_psf(&"tests/fixtures/handshake_no_payload.psf").await
    }

    #[tokio::test]
    async fn integration_psf_handshake_with_payload() {
        integration_with_psf(&"tests/fixtures/handshake_with_payload.psf").await
    }

    #[tokio::test]
    async fn integration_psf_tls_mimic() {
        integration_with_psf(&"tests/fixtures/tls_mimic.psf").await
    }

    #[tokio::test]
    async fn integration_psf_random() {
        integration_with_psf(&"tests/fixtures/random.psf").await
    }

    #[tokio::test]
    async fn integration_psf_random_noauth() {
        integration_with_psf(&"tests/fixtures/random_noauth.psf").await
    }

    #[tokio::test]
    async fn integration_psf_padding() {
        integration_with_psf(&"tests/fixtures/padding.psf").await
    }

    #[tokio::test]
    async fn integration_psf_key() {
        integration_with_psf(&"tests/fixtures/key.psf").await
    }

    #[tokio::test]
    async fn integration_separate_length_field() {
        integration_with_psf(&"tests/fixtures/separate.psf").await
    }
}
