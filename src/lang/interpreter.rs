use std::{
    fmt,
    ops::Range,
    sync::{Arc, Mutex},
    task::Poll,
};

use bytes::Bytes;

use crate::lang::{
    interpreter,
    mem::Heap,
    message::Message,
    spec::proteus::ProteusSpec,
    task::{Instruction, ReadNetLength, Task, TaskID, TaskProvider, TaskSet},
    types::{ConcreteFormat, Identifier},
};

#[derive(std::fmt::Debug)]
pub enum Error {
    ExecuteFailed,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::ExecuteFailed => write!(f, "Failed to execute instruction"),
        }
    }
}

impl From<interpreter::Error> for String {
    fn from(e: interpreter::Error) -> Self {
        e.to_string()
    }
}

pub struct SendArgs {
    // Send these bytes.
    pub bytes: Bytes,
}

pub struct RecvArgs {
    // Receive this many bytes.
    pub len: Range<usize>,
    // Store the bytes at this addr on the heap.
    pub addr: Identifier,
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

struct Program {
    task: Task,
    next_ins_index: usize,
    bytes_heap: Heap<Bytes>,
    format_heap: Heap<ConcreteFormat>,
    message_heap: Heap<Message>,
    number_heap: Heap<u128>,
}

impl Program {
    fn new(task: Task) -> Self {
        Self {
            task,
            next_ins_index: 0,
            bytes_heap: Heap::new(),
            format_heap: Heap::new(),
            message_heap: Heap::new(),
            number_heap: Heap::new(),
        }
    }

    fn has_next_instruction(&self) -> bool {
        self.next_ins_index < self.task.ins.len()
    }

    fn execute_next_instruction(
        &mut self,
        interpreter: &mut Interpreter,
    ) -> Result<(), interpreter::Error> {
        match &self.task.ins[self.next_ins_index] {
            Instruction::GetNumericValue(args) => {
                let msg = self.message_heap.get(&args.msg_name).unwrap();
                let num = msg.get_field_unsigned_numeric(&args.field_name).unwrap();
                self.number_heap.insert(args.name.clone(), num);
            }
            Instruction::ConcretizeFormat(args) => {
                let aformat = args.aformat.clone();

                // Get the fields that have dynamic lengths, and compute what the lengths
                // will be now that we should have the data for each field on the heap.
                let concrete_sizes: Vec<(Identifier, usize)> = aformat
                    .get_dynamic_arrays()
                    .iter()
                    .map(|id| (id.clone(), self.bytes_heap.get(&id).unwrap().len()))
                    .collect();

                // Now that we know the total size, we can allocate the full format block.
                let cformat = aformat.concretize(&concrete_sizes);

                // Store it for use by later instructions.
                self.format_heap.insert(args.name.clone(), cformat);
            }
            Instruction::CreateMessage(args) => {
                // Create a message with an existing concrete format.
                let cformat = self.format_heap.remove(&args.fmt_name).unwrap();
                let mut msg = Message::new(cformat).unwrap();

                // Copy the specified bytes over to the allocated message.
                for id in args.field_names.iter() {
                    msg.set_field_bytes(id, self.bytes_heap.get(&id).unwrap())
                        .unwrap();
                }

                // Store the message for use in later instructions.
                self.message_heap.insert(args.name.clone(), msg);
            }
            Instruction::GenRandomBytes(_args) => {
                todo!()
            }
            Instruction::ReadApp(args) => {
                let netop = NetOpOut::RecvApp(RecvArgs {
                    len: args.len.clone(),
                    addr: args.name.clone(),
                });
                interpreter.next_netop_out = Some(netop);
            }
            Instruction::ReadNet(args) => {
                let len = match &args.len {
                    ReadNetLength::Identifier(id) => {
                        let num = self.number_heap.get(&id).unwrap();
                        let val = *num as usize;
                        Range {
                            start: val,
                            end: val + 1,
                        }
                    }
                    ReadNetLength::Range(r) => r.clone(),
                };

                let netop = NetOpIn::RecvNet(RecvArgs {
                    len,
                    addr: args.name.clone(),
                });
                interpreter.next_netop_in = Some(netop);
            }
            Instruction::ComputeLength(args) => {
                let msg = self.message_heap.get(&args.msg_name).unwrap();
                let len = msg.len_suffix(&args.field_name);
                self.number_heap.insert(args.name.clone(), len as u128);
            }
            Instruction::SetNumericValue(args) => {
                let val = self.number_heap.get(&args.name).unwrap().clone();
                let mut msg = self.message_heap.remove(&args.msg_name).unwrap();
                msg.set_field_unsigned_numeric(&args.field_name, val)
                    .unwrap();
                self.message_heap.insert(args.msg_name.clone(), msg);
            }
            Instruction::WriteApp(args) => {
                let msg = self.message_heap.remove(&args.msg_name).unwrap();
                let netop = NetOpIn::SendApp(SendArgs {
                    bytes: msg.into_inner_field(&args.field_name).unwrap(),
                });
                interpreter.next_netop_in = Some(netop);
            }
            Instruction::WriteNet(args) => {
                let msg = self.message_heap.remove(&args.msg_name).unwrap();
                let netop = NetOpOut::SendNet(SendArgs {
                    bytes: msg.into_inner(),
                });
                interpreter.next_netop_out = Some(netop);
            }
        };

        self.next_ins_index += 1;

        Ok(())
    }

