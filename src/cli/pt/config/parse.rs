use std::collections::HashMap;
use std::env::VarsOs;
use std::ffi::OsString;
use std::fmt;
use std::iter::FromIterator;
use std::net::{AddrParseError, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::PathBuf;
use std::str::FromStr;

use super::keys::{self, ClientKey, CommonKey, ServerKey};

/// Parses PT config options. A `Parser` is created from an iterator of
/// `(String, String)` or `(&str, &str)` representing (key, value) pairs, or
/// from a `VarsOs` object such as that returned from `std::env::vars_os()`.
pub(super) struct Parser {
    map: HashMap<OsString, OsString>,
}

impl Parser {
    pub(super) fn log_all(&self) {
        for (k, v) in self.map.iter() {
            log::debug!("parser: key={k:?} value={v:?}");
        }
    }

    fn get<T>(&self, key: T) -> Result<String, ParseError>
    where
        OsString: From<T>,
    {
        let k = OsString::from(key);
        match self.map.get(&k) {
            Some(v) => v
                .clone()
                .into_string()
                .map_err(|_| ParseError::VariableUnparsable),
            None => Err(ParseError::VariableMissing),
        }
    }

    pub(super) fn is_version_supported(&self) -> Result<bool, ParseError> {
        let versions = self.get(CommonKey::TOR_PT_MANAGED_TRANSPORT_VER)?;
        Ok(versions
            .split(',')
            .collect::<Vec<&str>>()
            .contains(&keys::SUPPORTED_PT_VERSION))
    }

    pub(super) fn state_dir_path(&self) -> Result<PathBuf, ParseError> {
        let dir_str = self.get(CommonKey::TOR_PT_STATE_LOCATION)?;
        let dir_path = PathBuf::from(dir_str);
        match dir_path.is_absolute() {
            true => Ok(dir_path),
            false => Err(ParseError::ValuePathIsNotAbsolute),
        }
    }

    pub(super) fn should_gracefully_close(&self) -> Result<bool, ParseError> {
        let graceful_close = self.get(CommonKey::TOR_PT_EXIT_ON_STDIN_CLOSE)?;
        Ok(graceful_close == "1")
    }

    pub(super) fn bind_addr_v4(&self) -> Result<Ipv4Addr, ParseError> {
        let addr = self.get(CommonKey::TOR_PT_OUTBOUND_BIND_ADDRESS_V4)?;
        Ok(Ipv4Addr::from_str(addr.as_str())?)
    }

    pub(super) fn bind_addr_v6(&self) -> Result<Ipv6Addr, ParseError> {
        let addr = self.get(CommonKey::TOR_PT_OUTBOUND_BIND_ADDRESS_V6)?;
        let addr = addr.trim_matches(|c| c == '[' || c == ']');
        Ok(Ipv6Addr::from_str(addr)?)
    }

    pub(super) fn is_proteus_client(&self) -> Result<bool, ParseError> {
        let client_list = self.get(ClientKey::TOR_PT_CLIENT_TRANSPORTS)?;
        Ok(Self::name_list_has_proteus(client_list))
    }

    pub(super) fn is_proteus_server(&self) -> Result<bool, ParseError> {
        let server_list = self.get(ServerKey::TOR_PT_SERVER_TRANSPORTS)?;
        Ok(Self::name_list_has_proteus(server_list))
    }

    fn name_list_has_proteus(s: String) -> bool {
        s.split(',')
            .collect::<Vec<&str>>()
            .contains(&keys::TRANSPORT_NAME)
    }

    pub(super) fn is_proxy_protocol_supported(&self) -> Result<bool, ParseError> {
        let val = self.get(ClientKey::TOR_PT_PROXY)?;
        let (proto, _) = Self::split_in_two(val.as_str(), "://")?;
        Ok(proto == keys::SUPPORTED_SOCKS_VERSION)
    }

    pub(super) fn proxy(&self) -> Result<(Option<(String, String)>, SocketAddr), ParseError> {
        // Examples:
        // - TOR_PT_PROXY=socks5://198.51.100.1:8000
        // - TOR_PT_PROXY=socks5://tor:test1234@198.51.100.1:8000
        let val = self.get(ClientKey::TOR_PT_PROXY)?;
        let (_, mut remainder) = Self::split_in_two(val.as_str(), "://")?;

        let user_pass_opt = match remainder.contains('@') {
            true => {
                let (user_pass, rem) = Self::split_in_two(remainder, "@")?;
                let (u, p) = Self::split_in_two(user_pass, ":")?;
                remainder = rem;
                Some((u.to_string(), p.to_string()))
            }
            false => None,
        };

        // let s = SocketAddr::from_str(remainder)?;
        Ok((user_pass_opt, SocketAddr::from_str(remainder)?))
    }

    fn split_in_two<'a>(s: &'a str, sep: &str) -> Result<(&'a str, &'a str), ParseError> {
        let parts: Vec<&str> = s.split(sep).filter(|tok| !tok.is_empty()).collect();
        if parts.len() == 2 {
            Ok((parts.first().unwrap(), parts.get(1).unwrap()))
        } else {
            Err(ParseError::VariableUnparsable)
        }
    }

    pub(super) fn server_options(&self) -> Result<HashMap<String, String>, ParseError> {
        let val = self.get(ServerKey::TOR_PT_SERVER_TRANSPORT_OPTIONS)?;

        let mut map = HashMap::new();

        for option in val.split(';').collect::<Vec<&str>>() {
            let (k, v) = Self::split_in_two(option, ":")?;

            if k == keys::TRANSPORT_NAME {
                let (k, v) = Self::split_in_two(v, "=")?;
                map.insert(k.to_string(), v.to_string());
            }
        }

        match map.is_empty() {
            true => Err(ParseError::ValueNotApplicable),
            false => Ok(map),
        }
    }

    pub(super) fn server_bindaddr(&self) -> Result<SocketAddr, ParseError> {
        let val = self.get(ServerKey::TOR_PT_SERVER_BINDADDR)?;

        for option in val.split(',').collect::<Vec<&str>>() {
            let (k, v) = Self::split_in_two(option, "-")?;

            if k == keys::TRANSPORT_NAME {
                return Ok(SocketAddr::from_str(v)?);
            }
        }

        Err(ParseError::ValueNotApplicable)
    }

    pub(super) fn server_or_port(&self) -> Result<SocketAddr, ParseError> {
        let val = self.get(ServerKey::TOR_PT_ORPORT)?;
        Ok(SocketAddr::from_str(val.as_str())?)
    }

    pub(super) fn server_or_port_ext(&self) -> Result<SocketAddr, ParseError> {
        let val = self.get(ServerKey::TOR_PT_EXTENDED_SERVER_PORT)?;
        Ok(SocketAddr::from_str(val.as_str())?)
    }

    pub(super) fn server_auth_cookie_file(&self) -> Result<PathBuf, ParseError> {
        let path_str = self.get(ServerKey::TOR_PT_AUTH_COOKIE_FILE)?;
        let path = PathBuf::from(path_str);
        match path.is_absolute() {
            true => Ok(path),
            false => Err(ParseError::ValuePathIsNotAbsolute),
        }
    }
}

