use std::fs;

use crate::lang::{
    Role, compiler::TaskGraphImpl, parse::Parse, spec::proteus::ProteusSpec,
};

use anyhow::Result;

pub struct ProteusParser {}

impl Parse for ProteusParser {
    fn parse(psf_filename: &str, role: Role) -> Result<ProteusSpec> {
        // TODO check and return errors here.
        let psf_contents = fs::read_to_string(psf_filename).expect("cannot read filepath");
        let psf = crate::lang::parse::implementation::parse_psf(&psf_contents)?;
        let tg = crate::lang::compiler::compile_task_graph(psf.sequence.iter());
        let tgi = TaskGraphImpl::new(tg, role, psf);
        Ok(ProteusSpec::new(tgi))
    }
}
