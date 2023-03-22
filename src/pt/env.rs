use std::collections::HashMap;
use std::env::{self, VarError};
use std::fmt;
use std::net::{AddrParseError, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::num::ParseIntError;
use std::path::PathBuf;
use std::str::FromStr;

const TRANSPORT_NAME: &str = "proteus";
const SUPPORTED_PT_VERSION: &str = "1";
const SUPPORTED_SOCKS_VERSION: &str = "socks5";

#[derive(Debug)]
#[allow(non_camel_case_types)]
pub enum CommonKey {
    TOR_PT_MANAGED_TRANSPORT_VER,
    TOR_PT_STATE_LOCATION,
    TOR_PT_EXIT_ON_STDIN_CLOSE,
    TOR_PT_OUTBOUND_BIND_ADDRESS_V4,
    TOR_PT_OUTBOUND_BIND_ADDRESS_V6,
}

#[derive(Debug)]
#[allow(non_camel_case_types)]
pub enum ClientKey {
    TOR_PT_CLIENT_TRANSPORTS,
    TOR_PT_PROXY,
}

#[derive(Debug)]
#[allow(non_camel_case_types)]
pub enum ServerKey {
    TOR_PT_SERVER_TRANSPORTS,
    TOR_PT_SERVER_TRANSPORT_OPTIONS,
    TOR_PT_SERVER_BINDADDR,
    TOR_PT_ORPORT,
    TOR_PT_EXTENDED_SERVER_PORT,
    TOR_PT_AUTH_COOKIE_FILE,
}

impl fmt::Display for CommonKey {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(fmt, "{:?}", self)
    }
}

impl fmt::Display for ClientKey {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(fmt, "{:?}", self)
    }
}

impl fmt::Display for ServerKey {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(fmt, "{:?}", self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParseEnvError {
    VariableMissing,
    VariableUnparsable,
    ValuePathIsNotAbsolute,
    ValueNotApplicable,
}

impl fmt::Display for ParseEnvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            ParseEnvError::VariableMissing => {
                write!(f, "PT environment variable is missing")
            }
            ParseEnvError::VariableUnparsable => {
                write!(
                    f,
                    "PT environment variable was missing or contained an invalid value"
                )
            }
            ParseEnvError::ValuePathIsNotAbsolute => {
                write!(f, "got a relative path when we expected an absolute path")
            }
            ParseEnvError::ValueNotApplicable => {
                write!(
                    f,
                    "the value we parsed does not apply to {}",
                    TRANSPORT_NAME
                )
            }
        }
    }
}

impl From<VarError> for ParseEnvError {
    fn from(e: VarError) -> ParseEnvError {
        match e {
            VarError::NotPresent => ParseEnvError::VariableMissing,
            VarError::NotUnicode(_) => ParseEnvError::VariableUnparsable,
        }
    }
}

impl From<AddrParseError> for ParseEnvError {
    fn from(_: AddrParseError) -> ParseEnvError {
        ParseEnvError::VariableUnparsable
    }
}

impl From<ParseIntError> for ParseEnvError {
    fn from(_: ParseIntError) -> ParseEnvError {
        ParseEnvError::VariableUnparsable
    }
}

pub fn log_env_vars() {
    for (k, v) in env::vars() {
        log::debug!("env: key={} value={}", k, v);
    }
    for a in env::args() {
        log::debug!("arg: value={}", a);
    }
}

pub fn parse_is_version_supported() -> Result<bool, ParseEnvError> {
    let versions = env::var(CommonKey::TOR_PT_MANAGED_TRANSPORT_VER.to_string())?;
    Ok(versions
        .split(",")
        .collect::<Vec<&str>>()
        .contains(&SUPPORTED_PT_VERSION))
}

pub fn parse_state_dir_path() -> Result<PathBuf, ParseEnvError> {
    let dir_str = env::var(CommonKey::TOR_PT_STATE_LOCATION.to_string())?;
    let dir_path = PathBuf::from(dir_str);
    match dir_path.is_absolute() {
        true => Ok(dir_path),
        false => Err(ParseEnvError::ValuePathIsNotAbsolute),
    }
}

pub fn parse_should_gracefully_close() -> Result<bool, ParseEnvError> {
    let graceful_close = env::var(CommonKey::TOR_PT_EXIT_ON_STDIN_CLOSE.to_string())?;
    Ok(graceful_close == "1")
}

