use std::ops::Range;

use anyhow::anyhow;
use bytes::{BufMut, Bytes, BytesMut};

use crate::crypto::chacha::CipherKind;
use crate::crypto::kdf;
use crate::lang::data::Data;
use crate::lang::interpreter::crypto::{CryptoStream, SharedCryptoState};
use crate::lang::interpreter::io::IoStream;
use crate::lang::interpreter::mem::Heap;
use crate::lang::ir::v1::*;
use crate::lang::ir::Instruction;
use crate::lang::message::Message;
use crate::lang::types::{Identifier, PubkeyEncoding};
use crate::lang::{Execute, Role, Runtime};
use crate::net::{Reader, Writer};

pub struct VirtualMachine<R: Reader, W: Writer> {
    heap: Heap,
    io: IoStream<R, W>,
    crypto: CryptoStream,
}

#[derive(Clone)]
pub struct SharedVmState {
    crypto_state: SharedCryptoState,
}

impl<R: Reader, W: Writer> VirtualMachine<R, W> {
    pub fn new(src: R, dst: W, state: Option<SharedVmState>) -> Self {
        Self {
            heap: Heap::new(),
            io: IoStream::new(src, dst),
            crypto: CryptoStream::new(state.map(|x| x.crypto_state)),
        }
    }

    pub fn share(&self) -> SharedVmState {
        SharedVmState {
            crypto_state: self.crypto.share(),
        }
    }

    pub fn clear_heap(&mut self) {
        self.heap.clear();
    }
}

impl<R: Reader, W: Writer> Runtime for VirtualMachine<R, W> {
    fn store<T: Into<Data>>(&mut self, addr: Identifier, data: T) -> anyhow::Result<()> {
        self.heap.insert(addr, data)
    }

    fn load<'a, T: TryFrom<&'a Data>>(&'a self, addr: &Identifier) -> anyhow::Result<T> {
        self.heap.get(addr)
    }

    fn drop<T: TryFrom<Data>>(&mut self, addr: &Identifier) -> anyhow::Result<T> {
        self.heap.remove(addr)
    }

    fn init_key(&mut self, key: &[u8]) -> anyhow::Result<()> {
        self.crypto.init_key(key)
    }

    fn create_cipher(&mut self, secret_key: [u8; 32], kind: CipherKind) {
        self.crypto.create_cipher(secret_key, kind);
    }

    fn encrypt(&mut self, plaintext: &[u8]) -> anyhow::Result<(Vec<u8>, [u8; 16])> {
        self.crypto.encrypt(plaintext)
    }

    fn encrypt_unauth(&mut self, plaintext: &[u8]) -> anyhow::Result<Vec<u8>> {
        self.crypto.encrypt_unauth(plaintext)
    }

    fn decrypt(&mut self, ciphertext: &[u8], mac: &[u8; 16]) -> anyhow::Result<Vec<u8>> {
        self.crypto.decrypt(ciphertext, mac)
    }

    fn decrypt_unauth(&mut self, ciphertext: &[u8]) -> anyhow::Result<Vec<u8>> {
        self.crypto.decrypt_unauth(ciphertext)
    }

    async fn recv(&mut self, len: Range<usize>) -> anyhow::Result<Bytes> {
        self.io.recv(len).await
    }

    async fn send(&mut self, bytes: Bytes) -> anyhow::Result<usize> {
        self.io.send(bytes).await
    }

    async fn flush(&mut self) -> anyhow::Result<()> {
        self.io.flush().await
    }
}

impl Execute for Instruction {
    async fn execute(&self, runtime: &mut impl Runtime) -> anyhow::Result<()> {
        match self {
            Instruction::V1(ins) => ins.execute(runtime).await,
        }
    }
}

