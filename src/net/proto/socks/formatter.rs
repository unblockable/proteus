use bytes::{Bytes, BytesMut};

use crate::net::{proto::socks::frames::*, Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Copy)]
pub struct Formatter {
    // All socks frames can be formatted without extra state.
}

impl Formatter {
    pub fn new() -> Formatter {
        Formatter {}
    }
}

impl Serializer<Greeting> for Formatter {
    fn serialize_frame(&mut self, src: Greeting) -> Bytes {
        src.serialize()
    }
}

impl Deserializer<Greeting> for Formatter {
    fn deserialize_frame(&mut self, src: &mut std::io::Cursor<&BytesMut>) -> Option<Greeting> {
        Greeting::deserialize(src)
    }
}

impl Serializer<Choice> for Formatter {
    fn serialize_frame(&mut self, src: Choice) -> Bytes {
        src.serialize()
    }
}

impl Deserializer<Choice> for Formatter {
    fn deserialize_frame(&mut self, src: &mut std::io::Cursor<&BytesMut>) -> Option<Choice> {
        Choice::deserialize(src)
    }
}

impl Serializer<UserPassAuthRequest> for Formatter {
    fn serialize_frame(&mut self, src: UserPassAuthRequest) -> Bytes {
        src.serialize()
    }
}

impl Deserializer<UserPassAuthRequest> for Formatter {
    fn deserialize_frame(
        &mut self,
        src: &mut std::io::Cursor<&BytesMut>,
    ) -> Option<UserPassAuthRequest> {
        UserPassAuthRequest::deserialize(src)
    }
}

impl Serializer<UserPassAuthResponse> for Formatter {
    fn serialize_frame(&mut self, src: UserPassAuthResponse) -> Bytes {
        src.serialize()
    }
}

impl Deserializer<UserPassAuthResponse> for Formatter {
    fn deserialize_frame(
        &mut self,
        src: &mut std::io::Cursor<&BytesMut>,
    ) -> Option<UserPassAuthResponse> {
        UserPassAuthResponse::deserialize(src)
    }
}

impl Serializer<ConnectRequest> for Formatter {
    fn serialize_frame(&mut self, src: ConnectRequest) -> Bytes {
        src.serialize()
    }
}

impl Deserializer<ConnectRequest> for Formatter {
    fn deserialize_frame(&mut self, src: &mut std::io::Cursor<&BytesMut>) -> Option<ConnectRequest> {
        ConnectRequest::deserialize(src)
    }
}

impl Serializer<ConnectResponse> for Formatter {
    fn serialize_frame(&mut self, src: ConnectResponse) -> Bytes {
        src.serialize()
    }
}

impl Deserializer<ConnectResponse> for Formatter {
    fn deserialize_frame(&mut self, src: &mut std::io::Cursor<&BytesMut>) -> Option<ConnectResponse> {
        ConnectResponse::deserialize(src)
    }
}