impl<S> FromIterator<(S, S)> for Parser
where
    OsString: From<S>,
{
    fn from_iter<T: IntoIterator<Item = (S, S)>>(iter: T) -> Self
    where
        OsString: From<S>,
    {
        let mut map = HashMap::new();
        for (k, v) in iter {
            map.insert(k.into(), v.into());
        }
        Self { map }
    }
}

impl From<VarsOs> for Parser {
    fn from(vars: VarsOs) -> Self {
        Self::from_iter(vars)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum ParseError {
    VariableMissing,
    VariableUnparsable,
    ValuePathIsNotAbsolute,
    ValueNotApplicable,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            ParseError::VariableMissing => {
                write!(f, "PT environment variable is missing")
            }
            ParseError::VariableUnparsable => {
                write!(
                    f,
                    "PT environment variable was missing or contained an invalid value"
                )
            }
            ParseError::ValuePathIsNotAbsolute => {
                write!(f, "got a relative path when we expected an absolute path")
            }
            ParseError::ValueNotApplicable => {
                write!(
                    f,
                    "the value we parsed does not apply to {}",
                    keys::TRANSPORT_NAME
                )
            }
        }
    }
}

impl From<AddrParseError> for ParseError {
    fn from(_: AddrParseError) -> ParseError {
        ParseError::VariableUnparsable
    }
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use super::*;