    fn store_bytes(&mut self, addr: Identifier, bytes: Bytes) {
        self.bytes_heap.insert(addr, bytes);
    }
}

struct Interpreter {
    spec: Box<dyn TaskProvider + Send + 'static>,
    next_netop_out: Option<NetOpOut>,
    next_netop_in: Option<NetOpIn>,
    current_prog_out: Option<Program>,
    current_prog_in: Option<Program>,
    wants_tasks: bool,
    last_task_id: TaskID,
}

impl Interpreter {
    fn new(spec: Box<dyn TaskProvider + Send + 'static>) -> Self {
        Self {
            spec,
            next_netop_out: None,
            next_netop_in: None,
            current_prog_out: None,
            current_prog_in: None,
            wants_tasks: true,
            last_task_id: TaskID::default(),
        }
    }

    /// Loads task from the task provider. Panics if we already have a current
    /// task in/out, we receive another one from the provider, and the ID of the
    /// new task does not match that of the existing task.
    fn load_tasks(&mut self) {
        match self.spec.get_next_tasks(&self.last_task_id) {
            TaskSet::InTask(task) => Self::set_task(&mut self.current_prog_in, task),
            TaskSet::OutTask(task) => Self::set_task(&mut self.current_prog_out, task),
            TaskSet::InAndOutTasks(pair) => {
                Self::set_task(&mut self.current_prog_in, pair.in_task);
                Self::set_task(&mut self.current_prog_out, pair.out_task);
            }
        };
        self.wants_tasks = false;
    }

    /// Inserts the given new task into the old Option. Panics if the option
    /// is Some and its task id does not match the new task id.
    fn set_task(opt: &mut Option<Program>, new: Task) {
        match opt {
            Some(op) => {
                if op.task.id != new.id {
                    panic!("Cannot overwrite task")
                }
            }
            None => *opt = Some(Program::new(new)),
        };
    }

    /// Return the next incoming (app<-net) command we want the network protocol
    /// to run, or an error if the app<-net direction should block for now.
    fn next_net_cmd_in(&mut self) -> Result<NetOpIn, ()> {
        // TODO: refactor this and next_net_cmd_out.
        loop {
            if self.wants_tasks {
                self.load_tasks();
            }

            match self.current_prog_in.take() {
                Some(mut program) => {
                    while program.has_next_instruction() {
                        if let Err(e) = program.execute_next_instruction(self) {
                            self.next_netop_in = Some(NetOpIn::Error(e.into()));
                        };

                        if let Some(netop) = self.next_netop_in.take() {
                            self.current_prog_in = Some(program);
                            return Ok(netop);
                        }
                    }
                    self.wants_tasks = true;
                }
                None => return Err(()),
            }
        }
    }

    /// Return the next outgoing (app->net) command we want the network protocol
    /// to run, or an error if the app->net direction should block for now.
    fn next_net_cmd_out(&mut self) -> Result<NetOpOut, ()> {
        // TODO: refactor this and next_net_cmd_in.
        loop {
            if self.wants_tasks {
                self.load_tasks();
            }

            match self.current_prog_out.take() {
                Some(mut program) => {
                    while program.has_next_instruction() {
                        if let Err(e) = program.execute_next_instruction(self) {
                            self.next_netop_out = Some(NetOpOut::Error(e.into()));
                        };

                        if let Some(netop) = self.next_netop_out.take() {
                            self.current_prog_out = Some(program);
                            return Ok(netop);
                        }
                    }
                    self.wants_tasks = true;
                }
                None => return Err(()),
            }
        }
    }

