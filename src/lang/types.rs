#![allow(dead_code)]

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum NumericType {
    U8,
    U16,
    U32,
    U64,
    I8,
    I16,
    I32,
    I64,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum PrimitiveType {
    Numeric(NumericType),
    Bool,
    Char,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum CompoundType {
    // Non-primitive types needed for the interpreter
    Message,
    MessageSpec,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum DataType {
    Primitive(PrimitiveType),
    Compound(CompoundType),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Identifier(pub String);

#[derive(Clone, Debug, PartialEq)]
pub enum UnaryOp {
    SizeOf(Identifier),
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PrimitiveArray(pub PrimitiveType, pub usize);

#[derive(Clone, Debug, PartialEq)]
pub struct DynamicArray(pub PrimitiveType, pub UnaryOp);

#[derive(Clone, Debug, PartialEq)]
pub enum Array {
    Primitive(PrimitiveArray),
    Dynamic(DynamicArray)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub name: Identifier,
    pub dtype: Array,
}

pub trait StaticallySized {
    fn size_of(&self) -> usize;
}

impl StaticallySized for NumericType {
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

impl StaticallySized for PrimitiveType {
    fn size_of(&self) -> usize {
        match self {
            PrimitiveType::Numeric(t) => t.size_of(),
            PrimitiveType::Bool => std::mem::size_of::<bool>(),
            PrimitiveType::Char => std::mem::size_of::<char>(),
        }
    }
}

impl StaticallySized for PrimitiveArray {
    fn size_of(&self) -> usize {
        self.0.size_of() * self.1
    }
}
