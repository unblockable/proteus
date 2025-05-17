use std::collections::HashMap;
use std::future::Future;

use bytes::{Bytes, BytesMut};
use rand::distributions::{Alphanumeric, DistString};
use tokio::io::{AsyncWriteExt, DuplexStream};

use crate::lang::interpreter::Interpreter;
use crate::lang::task::TaskProvider;
use crate::net::{BufReader, Connection, Reader};

pub type MockConnection = Connection<BufReader<DuplexStream>, DuplexStream>;
pub type MockPayload = Bytes;

pub struct Result {
    /// Holds the app payloads that were (read, written).
    pub client_app: anyhow::Result<(MockPayload, MockPayload)>,
    pub client_proxy: anyhow::Result<()>,
    pub server_proxy: anyhow::Result<()>,
    /// Holds the app payloads that were (read, written).
    pub server_app: anyhow::Result<(MockPayload, MockPayload)>,
}

pub fn connection_pair(max_buf_size: usize) -> (MockConnection, MockConnection) {
    let (client_w, server_r) = tokio::io::duplex(max_buf_size);
    let (server_w, client_r) = tokio::io::duplex(max_buf_size);

    let client = Connection::new(BufReader::new(client_r), client_w);
    let server = Connection::new(BufReader::new(server_r), server_w);

    (client, server)
}

pub fn payload(len: usize) -> MockPayload {
    let mut rng = rand::thread_rng();
    let s = Alphanumeric.sample_string(&mut rng, len);
    MockPayload::from(s)
}

async fn application_read(mut reader: BufReader<DuplexStream>) -> anyhow::Result<MockPayload> {
    let mut payload = BytesMut::new();
    loop {
        match reader.read_bytes(1..2usize.pow(12u32)).await {
            Ok(bytes) => payload.extend(bytes),
            Err(_) => break,
        }
    }
    Ok(payload.freeze())
}

async fn application_write(mut writer: DuplexStream, len: usize) -> anyhow::Result<MockPayload> {
    let payload = payload(len);
    writer.write_all(&payload[..]).await?;
    writer.shutdown().await?;
    Ok(payload)
}

async fn run_application(
    conn: MockConnection,
    write_len: usize,
) -> anyhow::Result<(MockPayload, MockPayload)> {
    let (reader, writer) = conn.into_split();
    let (r_result, w_result) = tokio::join!(
        application_read(reader),
        application_write(writer, write_len)
    );
    Ok((r_result.unwrap(), w_result.unwrap()))
}

async fn run_proxy_network<T, F, Fut>(
    client_spec: T,
    server_spec: T,
    run_proxy: F,
    payload_len: usize,
) -> self::Result
where
    T: TaskProvider + Send,
    F: Fn(T, MockConnection, MockConnection) -> Fut,
    Fut: Future<Output = anyhow::Result<()>>,
{
    // We set up a mock network that represents the following:
    // c_app <--> c_proxy <--> s_proxy <--> s_app
    let (c_app_to_proxy, c_proxy_to_app) = connection_pair(payload_len);
    let (c_proxy_to_proxy, s_proxy_to_proxy) = connection_pair(payload_len);
    let (s_app_to_proxy, s_proxy_to_app) = connection_pair(payload_len);

    let (c_app_res, c_proxy_res, s_proxy_res, s_app_res) = tokio::join!(
        run_application(c_app_to_proxy, payload_len),
        run_proxy(client_spec, c_proxy_to_proxy, c_proxy_to_app),
        run_proxy(server_spec, s_proxy_to_proxy, s_proxy_to_app),
        run_application(s_app_to_proxy, payload_len),
    );

    self::Result {
        client_app: c_app_res,
        client_proxy: c_proxy_res,
        server_proxy: s_proxy_res,
        server_app: s_app_res,
    }
}