    /// Store the given bytes on the heap at the given address.
    fn store_in(&mut self, addr: Identifier, bytes: Bytes) {
        if let Some(t) = self.current_prog_in.as_mut() {
            t.store_bytes(addr, bytes);
        }
    }

    fn store_out(&mut self, addr: Identifier, bytes: Bytes) {
        if let Some(t) = self.current_prog_out.as_mut() {
            t.store_bytes(addr, bytes);
        }
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
            inner: Arc::new(Mutex::new(Interpreter::new(Box::new(spec)))),
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

    pub async fn store_out(&mut self, addr: Identifier, bytes: Bytes) {
        // Yield to the async runtime if we can't get the lock, or if the
        // interpreter is not wanting to execute a command yet.
        std::future::poll_fn(move |_| match self.inner.try_lock() {
            Ok(mut inner) => Poll::Ready(inner.store_out(addr.clone(), bytes.clone())),
            Err(_) => Poll::Pending,
        })
        .await
    }

    pub async fn store_in(&mut self, addr: Identifier, bytes: Bytes) {
        // Yield to the async runtime if we can't get the lock, or if the
        // interpreter is not wanting to execute a command yet.
        std::future::poll_fn(move |_| match self.inner.try_lock() {
            Ok(mut inner) => Poll::Ready(inner.store_in(addr.clone(), bytes.clone())),
            Err(_) => Poll::Pending,
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use bytes::{Buf, BufMut, BytesMut};
    use std::fs;

    use super::*;
    use crate::lang::common::Role;
    use crate::lang::task::*;
    use crate::lang::types::*;

    struct LengthPayloadSpec {
        abs_format_out: AbstractFormat,
        abs_format_in1: AbstractFormat,
        abs_format_in2: AbstractFormat,
    }

    impl LengthPayloadSpec {
        fn new() -> LengthPayloadSpec {
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
                abs_format_out,
                abs_format_in1,
                abs_format_in2,
            }
        }
    }

    impl TaskProvider for LengthPayloadSpec {
        fn get_next_tasks(&self, _last_task: &TaskID) -> TaskSet {
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
                        aformat: self.abs_format_out.clone(),
                    }
                    .into(),
                    CreateMessageArgs {
                        name: "message".id(),
                        fmt_name: "cformat".id(),
                        field_names: vec!["payload".id()],
                    }
                    .into(),
                    ComputeLengthArgs {
                        name: "length_value_on_heap".id(),
                        msg_name: "message".id(),
                        field_name: "length".id(),
                    }
                    .into(),
                    SetNumericValueArgs {
                        msg_name: "message".id(),
                        field_name: "length".id(),
                        name: "length_value_on_heap".id(),
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
                        aformat: self.abs_format_in1.clone(),
                    }
                    .into(),
                    CreateMessageArgs {
                        name: "message_length_part".id(),
                        fmt_name: "cformat1".id(),
                        field_names: vec!["length".id()],
                    }
                    .into(),
                    GetNumericValueArgs {
                        name: "num_payload_bytes".id(),
                        msg_name: "message_length_part".id(),
                        field_name: "length".id(),
                    }
                    .into(),
                    ReadNetArgs {
                        name: "payload".id(),
                        len: ReadNetLength::Identifier("num_payload_bytes".id()),
                    }
                    .into(),
                    ConcretizeFormatArgs {
                        name: "cformat2".id(),
                        aformat: self.abs_format_in2.clone(),
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
                        field_name: "payload".id(),
                    }
                    .into(),
                ],
                id: TaskID::default(),
            };

            // Concurrently execute tasks for both data forwarding directions.
            TaskSet::InAndOutTasks(TaskPair { out_task, in_task })
        }
    }

    fn parse_simple_proteus_spec() -> ProteusSpec {
        let filepath = "src/lang/parse/examples/simple.psf";
        let input = fs::read_to_string(filepath).expect("cannot read simple file");

        ProteusSpec::new(&input, Role::Client)
    }

    fn get_task_providers() -> Vec<Box<dyn TaskProvider + Send + 'static>> {
        vec![
            Box::new(LengthPayloadSpec {}),
            Box::new(parse_simple_proteus_spec()),
        ]
    }

