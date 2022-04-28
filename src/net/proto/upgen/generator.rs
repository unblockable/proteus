use crate::net::proto::upgen::protocols::*;

pub struct Generator {
    seed: u64
}

impl Generator {
    pub fn new(seed: u64) -> Generator {
        // Use the seed to generate an overt protocol which specifies the format
        // of all frames that are transferred over the network.
        Generator {
            seed
        }
    }

    pub fn generate_overt_protocol(&self) -> OvertProtocol {
        todo!()
    }
}
