use crate::lang::task::*;
use crate::lang::types::*;
use crate::lang::Role;

#[derive(Clone)]
pub struct EncryptedLengthPayloadSpec {
    role: Role,
    abs_format_out: AbstractFormat,
    abs_format_in1: AbstractFormat,
    abs_format_in2: AbstractFormat,
}

impl EncryptedLengthPayloadSpec {
    pub fn new(role: Role) -> Self {
        let abs_format_out: AbstractFormat = Format {
            name: "DataMessageOut".id(),
            fields: vec![
                Field {
                    name: "length".id(),
                    dtype: PrimitiveArray(NumericType::U16.into(), 1).into(),
                },
                Field {
                    name: "length_mac".id(),
                    dtype: PrimitiveArray(NumericType::U8.into(), 16).into(),
                },
                Field {
                    name: "payload".id(),
                    dtype: DynamicArray(UnaryOp::SizeOf("length".id())).into(),
                },
                Field {
                    name: "payload_mac".id(),
                    dtype: PrimitiveArray(NumericType::U8.into(), 16).into(),
                },
            ],
        }
        .into();

        let abs_format_in1: AbstractFormat = Format {
            name: "DataMessageIn1".id(),
            fields: vec![
                Field {
                    name: "length".id(),
                    dtype: PrimitiveArray(NumericType::U16.into(), 1).into(),
                },
                Field {
                    name: "length_mac".id(),
                    dtype: PrimitiveArray(NumericType::U8.into(), 16).into(),
                },
            ],
        }
        .into();

        let abs_format_in2: AbstractFormat = Format {
            name: "DataMessageIn2".id(),
            fields: vec![
                Field {
                    name: "payload".id(),
                    dtype: DynamicArray(UnaryOp::SizeOf("length".id())).into(),
                },
                Field {
                    name: "payload_mac".id(),
                    dtype: PrimitiveArray(NumericType::U8.into(), 16).into(),
                },
            ],
        }
        .into();

        Self {
            role,
            abs_format_out,
            abs_format_in1,
            abs_format_in2,
        }
    }
}

impl TaskProvider for EncryptedLengthPayloadSpec {
    fn get_init_task(&self) -> Task {
        let password = "hunter2";

        Task {
            id: Default::default(),
            ins: vec![InitFixedSharedKeyArgs {
                password: password.to_string(),
                role: self.role,
            }
            .into()],
        }
    }

