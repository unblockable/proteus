#![allow(dead_code)]

use crate::lang::types::*;
use bytes::{Buf, BufMut, Bytes, BytesMut};

#[derive(Debug)]
pub enum SetFieldError {
    NotDefined,
    DowncastError,
    TypeError,
}

#[derive(Debug)]
pub enum GetFieldError {
    NotDefined,
    TypeError,
}

#[derive(Debug)]
pub struct Message {
    format: ConcreteFormat,
    data: BytesMut,
}

impl Message {
    /// A message is constructed from a format with a concrete size.
    pub fn new(format: ConcreteFormat) -> Option<Self> {
        format.maybe_size_of().map(|size| Message {
            format,
            data: BytesMut::zeroed(size),
        })
    }

    fn get_field_slice(&self, offset: usize, size: usize) -> &[u8] {
        assert!(offset < self.data.len() && offset + size <= self.data.len());
        &self.data.as_ref()[offset..offset + size]
    }

    fn get_field_slice_mut(&mut self, offset: usize, size: usize) -> &mut [u8] {
        assert!(offset < self.data.len() && offset + size <= self.data.len());
        &mut self.data.as_mut()[offset..offset + size]
    }

    fn try_get_field_slice(&self, field_name: &Identifier) -> Option<&[u8]> {
        self.format
            .format
            .try_get_field_type_offset_and_size(field_name)
            .map(|(_, offset, size)| self.get_field_slice(offset, size))
    }

    fn try_get_field_slice_mut(&mut self, field_name: &Identifier) -> Option<&mut [u8]> {
        self.format
            .format
            .try_get_field_type_offset_and_size(field_name)
            .map(|(_, offset, size)| self.get_field_slice_mut(offset, size))
    }

    pub fn set_field_unsigned_numeric(
        &mut self,
        field_name: &Identifier,
        value: u128,
    ) -> Result<(), SetFieldError> {
        if let Some((dtype, offset, size)) = self
            .format
            .format
            .try_get_field_type_offset_and_size(field_name)
        {
            let mut field_bytes = self.get_field_slice_mut(offset, size);

            match dtype {
                Array::Primitive(PrimitiveArray(x, n)) => {
                    if n > 1 {
                        return Err(SetFieldError::TypeError);
                    }
                    match x {
                        PrimitiveType::Char => Err(SetFieldError::TypeError),
                        PrimitiveType::Bool => Err(SetFieldError::TypeError),
                        PrimitiveType::Numeric(y) => match y {
                            NumericType::U8 => {
                                if let Ok(z) = u8::try_from(value) {
                                    field_bytes.put_u8(z);
                                    Ok(())
                                } else {
                                    Err(SetFieldError::DowncastError)
                                }
                            }
                            NumericType::U16 => {
                                if let Ok(z) = u16::try_from(value) {
                                    field_bytes.put_u16(z);
                                    Ok(())
                                } else {
                                    Err(SetFieldError::DowncastError)
                                }
                            }
                            NumericType::U32 => {
                                if let Ok(z) = u32::try_from(value) {
                                    field_bytes.put_u32(z);
                                    Ok(())
                                } else {
                                    Err(SetFieldError::DowncastError)
                                }
                            }
                            NumericType::U64 => {
                                if let Ok(z) = u64::try_from(value) {
                                    field_bytes.put_u64(z);
                                    Ok(())
                                } else {
                                    Err(SetFieldError::DowncastError)
                                }
                            }
                            _ => Err(SetFieldError::TypeError),
                        },
                    }
                }
                _ => panic!(),
            }
        } else {
            Err(SetFieldError::NotDefined)
        }
    }