    #[test]
    fn common_version() {
        let key = CommonKey::TOR_PT_MANAGED_TRANSPORT_VER.as_str();

        assert_eq!(
            Parser::from_iter([("", "")]).is_version_supported(),
            Err(ParseError::VariableMissing)
        );

        for val in [
            "1",
            "1,1a,2b,this_is_a_valid_ver",
            "1a,2b,1,this_is_a_valid_ver",
            "1a,2b,this_is_a_valid_ver,1",
        ] {
            assert_eq!(
                Parser::from_iter([(key, val)]).is_version_supported(),
                Ok(true)
            );
        }

        for val in ["0", "", "1a"] {
            assert_eq!(
                Parser::from_iter([(key, val)]).is_version_supported(),
                Ok(false)
            );
        }
    }

    #[test]
    fn common_state_dir() {
        let key = CommonKey::TOR_PT_STATE_LOCATION.as_str();

        assert_eq!(
            Parser::from_iter([("", "")]).state_dir_path(),
            Err(ParseError::VariableMissing)
        );

        for val in ["/var/lib/tor/pt_state", "/tmp"] {
            assert_eq!(
                Parser::from_iter([(key, val)]).state_dir_path(),
                Ok(PathBuf::from(val))
            );
        }

        for val in ["", "tor/pt_state", "tmp"] {
            assert_eq!(
                Parser::from_iter([(key, val)]).state_dir_path(),
                Err(ParseError::ValuePathIsNotAbsolute)
            );
        }
    }

    #[test]
    fn common_graceful_close() {
        let key = CommonKey::TOR_PT_EXIT_ON_STDIN_CLOSE.as_str();

        assert_eq!(
            Parser::from_iter([("", "")]).should_gracefully_close(),
            Err(ParseError::VariableMissing)
        );

        assert_eq!(
            Parser::from_iter([(key, "1")]).should_gracefully_close(),
            Ok(true)
        );

        for val in ["", "0", "10", "01", "some_other_string"] {
            assert_eq!(
                Parser::from_iter([(key, val)]).should_gracefully_close(),
                Ok(false)
            );
        }
    }

    #[test]
    fn common_bind_addr_v4() {
        let key = CommonKey::TOR_PT_OUTBOUND_BIND_ADDRESS_V4.as_str();

        assert_eq!(
            Parser::from_iter([("", "")]).bind_addr_v4(),
            Err(ParseError::VariableMissing)
        );

        for val in ["203.0.113.4", "192.168.1.1"] {
            assert_eq!(
                Parser::from_iter([(key, val)]).bind_addr_v4(),
                Ok(Ipv4Addr::from_str(val).unwrap())
            );
        }

        for val in ["203.0.113", "0", "", "some_string"] {
            assert_eq!(
                Parser::from_iter([(key, val)]).bind_addr_v4(),
                Err(ParseError::VariableUnparsable)
            );
        }
    }

    #[test]
    fn common_bind_addr_v6() {
        let key = CommonKey::TOR_PT_OUTBOUND_BIND_ADDRESS_V6.as_str();

        assert_eq!(
            Parser::from_iter([("", "")]).bind_addr_v6(),
            Err(ParseError::VariableMissing)
        );

        // The PT spec requires brackets around the address.
        {
            let val = "2001:db8::4";
            assert_eq!(
                Parser::from_iter([(key.to_string(), format!("[{}]", val))]).bind_addr_v6(),
                Ok(Ipv6Addr::from_str(val).unwrap())
            );
        }

        for val in ["[203.0.113.4]", "[2001:db8]", "0", "", "some_string"] {
            assert_eq!(
                Parser::from_iter([(key, val)]).bind_addr_v6(),
                Err(ParseError::VariableUnparsable)
            );
        }
    }

