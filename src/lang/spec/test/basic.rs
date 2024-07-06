use crate::lang::{Role, task::*, types::*};

pub struct LengthPayloadSpec {
    _role: Role, // both sides are identical
    abs_format_out: AbstractFormat,
    abs_format_in1: AbstractFormat,
    abs_format_in2: AbstractFormat,
}

impl LengthPayloadSpec {
    pub fn new(role: Role) -> Self {
        let abs_format_out: AbstractFormat = Format {
            name: "DataMessageOut".id(),
            fields: vec![
                Field {
                    name: "length".id(),
                    dtype: PrimitiveArray(NumericType::U16.into(), 1).into(),
                },
                Field {
                    name: "payload".id(),
                    dtype: DynamicArray(UnaryOp::SizeOf("length".id())).into(),
                },
            ],
        }
        .into();

        let abs_format_in1: AbstractFormat = Format {
            name: "DataMessageIn1".id(),
            fields: vec![Field {
                name: "length".id(),
                dtype: PrimitiveArray(NumericType::U16.into(), 1).into(),
            }],
        }
        .into();

        let abs_format_in2: AbstractFormat = Format {
            name: "DataMessageIn2".id(),
            fields: vec![Field {
                name: "payload".id(),
                dtype: DynamicArray(UnaryOp::SizeOf("length".id())).into(),
            }],
        }
        .into();

        Self {
            _role: role,
            abs_format_out,
            abs_format_in1,
            abs_format_in2,
        }
    }
}

impl TaskProvider for LengthPayloadSpec {
    fn get_init_task(&self) -> Task {
        Task {
            ins: vec![],
            id: Default::default(),
        }
    }

    fn get_next_tasks(&self, _last_task: &TaskID) -> TaskSet {
        // Outgoing data forwarding direction.
        let out_task = Task {
            ins: vec![
                ReadAppArgs {
                    from_len: 1..u16::MAX as usize,
                    to_heap_id: "payload".id(),
                }
                .into(),
                ConcretizeFormatArgs {
                    from_format: self.abs_format_out.clone(),
                    to_heap_id: "cformat".id(),
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
                    from_field_id: "length".id(),
                    to_heap_id: "length_value_on_heap".id(),
                }
                .into(),
                SetNumericValueArgs {
                    from_heap_id: "length_value_on_heap".id(),
                    to_msg_heap_id: "message".id(),
                    to_field_id: "length".id(),
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
                ConcretizeFormatArgs {
                    from_format: self.abs_format_in1.clone(),
                    to_heap_id: "cformat1".id(),
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
                GetNumericValueArgs {
                    from_msg_heap_id: "message_length_part".id(),
                    from_field_id: "length".id(),
                    to_heap_id: "payload_len_value".id(),
                }
                .into(),
                ReadNetArgs {
                    from_len: ReadNetLength::Identifier("payload_len_value".id()),
                    to_heap_id: "payload".id(),
                }
                .into(),
                ConcretizeFormatArgs {
                    from_format: self.abs_format_in2.clone(),
                    to_heap_id: "cformat2".id(),
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
