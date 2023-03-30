use crate::lang::spec::{crypto::CryptoSpec, message::MessageSpec};

// Holds the immutable part of a proteus protocol as parsed from a PSF. This is
// used as input to a ProteusProtocol, which is newly created for each
// connection and holds mutable state.
pub struct ProteusSpec {
    // state_machine: RyanGraph,
    crypto: CryptoSpec,
    message: MessageSpec,
}

impl ProteusSpec {
    pub fn new() -> Self {
        todo!()
    }

    // pub fn start(&self) -> &Vec<Action> {
    //     self.actions.as_ref()
    // }

    // pub fn next(&self, prev_action: Action) -> &Vec<Action> {
    //     self.actions.as_ref()
    // }
}
