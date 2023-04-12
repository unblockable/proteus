use std::{
    ops::Range,
    sync::{Arc, Mutex},
    task::Poll,
};

use bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::lang::{
    mem::{Heap, HeapAddr},
    message::Message,
    spec::proteus::ProteusSpec,
    task::{Instruction, Task, TaskID, TaskProvider, TaskSet},
    types::ConcreteFormat,
};

pub struct SendArgs {
    // Send these bytes.
    pub bytes: Bytes,
}

pub struct RecvArgs {
    // Receive this many bytes.
    pub len: Range<usize>,
    // Store the bytes at this addr on the heap.
    pub addr: HeapAddr,
}

pub enum NetOpOut {
    RecvApp(RecvArgs),
    SendNet(SendArgs),
    Close,
    Error(String),
}

pub enum NetOpIn {
    RecvNet(RecvArgs),
    SendApp(SendArgs),
    Close,
    Error(String),
}

struct TaskOp {
    task: Task,
    next_ins_index: usize
}

struct Interpreter {
    spec: Box<dyn TaskProvider + Send + 'static>,
    bytes_heap: Heap<Bytes>,
    format_heap: Heap<ConcreteFormat>,
    message_heap: Heap<Message>,
    next_netop_out: Option<NetOpOut>,
    next_netop_in: Option<NetOpIn>,
    current_taskop_out: Option<TaskOp>,
    current_taskop_in: Option<TaskOp>,
    wants_tasks: bool,
    last_task_id: TaskID,
}

impl Interpreter {
    fn new(spec: impl TaskProvider + Send + 'static) -> Self {
        Self {
            spec: Box::new(spec),
            bytes_heap: Heap::new(),
            format_heap: Heap::new(),
            message_heap: Heap::new(),
            next_netop_out: None,
            next_netop_in: None,
            current_taskop_out: None,
            current_taskop_in: None,
            wants_tasks: true,
            last_task_id: TaskID::default(),
        }
    }

    /// Inserts the given new task into the old Option. Panics if the option
    /// is Some and its task id does not match the new task id.
    fn set_task(opt: &mut Option<TaskOp>, new: Task) {
        match opt {
            Some(op) => if op.task.id != new.id {panic!("Cannot overwrite task")},
            None => *opt = Some(TaskOp { task: new, next_ins_index: 0 })
        };
    }

    /// Loads task from the task provider. Panics if we already have a current
    /// task in/out, we receive another one from the provider, and the ID of the
    /// new task does not match that of the existing task.
    fn load_tasks(&mut self) {
        match self.spec.get_next_tasks(&self.last_task_id) {
            TaskSet::InTask(task) => Self::set_task(&mut self.current_taskop_in, task),
            TaskSet::OutTask(task) => Self::set_task(&mut self.current_taskop_out, task),
            TaskSet::InAndOutTasks(pair) => {
                Self::set_task(&mut self.current_taskop_in, pair.in_task);
                Self::set_task(&mut self.current_taskop_out, pair.out_task);
            },
        };
        self.wants_tasks = false;
    }

    fn execute_instruction(&mut self, ins: &Instruction) -> Result<(), ()> {
        match ins {
            Instruction::ComputeLength(args) => todo!(),
            Instruction::ConcretizeFormat(args) => todo!(),
            Instruction::CreateMessage(args) => todo!(),
            Instruction::GenRandomBytes(args) => todo!(),
            Instruction::ReadApp(args) => todo!(),
            Instruction::ReadNet(args) => todo!(),
            Instruction::WriteApp(args) => todo!(),
            Instruction::WriteNet(args) => todo!(),
        }
    }

    fn execute_instructions_until_blocked(&mut self, mut op: TaskOp) -> Option<TaskOp> {
        while op.next_ins_index < op.task.ins.len() {
            match self.execute_instruction(&op.task.ins[op.next_ins_index]) {
                Ok(_) => op.next_ins_index += 1,
                Err(_) => break
            };
        }

        if op.next_ins_index < op.task.ins.len() {
            Some(op)
        } else {
            self.wants_tasks = true;
            None
        }
    }

