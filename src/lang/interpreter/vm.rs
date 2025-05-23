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
use crate::lang::types::{ConcreteFormat, Identifier, PubkeyEncoding};
use crate::lang::Role;
use crate::net::{Reader, Writer};

pub struct VirtualMachine {
    task: Task,
    next_ins_index: usize,
    bytes_heap: Heap<Bytes>,
    format_heap: Heap<ConcreteFormat>,
    message_heap: Heap<Message>,
    number_heap: Heap<u128>,
}

impl VirtualMachine {
    pub fn new(task: Task) -> Self {
        Self {
            task,
            next_ins_index: 0,
            bytes_heap: Heap::new(),
            format_heap: Heap::new(),
            message_heap: Heap::new(),
            number_heap: Heap::new(),
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
                let msg = self
                    .message_heap
                    .get(&args.from_msg_heap_id)
                    .ok_or(anyhow!("No msg on heap"))?;
                let len = msg.len_suffix(&args.from_field_id);
                self.number_heap
                    .insert(args.to_heap_id.clone(), len as u128);
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
                    let payload_bytes = self.bytes_heap.get(payload_id).unwrap();

                    let payload_nbytes = payload_bytes.len();

                    let padding_nbytes = crate::lang::padding_nbytes(payload_nbytes, block_size);

                    let mut padding: Vec<u8> = vec![];
                    padding.resize_with(padding_nbytes, || 255);
                    let padding = bytes::Bytes::from(padding);

                    self.bytes_heap.insert(padding_field_id.clone(), padding);

                    use crate::lang::types::ToIdentifier;
                    // FIXME(rwails) MEGA HACK
                    self.number_heap
                        .insert("__padding_len_on_heap".id(), padding_nbytes as u128);
                }

                // Get the fields that have dynamic lengths, and compute what the lengths
                // will be now that we should have the data for each field on the heap.
                let concrete_bytes: Vec<(Identifier, Option<&Bytes>)> = aformat
                    .get_dynamic_arrays()
                    .iter()
                    .map(|id| {
                        (
                            id.clone(),
                            // self.bytes_heap.get(&id).unwrap().len(),
                            self.bytes_heap.get(id),
                        )
                    })
                    .collect();

                let mut concrete_sizes: Vec<(Identifier, usize)> = vec![];
                for (id, bytes_opt) in concrete_bytes {
                    concrete_sizes.push((id, bytes_opt.ok_or(anyhow!("No concrete bytes"))?.len()))
                }

                // Now that we know the total size, we can allocate the full format block.
                let cformat = aformat.concretize(&concrete_sizes);

                // Store it for use by later instructions.
                self.format_heap.insert(args.to_heap_id.clone(), cformat);
            }
            InstructionV1::CreateMessage(args) => {
                // Create a message with an existing concrete format.
                let cformat = self
                    .format_heap
                    .remove(&args.from_format_heap_id)
                    .ok_or(anyhow!("No fmt on heap"))?;
                let msg = Message::new(cformat);

                // Store the message for use in later instructions.
                self.message_heap.insert(args.to_heap_id.clone(), msg);
            }
            InstructionV1::DecryptField(args) => {
                // TODO way too much copying here :(
                let msg = self
                    .message_heap
                    .get(&args.from_msg_heap_id)
                    .ok_or(anyhow!("No msg on heap"))?;
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
                self.bytes_heap
                    .insert(args.to_plaintext_heap_id.clone(), buf.freeze());
            }
            InstructionV1::EncryptField(args) => {
                let msg = self
                    .message_heap
                    .get(&args.from_msg_heap_id)
                    .ok_or(anyhow!("No msg on heap"))?;
                let plaintext = msg
                    .get_field_bytes(&args.from_field_id)
                    .map_err(|_| anyhow!("No field bytes"))?;

                // TODO Should auth and unauth encrypt be separate instructions?
                if args.to_mac_heap_id.is_some() {
                    // We are doing authenticated encryption.
                    let (ciphertext, mac) = forwarder.encrypt(&plaintext).unwrap();

                    let mut buf = BytesMut::with_capacity(ciphertext.len());
                    buf.put_slice(&ciphertext);
                    self.bytes_heap
                        .insert(args.to_ciphertext_heap_id.clone(), buf.freeze());

                    let mut buf = BytesMut::with_capacity(mac.len());
                    buf.put_slice(&mac);
                    self.bytes_heap
                        .insert(args.to_mac_heap_id.as_ref().unwrap().clone(), buf.freeze());
                } else {
                    // We are doing unauthenticated encryption.
                    let ciphertext = forwarder.encrypt_unauth(&plaintext).unwrap();
                    let mut buf = BytesMut::with_capacity(ciphertext.len());
                    buf.put_slice(&ciphertext);
                    self.bytes_heap
                        .insert(args.to_ciphertext_heap_id.clone(), buf.freeze());
                }
            }
            InstructionV1::GenRandomBytes(_args) => {
                unimplemented!()
            }
            InstructionV1::GetArrayBytes(args) => {
                let msg = self
                    .message_heap
                    .get(&args.from_msg_heap_id)
                    .ok_or(anyhow!("No msg on heap"))?;
                let bytes = msg
                    .get_field_bytes(&args.from_field_id)
                    .map_err(|_| anyhow!("No field bytes"))?;
                self.bytes_heap.insert(args.to_heap_id.clone(), bytes);
            }
            InstructionV1::GetNumericValue(args) => {
                let msg = self
                    .message_heap
                    .get(&args.from_msg_heap_id)
                    .ok_or(anyhow!("No msg on heap"))?;
                let num = msg
                    .get_field_unsigned_numeric(&args.from_field_id)
                    .map_err(|_| anyhow!("No field num"))?;
                self.number_heap.insert(args.to_heap_id.clone(), num);
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
                self.bytes_heap.insert(args.to_heap_id.clone(), data);
            }
            InstructionV1::ReadNet(args) => {
                let len = match &args.from_len {
                    ReadNetLength::Identifier(id) => {
                        let num = self.number_heap.get(id).ok_or(anyhow!("No num on heap"))?;
                        let val = *num as usize;
                        Range {
                            start: val,
                            end: val + 1,
                        }
                    }
                    ReadNetLength::IdentifierMinus((id, sub)) => {
                        let num = self.number_heap.get(id).ok_or(anyhow!("No num on heap"))?;
                        let val = (*num as usize) - sub;
                        Range {
                            start: val,
                            end: val + 1,
                        }
                    }
                    ReadNetLength::IdentifierMinusMinus((id, id_sub, sub)) => {
                        let num = self.number_heap.get(id).ok_or(anyhow!("No num1 on heap"))?;
                        let num2 = self
                            .number_heap
                            .get(id_sub)
                            .ok_or(anyhow!("No num2 on heap"))?;
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
                self.bytes_heap.insert(args.to_heap_id.clone(), data);
            }
            InstructionV1::SetArrayBytes(args) => {
                let bytes = self
                    .bytes_heap
                    .get(&args.from_heap_id)
                    .ok_or(anyhow!("No bytes on heap"))?;
                let mut msg = self
                    .message_heap
                    .remove(&args.to_msg_heap_id)
                    .ok_or(anyhow!("No msg on heap"))?;
                msg.set_field_bytes(&args.to_field_id, bytes)
                    .map_err(|_| anyhow!("No field bytes"))?;
                self.message_heap.insert(args.to_msg_heap_id.clone(), msg);
            }
            InstructionV1::SetNumericValue(args) => {
                let val = *self
                    .number_heap
                    .get(&args.from_heap_id)
                    .ok_or(anyhow!("No num on heap"))?;
                let mut msg = self
                    .message_heap
                    .remove(&args.to_msg_heap_id)
                    .ok_or(anyhow!("No msg on heap"))?;
                msg.set_field_unsigned_numeric(&args.to_field_id, val)
                    .map_err(|_| anyhow!("Cannot set field num"))?;
                self.message_heap.insert(args.to_msg_heap_id.clone(), msg);
            }
            InstructionV1::WriteApp(args) => {
                let msg = self
                    .message_heap
                    .remove(&args.from_msg_heap_id)
                    .ok_or(anyhow!("No msg on heap"))?;
                let data = msg
                    .into_inner_field(&args.from_field_id)
                    .ok_or(anyhow!("No msg to bytes"))?;
                forwarder
                    .send(data)
                    .await
                    .map_err(|e| anyhow!("WriteApp error {e}"))?;
            }
            InstructionV1::WriteNet(args) => {
                let msg = self
                    .message_heap
                    .remove(&args.from_msg_heap_id)
                    .ok_or(anyhow!("No msg on heap"))?;
                let data = msg.into_inner();
                forwarder
                    .send(data)
                    .await
                    .map_err(|e| anyhow!("WriteNet error {e}"))?;
            }
            InstructionV1::WriteNetTwice(args) => {
                let msg = self
                    .message_heap
                    .remove(&args.from_msg_heap_id)
                    .ok_or(anyhow!("No msg on heap"))?;

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
                let _msg = self
                    .message_heap
                    .get(&args.from_msg_heap_id)
                    .ok_or(anyhow!("No msg on heap"))?;
            }
            InstructionV1::SaveKey(args) => {
                let msg = self
                    .message_heap
                    .get(&args.from_msg_heap_id)
                    .ok_or(anyhow!("No msg on heap"))?;

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
