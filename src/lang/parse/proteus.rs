use crate::lang::{
    common::Role,
    parse::{self, Parse},
    spec::proteus::ProteusSpec,
};

pub struct ProteusParser {
    // holds state needed to parse PSF files written in the proteus language
}

impl ProteusParser {
    pub fn new() -> Self {
        Self {}
    }
}

impl Parse for ProteusParser {
    fn parse(&mut self, psf_filename: &str, role: Role) -> Result<ProteusSpec, parse::Error> {
        todo!()
    }
}
