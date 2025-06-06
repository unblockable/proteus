use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;

mod keys;
mod parse;

use parse::{ParseError, Parser};

#[derive(Debug)]
#[allow(dead_code)]
pub enum ConfigError {
    Version(String),
    Proxy(String),
    Env(String),
}

#[derive(Debug)]
pub enum Mode {
    Client(ClientConfig),
    Server(ServerConfig),
}

#[derive(Debug, Clone)]
pub enum ForwardProtocol {
    Basic,
    Extended(PathBuf), // holds the auth cookie file location
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SocksAuth {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SocksProxy {
    pub auth: Option<SocksAuth>,
    pub addr: SocketAddr,
}

#[derive(Debug)]
pub struct Config {
    pub common: CommonConfig,
    pub mode: Mode,
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct CommonConfig {
    pub state_location: PathBuf,
    pub exit_on_stdin_close: bool,
    pub connect_bind_addr: IpAddr,
}

#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub proxy: Option<SocksProxy>,
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub options: HashMap<String, String>,
    pub listen_bind_addr: SocketAddr,
    pub forward_addr: SocketAddr,
    pub forward_proto: ForwardProtocol,
}

impl From<ParseError> for ConfigError {
    fn from(e: ParseError) -> ConfigError {
        ConfigError::Env(e.to_string())
    }
}

impl Config {
    pub fn from_env() -> Result<Config, ConfigError> {
        Self::from_parser(Parser::from(std::env::vars_os()))
    }

    fn from_parser(parser: Parser) -> Result<Config, ConfigError> {
        parser.log_all();

        if Ok(true) != parser.is_version_supported() {
            return Err(ConfigError::Version(String::from(
                "PT version is unsupported",
            )));
        }

        let common = Config::common_config_from_parser(&parser)?;

        let mode = {
            if Ok(true) == parser.is_proteus_client() {
                Mode::Client(Config::client_config_from_parser(&parser)?)
            } else if Ok(true) == parser.is_proteus_server() {
                Mode::Server(Config::server_config_from_parser(&parser)?)
            } else {
                return Err(ConfigError::Env(String::from(
                    "Unable to find supported client or server proteus transport",
                )));
            }
        };

        Ok(Config { common, mode })
    }

    fn common_config_from_parser(parser: &Parser) -> Result<CommonConfig, ConfigError> {
        // Required.
        let state_location = parser.state_dir_path()?;

        // Special handling: iff the value is set and set to true.
        let exit_on_stdin_close = Ok(true) == parser.should_gracefully_close();

        // Prefer v4 over v6, with default fallback.
        let connect_bind_addr = {
            if let Ok(bind_addr_v4) = parser.bind_addr_v4() {
                IpAddr::from(bind_addr_v4)
            } else if let Ok(bind_addr_v6) = parser.bind_addr_v6() {
                IpAddr::from(bind_addr_v6)
            } else {
                IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0))
            }
        };

        Ok(CommonConfig {
            state_location,
            exit_on_stdin_close,
            connect_bind_addr,
        })
    }

    fn client_config_from_parser(parser: &Parser) -> Result<ClientConfig, ConfigError> {
        // Optional.
        let proxy = match parser.is_proxy_protocol_supported() {
            Ok(is_supported) => match is_supported {
                true => match parser.proxy() {
                    Ok((user_pass_opt, addr)) => {
                        let auth = user_pass_opt
                            .map(|(username, password)| SocksAuth { username, password });
                        Some(SocksProxy { auth, addr })
                    }
                    Err(_) => {
                        return Err(ConfigError::Proxy(String::from(
                            "Requested proxy URI is malformed",
                        )));
                    }
                },
                false => {
                    return Err(ConfigError::Proxy(String::from(
                        "Requested proxy protocol is not supported",
                    )));
                }
            },
            Err(_) => None,
        };

        Ok(ClientConfig { proxy })
    }

