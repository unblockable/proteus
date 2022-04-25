use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;

use crate::pt::env::{self, ParseEnvError};

#[derive(Debug)]
pub enum ConfigError {
    VersionError(String),
    ProxyError(String),
    EnvError(String),
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
pub struct SocksAuth {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone)]
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

impl From<ParseEnvError> for ConfigError {
    fn from(e: ParseEnvError) -> ConfigError {
        ConfigError::EnvError(e.to_string())
    }
}

impl Config {
    pub fn from_env() -> Result<Config, ConfigError> {
        if Ok(true) != env::parse_is_version_supported() {
            return Err(ConfigError::VersionError(String::from(
                "PT version is unsupported",
            )));
        }

        let common = Config::common_config_from_env()?;

        let mode = {
            if Ok(true) == env::parse_is_upgen_client() {
                Mode::Client(Config::client_config_from_env()?)
            } else if Ok(true) == env::parse_is_upgen_server() {
                Mode::Server(Config::server_config_from_env()?)
            } else {
                return Err(ConfigError::EnvError(String::from(
                    "Unable to find supported client or server upgen transport",
                )));
            }
        };

        Ok(Config { common, mode })
    }

    fn common_config_from_env() -> Result<CommonConfig, ConfigError> {
        // Required.
        let state_location = env::parse_state_dir_path()?;

        // Special handling: iff the value is set and set to true.
        let exit_on_stdin_close = Ok(true) == env::parse_should_gracefully_close();

        // Prefer v4 over v6, with default fallback.
        let connect_bind_addr = {
            if let Ok(bind_addr_v4) = env::parse_bind_addr_v4() {
                IpAddr::from(bind_addr_v4)
            } else if let Ok(bind_addr_v6) = env::parse_bind_addr_v6() {
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

    fn client_config_from_env() -> Result<ClientConfig, ConfigError> {
        // Optional.
        let proxy = match env::parse_proxy_protocol_is_supported() {
            Ok(is_supported) => match is_supported {
                true => match env::parse_proxy() {
                    Ok((user_pass_opt, addr)) => {
                        let auth = match user_pass_opt {
                            Some((username, password)) => Some(SocksAuth { username, password }),
                            None => None,
                        };
                        Some(SocksProxy { auth, addr })
                    }
                    Err(_) => {
                        return Err(ConfigError::ProxyError(String::from(
                            "Requested proxy URI is malformed",
                        )))
                    }
                },
                false => {
                    return Err(ConfigError::ProxyError(String::from(
                        "Requested proxy protocol is not supported",
                    )))
                }
            },
            Err(_) => None,
        };

        Ok(ClientConfig { proxy })
    }

    fn server_config_from_env() -> Result<ServerConfig, ConfigError> {
        // Required for upgen.
        let options = env::parse_server_options()?;
        let listen_bind_addr = env::parse_server_bindaddr()?;

        // Special handling.
        let (forward_addr, forward_proto) = match env::parse_server_or_port() {
            Ok(sock_addr) => (sock_addr, ForwardProtocol::Basic),
            Err(e) => match e {
                // If regular orport is missing, check for extor port.
                ParseEnvError::VariableMissing => match env::parse_server_or_port_ext() {
                    Ok(sock_addr) => (
                        sock_addr,
                        // Now a auth cookie file is required too.
                        ForwardProtocol::Extended(env::parse_server_auth_cookie_file()?),
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
    use std::env;

    use crate::pt::config::Config;
    use crate::pt::env::{CommonKey, ClientKey, ServerKey};

    fn remove_all_pt_keys() {
        for (k, _) in env::vars() {
            if k.starts_with("TOR_PT_") {
                env::remove_var(k);
            }
        }
    }

    #[test]
    fn client_required() {
        remove_all_pt_keys();

        env::set_var(CommonKey::TOR_PT_MANAGED_TRANSPORT_VER.to_string(), "1");
        assert!(Config::from_env().is_err());
        env::set_var(CommonKey::TOR_PT_STATE_LOCATION.to_string(), "/tmp");
        assert!(Config::from_env().is_err());
        env::set_var(ClientKey::TOR_PT_CLIENT_TRANSPORTS.to_string(), "upgen");
        assert!(Config::from_env().is_ok());
    }

    #[test]
    fn client_optional() {
        client_required();

        env::set_var(CommonKey::TOR_PT_EXIT_ON_STDIN_CLOSE.to_string(), "1");
        assert!(Config::from_env().is_ok());
        env::set_var(CommonKey::TOR_PT_OUTBOUND_BIND_ADDRESS_V4.to_string(), "192.168.1.1");
        assert!(Config::from_env().is_ok());
        env::set_var(ClientKey::TOR_PT_PROXY.to_string(), "socks5://username:password@192.168.1.1:8000");
        assert!(Config::from_env().is_ok());
    }

    #[test]
    fn server_required() {
        remove_all_pt_keys();

        env::set_var(CommonKey::TOR_PT_MANAGED_TRANSPORT_VER.to_string(), "1");
        assert!(Config::from_env().is_err());
        env::set_var(CommonKey::TOR_PT_STATE_LOCATION.to_string(), "/tmp");
        assert!(Config::from_env().is_err());
        env::set_var(ServerKey::TOR_PT_SERVER_TRANSPORTS.to_string(), "upgen");
        assert!(Config::from_env().is_err());
        env::set_var(ServerKey::TOR_PT_SERVER_TRANSPORT_OPTIONS.to_string(), "upgen:seed=12345");
        assert!(Config::from_env().is_err());
        env::set_var(ServerKey::TOR_PT_SERVER_BINDADDR.to_string(), "upgen-192.168.100.1:8080");
        assert!(Config::from_env().is_err());
        env::set_var(ServerKey::TOR_PT_ORPORT.to_string(), "127.0.0.1:9000");
        assert!(Config::from_env().is_ok());
    }

    #[test]
    fn server_optional() {
        server_required();

        // Use extended port instead of normal port.
        env::remove_var(ServerKey::TOR_PT_ORPORT.to_string());
        assert!(Config::from_env().is_err());
        env::set_var(ServerKey::TOR_PT_EXTENDED_SERVER_PORT.to_string(), "127.0.0.1:9001");
        assert!(Config::from_env().is_err());
        env::set_var(ServerKey::TOR_PT_AUTH_COOKIE_FILE.to_string(), "/tmp/tor_auth_cookie");
        assert!(Config::from_env().is_ok());

        env::set_var(CommonKey::TOR_PT_EXIT_ON_STDIN_CLOSE.to_string(), "1");
        assert!(Config::from_env().is_ok());
        env::set_var(CommonKey::TOR_PT_OUTBOUND_BIND_ADDRESS_V4.to_string(), "192.168.1.1");
        assert!(Config::from_env().is_ok());
    }
}