    fn execute_until_blocked(&mut self) {
        if self.wants_tasks {
            self.load_tasks();
        }

        if let Some(op) = self.current_taskop_out.take() {
            self.current_taskop_out = self.execute_instructions_until_blocked(op);
        }

        if let Some(op) = self.current_taskop_in.take() {
            self.current_taskop_in = self.execute_instructions_until_blocked(op);
        }
    }

    /// Return the next outgoing (app->net) command we want the network protocol
    /// to run, or an error if the app->net direction should block for now.
    fn next_net_cmd_out(&mut self) -> Result<NetOpOut, ()> {
        self.execute_until_blocked();
        self.next_netop_out.take().ok_or(())
    }

    /// Return the next incoming (app<-net) command we want the network protocol
    /// to run, or an error if the app<-net direction should block for now.
    fn next_net_cmd_in(&mut self) -> Result<NetOpIn, ()> {
        self.execute_until_blocked();
        self.next_netop_in.take().ok_or(())
    }

    /// Store the given bytes on the heap at the given address.
    fn store(&mut self, addr: HeapAddr, bytes: Bytes) {
        self.bytes_heap.write(addr, bytes);
    }
}

/// Wraps the interpreter allowing us to safely share the internal interpreter
/// state across threads while concurrently running network commands.
#[derive(Clone)]
pub struct SharedAsyncInterpreter {
    // The interpreter is protected by a global interpreter lock.
    inner: Arc<Mutex<Interpreter>>,
}

impl SharedAsyncInterpreter {
    pub fn new(spec: ProteusSpec) -> SharedAsyncInterpreter {
        SharedAsyncInterpreter {
            inner: Arc::new(Mutex::new(Interpreter::new(spec))),
        }
    }

    pub async fn next_net_cmd_out(&mut self) -> NetOpOut {
        // Yield to the async runtime if we can't get the lock, or if the
        // interpreter is not wanting to execute a command yet.
        std::future::poll_fn(move |_| {
            let mut inner = match self.inner.try_lock() {
                Ok(inner) => inner,
                Err(_) => return Poll::Pending,
            };
            match inner.next_net_cmd_out() {
                Ok(cmd) => Poll::Ready(cmd),
                Err(_) => Poll::Pending,
            }
        })
        .await
    }

    pub async fn next_net_cmd_in(&mut self) -> NetOpIn {
        // Yield to the async runtime if we can't get the lock, or if the
        // interpreter is not wanting to execute a command yet.
        std::future::poll_fn(move |_| {
            let mut inner = match self.inner.try_lock() {
                Ok(inner) => inner,
                Err(_) => return Poll::Pending,
            };
            match inner.next_net_cmd_in() {
                Ok(cmd) => Poll::Ready(cmd),
                Err(_) => Poll::Pending,
            }
        })
        .await
    }

