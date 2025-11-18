use std::net::{IpAddr, Ipv4Addr};
use std::{fmt, io};

use address::Socks5Address;
use anyhow::bail;
use bytes::BytesMut;
use futures::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::{FramedRead, FramedWrite};

use crate::net::proto::socks;
use crate::net::proto::socks::address::Socks5Target;
use crate::net::proto::socks::codec::Socks5Codec;
use crate::net::proto::socks::message::{
    Choice, ConnectRequest, ConnectResponse, Greeting, Message, UserPassAuthRequest,
    UserPassAuthResponse,
};

pub mod address;
pub mod codec;
mod message;

const SOCKS_NULL: u8 = 0x00;
const SOCKS_VERSION_5: u8 = 0x05;
const SOCKS_AUTH_NONE: u8 = 0x00;
const SOCKS_AUTH_USERPASS: u8 = 0x02;
const SOCKS_AUTH_UNSUPPORTED: u8 = 0xff;
const SOCKS_AUTH_USERPASS_VERSION: u8 = 0x01;
const SOCKS_AUTH_STATUS_SUCCESS: u8 = 0x00;
const SOCKS_AUTH_STATUS_FAILURE: u8 = 0x01;
const SOCKS_COMMAND_CONNECT: u8 = 0x01;
const SOCKS_STATUS_REQ_GRANTED: u8 = 0x00;
const SOCKS_STATUS_GEN_FAILURE: u8 = 0x01;
const SOCKS_STATUS_PROTO_ERR: u8 = 0x07;
const SOCKS_STATUS_ADDR_ERR: u8 = 0x08;

pub struct Socks5Info {
    pub target: Socks5Target,
    pub creds: Option<SocksAuthCredentials>,
    pub remaining_read_buf: BytesMut,
}

pub struct SocksAuthCredentials {
    pub username: String,
    pub password: String,
}

enum Error {
    Version,
    Reserved,
    AuthMethod,
    AuthUserPassVersion,
    AuthUsernameEmpty,
    AuthPasswordEmpty,
    ConnectMethod,
    ConnectAddress,
    Other(io::Error),
}

impl From<io::Error> for socks::Error {
    fn from(e: io::Error) -> Self {
        Error::Other(e)
    }
}

impl fmt::Display for socks::Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Version => write!(f, "Socks version mismatch"),
            Error::Reserved => write!(f, "Socks non-zero reserved field"),
            Error::AuthMethod => write!(f, "No supported authentication methods"),
            Error::AuthUserPassVersion => {
                write!(f, "User/pass authentication failed: invalid version")
            }
            Error::AuthUsernameEmpty => {
                write!(f, "User/pass authentication failed: empty username")
            }
            Error::AuthPasswordEmpty => {
                write!(f, "User/pass authentication failed: empty password")
            }
            Error::ConnectMethod => write!(f, "No supported connect methods"),
            Error::ConnectAddress => write!(f, "Address type not supported"),
            Error::Other(e) => write!(f, "Other IO error: {}", e),
        }
    }
}

pub async fn run_socks5_server<R, W>(app_r: &mut R, app_w: &mut W) -> anyhow::Result<Socks5Info>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut reader = FramedRead::new(app_r, Socks5Codec::new());
    let mut writer = FramedWrite::new(app_w, Socks5Codec::new());

    // Initial handshake.
    let greeting = next_message::<R, Greeting>(&mut reader).await?;
    let choice = prepare_choice(greeting)?;
    send_message(&mut writer, choice.clone()).await?;

    // Username/password authentication is an optional, second handshake round.
    let maybe_creds = if choice.auth_method == SOCKS_AUTH_USERPASS {
        let auth_req = next_message::<R, UserPassAuthRequest>(&mut reader).await?;

        let auth_result = check_auth_request(&auth_req);
        let auth_resp = prepare_auth_response(auth_result.is_ok());
        send_message(&mut writer, auth_resp).await?;

        match auth_result {
            Ok(_) => Some(SocksAuthCredentials {
                username: auth_req.username,
                password: auth_req.password,
            }),
            Err(e) => bail!("{e}"),
        }
    } else {
        None
    };

    // Handshake is done. Now handle the main connection request.
    let conn_req = next_message::<R, ConnectRequest>(&mut reader).await?;

    match check_connect_request(&conn_req) {
        Ok(target) => {
            let resp = prepare_connect_response(SOCKS_STATUS_REQ_GRANTED);
            send_message(&mut writer, resp).await?;
            Ok(Socks5Info {
                target,
                creds: maybe_creds,
                remaining_read_buf: reader.read_buffer().clone(),
            })
        }
        Err(e) => {
            let resp = prepare_connect_response(get_connect_error_status(&e));
            send_message(&mut writer, resp).await?;
            bail!("{e}")
        }
    }
}

