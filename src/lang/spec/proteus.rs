use crate::lang::{
    common::Role,
    compiler::*,
    task::{Task, TaskID, TaskProvider, TaskSet},
};

// Holds the immutable part of a proteus protocol as parsed from a PSF. This is
// used as input to a ProteusProtocol, which is newly created for each
// connection and holds mutable state.
pub struct ProteusSpec {
    task_graph: TaskGraphImpl,
}

impl ProteusSpec {
    pub fn new(psf_contents: &String, my_role: Role) -> ProteusSpec {
        let psf = crate::lang::parse::implementation::parse_psf(psf_contents);
        let tg = crate::lang::compiler::compile_task_graph(psf.sequence.iter());

        ProteusSpec {
            task_graph: TaskGraphImpl::new(tg, my_role, psf),
        }
    }
}

impl TaskProvider for ProteusSpec {
    fn get_init_task(&self) -> Task {
        self.task_graph.init_task()
    }

    fn get_next_tasks(&self, last_task: &TaskID) -> TaskSet {
        self.task_graph.next(*last_task)
    }
}

#[cfg(test)]
pub fn parse_simple_proteus_spec(role: Role) -> ProteusSpec {
    use std::fs;

    let filepath = "src/lang/parse/examples/simple.psf";
    let input = fs::read_to_string(filepath).expect("cannot read simple file");

    ProteusSpec::new(&input, role)
}

#[cfg(test)]
pub fn parse_encrypted_proteus_spec(role: Role) -> ProteusSpec {
    use std::fs;

    let filepath = "src/lang/parse/examples/shadowsocks.psf";
    let input = fs::read_to_string(filepath).expect("cannot read shadowsocks file");

    ProteusSpec::new(&input, role)
}

#[cfg(test)]
pub fn parse_encrypted_padded_proteus_spec(role: Role) -> ProteusSpec {
    use std::fs;

    let filepath = "src/lang/parse/examples/shadowsocks_padded.psf";
    let input = fs::read_to_string(filepath).expect("cannot read shadowsocks file");

    ProteusSpec::new(&input, role)
}
