use std::ffi::OsString;
use std::fmt;

pub(crate) const TRANSPORT_NAME: &str = "proteus";
pub(crate) const SUPPORTED_PT_VERSION: &str = "1";
pub(crate) const SUPPORTED_SOCKS_VERSION: &str = "socks5";

#[derive(Debug)]
#[allow(non_camel_case_types)]
pub(crate) enum CommonKey {
    TOR_PT_MANAGED_TRANSPORT_VER,
    TOR_PT_STATE_LOCATION,
    TOR_PT_EXIT_ON_STDIN_CLOSE,
    TOR_PT_OUTBOUND_BIND_ADDRESS_V4,
    TOR_PT_OUTBOUND_BIND_ADDRESS_V6,
}

impl CommonKey {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            CommonKey::TOR_PT_MANAGED_TRANSPORT_VER => "TOR_PT_MANAGED_TRANSPORT_VER",
            CommonKey::TOR_PT_STATE_LOCATION => "TOR_PT_STATE_LOCATION",
            CommonKey::TOR_PT_EXIT_ON_STDIN_CLOSE => "TOR_PT_EXIT_ON_STDIN_CLOSE",
            CommonKey::TOR_PT_OUTBOUND_BIND_ADDRESS_V4 => "TOR_PT_OUTBOUND_BIND_ADDRESS_V4",
            CommonKey::TOR_PT_OUTBOUND_BIND_ADDRESS_V6 => "TOR_PT_OUTBOUND_BIND_ADDRESS_V6",
        }
    }
}

impl fmt::Display for CommonKey {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(fmt, "{:?}", self)
    }
}

impl From<CommonKey> for &str {
    fn from(value: CommonKey) -> Self {
        value.as_str()
    }
}

impl From<CommonKey> for String {
    fn from(value: CommonKey) -> Self {
        value.to_string()
    }
}

impl From<CommonKey> for OsString {
    fn from(value: CommonKey) -> Self {
        OsString::from(String::from(value))
    }
}

#[derive(Debug)]
#[allow(non_camel_case_types)]
pub(crate) enum ClientKey {
    TOR_PT_CLIENT_TRANSPORTS,
    TOR_PT_PROXY,
}

impl ClientKey {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            ClientKey::TOR_PT_CLIENT_TRANSPORTS => "TOR_PT_CLIENT_TRANSPORTS",
            ClientKey::TOR_PT_PROXY => "TOR_PT_PROXY",
        }
    }
}

impl fmt::Display for ClientKey {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(fmt, "{:?}", self)
    }
}

impl From<ClientKey> for &str {
    fn from(value: ClientKey) -> Self {
        value.as_str()
    }
}

impl From<ClientKey> for String {
    fn from(value: ClientKey) -> Self {
        value.to_string()
    }
}

impl From<ClientKey> for OsString {
    fn from(value: ClientKey) -> Self {
        OsString::from(String::from(value))
    }
}

#[derive(Debug)]
#[allow(non_camel_case_types)]
pub(crate) enum ServerKey {
    TOR_PT_SERVER_TRANSPORTS,
    TOR_PT_SERVER_TRANSPORT_OPTIONS,
    TOR_PT_SERVER_BINDADDR,
    TOR_PT_ORPORT,
    TOR_PT_EXTENDED_SERVER_PORT,
    TOR_PT_AUTH_COOKIE_FILE,
}

impl ServerKey {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            ServerKey::TOR_PT_SERVER_TRANSPORTS => "TOR_PT_SERVER_TRANSPORTS",
            ServerKey::TOR_PT_SERVER_TRANSPORT_OPTIONS => "TOR_PT_SERVER_TRANSPORT_OPTIONS",
            ServerKey::TOR_PT_SERVER_BINDADDR => "TOR_PT_SERVER_BINDADDR",
            ServerKey::TOR_PT_ORPORT => "TOR_PT_ORPORT",
            ServerKey::TOR_PT_EXTENDED_SERVER_PORT => "TOR_PT_EXTENDED_SERVER_PORT",
            ServerKey::TOR_PT_AUTH_COOKIE_FILE => "TOR_PT_AUTH_COOKIE_FILE",
        }
    }
}

impl fmt::Display for ServerKey {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(fmt, "{:?}", self)
    }
}

impl From<ServerKey> for &str {
    fn from(value: ServerKey) -> Self {
        value.as_str()
    }
}

impl From<ServerKey> for String {
    fn from(value: ServerKey) -> Self {
        value.to_string()
    }
}

impl From<ServerKey> for OsString {
    fn from(value: ServerKey) -> Self {
        OsString::from(String::from(value))
    }
}
