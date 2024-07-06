use std::ops::Range;

use crate::{
    crypto::{chacha::CipherKind, kdf},
    lang::{
        memory::Heap,
        message::Message,
        task::{Instruction, ReadNetLength, Task},
        types::{ConcreteFormat, Identifier},
        Role,
    },
};

use anyhow::anyhow;
use bytes::{BufMut, Bytes, BytesMut};

use super::forwarder::Forwarder;

pub struct Program {
    task: Task,
    next_ins_index: usize,
    bytes_heap: Heap<Bytes>,
    format_heap: Heap<ConcreteFormat>,
    message_heap: Heap<Message>,
    number_heap: Heap<u128>,
}

impl Program {
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

    pub async fn execute(&mut self, forwarder: &mut Forwarder) -> anyhow::Result<()> {
        while self.next_ins_index < self.task.ins.len() {
            self.execute_next_instruction(forwarder).await?;
            self.next_ins_index += 1;
        }
        Ok(())
    }

    async fn execute_next_instruction(&mut self, forwarder: &mut Forwarder) -> anyhow::Result<()> {
        match &self.task.ins[self.next_ins_index] {
            Instruction::ComputeLength(args) => {
                let msg = self
                    .message_heap
                    .get(&args.from_msg_heap_id)
                    .ok_or(anyhow!("No msg on heap"))?;
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
            Instruction::CreateMessage(args) => {
                // Create a message with an existing concrete format.
                let cformat = self
                    .format_heap
                    .remove(&args.from_format_heap_id)
                    .ok_or(anyhow!("No fmt on heap"))?;
                let msg = Message::new(cformat);

                // Store the message for use in later instructions.
                self.message_heap.insert(args.to_heap_id.clone(), msg);
            }
            Instruction::DecryptField(args) => {
                // TODO way too much copying here :(
                let msg = self
                    .message_heap
                    .get(&args.from_msg_heap_id)
                    .ok_or(anyhow!("No msg on heap"))?;
                let ciphertext = msg
                    .get_field_bytes(&args.from_ciphertext_field_id)
                    .map_err(|_| anyhow!("No ciphertext bytes"))?;
                let mac = msg
                    .get_field_bytes(&args.from_mac_field_id)
                    .map_err(|_| anyhow!("No mac bytes"))?;

                let mut mac_fixed = [0u8; 16];
                mac_fixed.copy_from_slice(&mac);

                let plaintext = forwarder.decrypt(&ciphertext, &mac_fixed).unwrap();

                let mut buf = BytesMut::with_capacity(plaintext.len());
                buf.put_slice(&plaintext);
                self.bytes_heap
                    .insert(args.to_plaintext_heap_id.clone(), buf.freeze());
            }
            Instruction::EncryptField(args) => {
                let msg = self
                    .message_heap
                    .get(&args.from_msg_heap_id)
                    .ok_or(anyhow!("No msg on heap"))?;
                let plaintext = msg
                    .get_field_bytes(&args.from_field_id)
                    .map_err(|_| anyhow!("No field bytes"))?;

                let (ciphertext, mac) = forwarder.encrypt(&plaintext).unwrap();

                let mut buf = BytesMut::with_capacity(ciphertext.len());
                buf.put_slice(&ciphertext);
                self.bytes_heap
                    .insert(args.to_ciphertext_heap_id.clone(), buf.freeze());

                let mut buf = BytesMut::with_capacity(mac.len());
                buf.put_slice(&mac);
                self.bytes_heap
                    .insert(args.to_mac_heap_id.clone(), buf.freeze());
            }
            Instruction::GenRandomBytes(_args) => {
                unimplemented!()
            }
            Instruction::GetArrayBytes(args) => {
                let msg = self
                    .message_heap
                    .get(&args.from_msg_heap_id)
                    .ok_or(anyhow!("No msg on heap"))?;
                let bytes = msg
                    .get_field_bytes(&args.from_field_id)
                    .map_err(|_| anyhow!("No field bytes"))?;
                self.bytes_heap.insert(args.to_heap_id.clone(), bytes);
            }
            Instruction::GetNumericValue(args) => {
                let msg = self
                    .message_heap
                    .get(&args.from_msg_heap_id)
                    .ok_or(anyhow!("No msg on heap"))?;
                let num = msg
                    .get_field_unsigned_numeric(&args.from_field_id)
                    .map_err(|_| anyhow!("No field num"))?;
                self.number_heap.insert(args.to_heap_id.clone(), num);
            }
            Instruction::InitFixedSharedKey(args) => {
                let salt = "stupid stupid stupid";
                let skey = kdf::derive_key_256(args.password.as_str(), salt);

                let kind = match args.role {
                    Role::Client => CipherKind::Sender,
                    Role::Server => CipherKind::Receiver,
                };

                forwarder.create_cipher(skey, kind);
            }
            Instruction::ReadApp(args) => {
                let data = forwarder
                    .recv(args.from_len.clone())
                    .await
                    .map_err(|e| anyhow!("Recv error {e}"))?;
                self.bytes_heap.insert(args.to_heap_id.clone(), data);
            }
            Instruction::ReadNet(args) => {
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
                    ReadNetLength::Range(r) => r.clone(),
                };
                let data = forwarder
                    .recv(len)
                    .await
                    .map_err(|e| anyhow!("Recv error {e}"))?;
                self.bytes_heap.insert(args.to_heap_id.clone(), data);
            }
            Instruction::SetArrayBytes(args) => {
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
            Instruction::SetNumericValue(args) => {
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
            Instruction::WriteApp(args) => {
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
                    .map_err(|e| anyhow!("Send error {e}"))?;
            }
            Instruction::WriteNet(args) => {
                let msg = self
                    .message_heap
                    .remove(&args.from_msg_heap_id)
                    .ok_or(anyhow!("No msg on heap"))?;
                let data = msg.into_inner();
                forwarder
                    .send(data)
                    .await
                    .map_err(|e| anyhow!("Send error {e}"))?;
            }
        }

        Ok(())
    }
}