pub fn parse_bind_addr_v4() -> Result<Ipv4Addr, ParseEnvError> {
    let addr = env::var(CommonKey::TOR_PT_OUTBOUND_BIND_ADDRESS_V4.to_string())?;
    Ok(Ipv4Addr::from_str(addr.as_str())?)
}

pub fn parse_bind_addr_v6() -> Result<Ipv6Addr, ParseEnvError> {
    let addr = env::var(CommonKey::TOR_PT_OUTBOUND_BIND_ADDRESS_V6.to_string())?;
    let addr = addr.trim_matches(|c| c == '[' || c == ']');
    Ok(Ipv6Addr::from_str(addr)?)
}

pub fn parse_is_proteus_client() -> Result<bool, ParseEnvError> {
    let client_list = env::var(ClientKey::TOR_PT_CLIENT_TRANSPORTS.to_string())?;
    Ok(name_list_has_proteus(client_list))
}

pub fn parse_is_proteus_server() -> Result<bool, ParseEnvError> {
    let server_list = env::var(ServerKey::TOR_PT_SERVER_TRANSPORTS.to_string())?;
    Ok(name_list_has_proteus(server_list))
}

fn name_list_has_proteus(s: String) -> bool {
    s.split(",")
        .collect::<Vec<&str>>()
        .contains(&TRANSPORT_NAME)
}

pub fn parse_proxy_protocol_is_supported() -> Result<bool, ParseEnvError> {
    let val = env::var(ClientKey::TOR_PT_PROXY.to_string())?;
    let (proto, _) = split_in_two(val.as_str(), "://")?;
    Ok(proto == SUPPORTED_SOCKS_VERSION)
}

pub fn parse_proxy() -> Result<(Option<(String, String)>, SocketAddr), ParseEnvError> {
    // Examples:
    // - TOR_PT_PROXY=socks5://198.51.100.1:8000
    // - TOR_PT_PROXY=socks5://tor:test1234@198.51.100.1:8000
    let val = env::var(ClientKey::TOR_PT_PROXY.to_string())?;
    let (_, mut remainder) = split_in_two(val.as_str(), "://")?;

    let user_pass_opt = match remainder.contains(&"@") {
        true => {
            let (user_pass, rem) = split_in_two(remainder, "@")?;
            let (u, p) = split_in_two(user_pass, ":")?;
            remainder = rem;
            Some((u.to_string(), p.to_string()))
        }
        false => None,
    };

    // let s = SocketAddr::from_str(remainder)?;
    Ok((user_pass_opt, SocketAddr::from_str(remainder)?))
}

fn split_in_two<'a>(s: &'a str, sep: &str) -> Result<(&'a str, &'a str), ParseEnvError> {
    let parts: Vec<&str> = s.split(sep).filter(|tok| !tok.is_empty()).collect();
    if parts.len() == 2 && parts.get(0).is_some() && parts.get(1).is_some() {
        Ok((parts.get(0).unwrap(), parts.get(1).unwrap()))
    } else {
        Err(ParseEnvError::VariableUnparsable)
    }
}

pub fn parse_server_options() -> Result<HashMap<String, String>, ParseEnvError> {
    let val = env::var(ServerKey::TOR_PT_SERVER_TRANSPORT_OPTIONS.to_string())?;

    let mut map = HashMap::new();

    for option in val.split(";").collect::<Vec<&str>>() {
        let (k, v) = split_in_two(option, ":")?;

        if k == TRANSPORT_NAME {
            let (k, v) = split_in_two(v, "=")?;
            map.insert(k.to_string(), v.to_string());
        }
    }

    match map.is_empty() {
        true => Err(ParseEnvError::ValueNotApplicable),
        false => Ok(map),
    }
}

pub fn parse_server_bindaddr() -> Result<SocketAddr, ParseEnvError> {
    let val = env::var(ServerKey::TOR_PT_SERVER_BINDADDR.to_string())?;

    for option in val.split(",").collect::<Vec<&str>>() {
        let (k, v) = split_in_two(option, "-")?;

        if k == TRANSPORT_NAME {
            return Ok(SocketAddr::from_str(v)?);
        }
    }

    Err(ParseEnvError::ValueNotApplicable)
}

pub fn parse_server_or_port() -> Result<SocketAddr, ParseEnvError> {
    let val = env::var(ServerKey::TOR_PT_ORPORT.to_string())?;
    Ok(SocketAddr::from_str(val.as_str())?)
}

