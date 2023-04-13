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
}

impl TaskGraphImpl {
    fn next(&self, current_task: TaskID) -> TaskSet {
        let edges: Vec<_> = self.graph.edges(usize::from(current_task).into()).collect();

        match edges.len() {
            1 => {
                let t = Task {
                    ins: vec![],
                    id: edges[0].target().index().into(),
                };
                TaskSet::InTask(t)
            }
            2 => {
                todo!()
            }
            _ => panic!(),
        }
    }
}

fn compile_task_graph<'a, T: Iterator<Item = &'a SequenceSpecifier>>(itr: T) -> TaskGraphImpl {
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

    TaskGraphImpl { graph }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::parse::implementation::tests::*;

    #[test]
    fn test_compile_task_graph() {
        let psf = parse_example_psf();
        let tg = compile_task_graph(psf.sequence.iter());
        let mut t: TaskID = Default::default();

        {
            if let TaskSet::InTask(next_task) = tg.next(t) {
                // t = tg.next(next_task.id);
            }
        }
    }
}
