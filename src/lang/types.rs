#![allow(dead_code)]

use std::convert::From;
use std::str::FromStr;

/*
 * No error is possible. No way to construct an empty enum.
 */
#[derive(Debug)]
pub enum NeverError {}

#[derive(Debug)]
pub struct ParseError;

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

impl FromStr for NumericType {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, ParseError> {
        match s {
            "u8" => Ok(NumericType::U8),
            "u16" => Ok(NumericType::U16),
            "u32" => Ok(NumericType::U32),
            "u64" => Ok(NumericType::U64),
            "i8" => Ok(NumericType::I8),
            "i16" => Ok(NumericType::I16),
            "i32" => Ok(NumericType::I32),
            "i64" => Ok(NumericType::I64),
            _ => Err(ParseError {}),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum PrimitiveType {
    Numeric(NumericType),
    Bool,
    Char,
}

impl FromStr for PrimitiveType {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, ParseError> {
        match s {
            "bool" => Ok(PrimitiveType::Bool),
            "char" => Ok(PrimitiveType::Char),
            _ => match s.parse::<NumericType>() {
                Ok(n) => Ok(n.into()),
                Err(e) => Err(e),
            },
        }
    }
}

impl From<NumericType> for PrimitiveType {
    fn from(item: NumericType) -> PrimitiveType {
        PrimitiveType::Numeric(item)
    }
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

impl FromStr for Identifier {
    type Err = NeverError;

    fn from_str(s: &str) -> Result<Self, NeverError> {
        Ok(Identifier(s.to_string()))
    }
}

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
    Dynamic(DynamicArray),
}

impl From<PrimitiveArray> for Array {
    fn from(item: PrimitiveArray) -> Array {
        Array::Primitive(item)
    }
}

impl From<DynamicArray> for Array {
    fn from(item: DynamicArray) -> Array {
        Array::Dynamic(item)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub name: Identifier,
    pub dtype: Array,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Format {
    pub name: Identifier,
    pub fields: Vec<Field>,
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