    #[test]
    fn client_transports() {
        let key = ClientKey::TOR_PT_CLIENT_TRANSPORTS.as_str();

        assert_eq!(
            Parser::from_iter([("", "")]).is_proteus_client(),
            Err(ParseError::VariableMissing)
        );

        for val in ["obfs3,obfs4,proteus", "proteus,proteus", "proteus"] {
            assert_eq!(
                Parser::from_iter([(key, val)]).is_proteus_client(),
                Ok(true)
            );
        }

        for val in ["", "obfs4,proteu", "pro,teus", "proteus1"] {
            assert_eq!(
                Parser::from_iter([(key, val)]).is_proteus_client(),
                Ok(false)
            );
        }
    }

    #[test]
    fn server_transports() {
        let key = ServerKey::TOR_PT_SERVER_TRANSPORTS.as_str();

        assert_eq!(
            Parser::from_iter([("", "")]).is_proteus_server(),
            Err(ParseError::VariableMissing)
        );

        for val in ["obfs3,obfs4,proteus", "proteus,proteus", "proteus"] {
            assert_eq!(
                Parser::from_iter([(key, val)]).is_proteus_server(),
                Ok(true)
            );
        }

        for val in ["", "obfs4,proteu", "pro,teus", "proteus1"] {
            assert_eq!(
                Parser::from_iter([(key, val)]).is_proteus_server(),
                Ok(false)
            );
        }
    }

    #[test]
    fn client_proxy_protocol() {
        let key = ClientKey::TOR_PT_PROXY.as_str();

        assert_eq!(
            Parser::from_iter([("", "")]).is_proxy_protocol_supported(),
            Err(ParseError::VariableMissing)
        );

        assert_eq!(
            Parser::from_iter([(key, "")]).is_proxy_protocol_supported(),
            Err(ParseError::VariableUnparsable)
        );

        for val in [
            "socks5://198.51.100.1:8000",
            "socks5://tor:test1234@198.51.100.1:8000",
        ] {
            assert_eq!(
                Parser::from_iter([(key, val)]).is_proxy_protocol_supported(),
                Ok(true)
            );
        }

        for val in ["socks4a://198.51.100.2:8001", "http://198.51.100.3:443"] {
            assert_eq!(
                Parser::from_iter([(key, val)]).is_proxy_protocol_supported(),
                Ok(false)
            );
        }
    }

