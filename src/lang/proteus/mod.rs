use crate::lang::ProteusSpecification;

pub struct ProteusParser {
    // holds state needed to parse PSF files written in the proteus language
}

impl ProteusParser {
    pub fn new() -> Self {
        Self {}
    }

    pub fn parse(&mut self, psf_filename: &str) -> ProteusSpecification {
        ProteusSpecification::default()
    }
}
