#![allow(dead_code)]

use crate::lang::types::*;
use bytes::BytesMut;

#[derive(Debug)]
pub struct Message {
    format: Format,
    data: BytesMut,
}

impl Message {
    fn try_get_field_type_offset_and_size(
        &self,
        field_name: &Identifier,
    ) -> Option<(Array, usize, usize)> {
        let mut acc: usize = 0;

        for field in &self.format.fields {
            let size = field.dtype.maybe_size_of().unwrap();

            if &field.name == field_name {
                return Some((field.dtype.clone(), acc, size));
            } else {
                acc += size;
            }
        }

        None
    }

    /*
     * A message is constructed from a format with a concrete size.
     */
    pub fn new(format: Format) -> Option<Self> {
        format.maybe_size_of().map(|size| Message {
            format,
            data: BytesMut::zeroed(size),
        })
    }

    pub fn try_get_field(&mut self, field_name: &Identifier) -> Option<&mut [u8]> {
        self.try_get_field_type_offset_and_size(field_name)
            .map(|(_, offset, size)| &mut self.data.as_mut()[offset..offset + size])
    }

    pub fn try_get_field_typed<T: ArrayCoorespondence>(
        &mut self,
        field_name: &Identifier,
    ) -> Option<&mut T> {
        if let Some((f_dtype, offset, size)) = self.try_get_field_type_offset_and_size(field_name) {
            match T::corresponded_array_type() {
                Array::Primitive(t_dtype) => {
                    if f_dtype == Array::Primitive(t_dtype) {
                        unsafe {
                            Some(std::mem::transmute::<*mut u8, &mut T>(
                                self.data.as_mut()[offset..offset + size].as_mut_ptr(),
                            ))
                        }
                    } else {
                        None
                    }
                }
                Array::PrimitiveSlice(p) => {
                    if TryInto::<PrimitiveArray>::try_into(f_dtype).unwrap().0 == p {
                        unsafe {
                            Some(std::mem::transmute::<*mut u8, &mut T>(
                                self.data.as_mut()[offset..offset + size].as_mut_ptr(),
                            ))
                        }
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

    pub fn into_inner(self) -> BytesMut {
        self.data
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
        let foo_field = message
            .try_get_field_typed::<u8>(&"Foo".parse().unwrap())
            .unwrap();
        *foo_field = 1;

        // Doesn't work yet.
        //let bar_field = message.try_get_field_typed::<[u32; 4]>(&"Bar".parse().unwrap());

        let data = message.into_inner();
        println!("{:?}", data);
    }
}