    #[test]
    fn client_proxy() {
        let key = ClientKey::TOR_PT_PROXY.as_str();

        assert_eq!(
            Parser::from_iter([("", "")]).proxy(),
            Err(ParseError::VariableMissing)
        );

        // version with no user or pass.
        let proxy = Parser::from_iter([(key, "socks5://198.51.100.1:8000")]).proxy();
        assert!(proxy.is_ok(),);

        let (user_pass_opt, sock_addr) = proxy.unwrap();
        assert!(user_pass_opt.is_none());
        assert!(sock_addr.is_ipv4());
        assert_eq!(sock_addr.ip(), IpAddr::from_str("198.51.100.1").unwrap());
        assert_eq!(sock_addr.port(), u16::from_str("8000").unwrap());

        // version with user and pass.
        let proxy = Parser::from_iter([(key, "socks5://tor:test1234@198.51.100.1:8000")]).proxy();
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
            assert_eq!(
                Parser::from_iter([(key, val)]).proxy(),
                Err(ParseError::VariableUnparsable)
            );
        }
    }

    #[test]
    fn server_options() {
        let key = ServerKey::TOR_PT_SERVER_TRANSPORT_OPTIONS.as_str();

        assert_eq!(
            Parser::from_iter([("", "")]).server_options(),
            Err(ParseError::VariableMissing)
        );

        assert_eq!(
            Parser::from_iter([(key, "scramblesuit:key=banana")]).server_options(),
            Err(ParseError::ValueNotApplicable)
        );

        let opts = Parser::from_iter([(
            key,
            "proteus:key1=value1;proteus:key2=value2;scramblesuit:key3=banana",
        )])
        .server_options();
        assert!(opts.is_ok());
        let opt_map = opts.unwrap();
        assert_eq!(opt_map.len(), 2);
        assert!(opt_map.contains_key("key1"));
        assert!(opt_map.get("key1").unwrap() == "value1");
        assert!(opt_map.contains_key("key2"));
        assert!(opt_map.get("key2").unwrap() == "value2");
        assert!(!opt_map.contains_key("key3"));

        for val in ["proteus:key", "proteus:key=", "proteus:=value"] {
            assert_eq!(
                Parser::from_iter([(key, val)]).server_options(),
                Err(ParseError::VariableUnparsable)
            );
        }
    }

    #[test]
    fn server_bindaddr() {
        let key = ServerKey::TOR_PT_SERVER_BINDADDR.as_str();

        assert_eq!(
            Parser::from_iter([("", "")]).server_bindaddr(),
            Err(ParseError::VariableMissing)
        );

        // proteus is missing
        assert_eq!(
            Parser::from_iter([(key, "scramblesuit-127.0.0.1:4891")]).server_bindaddr(),
            Err(ParseError::ValueNotApplicable)
        );

        let info =
            Parser::from_iter([(key, "proteus-127.0.0.1:54321,scramblesuit-127.0.0.1:4891")])
                .server_bindaddr();
        assert!(info.is_ok());
        let info = info.unwrap();
        assert_eq!(info.ip(), IpAddr::from_str("127.0.0.1").unwrap());
        assert_eq!(info.port(), u16::from_str("54321").unwrap());

        for val in ["proteus-127.0.0.1", "proteus-127.0.0.1:", "proteus-:54321"] {
            assert_eq!(
                Parser::from_iter([(key, val)]).server_bindaddr(),
                Err(ParseError::VariableUnparsable)
            );
        }
    }

    fn _test_sock_addr<F>(key: ServerKey, f: F)
    where
        F: Fn(&Parser) -> Result<SocketAddr, ParseError>,
    {
        let key = key.as_str();

        let parser = Parser::from_iter([("", "")]);
        assert_eq!(f(&parser), Err(ParseError::VariableMissing));

        let parser = Parser::from_iter([(key, "127.0.0.1:9000")]);
        let sock_addr = f(&parser);

        assert!(sock_addr.is_ok());
        let sock_addr = sock_addr.unwrap();
        assert_eq!(sock_addr.ip(), Ipv4Addr::from_str("127.0.0.1").unwrap());
        assert_eq!(sock_addr.port(), u16::from_str("9000").unwrap());

        for val in ["string:string", "9000:127.0.0.1", "127.0.0.1:", ":9000"] {
            let parser = Parser::from_iter([(key, val)]);
            assert_eq!(f(&parser), Err(ParseError::VariableUnparsable));
        }
    }

    #[test]
    fn server_orport() {
        _test_sock_addr(ServerKey::TOR_PT_ORPORT, Parser::server_or_port);
    }

    #[test]
    fn server_orport_extended() {
        _test_sock_addr(
            ServerKey::TOR_PT_EXTENDED_SERVER_PORT,
            Parser::server_or_port_ext,
        );
    }

    #[test]
    fn server_auth_cookie_file() {
        let key = ServerKey::TOR_PT_AUTH_COOKIE_FILE.as_str();

        assert_eq!(
            Parser::from_iter([("", "")]).server_auth_cookie_file(),
            Err(ParseError::VariableMissing)
        );

        {
            let val = "/var/lib/tor/extended_orport_auth_cookie";
            assert_eq!(
                Parser::from_iter([(key, val)]).server_auth_cookie_file(),
                Ok(PathBuf::from(val))
            );
        }

        for val in ["", "tor/auth_cookie_file", "tmp"] {
            assert_eq!(
                Parser::from_iter([(key, val)]).server_auth_cookie_file(),
                Err(ParseError::ValuePathIsNotAbsolute)
            );
        }
    }
}
