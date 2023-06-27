use std::fmt;

use crate::lang::{common::Role, parse, spec::proteus::ProteusSpec};
use anyhow::Result;

pub mod implementation;
pub mod proteus;

pub trait Parse {
    fn parse(psf_filename: &str, role: Role) -> Result<ProteusSpec>;
}
