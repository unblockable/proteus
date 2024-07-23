use bytes::{Bytes, BytesMut};

use crate::net::{proto::or::frames::*, Deserialize, Deserializer, Serialize, Serializer};

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

impl Serializer<ClientNonce> for Formatter {
    fn serialize_frame(&mut self, src: ClientNonce) -> Bytes {
        src.serialize()
    }
}

impl Deserializer<ClientNonce> for Formatter {
    fn deserialize_frame(
        &mut self,
        src: &mut std::io::Cursor<&BytesMut>,
    ) -> Option<ClientNonce> {
        ClientNonce::deserialize(src)
    }
}

impl Serializer<ServerHashNonce> for Formatter {
    fn serialize_frame(&mut self, src: ServerHashNonce) -> Bytes {
        src.serialize()
    }
}

impl Deserializer<ServerHashNonce> for Formatter {
    fn deserialize_frame(
        &mut self,
        src: &mut std::io::Cursor<&BytesMut>,
    ) -> Option<ServerHashNonce> {
        ServerHashNonce::deserialize(src)
    }
}

impl Serializer<ClientHash> for Formatter {
    fn serialize_frame(&mut self, src: ClientHash) -> Bytes {
        src.serialize()
    }
}

impl Deserializer<ClientHash> for Formatter {
    fn deserialize_frame(
        &mut self,
        src: &mut std::io::Cursor<&BytesMut>,
    ) -> Option<ClientHash> {
        ClientHash::deserialize(src)
    }
}

impl Serializer<ServerStatus> for Formatter {
    fn serialize_frame(&mut self, src: ServerStatus) -> Bytes {
        src.serialize()
    }
}

impl Deserializer<ServerStatus> for Formatter {
    fn deserialize_frame(
        &mut self,
        src: &mut std::io::Cursor<&BytesMut>,
    ) -> Option<ServerStatus> {
        ServerStatus::deserialize(src)
    }
}

impl Serializer<Command> for Formatter {
    fn serialize_frame(&mut self, src: Command) -> Bytes {
        src.serialize()
    }
}

impl Deserializer<Command> for Formatter {
    fn deserialize_frame(
        &mut self,
        src: &mut std::io::Cursor<&BytesMut>,
    ) -> Option<Command> {
        Command::deserialize(src)
    }
}

impl Serializer<Reply> for Formatter {
    fn serialize_frame(&mut self, src: Reply) -> Bytes {
        src.serialize()
    }
}

impl Deserializer<Reply> for Formatter {
    fn deserialize_frame(
        &mut self,
        src: &mut std::io::Cursor<&BytesMut>,
    ) -> Option<Reply> {
        Reply::deserialize(src)
    }
}