async fn next_message<R, T>(reader: &mut FramedRead<&mut R, Socks5Codec>) -> anyhow::Result<T>
where
    R: AsyncRead + Unpin,
    T: Default + TryFrom<Message> + fmt::Debug,
    Message: From<T>,
{
    // What kind of message are we trying to decode next.
    let msg_kind = Message::from(T::default()).kind();

    // Update the decoder state so it knows how to decode.
    reader.decoder_mut().set_next_decode(msg_kind);

    // Decode the next message and unwrap into the inner type T.
    let inner = match reader.next().await {
        Some(result) => match result {
            Ok(msg) => match T::try_from(msg) {
                Ok(inner) => inner,
                Err(_) => bail!("Socks5Codec decoded message was incorrect variant"),
            },
            Err(e) => bail!("Socks5Codec decode error: {e}"),
        },
        None => bail!("Socks5Codec stream unexpectedly closed"),
    };

    log::debug!("Read Socks5 message {:?}", inner);
    Ok(inner)
}

async fn send_message<W, T>(
    writer: &mut FramedWrite<&mut W, Socks5Codec>,
    inner: T,
) -> anyhow::Result<()>
where
    W: AsyncWrite + Unpin,
    T: fmt::Debug,
    Message: From<T>,
{
    log::debug!("Writing Socks5 message {:?}", inner);
    writer.send(Message::from(inner)).await?;
    Ok(())
}

fn prepare_choice(greeting: Greeting) -> anyhow::Result<Choice> {
    // Must be socks version 5, or we close the connection without a response.
    if greeting.version != SOCKS_VERSION_5 {
        bail!("{}", Error::Version);
    }

    // Check the auth methods supported by the client.
    let methods = greeting.supported_auth_methods;

    // We support user/pass or none; prefer user/pass.
    let auth_method = if methods.contains(&SOCKS_AUTH_USERPASS) {
        log::debug!("Choosing username/password authentication");
        SOCKS_AUTH_USERPASS
    } else if methods.contains(&SOCKS_AUTH_NONE) {
        log::debug!("Choosing no authentication");
        SOCKS_AUTH_NONE
    } else {
        log::debug!("Authentication methods are unsupported");
        SOCKS_AUTH_UNSUPPORTED
    };

    Ok(Choice {
        version: SOCKS_VERSION_5,
        auth_method,
    })
}

fn check_auth_request(req: &UserPassAuthRequest) -> Result<(), self::Error> {
    if req.version != SOCKS_AUTH_USERPASS_VERSION {
        log::debug!("Incorrect version in authentication request");
        Err(Error::AuthUserPassVersion)
    } else if req.username.is_empty() {
        log::debug!("Empty username in authentication request");
        Err(Error::AuthUsernameEmpty)
    } else if req.password.is_empty() {
        log::debug!("Empty password in authentication request");
        Err(Error::AuthPasswordEmpty)
    } else {
        log::debug!("Authentication request OK");
        Ok(())
    }
}

fn prepare_auth_response(is_success: bool) -> UserPassAuthResponse {
    let status = if is_success {
        SOCKS_AUTH_STATUS_SUCCESS
    } else {
        SOCKS_AUTH_STATUS_FAILURE
    };

    UserPassAuthResponse {
        version: SOCKS_AUTH_USERPASS_VERSION,
        status,
    }
}

fn check_connect_request(request: &ConnectRequest) -> Result<Socks5Target, self::Error> {
    if request.version != SOCKS_VERSION_5 {
        return Err(Error::Version);
    }

    if request.command != SOCKS_COMMAND_CONNECT {
        return Err(Error::ConnectMethod);
    }

    if request.reserved != SOCKS_NULL {
        return Err(Error::Reserved);
    }

    match request.dest_addr {
        Socks5Address::Unknown => Err(Error::ConnectAddress),
        _ => Ok(Socks5Target::new(
            request.dest_addr.clone(),
            request.dest_port,
        )),
    }
}

fn get_connect_error_status(error: &self::Error) -> u8 {
    match error {
        Error::ConnectAddress => SOCKS_STATUS_ADDR_ERR,
        Error::ConnectMethod => SOCKS_STATUS_PROTO_ERR,
        _ => SOCKS_STATUS_GEN_FAILURE,
    }
}

