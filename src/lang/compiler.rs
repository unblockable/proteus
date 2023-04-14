#![allow(dead_code)]

use std::iter::Iterator;

use petgraph::visit::EdgeRef;
use petgraph::Directed;

use crate::lang::common::Role;
use crate::lang::task::*;
use crate::lang::types::*;

/*
 * Some assumptions the compiler is making that the parser should check for:
 *
 * - There is one length field and one payload field for payload-carrying messages
 * - The length field for the payload field is not undefined
 * - The fixed-size length field for the payload should be in the prefix
 */

/*
 * Identifier here is a format identifier
*/
type Graph = petgraph::graph::Graph<(), (Role, Identifier), Directed, usize>;

pub struct TaskGraphImpl {
    graph: Graph,
    my_role: Role,
    psf: PSF,
}

impl TaskGraphImpl {
    pub fn new(graph: Graph, my_role: Role, psf: PSF) -> TaskGraphImpl {
        TaskGraphImpl {
            graph,
            my_role,
            psf,
        }
    }

    pub fn next(&self, task_completed: TaskID) -> TaskSet {
        let edges: Vec<_> = self
            .graph
            .edges(usize::from(task_completed).into())
            .collect();

        match edges.len() {
            1 => {
                let edge_role = &(edges[0].weight()).0;
                let edge_format = &(edges[0].weight()).1;

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
                let edge0_role = &(edges[0].weight()).0;
                let edge0_format = &(edges[0].weight()).1;

                let edge1_role = &(edges[1].weight()).0;
                let edge1_format = &(edges[1].weight()).1;

                let ins0 =
                    compile_message_to_instrs(self.my_role, *edge0_role, edge0_format, &self.psf);

                let ins1 =
                    compile_message_to_instrs(self.my_role, *edge1_role, edge1_format, &self.psf);

                let t0 = Task {
                    ins: ins0,
                    id: edges[0].target().index().into(),
                };

                let t1 = Task {
                    ins: ins1,
                    id: edges[1].target().index().into(),
                };

                if self.my_role == (edges[0].weight()).0 {
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

pub fn compile_task_graph<'a, T: Iterator<Item = &'a SequenceSpecifier>>(itr: T) -> Graph {
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

#[derive(Debug)]
struct HintsDynamicPayload {
    payload_field_name: Identifier,
    length_field_name: Identifier,
    length_field_max: usize,
    length_field_nbytes: usize,
}

fn generate_dynamic_payload_hints(
    format: &Format,
    semantics: &Semantics,
) -> Option<HintsDynamicPayload> {
    // Need to figure out if the payload field is encoded with a length
    let payload_field_id = semantics.find_field_id(FieldSemantic::Payload).unwrap();
    let payload_field = format.try_get_field_by_name(&payload_field_id).unwrap();

    let len_field_max: usize;
    let length_field_id: Identifier;
    let length_field_nbytes: usize;

    if let Array::Dynamic(d) = payload_field.dtype {
        length_field_id = d.try_get_length_field().unwrap();
        let length_field = format.try_get_field_by_name(&length_field_id).unwrap();

        let len_field_type = TryInto::<NumericType>::try_into(
            TryInto::<PrimitiveArray>::try_into(length_field.dtype).unwrap(),
        )
        .unwrap();

        len_field_max = len_field_type.bounds().1.try_into().unwrap();

        length_field_nbytes = len_field_type.size_of();
    } else {
        return None;
    }

    Some(HintsDynamicPayload {
        payload_field_name: payload_field_id,
        length_field_name: length_field_id,
        length_field_max: len_field_max,
        length_field_nbytes,
    })
}

fn compile_message_to_instrs(
    my_role: Role,
    edge_role: Role,
    format_id: &Identifier,
    psf: &PSF,
) -> Vec<Instruction> {
    let mut instrs: Vec<Instruction> = vec![];

    let afs = psf.formats.get(format_id).unwrap();
    let format = &afs.format.format;
    let semantics = &afs.semantics;

    let is_sender = my_role == edge_role;

    let maybe_hints_dynamic_payload = generate_dynamic_payload_hints(&format, &semantics);

    if is_sender {
        const CFORMAT_HEAP_NAME: &'static str = "cformat_on_heap";
        const MESSAGE_HEAP_NAME: &'static str = "message_on_heap";
        const LEN_FIELD_HEAP_NAME: &'static str = "length_value_on_heap";

        // Handle dynamic length fields
        let mut dynamic_field_names = vec![];

        if let Some(ref hints_dynamic_payload) = maybe_hints_dynamic_payload {
            instrs.push(
                ReadAppArgs {
                    name: hints_dynamic_payload.payload_field_name.clone(),
                    len: 1..hints_dynamic_payload.length_field_max.into(),
                }
                .into(),
            );

            dynamic_field_names.push(hints_dynamic_payload.payload_field_name.clone());
        }

        instrs.push(
            ConcretizeFormatArgs {
                name: CFORMAT_HEAP_NAME.id(),
                aformat: AbstractFormat {
                    format: format.clone(),
                },
            }
            .into(),
        );

        instrs.push(
            CreateMessageArgs {
                name: MESSAGE_HEAP_NAME.id(),
                fmt_name: CFORMAT_HEAP_NAME.id(),
                field_names: dynamic_field_names,
            }
            .into(),
        );

        // If there's a length field to set, set it here.

        if let Some(ref hints_dynamic_payload) = maybe_hints_dynamic_payload {
            instrs.push(
                ComputeLengthArgs {
                    name: LEN_FIELD_HEAP_NAME.id(),
                    msg_name: MESSAGE_HEAP_NAME.id(),
                    field_name: hints_dynamic_payload.length_field_name.clone(),
                }
                .into(),
            );

            instrs.push(
                SetNumericValueArgs {
                    msg_name: MESSAGE_HEAP_NAME.id(),
                    field_name: hints_dynamic_payload.length_field_name.clone(),
                    name: LEN_FIELD_HEAP_NAME.id(),
                }
                .into(),
            );
        }

        instrs.push(
            WriteNetArgs {
                msg_name: MESSAGE_HEAP_NAME.id(),
            }
            .into(),
        );
    } else {
        // Is receiver
        let (prefix, suffix) = format.split_into_fixed_sized_prefix_dynamic_suffix();

        let has_prefix = prefix.fields.len() > 0;
        let has_suffix = suffix.fields.len() > 0;

        const CFORMAT_PFX_HEAP_NAME: &'static str = "cformat_prefix_on_heap";
        const CFORMAT_SFX_HEAP_NAME: &'static str = "cformat_suffix_on_heap";

        const MSG_PFX_HEAP_NAME: &'static str = "message_prefix_on_heap";
        const MSG_SFX_HEAP_NAME: &'static str = "message_suffix_on_heap";

        const LENGTH_ON_HEAP_NAME: &'static str = "num_payload_bytes_on_heap";

        if has_prefix {
            // Read the fixed-size elements
            for field in &prefix.fields[..] {
                let field_nbytes = field.maybe_size_of().unwrap();

                instrs.push(
                    ReadNetArgs {
                        name: field.name.clone(),
                        len: ReadNetLength::Range(field_nbytes..field_nbytes + 1),
                    }
                    .into(),
                );
            }

            instrs.push(
                ConcretizeFormatArgs {
                    name: CFORMAT_PFX_HEAP_NAME.id(),
                    aformat: AbstractFormat {
                        format: prefix.clone(),
                    },
                }
                .into(),
            );

            instrs.push(
                CreateMessageArgs {
                    name: MSG_PFX_HEAP_NAME.id(),
                    fmt_name: CFORMAT_PFX_HEAP_NAME.id(),
                    field_names: prefix.fields.iter().map(|e| e.name.clone()).collect(),
                }
                .into(),
            )
        } // has_prefix

        if has_suffix {
            // Only support one payload field
            assert!(suffix.fields.len() == 1);

            if let Some(ref hints_dynamic_payload) = maybe_hints_dynamic_payload {
                // The length field must exist in the fixed-size prefix.
                instrs.push(
                    GetNumericValueArgs {
                        name: LENGTH_ON_HEAP_NAME.id(),
                        msg_name: MSG_PFX_HEAP_NAME.id(),
                        field_name: hints_dynamic_payload.length_field_name.clone(),
                    }
                    .into(),
                );

                instrs.push(
                    ReadNetArgs {
                        name: hints_dynamic_payload.payload_field_name.clone(),
                        len: ReadNetLength::Identifier(LENGTH_ON_HEAP_NAME.id()),
                    }
                    .into(),
                );

                instrs.push(
                    ConcretizeFormatArgs {
                        name: CFORMAT_SFX_HEAP_NAME.id(),
                        aformat: AbstractFormat {
                            format: format.clone(),
                        },
                    }
                    .into(),
                );

                instrs.push(
                    CreateMessageArgs {
                        name: MSG_SFX_HEAP_NAME.id(),
                        fmt_name: CFORMAT_SFX_HEAP_NAME.id(),
                        field_names: suffix.fields.iter().map(|e| e.name.clone()).collect(),
                    }
                    .into(),
                );

                instrs.push(
                    WriteAppArgs {
                        msg_name: MSG_SFX_HEAP_NAME.id(),
                        field_name: hints_dynamic_payload.payload_field_name.clone(),
                    }
                    .into(),
                );
            }
        } // has_suffix
    } // receiver

    instrs
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
            my_role: Role::Server,
            psf,
        };

        let mut task_completed: TaskID = Default::default();

        for _ in 1..10 {
            let next_task = tg.next(task_completed);

            match next_task {
                TaskSet::InTask(t) => {
                    task_completed = t.id;
                }
                TaskSet::OutTask(t) => {
                    task_completed = t.id;
                }
                TaskSet::InAndOutTasks(tp) => {
                    task_completed = tp.in_task.id;
                }
            }
        }
    }
}
