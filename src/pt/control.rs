use std::net::SocketAddr;

use log;

pub enum Message<'a> {
    Version,
    VersionError,
    EnvError(&'a str),
    ProxyDone,
    ProxyError(&'a str),
    ClientReady(SocketAddr),
    ClientError(&'a str),
    ServerReady(SocketAddr),
    ServerError(&'a str),
    Status(&'a str),
    Log((log::Level, &'a str)),
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
        Message::ProxyDone => println!("PROXY DONE"),
        Message::ProxyError(s) => println!("PROXY-ERROR {}", s),
        Message::ClientReady(a) => println!("CMETHOD proteus socks5 {}\nCMETHODS DONE", a),
        Message::ClientError(s) => println!("CMETHOD-ERROR proteus {}\nCMETHODS DONE", s),
        Message::ServerReady(a) => println!("SMETHOD proteus {}\nSMETHODS DONE", a),
        Message::ServerError(s) => println!("SMETHOD-ERROR proteus {}\nSMETHODS DONE", s),
        Message::Status(s) => println!("STATUS TRANSPORT=proteus {}", s),
        Message::Log((l, s)) => match l {
            log::Level::Error => {
                println!("LOG SEVERITY=error MESSAGE=\"{}\"", s.replace("\"", "'"))
            }
            log::Level::Warn => {
                println!("LOG SEVERITY=warning MESSAGE=\"{}\"", s.replace("\"", "'"))
            }
            log::Level::Info => {
                println!("LOG SEVERITY=notice MESSAGE=\"{}\"", s.replace("\"", "'"))
            }
            log::Level::Debug => println!("LOG SEVERITY=info MESSAGE=\"{}\"", s.replace("\"", "'")),
            log::Level::Trace => {
                println!("LOG SEVERITY=debug MESSAGE=\"{}\"", s.replace("\"", "'"))
            }
        },
    }
}

static LOGGER: Logger = Logger;

struct Logger;

impl log::Log for Logger {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            // Saves an allocation when no formatting is needed.
            if let Some(s) = record.args().as_str() {
                send_to_parent(Message::Log((record.level(), s)));
            } else {
                send_to_parent(Message::Log((
                    record.level(),
                    record.args().to_string().as_str(),
                )));
            }
        }
    }

    fn flush(&self) {}
}

pub fn init_logger() {
    log::set_logger(&LOGGER)
        .expect("control::init_logger should not be called after logger initialized");
    log::set_max_level(log::LevelFilter::Trace);
}
