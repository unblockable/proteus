#![allow(dead_code)]

use std::io::Cursor;

use crate::lang::types::*;
use bytes::{buf::Limit, BufMut, Bytes, BytesMut};

#[derive(Debug)]
pub struct Message {
    format: ConcreteFormat,
    data: BytesMut,
}

impl Message {
    /*
     * A message is constructed from a format with a concrete size.
     */
    pub fn new(format: ConcreteFormat) -> Option<Self> {
        format.maybe_size_of().map(|size| Message {
            format,
            data: BytesMut::zeroed(size),
        })
    }

    pub fn try_get_field(&mut self, field_name: &Identifier) -> Option<&mut [u8]> {
        self.format
            .format
            .try_get_field_type_offset_and_size(field_name)
            .map(|(_, offset, size)| &mut self.data.as_mut()[offset..offset + size])
    }

    pub fn try_get_field_writer(&self, field_name: &Identifier) -> Cursor<Limit<&BytesMut>> {
        // let mut write_cursor = Cursor::new(&self.data.limit(size));
        // write_cursor.set_position(offset);
        todo!()
    }

    pub fn try_get_field_typed<T: ArrayCoorespondence>(
        &mut self,
        field_name: &Identifier,
    ) -> Option<Vec<T>> {
        if let Some((f_dtype, offset, size)) = self
            .format
            .format
            .try_get_field_type_offset_and_size(field_name)
        {
            match T::corresponded_array_type() {
                Array::Primitive(t_dtype) => {
                    if f_dtype == Array::Primitive(t_dtype) {
                        let mp = self.data.as_mut()[offset..offset + size].as_mut_ptr();
                        unsafe { Some(Vec::<T>::from_raw_parts(mp as *mut T, 1, 1)) }
                    } else {
                        None
                    }
                }
                _ => panic!(),
            }
        } else {
            None
        }
    }

    pub fn into_inner(self) -> Bytes {
        self.data.freeze()
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
        /*
        let foo_field = message
            .try_get_field_typed::<u8>(&"Foo".parse().unwrap())
            .unwrap();
        *foo_field = 1;
        */

        // Doesn't work yet.
        let bar_field = message
            .try_get_field_typed::<u32>(&"Bar".parse().unwrap())
            .unwrap();
        println!("{:?}", bar_field);
        // (*bar_field)[0] = 0xBEEF;

        let data = message.into_inner();
        println!("{:?}", data);
    }
}