    pub async fn store(&mut self, addr: HeapAddr, bytes: Bytes) {
        // Yield to the async runtime if we can't get the lock, or if the
        // interpreter is not wanting to execute a command yet.
        std::future::poll_fn(move |_| match self.inner.try_lock() {
            Ok(mut inner) => Poll::Ready(inner.store(addr.clone(), bytes.clone())),
            Err(_) => Poll::Pending,
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::spec::proteus::ProteusSpecBuilder;
    use crate::lang::task::*;
    use crate::lang::types::*;

    struct LengthPayloadSpec {}

    impl TaskProvider for LengthPayloadSpec {
        fn get_next_tasks(&self, _last_task: &TaskID) -> TaskSet {
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

            // Outgoing data forwarding direction.
            let out_task = Task {
                ins: vec![
                    ReadAppArgs {
                        name: "payload".id(),
                        len: 1..u16::MAX as usize,
                    }
                    .into(),
                    ConcretizeFormatArgs {
                        name: "cformat".id(),
                        aformat: abs_format_out,
                    }
                    .into(),
                    CreateMessageArgs {
                        name: "message".id(),
                        fmt_name: "cformat".id(),
                        field_names: vec!["payload".id()],
                    }
                    .into(),
                    WriteNetArgs {
                        msg_name: "message".id(),
                    }
                    .into(),
                ],
                id: TaskID::default(),
            };

            // Incoming data forwarding direction.
            let in_task = Task {
                ins: vec![
                    ReadNetArgs {
                        name: "length".id(),
                        len: ReadNetLength::Range(2..3 as usize),
                    }
                    .into(),
                    ConcretizeFormatArgs {
                        name: "cformat1".id(),
                        aformat: abs_format_in1,
                    }
                    .into(),
                    CreateMessageArgs {
                        name: "message_length_part".id(),
                        fmt_name: "cformat1".id(),
                        field_names: vec!["length".id()],
                    }
                    .into(),
                    ComputeLengthArgs {
                        name: "num_payload_bytes".id(),
                        msg_name: "message_length_part".id(),
                    }
                    .into(),
                    ReadNetArgs {
                        name: "payload".id(),
                        len: ReadNetLength::Identifier("num_payload_bytes".id()),
                    }
                    .into(),
                    ConcretizeFormatArgs {
                        name: "cformat2".id(),
                        aformat: abs_format_in2,
                    }
                    .into(),
                    CreateMessageArgs {
                        name: "message_payload_part".id(),
                        fmt_name: "cformat2".id(),
                        field_names: vec!["payload".id()],
                    }
                    .into(),
                    WriteAppArgs {
                        msg_name: "message_payload_part".id(),
                    }
                    .into(),
                ],
                id: TaskID::default(),
            };

            // Concurrently execute tasks for both data forwarding directions.
            TaskSet::InAndOutTasks(TaskPair { out_task, in_task })
        }
    }

    #[test]
    fn read_app_write_net() {
        let spec = ProteusSpec::from(ProteusSpecBuilder::new());
        let mut int = Interpreter::new(spec);

        let args = match int.next_net_cmd_out().unwrap() {
            NetOpOut::RecvApp(args) => args,
            _ => panic!("Unexpected interpreter command"),
        };

        let payload = Bytes::from("Attack at dawn");
        assert!(args.len.contains(&payload.len()));

        int.store(args.addr, payload.clone());

        let args = match int.next_net_cmd_out().unwrap() {
            NetOpOut::SendNet(args) => args,
            _ => panic!("Unexpected interpreter command"),
        };

        let mut msg = args.bytes;
        assert_eq!(msg.len(), payload.len() + 2); // 2 for length field
        assert_eq!(msg[2..], payload[..]);

        let len = msg.get_u16();
        assert_eq!(len as usize, payload.len());
    }

    #[test]
    fn read_net_write_app() {
        let spec = ProteusSpec::from(ProteusSpecBuilder::new());
        let mut int = Interpreter::new(spec);

        let args = match int.next_net_cmd_in().unwrap() {
            NetOpIn::RecvNet(args) => args,
            _ => panic!("Unexpected interpreter command"),
        };

        assert!(args.len.contains(&2));
        let payload = Bytes::from("Attack at dawn");
        let mut buf = BytesMut::new();
        buf.put_u16(payload.len() as u16);
        int.store(args.addr, buf.freeze());

        let args = match int.next_net_cmd_in().unwrap() {
            NetOpIn::RecvNet(args) => args,
            _ => panic!("Unexpected interpreter command"),
        };

        assert!(args.len.contains(&payload.len()));
        int.store(args.addr, payload.clone());

        let args = match int.next_net_cmd_in().unwrap() {
            NetOpIn::SendApp(args) => args,
            _ => panic!("Unexpected interpreter command"),
        };

        assert_eq!(args.bytes.len(), payload.len());
        assert_eq!(args.bytes[..], payload[..]);
    }
}
