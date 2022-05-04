pub mod prototype;

use bytes::Bytes;

trait Encrypt {
    fn encrypt(&mut self, payload: &Bytes) -> Option<Bytes>;
}

pub enum CryptoProtocol {
    Prototype(prototype::CryptoModule),
}
