use std::net::SocketAddr;

#[derive(Debug)]
#[allow(dead_code)]
pub enum PtLogLevel {
    Error,
    Warning,
    Notice,
    Info,
    Debug,
}

pub enum Message<'a> {
    Version,
    VersionError,
    EnvError(&'a str),
    _ProxyDone, // Connecting through upstream proxy unimplemented
    ProxyError(&'a str),
    ClientReady(SocketAddr),
    ClientError(&'a str),
    ServerReady(SocketAddr),
    ServerError(&'a str),
    Status(&'a str),
    Log((PtLogLevel, &'a str)),
}

/// Sends a control message to our parent process over stdout. The strings
/// attached to the `*Error` variants, as well as those inside the `Log`
/// variant, can be any human readable ascii message. The string attached to the
/// `Status` variant should be in `KEY=VALUE` form.
pub fn send_to_parent(msg: Message) {
    match msg {
        Message::Version => println!("VERSION 1"),
        Message::VersionError => println!("VERSION-ERROR no-version"),
        Message::EnvError(s) => println!("ENV-ERROR {}", s),
        Message::_ProxyDone => println!("PROXY DONE"),
        Message::ProxyError(s) => println!("PROXY-ERROR {}", s),
        Message::ClientReady(a) => println!("CMETHOD proteus socks5 {}\nCMETHODS DONE", a),
        Message::ClientError(s) => println!("CMETHOD-ERROR proteus {}\nCMETHODS DONE", s),
        Message::ServerReady(a) => println!("SMETHOD proteus {}\nSMETHODS DONE", a),
        Message::ServerError(s) => println!("SMETHOD-ERROR proteus {}\nSMETHODS DONE", s),
        Message::Status(s) => println!("STATUS TRANSPORT=proteus {}", s),
        Message::Log((l, s)) => println!("LOG SEVERITY={l:?} MESSAGE=\"{}\"", s.replace('\"', "'")),
    }
}