    fn server_config_from_parser(parser: &Parser) -> Result<ServerConfig, ConfigError> {
        // Required for proteus.
        let options = parser.server_options()?;
        let listen_bind_addr = parser.server_bindaddr()?;

        // Special handling.
        let (forward_addr, forward_proto) = match parser.server_or_port() {
            Ok(sock_addr) => (sock_addr, ForwardProtocol::Basic),
            Err(e) => match e {
                // If regular orport is missing, check for extor port.
                ParseError::VariableMissing => match parser.server_or_port_ext() {
                    Ok(sock_addr) => (
                        sock_addr,
                        // Now a auth cookie file is required too.
                        ForwardProtocol::Extended(parser.server_auth_cookie_file()?),
                    ),
                    Err(ext_e) => return Err(ConfigError::from(ext_e)),
                },
                _ => return Err(ConfigError::from(e)),
            },
        };

        Ok(ServerConfig {
            options,
            listen_bind_addr,
            forward_addr,
            forward_proto,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::keys::{ClientKey, CommonKey, ServerKey};
    use super::parse::Parser;
    use super::{Config, ConfigError};

    fn make_config(v: Vec<(&str, &str)>) -> Result<Config, ConfigError> {
        Config::from_parser(Parser::from_iter(v))
    }

    fn build_client_required() -> Vec<(&'static str, &'static str)> {
        let mut conf_pairs = vec![];

        conf_pairs.push((CommonKey::TOR_PT_MANAGED_TRANSPORT_VER.into(), "1"));
        assert!(make_config(conf_pairs.clone()).is_err());

        conf_pairs.push((CommonKey::TOR_PT_STATE_LOCATION.into(), "/tmp"));
        assert!(make_config(conf_pairs.clone()).is_err());

        conf_pairs.push((ClientKey::TOR_PT_CLIENT_TRANSPORTS.into(), "proteus"));
        assert!(make_config(conf_pairs.clone()).is_ok());

        conf_pairs
    }

    #[test]
    fn client_required() {
        assert!(!build_client_required().is_empty());
    }

    #[test]
    fn client_optional() {
        let mut conf_pairs = build_client_required();

        conf_pairs.push((CommonKey::TOR_PT_EXIT_ON_STDIN_CLOSE.into(), "1"));
        assert!(make_config(conf_pairs.clone()).is_ok());

        conf_pairs.push((
            CommonKey::TOR_PT_OUTBOUND_BIND_ADDRESS_V4.into(),
            "192.168.1.1",
        ));
        assert!(make_config(conf_pairs.clone()).is_ok());

        conf_pairs.push((
            ClientKey::TOR_PT_PROXY.into(),
            "socks5://username:password@192.168.1.1:8000",
        ));
        assert!(make_config(conf_pairs).is_ok());
    }

    fn build_server() -> Vec<(&'static str, &'static str)> {
        let mut conf_pairs = vec![];

        conf_pairs.push((CommonKey::TOR_PT_MANAGED_TRANSPORT_VER.into(), "1"));
        assert!(make_config(conf_pairs.clone()).is_err());

        conf_pairs.push((CommonKey::TOR_PT_STATE_LOCATION.into(), "/tmp"));
        assert!(make_config(conf_pairs.clone()).is_err());

        conf_pairs.push((ServerKey::TOR_PT_SERVER_TRANSPORTS.into(), "proteus"));
        assert!(make_config(conf_pairs.clone()).is_err());

        conf_pairs.push((
            ServerKey::TOR_PT_SERVER_TRANSPORT_OPTIONS.into(),
            "proteus:seed=12345",
        ));
        assert!(make_config(conf_pairs.clone()).is_err());

        conf_pairs.push((
            ServerKey::TOR_PT_SERVER_BINDADDR.into(),
            "proteus-192.168.100.1:8080",
        ));
        assert!(make_config(conf_pairs.clone()).is_err());

        conf_pairs
    }

    #[test]
    fn server_required() {
        let mut conf_pairs = build_server();

        conf_pairs.push((ServerKey::TOR_PT_ORPORT.into(), "127.0.0.1:9000"));
        assert!(make_config(conf_pairs).is_ok());
    }

    #[test]
    fn server_optional() {
        let mut conf_pairs = build_server();

        conf_pairs.push((
            ServerKey::TOR_PT_EXTENDED_SERVER_PORT.into(),
            "127.0.0.1:9001",
        ));
        assert!(make_config(conf_pairs.clone()).is_err());

        conf_pairs.push((
            ServerKey::TOR_PT_AUTH_COOKIE_FILE.into(),
            "/tmp/tor_auth_cookie",
        ));
        assert!(make_config(conf_pairs.clone()).is_ok());

        conf_pairs.push((CommonKey::TOR_PT_EXIT_ON_STDIN_CLOSE.into(), "1"));
        assert!(make_config(conf_pairs.clone()).is_ok());

        conf_pairs.push((
            CommonKey::TOR_PT_OUTBOUND_BIND_ADDRESS_V4.into(),
            "192.168.1.1",
        ));
        assert!(make_config(conf_pairs).is_ok());
    }
}
