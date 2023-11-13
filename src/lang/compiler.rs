#![allow(dead_code)]

use std::iter::Iterator;

use petgraph::visit::EdgeRef;
use petgraph::Directed;

use crate::lang::Role;
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

#[derive(Clone)]
pub struct TaskGraphImpl {
    graph: Graph,
    my_role: Role,
    psf: Psf,
}

impl TaskGraphImpl {
    pub fn new(graph: Graph, my_role: Role, psf: Psf) -> TaskGraphImpl {
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

                let mut ins =
                    compile_message_to_instrs(self.my_role, *edge_role, edge_format, &self.psf);

                // This adjusts read app instructions during the handshake phase to not necessarily
                // require bytes
                for i in &mut ins {
                    match i {
                        Instruction::ReadApp(ReadAppArgs{from_len: x, ..}) => {
                            *x = 0..x.end;
                        },
                        _ => {}
                    }
                }

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

    pub fn init_task(&self) -> Task {
        let mut ins: Vec<Instruction> = vec![];

        if let Some(ref crypto_spec) = self.psf.crypto_spec {
            if let Some(ref password) = crypto_spec.password {
                ins.push(
                    InitFixedSharedKeyArgs {
                        password: password.0.clone(),
                        role: self.my_role,
                    }
                    .into(),
                );
            }
        }

        Task {
            id: Default::default(),
            ins,
        }
    }
}

pub fn compile_task_graph<'a, T: Iterator<Item = &'a SequenceSpecifier>>(itr: T) -> Graph {
    let mut graph: Graph = Default::default();

    let start_node = graph.add_node(());

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
    static_prefix_last_field: Identifier,
}

fn generate_dynamic_payload_hints(
    format: &Format,
    semantics: &Semantics,
) -> Option<HintsDynamicPayload> {
    // Need to figure out if the payload field is encoded with a length
    let payload_field_id = semantics.find_field_id(FieldSemantic::Payload).unwrap();
    let payload_field = format.try_get_field_by_name(&payload_field_id).unwrap();

    let (static_prefix, dynamic_suffix) = format.split_into_fixed_sized_prefix_dynamic_suffix();

    let (_, dynamic_suffix_fixed_part) =
        dynamic_suffix.split_into_dynamic_prefix_and_fixed_suffix();
    let suffix_fixed_size = dynamic_suffix_fixed_part.fixed_fields_size();

    let static_prefix_last_field: Identifier;
    if let Some(last_field) = static_prefix.fields.last() {
        static_prefix_last_field = last_field.name.clone();
    } else {
        return None;
    }

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

        len_field_max = <u128 as TryInto<usize>>::try_into(len_field_type.bounds().1).unwrap()
            - suffix_fixed_size;

        length_field_nbytes = len_field_type.size_of();
    } else {
        return None;
    }

    Some(HintsDynamicPayload {
        payload_field_name: payload_field_id,
        length_field_name: length_field_id,
        length_field_max: len_field_max,
        length_field_nbytes,
        static_prefix_last_field,
    })
}

#[derive(Debug)]
struct HintsEncryption {
    starting_format: Identifier,
    enc_field_dirs: Vec<EncryptionFieldDirective>,
}

fn generate_encryption_hints(format: &Format, crypto_spec: &CryptoSpec) -> Option<HintsEncryption> {
    let fname = &format.name;

    let directive: Vec<_> = crypto_spec
        .directives
        .iter()
        .filter(|(k, _)| k.to_format_name == *fname)
        .collect();

    match directive.len() {
        0 => None,
        1 => {
            let d = &directive[0].1;
            Some(HintsEncryption {
                starting_format: d.enc_fmt_bnd.from_format_name.clone(),
                enc_field_dirs: d.enc_field_dirs.clone(),
            })
        }
        _ => panic!("Ambigious encryption field directives"),
    }
}

static CFORMAT_HEAP_NAME: &str = "cformat_on_heap";
static MESSAGE_HEAP_NAME: &str = "message_on_heap";
static LEN_FIELD_HEAP_NAME: &str = "length_value_on_heap";

