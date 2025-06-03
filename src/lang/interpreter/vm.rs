use std::ops::Range;

use anyhow::anyhow;
use bytes::{BufMut, Bytes, BytesMut};

use super::forwarder::Forwarder;
use crate::crypto::chacha::CipherKind;
use crate::crypto::kdf;
use crate::lang::interpreter::memory::Heap;
use crate::lang::ir::bridge::{Task, TaskID};
use crate::lang::ir::v1::{InstructionV1, ReadNetLength};
use crate::lang::message::Message;
use crate::lang::types::{Identifier, PubkeyEncoding};
use crate::lang::Role;
use crate::net::{Reader, Writer};

pub struct VirtualMachine {
    task: Task,
    next_ins_index: usize,
    heap: Heap,
}

impl VirtualMachine {
    pub fn new(task: Task) -> Self {
        Self {
            task,
            next_ins_index: 0,
            heap: Heap::new(),
        }
    }

    pub fn task_id(&self) -> TaskID {
        self.task.id
    }

    pub async fn execute<R: Reader, W: Writer>(
        &mut self,
        forwarder: &mut Forwarder<R, W>,
    ) -> anyhow::Result<()> {
        while self.next_ins_index < self.task.ins.len() {
            self.execute_next_instruction(forwarder).await?;
            self.next_ins_index += 1;
        }
        Ok(())
    }

