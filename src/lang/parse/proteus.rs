use std::fs;

use crate::lang::compiler::TaskGraphImpl;
use crate::lang::parse::Parse;
use crate::lang::spec::proteus::ProteusSpec;
use crate::lang::Role;

pub struct ProteusParser {}

impl Parse for ProteusParser {
    fn parse_path(psf_filename: &str, role: Role) -> anyhow::Result<ProteusSpec> {
        ProteusParser::parse_content(&fs::read_to_string(psf_filename)?, role)
    }

    fn parse_content(psf_content: &str, role: Role) -> anyhow::Result<ProteusSpec> {
        let psf = crate::lang::parse::implementation::parse_psf(psf_content)?;
        let tg = crate::lang::compiler::compile_task_graph(psf.sequence.iter());
        let tgi = TaskGraphImpl::new(tg, role, psf);
        Ok(ProteusSpec::new(tgi))
    }
}
