use std::{
    ops::Range,
    sync::{Arc, Mutex},
    task::Poll,
};

use bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::lang::{
    mem::{Data, Heap, HeapAddr},
    spec::proteus::ProteusSpec,
    types::{DataType, NumericType, PrimitiveType},
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
}

pub enum NetOpIn {
    RecvNet(RecvArgs),
    SendApp(SendArgs),
    Close,
}

enum NetState {
    Read(Option<HeapAddr>),
    Write(HeapAddr),
}

struct Interpreter {
    spec: ProteusSpec,
    heap: Heap,
    next_net_state_out: NetState,
    next_net_state_in: NetState,
}

impl Interpreter {
    fn new(spec: ProteusSpec) -> Self {
        Self {
            spec,
            heap: Heap::new(),
            next_net_state_in: NetState::Read(None),
            next_net_state_out: NetState::Read(None),
        }
    }

    /// Return the next outgoing (app->net) command we want the network protocol
    /// to run, or an error if the app->net direction should block for now.
    fn next_net_cmd_out(&mut self) -> Result<NetOpOut, ()> {
        // TODO this should look through the spec to figure out what to do.
        let cmd = match &self.next_net_state_out {
            NetState::Read(_) => {
                // Read from the app and store the bytes on the heap.
                let addr = self.heap.alloc();
                self.next_net_state_out = NetState::Write(addr.clone());
                NetOpOut::RecvApp(RecvArgs {
                    len: 1..65536, // TODO: set based on size of length field
                    addr,
                })
            }
            NetState::Write(addr) => {
                // Read the app payload stored on the heap.
                let data = self.heap.free(&addr).unwrap();
                let len = data.data.len();
                assert!(len > 0 && len <= 65536);

                // Construct the outgoing proteus message.
                // TODO: for now just use length+payload fields.
                let mut msg_buf = BytesMut::new();
                msg_buf.put_u16(len as u16);
                msg_buf.put_slice(&data.data);

                // Next we'll want to read more payload from the app again.
                self.next_net_state_out = NetState::Read(None);

                // Now hand the message bytes back to the network for sending.
                NetOpOut::SendNet(SendArgs {
                    bytes: msg_buf.freeze(),
                })
            }
        };
        Ok(cmd)
    }

    /// Return the next incoming (app<-net) command we want the network protocol
    /// to run, or an error if the app<-net direction should block for now.
    fn next_net_cmd_in(&mut self) -> Result<NetOpIn, ()> {
        let cmd = match &self.next_net_state_in {
            NetState::Read(maybe_addr) => {
                match maybe_addr {
                    None => {
                        // Need to do a partial read to get the msg len.
                        let addr = self.heap.alloc();
                        self.next_net_state_in = NetState::Read(Some(addr.clone()));
                        NetOpIn::RecvNet(RecvArgs {
                            len: 2..3, // TODO: set based on spec
                            addr,
                        })
                    }
                    Some(addr) => {
                        // Already read the length, but not the payload.
                        let mut data = self.heap.free(&addr).unwrap();
                        let len = data.data.len();
                        assert!(len == 2);

                        let payload_len = data.data.get_u16();
                        assert!(payload_len > 0);

                        let addr = self.heap.alloc();
                        self.next_net_state_in = NetState::Write(addr.clone());

                        NetOpIn::RecvNet(RecvArgs {
                            len: 1..((payload_len as usize) + 1), // TODO: set based on spec
                            addr,
                        })
                    }
                }
                // Need to read an incoming proteus message, but we don't know the
                // total message size yet until we first read the length field.
            }
            NetState::Write(addr) => {
                // Read the message payload stored on the heap.
                let data = self.heap.free(&addr).unwrap();
                let len = data.data.len();
                assert!(len > 0 && len <= 65536);

                // Next we'll want to read another message from the net.
                self.next_net_state_in = NetState::Read(None);

                // Now hand the app bytes back for sending.
                NetOpIn::SendApp(SendArgs { bytes: data.data })
            }
        };
        Ok(cmd)
    }

    /// Store the given bytes on the heap at the given address.
    fn store(&mut self, addr: HeapAddr, bytes: Bytes) {
        // Convert to Data that can be pushed onto the heap.
        let data = Data {
            kind: DataType::Primitive(PrimitiveType::Numeric(NumericType::U8)),
            data: bytes,
        };
        self.heap.write(addr, data);
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
    use crate::lang::spec::proteus::ProteusSpecBuilder;

    use super::*;

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
