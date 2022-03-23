use super::socks5_protocol::*;

use crate::net::{self, socks, Connection};

fn to_error_state(message: String) -> Socks5Protocol<Error> {
    Socks5Protocol::<Error> {
        state: Error { message },
    }
}

impl InitializationState for Socks5Protocol<Initialization> {
    fn new(conn: Connection) -> Socks5Protocol<Initialization> {
        Socks5Protocol::<Initialization> {
            state: Initialization { conn },
        }
    }

    fn start(self) -> Socks5Protocol<ClientHandshake> {
        Socks5Protocol::<ClientHandshake> {
            state: ClientHandshake {
                conn: self.state.conn,
            },
        }
    }
}

impl ClientHandshakeState for Socks5Protocol<ClientHandshake> {
    fn greeting(mut self) -> ClientHandshakeResult {
        match self.state.conn.read_frame::<socks::Greeting>() {
            Ok(greeting) => {
                let next = Socks5Protocol::<ServerHandshake> {
                    state: ServerHandshake {
                        conn: self.state.conn,
                        greeting,
                    },
                };
                ClientHandshakeResult::ServerHandshake(next)
            }
            Err(net::Error::Eof) => {
                let msg = format!("Unexpectedly reached EOF while trying to read socks greeting");
                ClientHandshakeResult::Error(to_error_state(msg))
            }
            Err(net::Error::IoError(e)) => {
                let msg = format!("IO Error while trying to read socks greeting: {}", e);
                ClientHandshakeResult::Error(to_error_state(msg))
            }
        }
    }
}

// impl ServerHandshakeState for Socks5Protocol<ServerHandshake> {
//     fn choice(self) -> ServerHandshakeResult {
//         let r = Socks5Protocol::<ServerAuthentication> {state: ServerAuthentication{conn: self.state.conn}};
//         ServerHandshakeResult::ServerAuthentication(r)
//     }
// }

// impl ClientAuthenticationState for Socks5Protocol<ClientAuthentication> {
//     fn request_no_auth(self) -> ClientAuthenticationResult {
//         let r = Socks5Protocol::<ClientCommand> {state: ClientCommand{conn: self.state.conn}};
//         ClientAuthenticationResult::ClientCommand(r)
//     }

//     fn request_user_pass_auth(self) -> ClientAuthenticationResult {
//         let r = Socks5Protocol::<ClientCommand> {state: ClientCommand{conn: self.state.conn}};
//         ClientAuthenticationResult::ClientCommand(r)
//     }
// }

// impl ClientCommandState for Socks5Protocol<ClientCommand> {
//     fn request_connect(self) -> CommandResult {
//         let r = Socks5Protocol::<Success> {state: Success{conn: self.state.conn}};
//         CommandResult::Success(r)
//     }
// }

// impl ServerAuthenticationState for Socks5Protocol<ServerAuthentication> {
//     fn response(self) -> ServerAuthenticationResult {
//         let r = Socks5Protocol::<ServerCommand> {state: ServerCommand{conn: self.state.conn}};
//         ServerAuthenticationResult::ServerCommand(r)
//     }
// }

// impl ServerCommandState for Socks5Protocol<ServerCommand> {
//     fn response(self) -> CommandResult {
//         let r = Socks5Protocol::<Success> {state: Success{conn: self.state.conn}};
//         CommandResult::Success(r)
//     }
// }

impl SuccessState for Socks5Protocol<Success> {
    fn take(self) -> Connection {
        self.state.conn
    }
}

impl ErrorState for Socks5Protocol<Error> {
    fn take(self) -> String {
        self.state.message
    }
}

pub fn run_protocol(conn: Connection) -> Result<(), String> {
    let proto = Socks5Protocol::new(conn).start();

    let proto = match proto.greeting() {
        ClientHandshakeResult::ServerHandshake(s) => s,
        ClientHandshakeResult::Error(e) => return Err(e.state.message),
    };

    log::debug!(
        "Got greeting version {} num methods {}: {:?}",
        proto.state.greeting.version,
        proto.state.greeting.num_auth_methods,
        proto.state.greeting.supported_auth_methods
    );

    Ok(())
}