impl Execute for InstructionV1 {
    async fn execute(&self, runtime: &mut impl Runtime) -> anyhow::Result<()> {
        match &self {
            InstructionV1::ComputeLength(ins) => ins.execute(runtime).await,
            InstructionV1::ConcretizeFormat(ins) => ins.execute(runtime).await,
            InstructionV1::CreateMessage(ins) => ins.execute(runtime).await,
            InstructionV1::DecryptField(ins) => ins.execute(runtime).await,
            InstructionV1::EncryptField(ins) => ins.execute(runtime).await,
            InstructionV1::GetArrayBytes(ins) => ins.execute(runtime).await,
            InstructionV1::GetNumericValue(ins) => ins.execute(runtime).await,
            InstructionV1::InitFixedSharedKey(ins) => ins.execute(runtime).await,
            InstructionV1::ReadApp(ins) => ins.execute(runtime).await,
            InstructionV1::ReadNet(ins) => ins.execute(runtime).await,
            InstructionV1::SetArrayBytes(ins) => ins.execute(runtime).await,
            InstructionV1::SetNumericValue(ins) => ins.execute(runtime).await,
            InstructionV1::WriteApp(ins) => ins.execute(runtime).await,
            InstructionV1::WriteNet(ins) => ins.execute(runtime).await,
            InstructionV1::WriteNetTwice(ins) => ins.execute(runtime).await,
            InstructionV1::SaveKey(ins) => ins.execute(runtime).await,
        }
    }
}

impl Execute for ComputeLengthArgs {
    async fn execute(&self, runtime: &mut impl Runtime) -> anyhow::Result<()> {
        let msg: &Message = runtime.load(&self.from_msg_heap_id)?;
        let len = msg.len_suffix(&self.from_field_id);
        runtime.store(self.to_heap_id.clone(), len as u128)?;
        Ok(())
    }
}

impl Execute for ConcretizeFormatArgs {
    async fn execute(&self, runtime: &mut impl Runtime) -> anyhow::Result<()> {
        let aformat = self.from_format.clone();

        // The following block is ryans hack to support padding.
        if let Some(padding_field_id) = &self.padding_field {
            let block_size = self.block_size_nbytes.unwrap();

            // Crummy hack. Infer the length of the payload field...
            let darrays = aformat.get_dynamic_arrays();
            assert!(darrays.len() == 2);

            let payload_id = darrays.iter().find(|&id| id != padding_field_id).unwrap();

            // We know the payload bytes are there...
            let payload_bytes: &Bytes = runtime.load(payload_id)?;

            let payload_nbytes = payload_bytes.len();

            let padding_nbytes = crate::lang::padding_nbytes(payload_nbytes, block_size);

            let mut padding: Vec<u8> = vec![];
            padding.resize_with(padding_nbytes, || 255);
            let padding = bytes::Bytes::from(padding);

            runtime.store(padding_field_id.clone(), padding)?;

            use crate::lang::types::ToIdentifier;
            // FIXME(rwails) MEGA HACK
            runtime.store("__padding_len_on_heap".id(), padding_nbytes as u128)?;
        }

        // Get the fields that have dynamic lengths, and compute what the lengths
        // will be now that we should have the data for each field on the heap.
        let concrete_bytes: Vec<(Identifier, anyhow::Result<&Bytes>)> = aformat
            .get_dynamic_arrays()
            .iter()
            .map(|id| {
                (
                    id.clone(),
                    // self.bytes_heap.get(&id).unwrap().len(),
                    runtime.load(id),
                )
            })
            .collect();

        let mut concrete_sizes: Vec<(Identifier, usize)> = vec![];
        for (id, bytes_res) in concrete_bytes {
            concrete_sizes.push((id, bytes_res?.len()))
        }

        // Now that we know the total size, we can allocate the full format block.
        let cformat = aformat.concretize(&concrete_sizes);

        // Store it for use by later instructions.
        runtime.store(self.to_heap_id.clone(), cformat)?;

        Ok(())
    }
}

impl Execute for CreateMessageArgs {
    async fn execute(&self, runtime: &mut impl Runtime) -> anyhow::Result<()> {
        // Create a message with an existing concrete format.
        let cformat = runtime.drop(&self.from_format_heap_id)?;
        let msg = Message::new(cformat);

        // Store the message for use in later instructions.
        runtime.store(self.to_heap_id.clone(), msg)?;

        Ok(())
    }
}

