use std::fmt;

use anyhow::bail;
use formatter::Formatter;
use frames::{Choice, ClientNonce, Greeting, ServerHashNonce, ServerStatus};

use crate::net::{self, proto::or, Connection};

mod formatter;
mod frames;

pub enum Error {
    AuthMethod,
    AuthStatusFailed,
    AuthStatusUnknown,
    Auth(String),
    Address(String),
    Transport(String),
    Command(String),
    Network(net::Error),
}

impl From<net::Error> for or::Error {
    fn from(e: net::Error) -> Self {
        Error::Network(e)
    }
}

impl fmt::Display for or::Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::AuthMethod => write!(f, "Ext OR authentication method unsupported"),
            Error::AuthStatusFailed => write!(f, "Ext OR authentication failed"),
            Error::AuthStatusUnknown => write!(f, "Ext OR authentication status unknown"),
            Error::Auth(s) => write!(f, "Chosen Ext OR authentication method failed: {}", s),
            Error::Address(s) => write!(f, "User address denied: {}", s),
            Error::Transport(s) => write!(f, "Transport denied: {}", s),
            Error::Command(s) => write!(f, "Command denied: {}", s),
            Error::Network(e) => write!(f, "Network error: {}", e),
        }
    }
}

pub async fn run_extor_client(conn: Connection) -> anyhow::Result<Connection> {
    let mut proto = Init::new(conn)
        .start_client()
        .recv_greeting()
        .await?
        .send_choice()
        .await?
        .send_nonce()
        .await?
        .recv_nonce_hash()
        .await?
        .send_hash()
        .await?
        .recv_status()
        .await?;

    // Keep processing commands until done.
    loop {
        proto = match proto.send_command().await?.recv_reply().await? {
            CommandOrDone::Command(s) => s,
            CommandOrDone::Done(c) => return Ok(c),
        };
    }
}

const EXTOR_AUTH_TYPE_SAFE_COOKIE: u8 = 0x01;
const EXTOR_AUTH_TYPE_END: u8 = 0x00;
const EXTOR_AUTH_STATUS_SUCCESS: u8 = 0x01;
const EXTOR_AUTH_STATUS_FAILURE: u8 = 0x00;
const EXTOR_COMMAND_DONE: u16 = 0x0000;
const EXTOR_COMMAND_USERADDR: u16 = 0x0001;
const EXTOR_COMMAND_TRANSPORT: u16 = 0x0002;
const EXTOR_REPLY_OK: u16 = 0x1000;
const EXTOR_REPLY_DENY: u16 = 0x1001;

struct Init {
    conn: Connection,
    fmt: Formatter,
}
struct ClientHandshake1 {
    conn: Connection,
    fmt: Formatter,
}
struct ClientHandshake2 {
    conn: Connection,
    fmt: Formatter,
    greeting: Greeting,
}
struct ClientAuth1 {
    conn: Connection,
    fmt: Formatter,
}
struct ClientAuth2 {
    conn: Connection,
    fmt: Formatter,
    client_auth: ClientNonce,
}
struct ClientAuth3 {
    conn: Connection,
    fmt: Formatter,
    client_auth: ClientNonce,
    server_auth: ServerHashNonce,
}
struct ClientAuth4 {
    conn: Connection,
    fmt: Formatter,
}
struct ClientCommand1 {
    conn: Connection,
    fmt: Formatter,
}
struct ClientCommand2 {
    conn: Connection,
    fmt: Formatter,
}
enum CommandOrDone {
    Command(ClientCommand1),
    Done(Connection),
}

impl Init {
    fn new(conn: Connection) -> Init {
        Init {
            conn,
            fmt: Formatter::new(),
        }
    }

    fn start_client(self) -> ClientHandshake1 {
        ClientHandshake1 {
            conn: self.conn,
            fmt: self.fmt,
        }
    }
}

impl ClientHandshake1 {
    async fn recv_greeting(mut self) -> anyhow::Result<ClientHandshake2> {
        log::debug!("Waiting for greeting");

        match self
            .conn
            .read_frame::<Greeting, Formatter>(&mut self.fmt)
            .await
        {
            Ok(greeting) => Ok(ClientHandshake2 {
                conn: self.conn,
                fmt: self.fmt,
                greeting,
            }),
            Err(net_err) => bail!(net_err),
        }
    }
}

