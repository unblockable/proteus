#![allow(dead_code)]

use std::iter::Iterator;

use itertools::Itertools;
use petgraph::visit::EdgeRef;
use petgraph::Directed;

use crate::lang::task::*;
use crate::lang::types::*;
use crate::lang::Role;

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
                        Instruction::ReadApp(ReadAppArgs { from_len: x, .. }) => {
                            *x = 0..x.end;
                        }
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
struct HintsPadding {
    field_id: Identifier,
    length_field_id: Identifier,
    length_field_nbytes: usize,
}

#[derive(Debug)]
struct HintsDynamicPayload {
    payload_field_name: Identifier,
    length_field_name: Identifier,
    length_field_max: usize,
    length_field_nbytes: usize,
    static_prefix_last_field: Identifier,
    // Padding fields
    hints_padding: Option<HintsPadding>,
}

fn generate_dynamic_payload_hints(
    format: &Format,
    semantics: &Semantics,
    crypto: Option<&CryptoSpec>,
) -> Option<HintsDynamicPayload> {
    if semantics.find_field_id(FieldSemantic::Payload).is_none() {
        return None;
    }

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

    let hints_padding =
        if let Some(padding_field_id) = semantics.find_field_id(FieldSemantic::Padding) {
            let padding_field = format.try_get_field_by_name(&padding_field_id).unwrap();
            assert!(matches!(padding_field.dtype, Array::Dynamic(_)));

            let padding_length_field_id = semantics
                .find_field_id(FieldSemantic::PaddingLength)
                .unwrap();

            let padding_length_field = format
                .try_get_field_by_name(&padding_length_field_id)
                .unwrap();

            let padding_len_field_type = TryInto::<NumericType>::try_into(
                TryInto::<PrimitiveArray>::try_into(padding_length_field.dtype).unwrap(),
            )
            .unwrap();

            Some(HintsPadding {
                field_id: padding_field_id,
                length_field_id: padding_length_field_id,
                length_field_nbytes: padding_len_field_type.size_of(),
            })
        } else {
            None
        };

    let padding_space = if hints_padding.is_some() {
        if let Some(block_nbytes) = crypto.as_ref().unwrap().cipher.block_size_nbytes() {
            block_nbytes
        } else {
            0
        }
    } else {
        0
    };

    Some(HintsDynamicPayload {
        payload_field_name: payload_field_id,
        length_field_name: length_field_id,
        length_field_max: len_field_max - (padding_space as usize),
        length_field_nbytes,
        static_prefix_last_field,
        hints_padding,
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

    let maybe_hints_dynamic_payload =
        generate_dynamic_payload_hints(format, semantics, psf.crypto_spec.as_ref());

    let has_padding = maybe_hints_dynamic_payload.is_some()
        && maybe_hints_dynamic_payload
            .as_ref()
            .unwrap()
            .hints_padding
            .is_some();

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
            padding_field: if has_padding {
                Some(
                    maybe_hints_dynamic_payload
                        .as_ref()
                        .unwrap()
                        .hints_padding
                        .as_ref()
                        .unwrap()
                        .field_id
                        .clone(),
                )
            } else {
                None
            },
            block_size_nbytes: if has_padding {
                Some(
                    psf.crypto_spec
                        .as_ref()
                        .unwrap()
                        .cipher
                        .block_size_nbytes()
                        .unwrap()
                        .into(),
                )
            } else {
                None
            },
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

        if let Some(ref hints_padding) = hints_dynamic_payload.hints_padding {
            instrs.push(
                SetNumericValueArgs {
                    // FIXME(rwails) MEGA HACK
                    from_heap_id: "__padding_len_on_heap".id(),
                    to_msg_heap_id: MESSAGE_HEAP_NAME.id(),
                    to_field_id: hints_padding.length_field_id.clone(),
                }
                .into(),
            );

            instrs.push(
                SetArrayBytesArgs {
                    // FIXME(rwails) MEGA HACK
                    from_heap_id: hints_padding.field_id.clone(),
                    to_msg_heap_id: MESSAGE_HEAP_NAME.id(),
                    to_field_id: hints_padding.field_id.clone(),
                }
                .into(),
            );
        }
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

    let mut has_pubkey: Option<Identifier> = None;
    let mut pubkey_enc: Option<PubkeyEncoding> = None;

    let mut length_present_and_nbytes: (bool, usize) = (false, 0);

    for field in &format.fields {
        //field.name
        if field.name.0 == "length" {
            length_present_and_nbytes = (true, field.dtype.maybe_size_of().unwrap());
        }
    }

    for semantic in semantics.as_ref().iter().sorted() {
        let id = semantic.0;
        let fs = semantic.1;

        if let crate::lang::types::FieldSemantic::Pubkey(enc) = fs {
            if has_pubkey.is_none() {
                has_pubkey = Some(id.clone());
                pubkey_enc = Some(*enc);
            }
        }
    }

    let is_sender = my_role == edge_role;

    let maybe_hints_dynamic_payload =
        generate_dynamic_payload_hints(format, semantics, psf.crypto_spec.as_ref());

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

                if let Some(id) = has_pubkey {
                    instrs.push(
                        SaveKeyArgs {
                            from_msg_heap_id: MESSAGE_HEAP_NAME.id(),
                            from_field_id: id.clone(),
                            pubkey_encoding: pubkey_enc.unwrap(),
                        }
                        .into(),
                    );
                }

                // Then encrypt whatever fields we need to encrypt
                for field_dir in &hints_encryption.enc_field_dirs {
                    let ctext_heap_id =
                        (field_dir.ctext_name.0.to_string() + "_heap").as_str().id();

                    let mac_heap_id: Option<Identifier> = match &field_dir.mac_name {
                        Some(x) => Some((x.0.to_string() + "_heap").as_str().id()),
                        None => None,
                    };

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

                    if mac_heap_id.is_some() {
                        instrs.push(
                            SetArrayBytesArgs {
                                from_heap_id: mac_heap_id.unwrap(),
                                to_msg_heap_id: MESSAGE_HEAP_NAME.id(),
                                to_field_id: field_dir
                                    .mac_name
                                    .clone()
                                    .expect("mac field must be present"),
                            }
                            .into(),
                        );
                    }
                }
            } else {
                instrs.extend(compile_plaintext_commands_sender(format_id, psf));
                if let Some(id) = has_pubkey {
                    instrs.push(
                        SaveKeyArgs {
                            from_msg_heap_id: MESSAGE_HEAP_NAME.id(),
                            from_field_id: id.clone(),
                            pubkey_encoding: pubkey_enc.unwrap(),
                        }
                        .into(),
                    );
                }
            }
        } else {
            instrs.extend(compile_plaintext_commands_sender(format_id, psf));

            if let Some(id) = has_pubkey {
                instrs.push(
                    SaveKeyArgs {
                        from_msg_heap_id: MESSAGE_HEAP_NAME.id(),
                        from_field_id: id.clone(),
                        pubkey_encoding: pubkey_enc.unwrap(),
                    }
                    .into(),
                );
            }
        }

        if psf.options.is_some()
            && psf.options.as_ref().unwrap().separate_length_field_setting
            && length_present_and_nbytes.0
        {
            instrs.push(
                WriteNetTwiceArgs {
                    from_msg_heap_id: MESSAGE_HEAP_NAME.id(),
                    len_first_write: length_present_and_nbytes.1,
                }
                .into(),
            );
        } else {
            instrs.push(
                WriteNetArgs {
                    from_msg_heap_id: MESSAGE_HEAP_NAME.id(),
                }
                .into(),
            );
        }
    } else {
        // Is receiver
        let (prefix, suffix) = format.split_into_fixed_sized_prefix_dynamic_suffix();

        let hints_padding = if let Some(payload_hints) =
            generate_dynamic_payload_hints(format, semantics, psf.crypto_spec.as_ref())
        {
            payload_hints.hints_padding
        } else {
            None
        };

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

                    // FIXME(rwails)
                    padding_field: None,
                    block_size_nbytes: None,
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

            if let Some(id) = has_pubkey {
                instrs.push(
                    SaveKeyArgs {
                        from_msg_heap_id: MSG_PFX_HEAP_NAME.id(),
                        from_field_id: id.clone(),
                        pubkey_encoding: pubkey_enc.unwrap(),
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
                            let from_mac_field_id = match &field_dir.mac_name {
                                Some(x) => Some(x.clone()),
                                None => None,
                            };

                            instrs.push(
                                DecryptFieldArgs {
                                    from_msg_heap_id: MSG_PFX_HEAP_NAME.id(),
                                    from_ciphertext_field_id: ctext_name.clone(),
                                    from_mac_field_id,
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

        // RSW - What I think I need to do is add a new instruction
        // which happens after this point to set the key.

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

                if let Some(ref hints_padding) = hints_padding {
                    instrs.push(
                        GetNumericValueArgs {
                            from_msg_heap_id: MSG_PFX_HEAP_NAME.id(),
                            from_field_id: hints_padding.length_field_id.clone(),
                            to_heap_id: "__padding_len_on_heap".id(),
                        }
                        .into(),
                    );

                    instrs.push(
                        ReadNetArgs {
                            from_len: ReadNetLength::IdentifierMinusMinus((
                                LENGTH_ON_HEAP_NAME.id(),
                                "__padding_len_on_heap".id(),
                                fixed_tail_size,
                            )),
                            to_heap_id: hints_dynamic_payload.payload_field_name.clone(),
                        }
                        .into(),
                    );

                    instrs.push(
                        ReadNetArgs {
                            from_len: ReadNetLength::Identifier("__padding_len_on_heap".id()),
                            to_heap_id: hints_padding.field_id.clone(),
                        }
                        .into(),
                    );
                } else {
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
                }

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

                        // FIXME
                        padding_field: None,
                        block_size_nbytes: None,
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

                                let from_mac_field_id = match &field_dir.mac_name {
                                    Some(x) => Some(x.clone()),
                                    None => None,
                                };
                                // Decrypt it
                                instrs.push(
                                    DecryptFieldArgs {
                                        from_msg_heap_id: MSG_SFX_HEAP_NAME.id(),
                                        from_ciphertext_field_id: ctext_name.clone(),
                                        from_mac_field_id,
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

            println!("{:?}\n\n", next_task);

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
            my_role: Role::Client,
            psf,
        };

        let mut task_completed: TaskID = Default::default();

        for _ in 1..10 {
            let next_task = tg.next(task_completed);

            println!("{:?}", next_task);

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