fn compile_plaintext_commands_sender(format_id: &Identifier, psf: &Psf) -> Vec<Instruction> {
    let mut instrs: Vec<Instruction> = vec![];

    let afs = psf.formats.get(format_id).unwrap();
    let format = &afs.format.format;
    let semantics = &afs.semantics;

    let maybe_hints_dynamic_payload = generate_dynamic_payload_hints(format, semantics);

    // Handle dynamic length fields
    let mut dynamic_field_names = vec![];

    if let Some(ref hints_dynamic_payload) = maybe_hints_dynamic_payload {
        instrs.push(
            ReadAppArgs {
                from_len: 1..hints_dynamic_payload.length_field_max,
                to_heap_id: hints_dynamic_payload.payload_field_name.clone(),
            }
            .into(),
        );

        dynamic_field_names.push(hints_dynamic_payload.payload_field_name.clone());
    }

    instrs.push(
        ConcretizeFormatArgs {
            from_format: AbstractFormat {
                format: format.clone(),
                fixed_fields: afs.semantics.get_fixed_fields(),
            },
            to_heap_id: CFORMAT_HEAP_NAME.id(),
        }
        .into(),
    );

    instrs.push(
        CreateMessageArgs {
            from_format_heap_id: CFORMAT_HEAP_NAME.id(),
            to_heap_id: MESSAGE_HEAP_NAME.id(),
        }
        .into(),
    );

    for name in dynamic_field_names {
        instrs.push(
            SetArrayBytesArgs {
                from_heap_id: name.clone(),
                to_msg_heap_id: MESSAGE_HEAP_NAME.id(),
                to_field_id: name.clone(),
            }
            .into(),
        );
    }

    // If there's a length field to set, set it here.

    if let Some(ref hints_dynamic_payload) = maybe_hints_dynamic_payload {
        instrs.push(
            ComputeLengthArgs {
                from_msg_heap_id: MESSAGE_HEAP_NAME.id(),
                from_field_id: hints_dynamic_payload.static_prefix_last_field.clone(),
                to_heap_id: LEN_FIELD_HEAP_NAME.id(),
            }
            .into(),
        );

        instrs.push(
            SetNumericValueArgs {
                from_heap_id: LEN_FIELD_HEAP_NAME.id(),
                to_msg_heap_id: MESSAGE_HEAP_NAME.id(),
                to_field_id: hints_dynamic_payload.length_field_name.clone(),
            }
            .into(),
        );
    }

    instrs
}

