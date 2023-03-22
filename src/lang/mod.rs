pub mod proteus;

pub struct ProteusSpecification {
    // Holds the immutable part of a proteus protocol as parsed from
    // a psf. This is used as input to a ProteusProtocol, which is
    // newly created for each connection and holds mutable state.
}

impl ProteusSpecification {
    pub fn new() -> Self {
        Self {

        }
    }
}
