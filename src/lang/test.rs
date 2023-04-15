use crate::lang::{
    common::Role,
    interpreter::{Interpreter, NetOpIn, NetOpOut},
    spec::test::basic::LengthPayloadSpec,
    task::TaskProvider,
};
use bytes::{Buf, BufMut, Bytes, BytesMut};
use rand::{
    distributions::{Alphanumeric, DistString},
    Rng,
};

use std::ops::Range;

use crate::lang::spec::proteus::parse_simple_proteus_spec;

struct Network {
    client_to_server: BytesMut,
    server_to_client: BytesMut,
}

impl Network {
    fn new() -> Self {
        Self {
            client_to_server: BytesMut::new(),
            server_to_client: BytesMut::new(),
        }
    }

    fn send(&mut self, role: &Role, bytes: Bytes) {
        match role {
            Role::Client => self.client_to_server.put(bytes),
            Role::Server => self.server_to_client.put(bytes),
        };
    }

    fn recv(&mut self, role: &Role, range: Range<usize>) -> Result<Bytes, ()> {
        let net_src = match role {
            Role::Client => &self.client_to_server,
            Role::Server => &self.server_to_client,
        };

        match net_src.remaining() >= range.start {
            true => {
                let mut src = net_src.clone().take(range.end - 1);

                let mut dst = BytesMut::new();
                dst.put(&mut src);

                match role {
                    Role::Client => self.client_to_server = src.into_inner(),
                    Role::Server => self.server_to_client = src.into_inner(),
                };
                Ok(dst.freeze())
            }
            false => Err(()),
        }
    }
}

struct Host {
    interpreter: Interpreter,
    role: Role,
    app_src_orig: Bytes,
    app_src: BytesMut,
    app_dst: BytesMut,
}

impl Host {
    fn new<T>(protospec: Box<T>, role: Role, msg: Bytes) -> Self
    where
        T: TaskProvider + Send + 'static,
    {
        Self {
            interpreter: Interpreter::new(protospec),
            role,
            app_src_orig: msg.clone(),
            app_src: BytesMut::new(),
            app_dst: BytesMut::new(),
        }
    }

    fn read_app(&mut self, range: Range<usize>) -> Result<Bytes, ()> {
        match self.app_src.remaining() >= range.start {
            true => {
                let mut src = self.app_src.clone().take(range.end - 1);

                let mut dst = BytesMut::new();
                dst.put(&mut src);

                self.app_src = src.into_inner();
                Ok(dst.freeze())
            }
            false => Err(()),
        }
    }

    fn write_app(&mut self, bytes: Bytes) {
        self.app_dst.put(bytes)
    }

    /// Returns `Ok()` if some progress was made, `Err()` if not.
    fn run_outgoing(&mut self, net: &mut Network) -> Result<(), ()> {
        match self.interpreter.next_net_cmd_out() {
            Ok(op) => {
                match op {
                    NetOpOut::RecvApp(args) => match self.read_app(args.len) {
                        Ok(bytes) => self.interpreter.store_out(args.addr, bytes),
                        Err(_) => return Err(()),
                    },
                    NetOpOut::SendNet(args) => net.send(&self.role, args.bytes),
                    NetOpOut::Close => todo!(),
                    NetOpOut::Error(e) => panic!("NetOpOut error {}", e),
                };
                Ok(())
            }
            Err(_) => Err(()),
        }
    }

    /// Returns `Ok()` if some progress was made, `Err()` if not.
    fn run_incoming(&mut self, net: &mut Network) -> Result<(), ()> {
        match self.interpreter.next_net_cmd_in() {
            Ok(op) => {
                match op {
                    NetOpIn::RecvNet(args) => match net.recv(&self.role, args.len) {
                        Ok(bytes) => self.interpreter.store_in(args.addr, bytes),
                        Err(_) => return Err(()),
                    },
                    NetOpIn::SendApp(args) => self.write_app(args.bytes),
                    NetOpIn::Close => todo!(),
                    NetOpIn::Error(e) => panic!("NetOpIn error {}", e),
                };
                Ok(())
            }
            Err(_) => Err(()),
        }
    }

    /// Returns `Ok()` if some progress was made, `Err()` if not.
    fn run_until_blocked(&mut self, net: &mut Network) -> Result<(), ()> {
        let mut progress = false;

        loop {
            let out_res = self.run_outgoing(net);
            let in_res = self.run_incoming(net);
            if out_res.is_ok() || in_res.is_ok() {
                progress = true;
            } else {
                break;
            }
        }

        progress.then_some(()).ok_or(())
    }

    fn into_inner(self) -> (Bytes, Bytes) {
        (self.app_src_orig, self.app_dst.freeze())
    }
}

struct ProtocolTester {
    client: Host,
    server: Host,
    net: Network,
}

impl ProtocolTester {
    fn new<T>(protospec: Box<T>) -> Self
    where
        T: TaskProvider + Clone + Send + 'static,
    {
        let client_msg = ProtocolTester::generate_payload(10..1000);
        let server_msg = ProtocolTester::generate_payload(10..1000);
        Self {
            client: Host::new(protospec.clone(), Role::Client, client_msg),
            server: Host::new(protospec, Role::Server, server_msg),
            net: Network::new(),
        }
    }

    fn generate_payload(len_range: Range<usize>) -> Bytes {
        let mut rng = rand::thread_rng();
        let len = rng.gen_range(len_range);
        let s = Alphanumeric.sample_string(&mut rng, len);
        Bytes::from(s)
    }

    fn test(mut self) {
        let (mut c_progress, mut s_progress) = (true, true);

        while c_progress || s_progress {
            match self.client.run_until_blocked(&mut self.net) {
                Ok(_) => c_progress = true,
                Err(_) => c_progress = false,
            };
            match self.server.run_until_blocked(&mut self.net) {
                Ok(_) => s_progress = true,
                Err(_) => s_progress = false,
            };
        }

        let (c_src, c_dst) = self.client.into_inner();
        let (s_src, s_dst) = self.server.into_inner();

        assert_eq!(c_src.len(), s_dst.len());
        assert_eq!(s_src.len(), c_dst.len());

        assert_eq!(c_src[..], s_dst[..]);
        assert_eq!(s_src[..], c_dst[..]);
    }
}

#[test]
fn integration_static_basic() {
    ProtocolTester::new(Box::new(LengthPayloadSpec::new())).test()
}

#[test]
fn integration_psf_basic() {
    let spec = parse_simple_proteus_spec();
    // ProtocolTester::new(Box::new(spec)).test()
}