impl Execute for DecryptFieldArgs {
    async fn execute(&self, runtime: &mut impl Runtime) -> anyhow::Result<()> {
        // TODO way too much copying here :(
        let msg: &Message = runtime.load(&self.from_msg_heap_id)?;
        let ciphertext = msg
            .get_field_bytes(&self.from_ciphertext_field_id)
            .map_err(|_| anyhow!("No ciphertext bytes"))?;

        // TODO Should auth and unauth encrypt be separate instructions?
        let plaintext = if self.from_mac_field_id.is_some() {
            // We are doing authenticated encryption.
            let mac = msg
                .get_field_bytes(self.from_mac_field_id.as_ref().unwrap())
                .map_err(|_| anyhow!("No mac bytes"))?;

            let mut mac_fixed = [0u8; 16];
            mac_fixed.copy_from_slice(&mac);

            runtime.decrypt(&ciphertext, &mac_fixed).unwrap()
        } else {
            // We are doing unauthenticated encryption.
            runtime.decrypt_unauth(&ciphertext).unwrap()
        };

        let mut buf = BytesMut::with_capacity(plaintext.len());
        buf.put_slice(&plaintext);
        runtime.store(self.to_plaintext_heap_id.clone(), buf.freeze())?;

        Ok(())
    }
}

impl Execute for EncryptFieldArgs {
    async fn execute(&self, runtime: &mut impl Runtime) -> anyhow::Result<()> {
        let msg: &Message = runtime.load(&self.from_msg_heap_id)?;
        let plaintext = msg
            .get_field_bytes(&self.from_field_id)
            .map_err(|_| anyhow!("No field bytes"))?;

        // TODO Should auth and unauth encrypt be separate instructions?
        if self.to_mac_heap_id.is_some() {
            // We are doing authenticated encryption.
            let (ciphertext, mac) = runtime.encrypt(&plaintext).unwrap();

            let mut buf = BytesMut::with_capacity(ciphertext.len());
            buf.put_slice(&ciphertext);
            runtime.store(self.to_ciphertext_heap_id.clone(), buf.freeze())?;

            let mut buf = BytesMut::with_capacity(mac.len());
            buf.put_slice(&mac);
            runtime.store(self.to_mac_heap_id.as_ref().unwrap().clone(), buf.freeze())?;
        } else {
            // We are doing unauthenticated encryption.
            let ciphertext = runtime.encrypt_unauth(&plaintext).unwrap();
            let mut buf = BytesMut::with_capacity(ciphertext.len());
            buf.put_slice(&ciphertext);
            runtime.store(self.to_ciphertext_heap_id.clone(), buf.freeze())?;
        }

        Ok(())
    }
}

impl Execute for GetArrayBytesArgs {
    async fn execute(&self, runtime: &mut impl Runtime) -> anyhow::Result<()> {
        let msg: &Message = runtime.load(&self.from_msg_heap_id)?;
        let bytes = msg
            .get_field_bytes(&self.from_field_id)
            .map_err(|_| anyhow!("No field bytes"))?;
        runtime.store(self.to_heap_id.clone(), bytes)?;

        Ok(())
    }
}

impl Execute for GetNumericValueArgs {
    async fn execute(&self, runtime: &mut impl Runtime) -> anyhow::Result<()> {
        let msg: &Message = runtime.load(&self.from_msg_heap_id)?;
        let num = msg
            .get_field_unsigned_numeric(&self.from_field_id)
            .map_err(|_| anyhow!("No field num"))?;
        runtime.store(self.to_heap_id.clone(), num)?;

        Ok(())
    }
}

impl Execute for InitFixedSharedKeyArgs {
    async fn execute(&self, runtime: &mut impl Runtime) -> anyhow::Result<()> {
        let salt = "stupid stupid stupid";
        let skey = kdf::derive_key_256(self.password.as_str(), salt);

        let kind = match self.role {
            Role::Client => CipherKind::Sender,
            Role::Server => CipherKind::Receiver,
        };

        runtime.create_cipher(skey, kind);

        Ok(())
    }
}

impl Execute for ReadAppArgs {
    async fn execute(&self, runtime: &mut impl Runtime) -> anyhow::Result<()> {
        let data = runtime
            .recv(self.from_len.clone())
            .await
            .map_err(|e| anyhow!("ReadApp error {e}"))?;
        runtime.store(self.to_heap_id.clone(), data)?;

        Ok(())
    }
}

