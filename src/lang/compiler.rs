#![allow(dead_code)]

use std::iter::Iterator;

use petgraph::visit::EdgeRef;
use petgraph::Directed;

use crate::lang::common::Role;
use crate::lang::task::*;
use crate::lang::types::*;

/*
 * Identifier here is a format identifier
*/
type Graph = petgraph::graph::Graph<(), (Role, Identifier), Directed, usize>;

struct TaskGraphImpl {
    graph: Graph,
    my_role: Role,
    psf: PSF,
}

impl TaskGraphImpl {
    fn next(&self, task_completed: TaskID) -> TaskSet {
        let edges: Vec<_> = self
            .graph
            .edges(usize::from(task_completed).into())
            .collect();

        match edges.len() {
            1 => {
                let edge_role = &(*edges[0].weight()).0;
                let edge_format = &(*edges[0].weight()).1;

                let ins =
                    compile_message_to_instrs(self.my_role, *edge_role, edge_format, &self.psf);

                let t = Task {
                    ins,
                    id: edges[0].target().index().into(),
                };

                // I'm the sender
                if self.my_role == *edge_role {
                    TaskSet::OutTask(t)
                } else {
                    TaskSet::InTask(t)
                }
            }
            2 => {
                let t0 = Task {
                    ins: vec![],
                    id: edges[0].target().index().into(),
                };

                let t1 = Task {
                    ins: vec![],
                    id: edges[1].target().index().into(),
                };

                if self.my_role == (*edges[0].weight()).0 {
                    // TaskSet::OutTask(t0)
                    let tp = TaskPair {
                        in_task: t1,
                        out_task: t0,
                    };
                    TaskSet::InAndOutTasks(tp)
                } else {
                    let tp = TaskPair {
                        in_task: t0,
                        out_task: t1,
                    };
                    TaskSet::InAndOutTasks(tp)
                }
            }
            _ => panic!(),
        }
    }
}

fn compile_task_graph<'a, T: Iterator<Item = &'a SequenceSpecifier>>(itr: T) -> Graph {
    let mut graph: Graph = Default::default();

    let start_node = graph.add_node(());
    println!("{:?}", start_node);

    let mut prev_node = start_node;
    for seqspec in itr {
        let edge_weight = (seqspec.role, seqspec.format.clone());

        match seqspec.phase {
            Phase::Handshake => {
                let next_node = graph.add_node(());
                graph.add_edge(prev_node, next_node, edge_weight);
                prev_node = next_node;
            }
            Phase::Data => {
                graph.add_edge(prev_node, prev_node, edge_weight);
            }
        }
    }

    graph
}

fn compile_payload_instrs(
    is_sender: bool,
    format: &Format,
    semantics: &Semantics,
) -> Vec<Instruction> {
    // Need to figure out if the payload field is encoded with a length
    let payload_field_id = semantics.find_field_id(FieldSemantic::Payload).unwrap();
    let payload_field = format.try_get_field_by_name(&payload_field_id).unwrap();

    if let Array::Dynamic(d) = payload_field.dtype {
        let length_field_id = d.try_get_length_field().unwrap();
        let length_field = format.try_get_field_by_name(&length_field_id).unwrap();

        println!("{:?}", length_field);
    }

    if is_sender {
        todo!()
    } else {
        todo!()
    }
}

fn compile_message_to_instrs(
    my_role: Role,
    edge_role: Role,
    format_id: &Identifier,
    psf: &PSF,
) -> Vec<Instruction> {
    let mut instrs: Vec<Instruction> = vec![];

    let afs = psf.formats.get(&format_id).unwrap();
    let format = &afs.format.format;
    let semantics = &afs.semantics;

    let is_sender = my_role == edge_role;

    if is_sender {
        if semantics.find_field_id(FieldSemantic::Payload).is_some() {
            compile_payload_instrs(is_sender, format, semantics);
        }
    }

    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::parse::implementation::tests::*;

    #[test]
    fn test_compile_task_graph() {
        let psf = parse_example_psf();
        let graph = compile_task_graph(psf.sequence.iter());

        let tg = TaskGraphImpl {
            graph,
            my_role: Role::Client,
            psf,
        };

        let mut task_completed: TaskID = Default::default();

        for _ in 1..10 {
            let next_task = tg.next(task_completed);

            match next_task {
                TaskSet::InTask(t) => {
                    println!("In task: {:?}", t);
                    task_completed = t.id;
                }
                TaskSet::OutTask(t) => {
                    println!("Out task: {:?}", t);
                    task_completed = t.id;
                }
                TaskSet::InAndOutTasks(tp) => {
                    println!("{:?}", tp);
                    task_completed = tp.in_task.id;
                }
            }
        }
    }
}
