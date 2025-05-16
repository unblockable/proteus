use crate::lang::{spec::proteus::ProteusSpec, Role};
use anyhow::Result;

pub mod implementation;
pub mod proteus;

pub trait Parse {
    fn parse_path(psf_filename: &str, role: Role) -> Result<ProteusSpec>;
    fn parse_content(psf_content: &String, role: Role) -> Result<ProteusSpec>;
}