    fn get_next_tasks(&self, _last_task: &TaskID) -> TaskSet {
        // Outgoing data forwarding direction.
        let out_task = Task {
            ins: vec![
                ReadAppArgs {
                    from_len: 1..(u16::MAX - 32) as usize,
                    to_heap_id: "payload".id(),
                }
                .into(),
                ConcretizeFormatArgs {
                    from_format: self.abs_format_out.clone(),
                    to_heap_id: "cformat".id(),
                    padding_field: None,
                    block_size_nbytes: None,
                }
                .into(),
                CreateMessageArgs {
                    from_format_heap_id: "cformat".id(),
                    to_heap_id: "message".id(),
                }
                .into(),
                SetArrayBytesArgs {
                    from_heap_id: "payload".id(),
                    to_msg_heap_id: "message".id(),
                    to_field_id: "payload".id(),
                }
                .into(),
                ComputeLengthArgs {
                    from_msg_heap_id: "message".id(),
                    from_field_id: "length_mac".id(),
                    to_heap_id: "length_value_on_heap".id(),
                }
                .into(),
                SetNumericValueArgs {
                    from_heap_id: "length_value_on_heap".id(),
                    to_msg_heap_id: "message".id(),
                    to_field_id: "length".id(),
                }
                .into(),
                EncryptFieldArgs {
                    from_msg_heap_id: "message".id(),
                    from_field_id: "length".id(),
                    to_ciphertext_heap_id: "enc_length_heap".id(),
                    to_mac_heap_id: Some("length_mac_heap".id()),
                }
                .into(),
                EncryptFieldArgs {
                    from_msg_heap_id: "message".id(),
                    from_field_id: "payload".id(),
                    to_ciphertext_heap_id: "enc_payload_heap".id(),
                    to_mac_heap_id: Some("payload_mac_heap".id()),
                }
                .into(),
                SetArrayBytesArgs {
                    from_heap_id: "enc_length_heap".id(),
                    to_msg_heap_id: "message".id(),
                    to_field_id: "length".id(),
                }
                .into(),
                SetArrayBytesArgs {
                    from_heap_id: "enc_payload_heap".id(),
                    to_msg_heap_id: "message".id(),
                    to_field_id: "payload".id(),
                }
                .into(),
                SetArrayBytesArgs {
                    from_heap_id: "length_mac_heap".id(),
                    to_msg_heap_id: "message".id(),
                    to_field_id: "length_mac".id(),
                }
                .into(),
                SetArrayBytesArgs {
                    from_heap_id: "payload_mac_heap".id(),
                    to_msg_heap_id: "message".id(),
                    to_field_id: "payload_mac".id(),
                }
                .into(),
                WriteNetArgs {
                    from_msg_heap_id: "message".id(),
                }
                .into(),
            ],
            id: TaskID::default(),
        };

        // Incoming data forwarding direction.
        let in_task = Task {
            ins: vec![
                ReadNetArgs {
                    from_len: ReadNetLength::Range(2..3 as usize),
                    to_heap_id: "length".id(),
                }
                .into(),
                ReadNetArgs {
                    from_len: ReadNetLength::Range(16..17 as usize),
                    to_heap_id: "length_mac".id(),
                }
                .into(),
                ConcretizeFormatArgs {
                    from_format: self.abs_format_in1.clone(),
                    to_heap_id: "cformat1".id(),
                    padding_field: None,
                    block_size_nbytes: None,
                }
                .into(),
                CreateMessageArgs {
                    from_format_heap_id: "cformat1".id(),
                    to_heap_id: "message_length_part".id(),
                }
                .into(),
                SetArrayBytesArgs {
                    from_heap_id: "length".id(),
                    to_msg_heap_id: "message_length_part".id(),
                    to_field_id: "length".id(),
                }
                .into(),
                SetArrayBytesArgs {
                    from_heap_id: "length_mac".id(),
                    to_msg_heap_id: "message_length_part".id(),
                    to_field_id: "length_mac".id(),
                }
                .into(),
                DecryptFieldArgs {
                    from_msg_heap_id: "message_length_part".id(),
                    from_ciphertext_field_id: "length".id(),
                    from_mac_field_id: Some("length_mac".id()),
                    to_plaintext_heap_id: "dec_length_heap".id(),
                }
                .into(),
                SetArrayBytesArgs {
                    from_heap_id: "dec_length_heap".id(),
                    to_msg_heap_id: "message_length_part".id(),
                    to_field_id: "length".id(),
                }
                .into(),
                GetNumericValueArgs {
                    from_msg_heap_id: "message_length_part".id(),
                    from_field_id: "length".id(),
                    to_heap_id: "payload_len_value_heap".id(),
                }
                .into(),
                ReadNetArgs {
                    from_len: ReadNetLength::IdentifierMinus(("payload_len_value_heap".id(), 16)),
                    to_heap_id: "payload".id(),
                }
                .into(),
                ReadNetArgs {
                    from_len: ReadNetLength::Range(16..17 as usize),
                    to_heap_id: "payload_mac".id(),
                }
                .into(),
                ConcretizeFormatArgs {
                    from_format: self.abs_format_in2.clone(),
                    to_heap_id: "cformat2".id(),
                    padding_field: None,
                    block_size_nbytes: None,
                }
                .into(),
                CreateMessageArgs {
                    from_format_heap_id: "cformat2".id(),
                    to_heap_id: "message_payload_part".id(),
                }
                .into(),
                SetArrayBytesArgs {
                    from_heap_id: "payload".id(),
                    to_msg_heap_id: "message_payload_part".id(),
                    to_field_id: "payload".id(),
                }
                .into(),
                SetArrayBytesArgs {
                    from_heap_id: "payload_mac".id(),
                    to_msg_heap_id: "message_payload_part".id(),
                    to_field_id: "payload_mac".id(),
                }
                .into(),
                DecryptFieldArgs {
                    from_msg_heap_id: "message_payload_part".id(),
                    from_ciphertext_field_id: "payload".id(),
                    from_mac_field_id: Some("payload_mac".id()),
                    to_plaintext_heap_id: "dec_payload_heap".id(),
                }
                .into(),
                SetArrayBytesArgs {
                    from_heap_id: "dec_payload_heap".id(),
                    to_msg_heap_id: "message_payload_part".id(),
                    to_field_id: "payload".id(),
                }
                .into(),
                WriteAppArgs {
                    from_msg_heap_id: "message_payload_part".id(),
                    from_field_id: "payload".id(),
                }
                .into(),
            ],
            id: TaskID::default(),
        };

        // Concurrently execute tasks for both data forwarding directions.
        TaskSet::InAndOutTasks(TaskPair { out_task, in_task })
    }
}