impl ClientHandshake2 {
    async fn send_choice(mut self) -> anyhow::Result<ClientAuth1> {
        let types = self.greeting.auth_types;

        // We only support safe cookie.
        if let Some(_) = types
            .iter()
            .find(|&&val| val == EXTOR_AUTH_TYPE_SAFE_COOKIE)
        {
            log::debug!("Choosing SAFE_COOKIE authentication");

            let choice = Choice {
                auth_type: EXTOR_AUTH_TYPE_SAFE_COOKIE,
            };

            match self
                .conn
                .write_frame::<Choice, Formatter>(&mut self.fmt, choice)
                .await
            {
                Ok(_) => Ok(ClientAuth1 {
                    conn: self.conn,
                    fmt: self.fmt,
                }),
                Err(net_err) => bail!("{}", Error::from(net_err)),
            }
        } else {
            log::debug!("Authentication methods are unsupported");

            let choice = Choice {
                auth_type: EXTOR_AUTH_TYPE_END,
            };

            // Do not propagate any net error; the or error is more precise.
            match self
                .conn
                .write_frame::<Choice, Formatter>(&mut self.fmt, choice)
                .await
            {
                Ok(_) => log::debug!("Success writing choice failure message"),
                Err(e) => log::debug!("Error writing choice failure message: {}", e),
            }

            bail!("{}", or::Error::AuthMethod);
        }
    }
}

impl ClientAuth1 {
    async fn send_nonce(mut self) -> anyhow::Result<ClientAuth2> {
        let client_auth = ClientNonce {
            nonce: [1; 32], // XXX fill with random data
        };

        match self
            .conn
            .write_frame::<ClientNonce, Formatter>(&mut self.fmt, client_auth.clone())
            .await
        {
            Ok(_) => Ok(ClientAuth2 {
                conn: self.conn,
                fmt: self.fmt,
                client_auth,
            }),
            Err(net_err) => bail!("{}", Error::from(net_err)),
        }
    }
}

impl ClientAuth2 {
    async fn recv_nonce_hash(mut self) -> anyhow::Result<ClientAuth3> {
        log::debug!("Waiting for server auth nonce and hash");

        match self
            .conn
            .read_frame::<ServerHashNonce, Formatter>(&mut self.fmt)
            .await
        {
            Ok(server_auth) => Ok(ClientAuth3 {
                conn: self.conn,
                fmt: self.fmt,
                client_auth: self.client_auth,
                server_auth,
            }),
            Err(net_err) => bail!("{}", Error::from(net_err)),
        }
    }
}

impl ClientAuth3 {
    async fn send_hash(mut self) -> anyhow::Result<ClientAuth4> {
        // WIP: we have this state:
        // self.state.conn;
        // self.state.server_auth; // server nonce and hash
        // self.state.client_auth; // client nonce

        // now we need to read CookieString from the EXTOR port file from the config
        // we may need to go back and actually pass that path in when calling run_protocol()
        // and then propagate it to here.

        // then the client must compute ServerHash as:
        //   HMAC-SHA256(CookieString,
        //     "ExtORPort authentication server-to-client hash" | ClientNonce | ServerNonce)

        // then validate that the above matches what the server sent
        // in self.state.server_auth

        // if invalid, we must move to error state and terminate the connection

        // if valid, we compute ClientHash as:
        //   HMAC-SHA256(CookieString,
        //     "ExtORPort authentication client-to-server hash" | ClientNonce | ServerNonce)

        // and send to server, then move to next state (ClientAuth4)

        todo!()
    }
}

impl ClientAuth4 {
    async fn recv_status(mut self) -> anyhow::Result<ClientCommand1> {
        log::debug!("Waiting for server auth status");

        match self
            .conn
            .read_frame::<ServerStatus, Formatter>(&mut self.fmt)
            .await
        {
            Ok(auth_result) => {
                // Check the auth status.
                match auth_result.status {
                    EXTOR_AUTH_STATUS_SUCCESS => {
                        log::debug!("Server auth status succeeded");
                        Ok(ClientCommand1 {
                            conn: self.conn,
                            fmt: self.fmt,
                        })
                    }
                    EXTOR_AUTH_STATUS_FAILURE => {
                        log::debug!("Server auth status failed");
                        bail!("{}", Error::AuthStatusFailed)
                    }
                    _ => {
                        log::debug!("Received unknown server auth status");
                        bail!("{}", Error::AuthStatusUnknown)
                    }
                }
            }
            Err(net_err) => bail!("{}", Error::from(net_err)),
        }
    }
}

impl ClientCommand1 {
    async fn send_command(mut self) -> anyhow::Result<ClientCommand2> {
        // https://github.com/torproject/torspec/blob/26a2dc7470b1dc41720fd64080ab8386c47df31d/ext-orport-spec.txt#L150
        todo!()
    }
}

impl ClientCommand2 {
    async fn recv_reply(mut self) -> anyhow::Result<CommandOrDone> {
        todo!()
    }
}
