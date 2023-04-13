use std::fmt;

use crate::lang::{common::Role, parse, spec::proteus::ProteusSpec};

pub mod implementation;
pub mod proteus;

#[derive(Debug)]
pub enum Error {
    Syntax,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Syntax => write!(f, "Incorrect syntax detected in PSF"),
        }
    }
}

pub trait Parse {
    fn parse(&mut self, psf_filename: &str, role: Role) -> Result<ProteusSpec, parse::Error>;
}