    async fn execute_next_instruction<R: Reader, W: Writer>(
        &mut self,
        forwarder: &mut Forwarder<R, W>,
    ) -> anyhow::Result<()> {
        match &self.task.ins[self.next_ins_index] {
            InstructionV1::ComputeLength(args) => {
                let msg: &Message = self.heap.get(&args.from_msg_heap_id)?;
                let len = msg.len_suffix(&args.from_field_id);
                self.heap.insert(args.to_heap_id.clone(), len as u128)?;
            }
            InstructionV1::ConcretizeFormat(args) => {
                let aformat = args.from_format.clone();

                // The following block is ryans hack to support padding.
                if let Some(padding_field_id) = &args.padding_field {
                    let block_size = args.block_size_nbytes.unwrap();

                    // Crummy hack. Infer the length of the payload field...
                    let darrays = aformat.get_dynamic_arrays();
                    assert!(darrays.len() == 2);

                    let payload_id = darrays.iter().find(|&id| id != padding_field_id).unwrap();

                    // We know the payload bytes are there...
                    let payload_bytes: &Bytes = self.heap.get(payload_id)?;

                    let payload_nbytes = payload_bytes.len();

                    let padding_nbytes = crate::lang::padding_nbytes(payload_nbytes, block_size);

                    let mut padding: Vec<u8> = vec![];
                    padding.resize_with(padding_nbytes, || 255);
                    let padding = bytes::Bytes::from(padding);

                    self.heap.insert(padding_field_id.clone(), padding)?;

                    use crate::lang::types::ToIdentifier;
                    // FIXME(rwails) MEGA HACK
                    self.heap
                        .insert("__padding_len_on_heap".id(), padding_nbytes as u128)?;
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
                            self.heap.get(id),
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
                self.heap.insert(args.to_heap_id.clone(), cformat)?;
            }
            InstructionV1::CreateMessage(args) => {
                // Create a message with an existing concrete format.
                let cformat = self.heap.remove(&args.from_format_heap_id)?;
                let msg = Message::new(cformat);

                // Store the message for use in later instructions.
                self.heap.insert(args.to_heap_id.clone(), msg)?;
            }
            InstructionV1::DecryptField(args) => {
                // TODO way too much copying here :(
                let msg: &Message = self.heap.get(&args.from_msg_heap_id)?;
                let ciphertext = msg
                    .get_field_bytes(&args.from_ciphertext_field_id)
                    .map_err(|_| anyhow!("No ciphertext bytes"))?;

                // TODO Should auth and unauth encrypt be separate instructions?
                let plaintext = if args.from_mac_field_id.is_some() {
                    // We are doing authenticated encryption.
                    let mac = msg
                        .get_field_bytes(args.from_mac_field_id.as_ref().unwrap())
                        .map_err(|_| anyhow!("No mac bytes"))?;

                    let mut mac_fixed = [0u8; 16];
                    mac_fixed.copy_from_slice(&mac);

                    forwarder.decrypt(&ciphertext, &mac_fixed).unwrap()
                } else {
                    // We are doing unauthenticated encryption.
                    forwarder.decrypt_unauth(&ciphertext).unwrap()
                };

                let mut buf = BytesMut::with_capacity(plaintext.len());
                buf.put_slice(&plaintext);
                self.heap
                    .insert(args.to_plaintext_heap_id.clone(), buf.freeze())?;
            }
            InstructionV1::EncryptField(args) => {
                let msg: &Message = self.heap.get(&args.from_msg_heap_id)?;
                let plaintext = msg
                    .get_field_bytes(&args.from_field_id)
                    .map_err(|_| anyhow!("No field bytes"))?;

                // TODO Should auth and unauth encrypt be separate instructions?
                if args.to_mac_heap_id.is_some() {
                    // We are doing authenticated encryption.
                    let (ciphertext, mac) = forwarder.encrypt(&plaintext).unwrap();

                    let mut buf = BytesMut::with_capacity(ciphertext.len());
                    buf.put_slice(&ciphertext);
                    self.heap
                        .insert(args.to_ciphertext_heap_id.clone(), buf.freeze())?;

                    let mut buf = BytesMut::with_capacity(mac.len());
                    buf.put_slice(&mac);
                    self.heap
                        .insert(args.to_mac_heap_id.as_ref().unwrap().clone(), buf.freeze())?;
                } else {
                    // We are doing unauthenticated encryption.
                    let ciphertext = forwarder.encrypt_unauth(&plaintext).unwrap();
                    let mut buf = BytesMut::with_capacity(ciphertext.len());
                    buf.put_slice(&ciphertext);
                    self.heap
                        .insert(args.to_ciphertext_heap_id.clone(), buf.freeze())?;
                }
            }
            InstructionV1::GenRandomBytes(_args) => {
                unimplemented!()
            }
            InstructionV1::GetArrayBytes(args) => {
                let msg: &Message = self.heap.get(&args.from_msg_heap_id)?;
                let bytes = msg
                    .get_field_bytes(&args.from_field_id)
                    .map_err(|_| anyhow!("No field bytes"))?;
                self.heap.insert(args.to_heap_id.clone(), bytes)?;
            }
            InstructionV1::GetNumericValue(args) => {
                let msg: &Message = self.heap.get(&args.from_msg_heap_id)?;
                let num = msg
                    .get_field_unsigned_numeric(&args.from_field_id)
                    .map_err(|_| anyhow!("No field num"))?;
                self.heap.insert(args.to_heap_id.clone(), num)?;
            }
            InstructionV1::InitFixedSharedKey(args) => {
                let salt = "stupid stupid stupid";
                let skey = kdf::derive_key_256(args.password.as_str(), salt);

                let kind = match args.role {
                    Role::Client => CipherKind::Sender,
                    Role::Server => CipherKind::Receiver,
                };

                forwarder.create_cipher(skey, kind);
            }
            InstructionV1::ReadApp(args) => {
                let data = forwarder
                    .recv(args.from_len.clone())
                    .await
                    .map_err(|e| anyhow!("ReadApp error {e}"))?;
                self.heap.insert(args.to_heap_id.clone(), data)?;
            }
            InstructionV1::ReadNet(args) => {
                let len = match &args.from_len {
                    ReadNetLength::Identifier(id) => {
                        let num: &u128 = self.heap.get(id)?;
                        let val = *num as usize;
                        Range {
                            start: val,
                            end: val + 1,
                        }
                    }
                    ReadNetLength::IdentifierMinus((id, sub)) => {
                        let num: &u128 = self.heap.get(id)?;
                        let val = (*num as usize) - sub;
                        Range {
                            start: val,
                            end: val + 1,
                        }
                    }
                    ReadNetLength::IdentifierMinusMinus((id, id_sub, sub)) => {
                        let num: &u128 = self.heap.get(id)?;
                        let num2: &u128 = self.heap.get(id_sub)?;
                        let val = (*num as usize) - (*num2 as usize) - sub;
                        Range {
                            start: val,
                            end: val + 1,
                        }
                    }
                    ReadNetLength::Range(r) => r.clone(),
                };
                let data = forwarder
                    .recv(len)
                    .await
                    .map_err(|e| anyhow!("ReadNet error {e}"))?;
                self.heap.insert(args.to_heap_id.clone(), data)?;
            }
            InstructionV1::SetArrayBytes(args) => {
                let mut msg: Message = self.heap.remove(&args.to_msg_heap_id)?;
                let bytes = self.heap.get(&args.from_heap_id)?;
                msg.set_field_bytes(&args.to_field_id, bytes)
                    .map_err(|_| anyhow!("No field bytes"))?;
                self.heap.insert(args.to_msg_heap_id.clone(), msg)?;
            }
            InstructionV1::SetNumericValue(args) => {
                let mut msg: Message = self.heap.remove(&args.to_msg_heap_id)?;
                let val: &u128 = self.heap.get(&args.from_heap_id)?;
                msg.set_field_unsigned_numeric(&args.to_field_id, *val)
                    .map_err(|_| anyhow!("Cannot set field num"))?;
                self.heap.insert(args.to_msg_heap_id.clone(), msg)?;
            }
            InstructionV1::WriteApp(args) => {
                let msg: Message = self.heap.remove(&args.from_msg_heap_id)?;
                let data = msg
                    .into_inner_field(&args.from_field_id)
                    .ok_or(anyhow!("No msg to bytes"))?;
                forwarder
                    .send(data)
                    .await
                    .map_err(|e| anyhow!("WriteApp error {e}"))?;
            }
            InstructionV1::WriteNet(args) => {
                let msg: Message = self.heap.remove(&args.from_msg_heap_id)?;
                let data = msg.into_inner();
                forwarder
                    .send(data)
                    .await
                    .map_err(|e| anyhow!("WriteNet error {e}"))?;
            }
            InstructionV1::WriteNetTwice(args) => {
                let msg: Message = self.heap.remove(&args.from_msg_heap_id)?;

                let mut data: Bytes = msg.into_inner();
                let more = data.split_off(args.len_first_write);

                forwarder
                    .send(data)
                    .await
                    .map_err(|e| anyhow!("WriteNetTwice error on first write {e}"))?;

                if !more.is_empty() {
                    forwarder
                        .flush()
                        .await
                        .map_err(|e| anyhow!("WriteNetTwice flush error {e}"))?;
                    // If we decide we need a delay to ensure the second write
                    // is sent in a separate packet, we can use the following.
                    // std::thread::sleep(std::time::Duration::from_millis(1));
                    forwarder
                        .send(more)
                        .await
                        .map_err(|e| anyhow!("WriteNetTwice error on second write {e}"))?;
                }
            }
            InstructionV1::ReadKey(args) => {
                // TODO: dead code?
                let _msg: &Message = self.heap.get(&args.from_msg_heap_id)?;
            }
            InstructionV1::SaveKey(args) => {
                let msg: &Message = self.heap.get(&args.from_msg_heap_id)?;

                let bytes = msg
                    .get_field_bytes(&args.from_field_id)
                    .map_err(|_| anyhow!("No field bytes"))?;

                let decoded_key = match args.pubkey_encoding {
                    PubkeyEncoding::Raw => crate::crypto::pubkey::X25519PubKey::from_bytes(&bytes),
                    PubkeyEncoding::Pem => {
                        crate::crypto::pubkey::X25519PubKey::from_pem(bytes.to_vec())
                    }
                    PubkeyEncoding::Der => {
                        crate::crypto::pubkey::X25519PubKey::from_der(bytes.to_vec())
                    }
                };

                forwarder.init_key(decoded_key.as_bytes())?;
            }
        }

        Ok(())
    }
}
