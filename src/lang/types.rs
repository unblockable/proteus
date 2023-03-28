#![allow(dead_code)]

#[derive(Copy, Clone, Debug)]
enum NumericType {
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
}

#[derive(Copy, Clone, Debug)]
enum DataType {
    NumericType(NumericType),
    Bool,
    Char,
}

#[derive(Copy, Clone, Debug)]
struct ArrayType(DataType, usize);

trait Sizeable {
    fn size_of(&self) -> usize;
}

impl Sizeable for NumericType {
    fn size_of(&self) -> usize {
        match self {
            NumericType::U8 => std::mem::size_of::<u8>(),
            NumericType::U16 => std::mem::size_of::<u16>(),
            NumericType::U32 => std::mem::size_of::<u32>(),
            NumericType::U64 => std::mem::size_of::<u64>(),
            NumericType::I8 => std::mem::size_of::<i8>(),
            NumericType::I16 => std::mem::size_of::<i16>(),
            NumericType::I32 => std::mem::size_of::<i32>(),
            NumericType::I64 => std::mem::size_of::<i64>(),
        }
    }
}

impl Sizeable for DataType {
    fn size_of(&self) -> usize {
        match self {
            DataType::NumericType(x) => x.size_of(),
            DataType::Bool => std::mem::size_of::<bool>(),
            DataType::Char => std::mem::size_of::<char>(),
        }
    }
}

impl Sizeable for ArrayType {
    fn size_of(&self) -> usize {
        self.0.size_of() * self.1
    }
}
