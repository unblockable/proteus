use bytes::Bytes;

use crate::net::proto::socks::address::Socks5Address;

// Generates `From` impls so we can go from the inner value to an enum instance
// of the associated variant.
#[enum_from::enum_from]
// Generates `TryFrom` impls so we can go from an instance of the enum to the
// inner value, or an error if the inner value has an unexpected type.
#[enum_from::enum_try_from]
#[derive(Debug, PartialEq, Clone)]
pub enum Message {
    Greeting(Greeting),
    Choice(Choice),
    UserPassAuthRequest(UserPassAuthRequest),
    UserPassAuthResponse(UserPassAuthResponse),
    ConnectRequest(ConnectRequest),
    ConnectResponse(ConnectResponse),
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum MessageKind {
    Greeting,
    Choice,
    UserPassAuthRequest,
    UserPassAuthResponse,
    ConnectRequest,
    ConnectResponse,
}

impl Message {
    pub fn kind(&self) -> MessageKind {
        match self {
            Message::Greeting(_) => MessageKind::Greeting,
            Message::Choice(_) => MessageKind::Choice,
            Message::UserPassAuthRequest(_) => MessageKind::UserPassAuthRequest,
            Message::UserPassAuthResponse(_) => MessageKind::UserPassAuthResponse,
            Message::ConnectRequest(_) => MessageKind::ConnectRequest,
            Message::ConnectResponse(_) => MessageKind::ConnectResponse,
        }
    }
}

#[derive(Debug, Default, PartialEq, Clone)]
pub struct Greeting {
    pub version: u8,
    pub num_auth_methods: u8,
    pub supported_auth_methods: Bytes,
}

#[derive(Debug, Default, PartialEq, Clone)]
pub struct Choice {
    pub version: u8,
    pub auth_method: u8,
}

#[derive(Debug, Default, PartialEq, Clone)]
pub struct UserPassAuthRequest {
    pub version: u8,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Default, PartialEq, Clone)]
pub struct UserPassAuthResponse {
    pub version: u8,
    pub status: u8,
}

#[derive(Debug, Default, PartialEq, Clone)]
pub struct ConnectRequest {
    pub version: u8,
    pub command: u8,
    pub reserved: u8,
    pub dest_addr: Socks5Address,
    pub dest_port: u16,
}

#[derive(Debug, Default, PartialEq, Clone)]
pub struct ConnectResponse {
    pub version: u8,
    pub status: u8,
    pub reserved: u8,
    pub bind_addr: Socks5Address,
    pub bind_port: u16,
}