fn prepare_connect_response(status: u8) -> ConnectResponse {
    let bind_addr = if status == SOCKS_STATUS_REQ_GRANTED {
        // Normally we would set to addr:port of outgoing connection, but we defer
        // the connection attempt to a later stage, and it might change if we
        // eventually need to reconnect. I think using zeros here is common.
        Socks5Address::IpAddr(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)))
    } else {
        Socks5Address::Unknown
    };

    ConnectResponse {
        version: SOCKS_VERSION_5,
        status,
        reserved: SOCKS_NULL,
        bind_addr,
        bind_port: 0,
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    // use async_trait::async_trait;
    use bytes::Bytes;
    use tokio_test::io::{Builder, Mock};

    use super::*;
    // use crate::net::AsyncConnect;

    pub struct MockConnector {}

    impl MockConnector {
        fn default_addr() -> SocketAddr {
            // We return zeros for our localhost bind address.
            "0.0.0.0:0".parse().expect("Valid socket addr")
        }
    }

    // #[async_trait]
    // impl AsyncConnect<Mock, Mock> for MockConnector {
    //     async fn connect(&self) -> anyhow::Result<(Mock, Mock, SocketAddr)> {
    //         Ok((
    //             Builder::new().build(),
    //             Builder::new().build(),
    //             MockConnector::default_addr(),
    //         ))
    //     }
    // }

    fn greeting(auth_method: u8) -> Greeting {
        Greeting {
            version: SOCKS_VERSION_5,
            num_auth_methods: 1,
            supported_auth_methods: Bytes::from(vec![auth_method]),
        }
    }

    fn choice(auth_method: u8) -> Choice {
        Choice {
            version: SOCKS_VERSION_5,
            auth_method,
        }
    }

    fn userpass_auth_request() -> UserPassAuthRequest {
        UserPassAuthRequest {
            version: SOCKS_AUTH_USERPASS_VERSION,
            username: String::from("my_username"),
            password: String::from("my_password"),
        }
    }

    fn userpass_auth_response() -> UserPassAuthResponse {
        UserPassAuthResponse {
            version: SOCKS_AUTH_USERPASS_VERSION,
            status: SOCKS_AUTH_STATUS_SUCCESS,
        }
    }

    fn connect_request() -> ConnectRequest {
        let sock_addr: SocketAddr = "127.0.0.1:54321".parse().expect("Valid socket addr");
        ConnectRequest {
            version: SOCKS_VERSION_5,
            command: SOCKS_COMMAND_CONNECT,
            reserved: SOCKS_NULL,
            dest_addr: Socks5Address::IpAddr(sock_addr.ip()),
            dest_port: sock_addr.port(),
        }
    }

    fn connect_response() -> ConnectResponse {
        let sock_addr = MockConnector::default_addr();
        ConnectResponse {
            version: SOCKS_VERSION_5,
            status: SOCKS_STATUS_REQ_GRANTED,
            reserved: SOCKS_NULL,
            bind_addr: Socks5Address::IpAddr(sock_addr.ip()),
            bind_port: sock_addr.port(),
        }
    }

    // #[tokio::test]
    // async fn userpass_auth_method() {
    //     let reader = Builder::new()
    //         .read(&greeting(SOCKS_AUTH_USERPASS).serialize())
    //         .read(&userpass_auth_request().serialize())
    //         .read(&connect_request().serialize())
    //         .build();
    //     let writer = Builder::new()
    //         .write(&choice(SOCKS_AUTH_USERPASS).serialize())
    //         .write(&userpass_auth_response().serialize())
    //         .write(&connect_response().serialize())
    //         .build();

    //     let s = run_socks5_server(&mut reader, &mut writer).await;
    //     assert!(s.is_ok())
    // }

    // #[tokio::test]
    // async fn none_auth_method() {
    //     let reader = Builder::new()
    //         .read(&greeting(SOCKS_AUTH_NONE).serialize())
    //         .read(&connect_request().serialize())
    //         .build();
    //     let writer = Builder::new()
    //         .write(&choice(SOCKS_AUTH_NONE).serialize())
    //         .write(&connect_response().serialize())
    //         .build();
    //     let conn = Connection::new(BufReader::new(reader), writer);

    //     let s = run_socks5_server(conn).await;
    //     assert!(s.is_ok())
    // }

    // #[tokio::test]
    // async fn unsupported_auth_method() {
    //     let reader = Builder::new()
    //         .read(&greeting(SOCKS_AUTH_UNSUPPORTED).serialize())
    //         .build();
    //     let writer = Builder::new()
    //         .write(&choice(SOCKS_AUTH_UNSUPPORTED).serialize())
    //         .build();
    //     let conn = Connection::new(BufReader::new(reader), writer);

    //     let s = run_socks5_server(conn).await;
    //     assert!(s.is_err())
    // }
}