    #[test]
    fn load_tasks() {
        for tp in get_task_providers() {
            let mut int = Interpreter::new(tp);
            int.load_tasks();
            assert!(int.current_prog_in.is_some());
            assert!(int.current_prog_out.is_some());
        }
    }

    fn read_app(int: &mut Interpreter) -> Bytes {
        let args = match int.next_net_cmd_out().unwrap() {
            NetOpOut::RecvApp(args) => args,
            _ => panic!("Unexpected interpreter command"),
        };

        let payload = Bytes::from("When should I attack?");
        assert!(args.len.contains(&payload.len()));

        int.store_out(args.addr, payload.clone());
        payload
    }

    fn write_net(int: &mut Interpreter, payload: Bytes) {
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

    fn read_net(int: &mut Interpreter) -> Bytes {
        let args = match int.next_net_cmd_in().unwrap() {
            NetOpIn::RecvNet(args) => args,
            _ => panic!("Unexpected interpreter command"),
        };

        assert!(args.len.contains(&2));
        let payload = Bytes::from("Attack at dawn!");
        let mut buf = BytesMut::new();
        buf.put_u16(payload.len() as u16);
        int.store_in(args.addr, buf.freeze());

        let args = match int.next_net_cmd_in().unwrap() {
            NetOpIn::RecvNet(args) => args,
            _ => panic!("Unexpected interpreter command"),
        };

        assert!(args.len.contains(&payload.len()));
        int.store_in(args.addr, payload.clone());
        payload
    }

    fn write_app(int: &mut Interpreter, payload: Bytes) {
        let args = match int.next_net_cmd_in().unwrap() {
            NetOpIn::SendApp(args) => args,
            _ => panic!("Unexpected interpreter command"),
        };

        assert_eq!(args.bytes.len(), payload.len());
        assert_eq!(args.bytes[..], payload[..]);
    }

    fn read_app_write_net_pipeline(int: &mut Interpreter) {
        let payload = read_app(int);
        write_net(int, payload);
    }

    #[test]
    fn read_app_write_net_once() {
        for tp in get_task_providers() {
            let mut int = Interpreter::new(tp);
            read_app_write_net_pipeline(&mut int);
        }
    }

    #[test]
    fn read_app_write_net_many() {
        for tp in get_task_providers() {
            let mut int = Interpreter::new(tp);
            for _ in 0..10 {
                read_app_write_net_pipeline(&mut int);
            }
        }
    }

    fn read_net_write_app_pipeline(int: &mut Interpreter) {
        let payload = read_net(int);
        write_app(int, payload);
    }

    #[test]
    fn read_net_write_app_once() {
        for tp in get_task_providers() {
            let mut int = Interpreter::new(tp);
            read_net_write_app_pipeline(&mut int);
        }
    }

    #[test]
    fn read_net_write_app_many() {
        for tp in get_task_providers() {
            let mut int = Interpreter::new(tp);
            for _ in 0..10 {
                read_net_write_app_pipeline(&mut int);
            }
        }
    }

    #[test]
    fn interleaved_app_net_app_net() {
        for tp in get_task_providers() {
            let mut int = Interpreter::new(tp);
            for _ in 0..10 {
                let app_payload = read_app(&mut int);
                let net_payload = read_net(&mut int);
                write_app(&mut int, net_payload);
                write_net(&mut int, app_payload);
            }
        }
    }

    #[test]
    fn interleaved_net_app_net_app() {
        for tp in get_task_providers() {
            let mut int = Interpreter::new(tp);
            for _ in 0..10 {
                let net_payload = read_net(&mut int);
                let app_payload = read_app(&mut int);
                write_net(&mut int, app_payload);
                write_app(&mut int, net_payload);
            }
        }
    }

    #[test]
    fn interleaved_app_net_net_app() {
        for tp in get_task_providers() {
            let mut int = Interpreter::new(tp);
            for _ in 0..10 {
                let app_payload = read_app(&mut int);
                let net_payload = read_net(&mut int);
                write_net(&mut int, app_payload);
                write_app(&mut int, net_payload);
            }
        }
    }

    #[test]
    fn interleaved_net_app_app_net() {
        for tp in get_task_providers() {
            let mut int = Interpreter::new(tp);
            for _ in 0..10 {
                let net_payload = read_net(&mut int);
                let app_payload = read_app(&mut int);
                write_app(&mut int, net_payload);
                write_net(&mut int, app_payload);
            }
        }
    }
}