async fn run_interpreter<T: TaskProvider + Clone + Send>(
    protospec: T,
    net_conn: MockConnection,
    app_conn: MockConnection,
) -> anyhow::Result<()> {
    Interpreter::run(
        net_conn,
        app_conn,
        protospec,
        HashMap::<String, String>::new(),
    )
    .await
}

pub async fn check_protocol_interpretability<T>(
    client: T,
    server: T,
    payload_len: usize,
) -> self::Result
where
    T: TaskProvider + Clone + Send,
{
    run_proxy_network(client, server, &run_interpreter, payload_len).await
}

#[cfg(test)]
pub mod tests {
    use tokio::io::DuplexStream;

    use crate::common::mock;
    use crate::lang::task::{Task, TaskID, TaskProvider, TaskSet};

    use super::{MockConnection, MockPayload};

    pub fn payload_len_iter() -> impl Iterator<Item = usize> {
        [
            1, 10, 100, 1000, 1500, 2000, 5000, 10_000, 100_000, 1_000_000,
        ]
        .into_iter()
    }

    async fn forward(mut src: DuplexStream, mut dst: DuplexStream) -> anyhow::Result<u64> {
        Ok(tokio::io::copy(&mut src, &mut dst).await?)
    }

    async fn run_io_copier<T: TaskProvider + Send>(
        _: T,
        net_conn: MockConnection,
        app_conn: MockConnection,
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

    fn assert_payload_result(a: MockPayload, b: MockPayload, len: usize) {
        if len > 0 {
            assert!(a.len() > 0);
            assert!(b.len() > 0);
        }
        assert_eq!(a.len(), len);
        assert_eq!(b.len(), len);
        assert_eq!(&a[..], &b[..]);
    }

    #[tokio::test]
    async fn connection() {
        for len in payload_len_iter() {
            let (c, s) = mock::connection_pair(len);

            let (c_reader, c_writer) = c.into_split();
            let (s_reader, s_writer) = s.into_split();

            let c_sent = mock::application_write(c_writer, len).await.unwrap();
            let s_recv = mock::application_read(s_reader).await.unwrap();

            assert_payload_result(c_sent, s_recv, len);

            let s_sent = mock::application_write(s_writer, len).await.unwrap();
            let c_recv = mock::application_read(c_reader).await.unwrap();

            assert_payload_result(s_sent, c_recv, len);
        }
    }

    #[tokio::test]
    async fn processes() {
        for len in payload_len_iter() {
            let (c, s) = mock::connection_pair(len);

            let (c_result, s_result) =
                tokio::join!(mock::run_application(c, len), mock::run_application(s, len),);

            let (c_recv, c_sent) = c_result.unwrap();
            let (s_recv, s_sent) = s_result.unwrap();

            assert_payload_result(c_sent, s_recv, len);
            assert_payload_result(s_sent, c_recv, len);
        }
    }

    fn assert_mock_result(result: mock::Result, len: usize) {
        let (c_recv, c_sent) = result.client_app.unwrap();
        let (s_recv, s_sent) = result.server_app.unwrap();

        assert_payload_result(c_sent, s_recv, len);
        assert_payload_result(s_sent, c_recv, len);
    }

    pub async fn test_protocol_interpretability<T>(client: T, server: T)
    where
        T: TaskProvider + Clone + Send,
    {
        for len in payload_len_iter() {
            let result =
                mock::check_protocol_interpretability(client.clone(), server.clone(), len).await;
            assert_mock_result(result, len)
        }
    }

    // We use a null spec below because `run_io_copier` doesn't need it to test
    // the mock facilities.
    struct NullSpec {}

    impl TaskProvider for NullSpec {
        fn get_init_task(&self) -> Task {
            unimplemented!()
        }

        fn get_next_tasks(&self, _: &TaskID) -> TaskSet {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn proxy_network() {
        for len in payload_len_iter() {
            let result =
                mock::run_proxy_network(NullSpec {}, NullSpec {}, &run_io_copier, len).await;
            assert_mock_result(result, len)
        }
    }
}
