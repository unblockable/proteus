use std::io::{self, Cursor};

use bytes::BytesMut;

use crate::net::proto::socks::address::Socks5Target;
use crate::net::{Deserialize, Serialize};

pub struct Socks5Codec;

impl Socks5Codec {
    pub fn encode_target(&mut self, target: Socks5Target, dst: &mut BytesMut) -> io::Result<()> {
        let bytes = Socks5Target::serialize(&target);
        dst.reserve(bytes.len());
        dst.extend_from_slice(&bytes);
        Ok(())
    }

    pub fn decode_target(
        &mut self,
        src: &mut Cursor<&BytesMut>,
    ) -> io::Result<Option<Socks5Target>> {
        match Socks5Target::deserialize(src) {
            Some(target) => Ok(Some(target)),
            None => Ok(None),
        }
    }
}