    pub fn get_field_unsigned_numeric(
        &self,
        field_name: &Identifier,
    ) -> Result<u128, GetFieldError> {
        if let Some((dtype, offset, size)) = self
            .format
            .format
            .try_get_field_type_offset_and_size(field_name)
        {
            let mut field_bytes = self.get_field_slice(offset, size);

            match dtype {
                Array::Primitive(PrimitiveArray(x, n)) => {
                    if n > 1 {
                        return Err(GetFieldError::TypeError);
                    }
                    match x {
                        PrimitiveType::Char => Err(GetFieldError::TypeError),
                        PrimitiveType::Bool => Err(GetFieldError::TypeError),
                        PrimitiveType::Numeric(y) => match y {
                            NumericType::U8 => Ok(field_bytes.get_u8() as u128),
                            NumericType::U16 => Ok(field_bytes.get_u16() as u128),
                            NumericType::U32 => Ok(field_bytes.get_u32() as u128),
                            NumericType::U64 => Ok(field_bytes.get_u64() as u128),
                            _ => Err(GetFieldError::TypeError),
                        },
                    }
                }
                _ => panic!(),
            }
        } else {
            Err(GetFieldError::NotDefined)
        }
    }

    pub fn set_length_field(&mut self) {
        // FIXME(rwails) eventually we'll remove this function, I think
        let mut nbytes: usize = 0;

        for field in &self.format.format.fields {
            if field.name == "payload".id() {
                nbytes = field.maybe_size_of().unwrap();
            }
        }

        self.set_field_unsigned_numeric(&"length".id(), nbytes as u128)
            .expect("payload length too large for length field");
    }

    /// Computes the sum of the length of all fields after the given field.
    pub fn len_suffix(&self, field_name: &Identifier) -> usize {
        let mut nbytes: usize = 0;
        let mut do_sum = false;
        for field in &self.format.format.fields {
            if do_sum {
                nbytes += field.maybe_size_of().unwrap();
            } else if field.name.eq(field_name) {
                do_sum = true;
            }
        }
        nbytes
    }

    pub fn get_field_bytes(&self, field_name: &Identifier) -> Result<Bytes, GetFieldError> {
        match self.try_get_field_slice(field_name) {
            Some(slice) => {
                let mut buf = BytesMut::with_capacity(slice.len());
                buf.put_slice(slice);
                Ok(buf.freeze())
            }
            None => Err(GetFieldError::NotDefined),
        }
    }

    pub fn set_field_bytes(
        &mut self,
        field_name: &Identifier,
        bytes: &Bytes,
    ) -> Result<(), SetFieldError> {
        match self.try_get_field_slice_mut(field_name) {
            Some(slice) => {
                if slice.len() == bytes.len() {
                    slice.copy_from_slice(bytes);
                    Ok(())
                } else {
                    Err(SetFieldError::TypeError)
                }
            }
            None => Err(SetFieldError::NotDefined),
        }
    }

    pub fn into_inner(self) -> Bytes {
        self.data.freeze()
    }

    pub fn into_inner_field(mut self, field_name: &Identifier) -> Option<Bytes> {
        self.format
            .format
            .try_get_field_type_offset_and_size(field_name)
            .map(|(_, offset, size)| self.data.split_off(offset).split_to(size).freeze())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::types::tests::make_sized_format;

    #[test]
    fn test_message() {
        let format = make_sized_format();
        let mut message = Message::new(format).unwrap();

        message
            .set_field_unsigned_numeric(&"Foo".id(), 11)
            .expect("");

        assert_eq!(
            message.get_field_unsigned_numeric(&"Foo".id()).expect(""),
            11
        );

        println!("{:?}", message.into_inner());
    }

    #[test]
    fn test_message_set_length() {
        let format: ConcreteFormat = Format {
            name: "Handshake".parse().unwrap(),
            fields: vec![
                Field {
                    name: "length".parse().unwrap(),
                    dtype: PrimitiveArray(NumericType::U16.into(), 1).into(),
                },
                Field {
                    name: "payload".parse().unwrap(),
                    dtype: PrimitiveArray(NumericType::U8.into(), 40).into(),
                },
            ],
        }
        .try_into()
        .unwrap();

        let mut message = Message::new(format).unwrap();

        message.set_length_field();

        assert_eq!(
            40,
            message
                .get_field_unsigned_numeric(&"length".id())
                .expect("")
        );
    }
}
