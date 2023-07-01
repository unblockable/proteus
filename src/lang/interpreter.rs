use std::{
    fmt,
    ops::Range,
    sync::{Arc, Mutex},
    task::Poll,
};

use bytes::{BufMut, Bytes, BytesMut};

use crate::crypto::{
    chacha::{Cipher, CipherKind},
    kdf,
};
use crate::lang::{
    common::Role,
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

#[derive(Debug)]
pub struct SendArgs {
    // Send these bytes.
    pub bytes: Bytes,
}

#[derive(Debug)]
pub struct RecvArgs {
    // Receive this many bytes.
    pub len: Range<usize>,
    // Store the bytes at this addr on the heap.
    pub addr: Identifier,
}

#[derive(Debug)]
pub enum NetOpOut {
    RecvApp(RecvArgs),
    SendNet(SendArgs),
    _Close,
    Error(String),
}

#[derive(Debug)]
pub enum NetOpIn {
    RecvNet(RecvArgs),
    SendApp(SendArgs),
    _Close,
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
            Instruction::ComputeLength(args) => {
                let msg = self
                    .message_heap
                    .get(&args.from_msg_heap_id)
                    .ok_or(Error::ExecuteFailed)?;
                let len = msg.len_suffix(&args.from_field_id);
                self.number_heap
                    .insert(args.to_heap_id.clone(), len as u128);
            }
            Instruction::ConcretizeFormat(args) => {
                let aformat = args.from_format.clone();

                // Get the fields that have dynamic lengths, and compute what the lengths
                // will be now that we should have the data for each field on the heap.
                let concrete_bytes: Vec<(Identifier, Option<&Bytes>)> = aformat
                    .get_dynamic_arrays()
                    .iter()
                    .map(|id| {
                        (
                            id.clone(),
                            // self.bytes_heap.get(&id).unwrap().len(),
                            self.bytes_heap.get(&id),
                        )
                    })
                    .collect();

                let mut concrete_sizes: Vec<(Identifier, usize)> = vec![];
                for (id, bytes_opt) in concrete_bytes {
                    concrete_sizes.push((id, bytes_opt.ok_or(Error::ExecuteFailed)?.len()))
                }

                // Now that we know the total size, we can allocate the full format block.
                let cformat = aformat.concretize(&concrete_sizes);

                // Store it for use by later instructions.
                self.format_heap.insert(args.to_heap_id.clone(), cformat);
            }
            Instruction::CreateMessage(args) => {
                // Create a message with an existing concrete format.
                let cformat = self
                    .format_heap
                    .remove(&args.from_format_heap_id)
                    .ok_or(Error::ExecuteFailed)?;
                let msg = Message::new(cformat).ok_or(Error::ExecuteFailed)?;

                // Store the message for use in later instructions.
                self.message_heap.insert(args.to_heap_id.clone(), msg);
            }
            Instruction::DecryptField(args) => {
                match interpreter.cipher.as_mut() {
                    Some(cipher) => {
                        // TODO way too much copying here :(
                        let msg = self
                            .message_heap
                            .get(&args.from_msg_heap_id)
                            .ok_or(Error::ExecuteFailed)?;
                        let ciphertext = msg
                            .get_field_bytes(&args.from_ciphertext_field_id)
                            .map_err(|_| Error::ExecuteFailed)?;
                        let mac = msg
                            .get_field_bytes(&args.from_mac_field_id)
                            .map_err(|_| Error::ExecuteFailed)?;

                        let mut mac_fixed = [0u8; 16];
                        mac_fixed.copy_from_slice(&mac);

                        let plaintext = cipher.decrypt(&ciphertext, &mac_fixed);

                        let mut buf = BytesMut::with_capacity(plaintext.len());
                        buf.put_slice(&plaintext);
                        self.bytes_heap
                            .insert(args.to_plaintext_heap_id.clone(), buf.freeze());
                    }
                    None => panic!("No cipher for decryption"),
                }
            }
            Instruction::EncryptField(args) => match interpreter.cipher.as_mut() {
                Some(cipher) => {
                    let msg = self
                        .message_heap
                        .get(&args.from_msg_heap_id)
                        .ok_or(Error::ExecuteFailed)?;
                    let plaintext = msg
                        .get_field_bytes(&args.from_field_id)
                        .map_err(|_| Error::ExecuteFailed)?;

                    let (ciphertext, mac) = cipher.encrypt(&plaintext);

                    let mut buf = BytesMut::with_capacity(ciphertext.len());
                    buf.put_slice(&ciphertext);
                    self.bytes_heap
                        .insert(args.to_ciphertext_heap_id.clone(), buf.freeze());

                    let mut buf = BytesMut::with_capacity(mac.len());
                    buf.put_slice(&mac);
                    self.bytes_heap
                        .insert(args.to_mac_heap_id.clone(), buf.freeze());
                }
                None => panic!("No cipher for encryption"),
            },
            Instruction::GenRandomBytes(_args) => {
                unimplemented!()
            }
            Instruction::GetArrayBytes(args) => {
                let msg = self
                    .message_heap
                    .get(&args.from_msg_heap_id)
                    .ok_or(Error::ExecuteFailed)?;
                let bytes = msg
                    .get_field_bytes(&args.from_field_id)
                    .map_err(|_| Error::ExecuteFailed)?;
                self.bytes_heap.insert(args.to_heap_id.clone(), bytes);
            }
            Instruction::GetNumericValue(args) => {
                let msg = self
                    .message_heap
                    .get(&args.from_msg_heap_id)
                    .ok_or(Error::ExecuteFailed)?;
                let num = msg
                    .get_field_unsigned_numeric(&args.from_field_id)
                    .map_err(|_| Error::ExecuteFailed)?;
                self.number_heap.insert(args.to_heap_id.clone(), num);
            }
            Instruction::InitFixedSharedKey(args) => {
                let salt = "stupid stupid stupid";
                let skey = kdf::derive_key_256(args.password.as_str(), salt);

                let kind = match args.role {
                    Role::Client => CipherKind::Sender,
                    Role::Server => CipherKind::Receiver,
                };
                interpreter.cipher = Some(Cipher::new(skey, kind));
            }
            Instruction::ReadApp(args) => {
                let netop = NetOpOut::RecvApp(RecvArgs {
                    len: args.from_len.clone(),
                    addr: args.to_heap_id.clone(),
                });
                interpreter.next_netop_out = Some(netop);
            }
            Instruction::ReadNet(args) => {
                let len = match &args.from_len {
                    ReadNetLength::Identifier(id) => {
                        let num = self.number_heap.get(&id).ok_or(Error::ExecuteFailed)?;
                        let val = *num as usize;
                        Range {
                            start: val,
                            end: val + 1,
                        }
                    }
                    ReadNetLength::IdentifierMinus((id, sub)) => {
                        let num = self.number_heap.get(&id).ok_or(Error::ExecuteFailed)?;
                        let val = (*num as usize) - sub;
                        Range {
                            start: val,
                            end: val + 1,
                        }
                    }
                    ReadNetLength::Range(r) => r.clone(),
                };

                let netop = NetOpIn::RecvNet(RecvArgs {
                    len,
                    addr: args.to_heap_id.clone(),
                });
                interpreter.next_netop_in = Some(netop);
            }
            Instruction::SetArrayBytes(args) => {
                let bytes = self
                    .bytes_heap
                    .get(&args.from_heap_id)
                    .ok_or(Error::ExecuteFailed)?;
                let mut msg = self
                    .message_heap
                    .remove(&args.to_msg_heap_id)
                    .ok_or(Error::ExecuteFailed)?;
                msg.set_field_bytes(&args.to_field_id, &bytes)
                    .map_err(|_| Error::ExecuteFailed)?;
                self.message_heap.insert(args.to_msg_heap_id.clone(), msg);
            }
            Instruction::SetNumericValue(args) => {
                let val = self
                    .number_heap
                    .get(&args.from_heap_id)
                    .ok_or(Error::ExecuteFailed)?
                    .clone();
                let mut msg = self
                    .message_heap
                    .remove(&args.to_msg_heap_id)
                    .ok_or(Error::ExecuteFailed)?;
                msg.set_field_unsigned_numeric(&args.to_field_id, val)
                    .map_err(|_| Error::ExecuteFailed)?;
                self.message_heap.insert(args.to_msg_heap_id.clone(), msg);
            }
            Instruction::WriteApp(args) => {
                let msg = self
                    .message_heap
                    .remove(&args.from_msg_heap_id)
                    .ok_or(Error::ExecuteFailed)?;
                let netop = NetOpIn::SendApp(SendArgs {
                    bytes: msg
                        .into_inner_field(&args.from_field_id)
                        .ok_or(Error::ExecuteFailed)?,
                });
                interpreter.next_netop_in = Some(netop);
            }
            Instruction::WriteNet(args) => {
                let msg = self
                    .message_heap
                    .remove(&args.from_msg_heap_id)
                    .ok_or(Error::ExecuteFailed)?;
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

pub struct Interpreter {
    spec: Box<dyn TaskProvider + Send + 'static>,
    cipher: Option<Cipher>,
    next_netop_out: Option<NetOpOut>,
    next_netop_in: Option<NetOpIn>,
    current_prog_out: Option<Program>,
    current_prog_in: Option<Program>,
    last_task_id: TaskID,
    wants_tasks: bool,
}

impl Interpreter {
    pub fn new(spec: Box<dyn TaskProvider + Send + 'static>) -> Self {
        Self {
            spec,
            cipher: None,
            next_netop_out: None,
            next_netop_in: None,
            current_prog_out: None,
            current_prog_in: None,
            last_task_id: TaskID::default(),
            wants_tasks: true,
        }
    }

    pub fn init(&mut self) -> Result<(), interpreter::Error> {
        let mut init_prog = Program::new(self.spec.get_init_task());
        while init_prog.has_next_instruction() {
            init_prog.execute_next_instruction(self)?;
        }
        self.last_task_id = init_prog.task.id;
        Ok(())
    }

    /// Loads task from the task provider. Panics if we already have a current
    /// task in/out, we receive another one from the provider, and the ID of the
    /// new task does not match that of the existing task.
    pub fn load_tasks(&mut self) {
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
    pub fn next_net_cmd_in(&mut self) -> Result<NetOpIn, ()> {
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
                    self.last_task_id = program.task.id;
                    self.wants_tasks = true;
                }
                None => return Err(()),
            }
        }
    }

    /// Return the next outgoing (app->net) command we want the network protocol
    /// to run, or an error if the app->net direction should block for now.
    pub fn next_net_cmd_out(&mut self) -> Result<NetOpOut, ()> {
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
                    self.last_task_id = program.task.id;
                    self.wants_tasks = true;
                }
                None => return Err(()),
            }
        }
    }

    /// Store the given bytes on the heap at the given address.
    pub fn store_in(&mut self, addr: Identifier, bytes: Bytes) {
        if let Some(t) = self.current_prog_in.as_mut() {
            t.store_bytes(addr, bytes);
        }
    }

    pub fn store_out(&mut self, addr: Identifier, bytes: Bytes) {
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

    pub async fn init(&mut self) -> Result<(), interpreter::Error> {
        // Yield to the async runtime if we can't get the lock, or if the
        // interpreter is not wanting to execute a command yet.
        std::future::poll_fn(move |_| {
            let mut inner = match self.inner.try_lock() {
                Ok(inner) => inner,
                Err(_) => return Poll::Pending,
            };
            Poll::Ready(inner.init())
        })
        .await
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
    use crate::lang::{
        parse::{proteus::ProteusParser, Parse},
        spec::test::basic::LengthPayloadSpec,
    };
    use bytes::{Buf, BufMut, BytesMut};

    use super::*;

    fn get_task_providers() -> Vec<Box<dyn TaskProvider + Send + 'static>> {
        vec![
            Box::new(LengthPayloadSpec::new(Role::Client)),
            Box::new(ProteusParser::parse(&"examples/psf/simple.psf", Role::Client).unwrap()),
        ]
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

        let mut msg = args.bytes.clone();
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

    #[test]
    fn load_tasks() {
        for tp in get_task_providers() {
            let mut int = Interpreter::new(tp);
            assert!(int.init().is_ok());
            int.load_tasks();
            assert!(int.current_prog_in.is_some() || int.current_prog_out.is_some());
        }
    }

    fn read_app_write_net_pipeline(int: &mut Interpreter) {
        let payload = read_app(int);
        write_net(int, payload);
    }

    #[test]
    fn read_app_write_net_once() {
        for tp in get_task_providers() {
            let mut int = Interpreter::new(tp);
            assert!(int.init().is_ok());
            read_app_write_net_pipeline(&mut int);
        }
    }

    #[test]
    fn read_app_write_net_many() {
        for tp in get_task_providers() {
            let mut int = Interpreter::new(tp);
            assert!(int.init().is_ok());
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
            assert!(int.init().is_ok());
            read_net_write_app_pipeline(&mut int);
        }
    }

    #[test]
    fn read_net_write_app_many() {
        for tp in get_task_providers() {
            let mut int = Interpreter::new(tp);
            assert!(int.init().is_ok());
            for _ in 0..10 {
                read_net_write_app_pipeline(&mut int);
            }
        }
    }

    #[test]
    fn interleaved_app_net_app_net() {
        for tp in get_task_providers() {
            let mut int = Interpreter::new(tp);
            assert!(int.init().is_ok());
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
            assert!(int.init().is_ok());
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
            assert!(int.init().is_ok());
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
            assert!(int.init().is_ok());
            for _ in 0..10 {
                let net_payload = read_net(&mut int);
                let app_payload = read_app(&mut int);
                write_app(&mut int, net_payload);
                write_net(&mut int, app_payload);
            }
        }
    }
}