pub fn parse_server_or_port_ext() -> Result<SocketAddr, ParseEnvError> {
    let val = env::var(ServerKey::TOR_PT_EXTENDED_SERVER_PORT.to_string())?;
    Ok(SocketAddr::from_str(val.as_str())?)
}

pub fn parse_server_auth_cookie_file() -> Result<PathBuf, ParseEnvError> {
    let path_str = env::var(ServerKey::TOR_PT_AUTH_COOKIE_FILE.to_string())?;
    let path = PathBuf::from(path_str);
    match path.is_absolute() {
        true => Ok(path),
        false => Err(ParseEnvError::ValuePathIsNotAbsolute),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;

    #[test]
    fn common_version() {
        let key = CommonKey::TOR_PT_MANAGED_TRANSPORT_VER.to_string();

        env::remove_var(key.clone());
        assert_eq!(
            parse_is_version_supported(),
            Err(ParseEnvError::VariableMissing)
        );

        for val in [
            "1",
            "1,1a,2b,this_is_a_valid_ver",
            "1a,2b,1,this_is_a_valid_ver",
            "1a,2b,this_is_a_valid_ver,1",
        ] {
            env::set_var(key.clone(), val);
            assert_eq!(parse_is_version_supported(), Ok(true));
        }

        for val in ["0", "", "1a"] {
            env::set_var(key.clone(), val);
            assert_eq!(parse_is_version_supported(), Ok(false));
        }
    }

    #[test]
    fn common_state_dir() {
        let key = CommonKey::TOR_PT_STATE_LOCATION.to_string();

        env::remove_var(key.clone());
        assert_eq!(parse_state_dir_path(), Err(ParseEnvError::VariableMissing));

        for val in ["/var/lib/tor/pt_state", "/tmp"] {
            env::set_var(key.clone(), val);
            assert_eq!(parse_state_dir_path(), Ok(PathBuf::from(val)));
        }

        for val in ["", "tor/pt_state", "tmp"] {
            env::set_var(key.clone(), val);
            assert_eq!(
                parse_state_dir_path(),
                Err(ParseEnvError::ValuePathIsNotAbsolute)
            );
        }
    }

    #[test]
    fn common_graceful_close() {
        let key = CommonKey::TOR_PT_EXIT_ON_STDIN_CLOSE.to_string();

        env::remove_var(key.clone());
        assert_eq!(
            parse_should_gracefully_close(),
            Err(ParseEnvError::VariableMissing)
        );

        env::set_var(key.clone(), "1");
        assert_eq!(parse_should_gracefully_close(), Ok(true));

        for val in ["", "0", "10", "01", "some_other_string"] {
            env::set_var(key.clone(), val);
            assert_eq!(parse_should_gracefully_close(), Ok(false));
        }
    }

    #[test]
    fn common_bind_addr_v4() {
        let key = CommonKey::TOR_PT_OUTBOUND_BIND_ADDRESS_V4.to_string();

        env::remove_var(key.clone());
        assert_eq!(parse_bind_addr_v4(), Err(ParseEnvError::VariableMissing));

        for val in ["203.0.113.4", "192.168.1.1"] {
            env::set_var(key.clone(), val);
            assert_eq!(parse_bind_addr_v4(), Ok(Ipv4Addr::from_str(val).unwrap()));
        }

        for val in ["203.0.113", "0", "", "some_string"] {
            env::set_var(key.clone(), val);
            assert_eq!(parse_bind_addr_v4(), Err(ParseEnvError::VariableUnparsable));
        }
    }

    #[test]
    fn common_bind_addr_v6() {
        let key = CommonKey::TOR_PT_OUTBOUND_BIND_ADDRESS_V6.to_string();

        env::remove_var(key.clone());
        assert_eq!(parse_bind_addr_v6(), Err(ParseEnvError::VariableMissing));

        // The PT spec requires brackets around the address.
        for val in ["2001:db8::4"] {
            env::set_var(key.clone(), format!("[{}]", val));
            assert_eq!(parse_bind_addr_v6(), Ok(Ipv6Addr::from_str(val).unwrap()));
        }

        for val in ["[203.0.113.4]", "[2001:db8]", "0", "", "some_string"] {
            env::set_var(key.clone(), val);
            assert_eq!(parse_bind_addr_v6(), Err(ParseEnvError::VariableUnparsable));
        }
    }

    #[test]
    fn client_transports() {
        let key = ClientKey::TOR_PT_CLIENT_TRANSPORTS.to_string();

        env::remove_var(key.clone());
        assert_eq!(parse_is_proteus_client(), Err(ParseEnvError::VariableMissing));

        for val in ["obfs3,obfs4,proteus", "proteus,proteus", "proteus"] {
            env::set_var(key.clone(), val);
            assert_eq!(parse_is_proteus_client(), Ok(true));
        }

        for val in ["", "obfs4,proteu", "pro,teus", "proteus1"] {
            env::set_var(key.clone(), val);
            assert_eq!(parse_is_proteus_client(), Ok(false));
        }
    }

    #[test]
    fn server_transports() {
        let key = ServerKey::TOR_PT_SERVER_TRANSPORTS.to_string();

        env::remove_var(key.clone());
        assert_eq!(parse_is_proteus_server(), Err(ParseEnvError::VariableMissing));

        for val in ["obfs3,obfs4,proteus", "proteus,proteus", "proteus"] {
            env::set_var(key.clone(), val);
            assert_eq!(parse_is_proteus_server(), Ok(true));
        }

        for val in ["", "obfs4,proteu", "pro,teus", "proteus1"] {
            env::set_var(key.clone(), val);
            assert_eq!(parse_is_proteus_server(), Ok(false));
        }
    }

    #[test]
    fn client_proxy_protocol() {
        let key = ClientKey::TOR_PT_PROXY.to_string();

        env::remove_var(key.clone());
        assert_eq!(
            parse_proxy_protocol_is_supported(),
            Err(ParseEnvError::VariableMissing)
        );

        env::set_var(key.clone(), "");
        assert_eq!(
            parse_proxy_protocol_is_supported(),
            Err(ParseEnvError::VariableUnparsable)
        );

        for val in [
            "socks5://198.51.100.1:8000",
            "socks5://tor:test1234@198.51.100.1:8000",
        ] {
            env::set_var(key.clone(), val);
            assert_eq!(parse_proxy_protocol_is_supported(), Ok(true));
        }

        for val in ["socks4a://198.51.100.2:8001", "http://198.51.100.3:443"] {
            env::set_var(key.clone(), val);
            assert_eq!(parse_proxy_protocol_is_supported(), Ok(false));
        }
    }

    #[test]
    fn client_proxy() {
        let key = ClientKey::TOR_PT_PROXY.to_string();

        env::remove_var(key.clone());
        assert_eq!(parse_proxy(), Err(ParseEnvError::VariableMissing));

        // version with no user or pass.
        env::set_var(key.clone(), "socks5://198.51.100.1:8000");
        let proxy = parse_proxy();
        assert!(proxy.is_ok());

        let (user_pass_opt, sock_addr) = proxy.unwrap();
        assert!(user_pass_opt.is_none());
        assert!(sock_addr.is_ipv4());
        assert_eq!(sock_addr.ip(), IpAddr::from_str("198.51.100.1").unwrap());
        assert_eq!(sock_addr.port(), u16::from_str("8000").unwrap());

        // version with user and pass.
        env::set_var(key.clone(), "socks5://tor:test1234@198.51.100.1:8000");
        let proxy = parse_proxy();
        assert!(proxy.is_ok());

        let (user_pass_opt, sock_addr) = proxy.unwrap();
        assert!(user_pass_opt.is_some());
        let (user, pass) = user_pass_opt.unwrap();
        assert_eq!(user.as_str(), "tor");
        assert_eq!(pass.as_str(), "test1234");
        assert!(sock_addr.is_ipv4());
        assert_eq!(sock_addr.ip(), IpAddr::from_str("198.51.100.1").unwrap());
        assert_eq!(sock_addr.port(), u16::from_str("8000").unwrap());

        for val in [
            "",
            "socks5://198.51.100.1:80a00",
            "socks5://19a8.51.100.1:8000",
            "socks5://tor:test1234@198:51:100:1:8000",
        ] {
            env::set_var(key.clone(), val);
            assert_eq!(parse_proxy(), Err(ParseEnvError::VariableUnparsable));
        }
    }

    #[test]
    fn server_options() {
        let key = ServerKey::TOR_PT_SERVER_TRANSPORT_OPTIONS.to_string();

        env::remove_var(key.clone());
        assert_eq!(parse_server_options(), Err(ParseEnvError::VariableMissing));

        env::set_var(key.clone(), "scramblesuit:key=banana");
        assert_eq!(
            parse_server_options(),
            Err(ParseEnvError::ValueNotApplicable)
        );

        env::set_var(
            key.clone(),
            "proteus:key1=value1;proteus:key2=value2;scramblesuit:key3=banana",
        );
        let opts = parse_server_options();
        assert!(opts.is_ok());
        let opt_map = opts.unwrap();
        assert_eq!(opt_map.len(), 2);
        assert!(opt_map.contains_key("key1"));
        assert!(opt_map.get("key1").unwrap() == "value1");
        assert!(opt_map.contains_key("key2"));
        assert!(opt_map.get("key2").unwrap() == "value2");
        assert!(!opt_map.contains_key("key3"));

        for val in ["proteus:key", "proteus:key=", "proteus:=value"] {
            env::set_var(key.clone(), val);
            assert_eq!(
                parse_server_options(),
                Err(ParseEnvError::VariableUnparsable)
            );
        }
    }

    #[test]
    fn server_bindaddr() {
        let key = ServerKey::TOR_PT_SERVER_BINDADDR.to_string();

        env::remove_var(key.clone());
        assert_eq!(parse_server_bindaddr(), Err(ParseEnvError::VariableMissing));

        // proteus is missing
        env::set_var(key.clone(), "scramblesuit-127.0.0.1:4891");
        assert_eq!(
            parse_server_bindaddr(),
            Err(ParseEnvError::ValueNotApplicable)
        );

        env::set_var(
            key.clone(),
            "proteus-127.0.0.1:54321,scramblesuit-127.0.0.1:4891",
        );
        let info = parse_server_bindaddr();
        assert!(info.is_ok());
        let info = info.unwrap();
        assert_eq!(info.ip(), IpAddr::from_str("127.0.0.1").unwrap());
        assert_eq!(info.port(), u16::from_str("54321").unwrap());

        for val in ["proteus-127.0.0.1", "proteus-127.0.0.1:", "proteus-:54321"] {
            env::set_var(key.clone(), val);
            assert_eq!(
                parse_server_bindaddr(),
                Err(ParseEnvError::VariableUnparsable)
            );
        }
    }

    fn _test_sock_addr<F>(key: ServerKey, f: F)
    where
        F: Fn() -> Result<SocketAddr, ParseEnvError>,
    {
        let key = key.to_string();
        env::remove_var(key.clone());
        assert_eq!(f(), Err(ParseEnvError::VariableMissing));

        env::set_var(key.clone(), "127.0.0.1:9000");
        let sock_addr = f();
        assert!(sock_addr.is_ok());
        let sock_addr = sock_addr.unwrap();
        assert_eq!(sock_addr.ip(), Ipv4Addr::from_str("127.0.0.1").unwrap());
        assert_eq!(sock_addr.port(), u16::from_str("9000").unwrap());

        for val in ["string:string", "9000:127.0.0.1", "127.0.0.1:", ":9000"] {
            env::set_var(key.clone(), val);
            assert_eq!(f(), Err(ParseEnvError::VariableUnparsable));
        }
    }

    #[test]
    fn server_orport() {
        _test_sock_addr(ServerKey::TOR_PT_ORPORT, &parse_server_or_port);
    }

    #[test]
    fn server_orport_extended() {
        _test_sock_addr(
            ServerKey::TOR_PT_EXTENDED_SERVER_PORT,
            &parse_server_or_port_ext,
        );
    }

    #[test]
    fn server_auth_cookie_file() {
        let key = ServerKey::TOR_PT_AUTH_COOKIE_FILE.to_string();

        env::remove_var(key.clone());
        assert_eq!(
            parse_server_auth_cookie_file(),
            Err(ParseEnvError::VariableMissing)
        );

        for val in ["/var/lib/tor/extended_orport_auth_cookie"] {
            env::set_var(key.clone(), val);
            assert_eq!(parse_server_auth_cookie_file(), Ok(PathBuf::from(val)));
        }

        for val in ["", "tor/auth_cookie_file", "tmp"] {
            env::set_var(key.clone(), val);
            assert_eq!(
                parse_server_auth_cookie_file(),
                Err(ParseEnvError::ValuePathIsNotAbsolute)
            );
        }
    }
}