fn compile_message_to_instrs(
    my_role: Role,
    edge_role: Role,
    format_id: &Identifier,
    psf: &Psf,
) -> Vec<Instruction> {
    let mut instrs: Vec<Instruction> = vec![];

    let afs = psf.formats.get(format_id).unwrap();
    let format = &afs.format.format;
    let semantics = &afs.semantics;

    let is_sender = my_role == edge_role;

    let maybe_hints_dynamic_payload = generate_dynamic_payload_hints(format, semantics);

    if is_sender {
        if let Some(ref crypto_spec) = psf.crypto_spec {
            let maybe_hints_encryption = generate_encryption_hints(format, crypto_spec);

            if let Some(ref hints_encryption) = maybe_hints_encryption {
                if hints_encryption.starting_format != format.name {
                    unimplemented!();
                }

                // Set up the original message
                instrs.extend(compile_plaintext_commands_sender(
                    &hints_encryption.starting_format,
                    psf,
                ));

                // Then encrypt whatever fields we need to encrypt
                for field_dir in &hints_encryption.enc_field_dirs {
                    let ctext_heap_id =
                        (field_dir.ctext_name.0.to_string() + "_heap").as_str().id();
                    let mac_heap_id = (field_dir.mac_name.0.to_string() + "_heap").as_str().id();

                    instrs.push(
                        EncryptFieldArgs {
                            from_msg_heap_id: MESSAGE_HEAP_NAME.id(),
                            from_field_id: field_dir.ptext_name.clone(),
                            to_ciphertext_heap_id: ctext_heap_id.clone(),
                            to_mac_heap_id: mac_heap_id.clone(),
                        }
                        .into(),
                    );

                    instrs.push(
                        SetArrayBytesArgs {
                            from_heap_id: ctext_heap_id.clone(),
                            to_msg_heap_id: MESSAGE_HEAP_NAME.id(),
                            to_field_id: field_dir.ctext_name.clone(),
                        }
                        .into(),
                    );

                    instrs.push(
                        SetArrayBytesArgs {
                            from_heap_id: mac_heap_id,
                            to_msg_heap_id: MESSAGE_HEAP_NAME.id(),
                            to_field_id: field_dir.mac_name.clone(),
                        }
                        .into(),
                    );
                }
            } else {
                instrs.extend(compile_plaintext_commands_sender(format_id, psf));
            }
        } else {
            instrs.extend(compile_plaintext_commands_sender(format_id, psf));
        }

        instrs.push(
            WriteNetArgs {
                from_msg_heap_id: MESSAGE_HEAP_NAME.id(),
            }
            .into(),
        );
    } else {
        // Is receiver
        let (prefix, suffix) = format.split_into_fixed_sized_prefix_dynamic_suffix();

        let has_prefix = !prefix.fields.is_empty();
        let has_suffix = !suffix.fields.is_empty();

        const CFORMAT_PFX_HEAP_NAME: &str = "cformat_prefix_on_heap";
        const CFORMAT_SFX_HEAP_NAME: &str = "cformat_suffix_on_heap";

        const MSG_PFX_HEAP_NAME: &str = "message_prefix_on_heap";
        const MSG_SFX_HEAP_NAME: &str = "message_suffix_on_heap";

        const LENGTH_ON_HEAP_NAME: &str = "num_payload_bytes_on_heap";

        if has_prefix {
            // Read the fixed-size elements
            for field in &prefix.fields[..] {
                let field_nbytes = field.maybe_size_of().unwrap();

                instrs.push(
                    ReadNetArgs {
                        from_len: ReadNetLength::Range(field_nbytes..field_nbytes + 1),
                        to_heap_id: field.name.clone(),
                    }
                    .into(),
                );
            }

            instrs.push(
                ConcretizeFormatArgs {
                    from_format: AbstractFormat {
                        format: prefix.clone(),
                        fixed_fields: afs.semantics.get_fixed_fields(),
                    },
                    to_heap_id: CFORMAT_PFX_HEAP_NAME.id(),
                }
                .into(),
            );

            instrs.push(
                CreateMessageArgs {
                    from_format_heap_id: CFORMAT_PFX_HEAP_NAME.id(),
                    to_heap_id: MSG_PFX_HEAP_NAME.id(),
                }
                .into(),
            );

            for name in prefix
                .fields
                .iter()
                .map(|e| e.name.clone())
                .collect::<Vec<Identifier>>()
            {
                instrs.push(
                    SetArrayBytesArgs {
                        from_heap_id: name.clone(),
                        to_msg_heap_id: MSG_PFX_HEAP_NAME.id(),
                        to_field_id: name.clone(),
                    }
                    .into(),
                );
            }

            // Now, if there's anything to decrypt in the prefix, we do it here.

            if let Some(ref crypto_spec) = psf.crypto_spec {
                let maybe_hints_encryption = generate_encryption_hints(format, crypto_spec);

                if let Some(ref hints_encryption) = maybe_hints_encryption {
                    if hints_encryption.starting_format != format.name {
                        unimplemented!();
                    }

                    for field_dir in &hints_encryption.enc_field_dirs {
                        let ctext_name = &field_dir.ctext_name;

                        // If the field exists in the prefix:
                        if prefix.try_get_field_by_name(ctext_name).is_some() {
                            let ptext_heap_name = (field_dir.ptext_name.0.to_string()
                                + "_dec_heap")
                                .as_str()
                                .id();
                            // Decrypt it
                            instrs.push(
                                DecryptFieldArgs {
                                    from_msg_heap_id: MSG_PFX_HEAP_NAME.id(),
                                    from_ciphertext_field_id: ctext_name.clone(),
                                    from_mac_field_id: field_dir.mac_name.clone(),
                                    to_plaintext_heap_id: ptext_heap_name.clone(),
                                }
                                .into(),
                            );

                            // Copy it back
                            instrs.push(
                                SetArrayBytesArgs {
                                    from_heap_id: ptext_heap_name,
                                    to_msg_heap_id: MSG_PFX_HEAP_NAME.id(),
                                    to_field_id: field_dir.ptext_name.clone(),
                                }
                                .into(),
                            );
                        }
                    }
                }
            }
        } // has_prefix

        if has_suffix {
            // Figure out how much stuff is the the fixed-sized tail on the suffix,
            // which is covered by the length field.
            let (_suffix_dynamic_head, suffix_fixed_tail) =
                suffix.split_into_dynamic_prefix_and_fixed_suffix();

            let fixed_tail_size = suffix_fixed_tail.fixed_fields_size();

            if let Some(ref hints_dynamic_payload) = maybe_hints_dynamic_payload {
                // The length field must exist in the fixed-size prefix.
                // Assumes there's only one payload...
                instrs.push(
                    GetNumericValueArgs {
                        from_msg_heap_id: MSG_PFX_HEAP_NAME.id(),
                        from_field_id: hints_dynamic_payload.length_field_name.clone(),
                        to_heap_id: LENGTH_ON_HEAP_NAME.id(),
                    }
                    .into(),
                );

                instrs.push(
                    ReadNetArgs {
                        from_len: ReadNetLength::IdentifierMinus((
                            LENGTH_ON_HEAP_NAME.id(),
                            fixed_tail_size,
                        )),
                        to_heap_id: hints_dynamic_payload.payload_field_name.clone(),
                    }
                    .into(),
                );

                for field in &suffix_fixed_tail.fields {
                    let field_len = field.maybe_size_of().unwrap();

                    instrs.push(
                        ReadNetArgs {
                            from_len: ReadNetLength::Range(field_len..field_len + 1),
                            to_heap_id: field.name.clone(),
                        }
                        .into(),
                    );
                }

                instrs.push(
                    ConcretizeFormatArgs {
                        from_format: AbstractFormat {
                            format: suffix.clone(),
                            fixed_fields: afs.semantics.get_fixed_fields(),
                        },
                        to_heap_id: CFORMAT_SFX_HEAP_NAME.id(),
                    }
                    .into(),
                );

                instrs.push(
                    CreateMessageArgs {
                        from_format_heap_id: CFORMAT_SFX_HEAP_NAME.id(),
                        to_heap_id: MSG_SFX_HEAP_NAME.id(),
                    }
                    .into(),
                );

                for name in suffix
                    .fields
                    .iter()
                    .map(|e| e.name.clone())
                    .collect::<Vec<Identifier>>()
                {
                    instrs.push(
                        SetArrayBytesArgs {
                            from_heap_id: name.clone(),
                            to_msg_heap_id: MSG_SFX_HEAP_NAME.id(),
                            to_field_id: name.clone(),
                        }
                        .into(),
                    );
                }

                // And then we decrypt in the suffix
                if let Some(ref crypto_spec) = psf.crypto_spec {
                    let maybe_hints_encryption = generate_encryption_hints(format, crypto_spec);

                    if let Some(ref hints_encryption) = maybe_hints_encryption {
                        if hints_encryption.starting_format != format.name {
                            unimplemented!();
                        }

                        for field_dir in &hints_encryption.enc_field_dirs {
                            let ctext_name = &field_dir.ctext_name;

                            // If the field exists in the prefix:
                            if suffix.try_get_field_by_name(ctext_name).is_some() {
                                let ptext_heap_name = (field_dir.ptext_name.0.to_string()
                                    + "_dec_heap")
                                    .as_str()
                                    .id();
                                // Decrypt it
                                instrs.push(
                                    DecryptFieldArgs {
                                        from_msg_heap_id: MSG_SFX_HEAP_NAME.id(),
                                        from_ciphertext_field_id: ctext_name.clone(),
                                        from_mac_field_id: field_dir.mac_name.clone(),
                                        to_plaintext_heap_id: ptext_heap_name.clone(),
                                    }
                                    .into(),
                                );

                                // Copy it back
                                instrs.push(
                                    SetArrayBytesArgs {
                                        from_heap_id: ptext_heap_name,
                                        to_msg_heap_id: MSG_SFX_HEAP_NAME.id(),
                                        to_field_id: field_dir.ptext_name.clone(),
                                    }
                                    .into(),
                                );
                            }
                        }
                    }
                }

                instrs.push(
                    WriteAppArgs {
                        from_msg_heap_id: MSG_SFX_HEAP_NAME.id(),
                        from_field_id: hints_dynamic_payload.payload_field_name.clone(),
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
        let psf = parse_example_psf().unwrap();
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

    #[test]
    fn test_compile_shadow_socks() {
        let psf = parse_shadowsocks_psf().unwrap();
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