impl Execute for ReadNetArgs {
    async fn execute(&self, runtime: &mut impl Runtime) -> anyhow::Result<()> {
        let len = match &self.from_len {
            ReadNetLength::Identifier(id) => {
                let num: &u128 = runtime.load(id)?;
                let val = *num as usize;
                Range {
                    start: val,
                    end: val + 1,
                }
            }
            ReadNetLength::IdentifierMinus((id, sub)) => {
                let num: &u128 = runtime.load(id)?;
                let val = (*num as usize) - sub;
                Range {
                    start: val,
                    end: val + 1,
                }
            }
            ReadNetLength::IdentifierMinusMinus((id, id_sub, sub)) => {
                let num: &u128 = runtime.load(id)?;
                let num2: &u128 = runtime.load(id_sub)?;
                let val = (*num as usize) - (*num2 as usize) - sub;
                Range {
                    start: val,
                    end: val + 1,
                }
            }
            ReadNetLength::Range(r) => r.clone(),
        };
        let data = runtime
            .recv(len)
            .await
            .map_err(|e| anyhow!("ReadNet error {e}"))?;
        runtime.store(self.to_heap_id.clone(), data)?;

        Ok(())
    }
}

impl Execute for SetArrayBytesArgs {
    async fn execute(&self, runtime: &mut impl Runtime) -> anyhow::Result<()> {
        let mut msg: Message = runtime.drop(&self.to_msg_heap_id)?;
        let bytes = runtime.load(&self.from_heap_id)?;
        msg.set_field_bytes(&self.to_field_id, bytes)
            .map_err(|_| anyhow!("No field bytes"))?;
        runtime.store(self.to_msg_heap_id.clone(), msg)?;

        Ok(())
    }
}

impl Execute for SetNumericValueArgs {
    async fn execute(&self, runtime: &mut impl Runtime) -> anyhow::Result<()> {
        let mut msg: Message = runtime.drop(&self.to_msg_heap_id)?;
        let val: &u128 = runtime.load(&self.from_heap_id)?;
        msg.set_field_unsigned_numeric(&self.to_field_id, *val)
            .map_err(|_| anyhow!("Cannot set field num"))?;
        runtime.store(self.to_msg_heap_id.clone(), msg)?;

        Ok(())
    }
}

impl Execute for WriteAppArgs {
    async fn execute(&self, runtime: &mut impl Runtime) -> anyhow::Result<()> {
        let msg: Message = runtime.drop(&self.from_msg_heap_id)?;
        let data = msg
            .into_inner_field(&self.from_field_id)
            .ok_or(anyhow!("No msg to bytes"))?;
        runtime
            .send(data)
            .await
            .map_err(|e| anyhow!("WriteApp error {e}"))?;

        Ok(())
    }
}

impl Execute for WriteNetArgs {
    async fn execute(&self, runtime: &mut impl Runtime) -> anyhow::Result<()> {
        let msg: Message = runtime.drop(&self.from_msg_heap_id)?;
        let data = msg.into_inner();
        runtime
            .send(data)
            .await
            .map_err(|e| anyhow!("WriteNet error {e}"))?;

        Ok(())
    }
}

impl Execute for WriteNetTwiceArgs {
    async fn execute(&self, runtime: &mut impl Runtime) -> anyhow::Result<()> {
        let msg: Message = runtime.drop(&self.from_msg_heap_id)?;

        let mut data: Bytes = msg.into_inner();
        let more = data.split_off(self.len_first_write);

        runtime
            .send(data)
            .await
            .map_err(|e| anyhow!("WriteNetTwice error on first write {e}"))?;

        if !more.is_empty() {
            runtime
                .flush()
                .await
                .map_err(|e| anyhow!("WriteNetTwice flush error {e}"))?;
            // If we decide we need a delay to ensure the second write
            // is sent in a separate packet, we can use the following.
            // std::thread::sleep(std::time::Duration::from_millis(1));
            runtime
                .send(more)
                .await
                .map_err(|e| anyhow!("WriteNetTwice error on second write {e}"))?;
        }

        Ok(())
    }
}

impl Execute for SaveKeyArgs {
    async fn execute(&self, runtime: &mut impl Runtime) -> anyhow::Result<()> {
        let msg: &Message = runtime.load(&self.from_msg_heap_id)?;

        let bytes = msg
            .get_field_bytes(&self.from_field_id)
            .map_err(|_| anyhow!("No field bytes"))?;

        let decoded_key = match self.pubkey_encoding {
            PubkeyEncoding::Raw => crate::crypto::pubkey::X25519PubKey::from_bytes(&bytes),
            PubkeyEncoding::Pem => crate::crypto::pubkey::X25519PubKey::from_pem(bytes.to_vec()),
            PubkeyEncoding::Der => crate::crypto::pubkey::X25519PubKey::from_der(bytes.to_vec()),
        };

        runtime.init_key(decoded_key.as_bytes())?;

        Ok(())
    }
}
