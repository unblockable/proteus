#![allow(dead_code)]

use crate::lang::common::Role;
use std::collections::hash_map::HashMap;
use std::convert::{From, TryFrom};
use std::str::FromStr;

pub trait StaticallySized {
    fn size_of(&self) -> usize;
}

// Trait for types that may have a concrete size, but might not yet
// (e.g., a dynamic array does not have a size).
pub trait MaybeSized {
    fn maybe_size_of(&self) -> Option<usize>;
}

// All statically sized objects are maybe sized.
impl<T> MaybeSized for T
where
    T: StaticallySized,
{
    fn maybe_size_of(&self) -> Option<usize> {
        Some(self.size_of())
    }
}

pub trait ArrayCoorespondence {
    fn corresponded_array_type() -> Array;
}

impl ArrayCoorespondence for u8 {
    fn corresponded_array_type() -> Array {
        PrimitiveArray(NumericType::U8.into(), 1).into()
    }
}

impl ArrayCoorespondence for u16 {
    fn corresponded_array_type() -> Array {
        PrimitiveArray(NumericType::U16.into(), 1).into()
    }
}

impl ArrayCoorespondence for u32 {
    fn corresponded_array_type() -> Array {
        PrimitiveArray(NumericType::U32.into(), 1).into()
    }
}

impl ArrayCoorespondence for u64 {
    fn corresponded_array_type() -> Array {
        PrimitiveArray(NumericType::U64.into(), 1).into()
    }
}

impl ArrayCoorespondence for i8 {
    fn corresponded_array_type() -> Array {
        PrimitiveArray(NumericType::I8.into(), 1).into()
    }
}

impl ArrayCoorespondence for i16 {
    fn corresponded_array_type() -> Array {
        PrimitiveArray(NumericType::I16.into(), 1).into()
    }
}

impl ArrayCoorespondence for i32 {
    fn corresponded_array_type() -> Array {
        PrimitiveArray(NumericType::I32.into(), 1).into()
    }
}

impl ArrayCoorespondence for i64 {
    fn corresponded_array_type() -> Array {
        PrimitiveArray(NumericType::I64.into(), 1).into()
    }
}

impl ArrayCoorespondence for bool {
    fn corresponded_array_type() -> Array {
        PrimitiveArray(PrimitiveType::Bool.into(), 1).into()
    }
}

impl ArrayCoorespondence for char {
    fn corresponded_array_type() -> Array {
        PrimitiveArray(PrimitiveType::Char.into(), 1).into()
    }
}

/*
 * No error is possible. No way to construct an empty enum.
 */
#[derive(Debug)]
pub enum NeverError {}

#[derive(Debug)]
pub struct ParseError;

#[derive(Debug)]
pub struct DowncastError;

#[derive(Debug)]
pub struct ConversionError;

pub trait NumericallyBounded {
    pub fn min();
    pub fn max();
}

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

impl StaticallySized for PrimitiveType {
    fn size_of(&self) -> usize {
        match self {
            PrimitiveType::Numeric(t) => t.size_of(),
            PrimitiveType::Bool => std::mem::size_of::<bool>(),
            PrimitiveType::Char => std::mem::size_of::<char>(),
        }
    }
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

#[derive(Clone, Debug, Eq, PartialEq, Hash, Default)]
pub struct Identifier(pub String);

impl FromStr for Identifier {
    type Err = NeverError;

    fn from_str(s: &str) -> Result<Self, NeverError> {
        Ok(Identifier(s.to_string()))
    }
}

pub trait ToIdentifier {
    fn id(&self) -> Identifier;
}

impl ToIdentifier for &str {
    fn id(&self) -> Identifier {
        self.parse().unwrap()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum UnaryOp {
    SizeOf(Identifier),
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PrimitiveArray(pub PrimitiveType, pub usize);

impl StaticallySized for PrimitiveArray {
    fn size_of(&self) -> usize {
        self.0.size_of() * self.1
    }
}

/// A dynamic array is always an array of U8 (bytes).
#[derive(Clone, Debug, PartialEq)]
pub struct DynamicArray(pub UnaryOp);

impl DynamicArray {
    // Tries to ge tthe length field assoc. with this dynamic array.
    pub fn try_get_length_field(&self) -> Option<Identifier> {
        if let UnaryOp::SizeOf(id) = &self.0 {
            Some(id.clone())
        } else {
            None
        }
    }
}

// Dynamic arrays do not have a size defined.
impl MaybeSized for DynamicArray {
    fn maybe_size_of(&self) -> Option<usize> {
        None
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Array {
    Primitive(PrimitiveArray),
    Dynamic(DynamicArray),
}

impl TryFrom<Array> for PrimitiveArray {
    type Error = DowncastError;

    fn try_from(value: Array) -> Result<Self, Self::Error> {
        if let Array::Primitive(x) = value {
            Ok(x)
        } else {
            Err(DowncastError {})
        }
    }
}

impl TryFrom<Array> for DynamicArray {
    type Error = DowncastError;

    fn try_from(value: Array) -> Result<Self, Self::Error> {
        if let Array::Dynamic(x) = value {
            Ok(x)
        } else {
            Err(DowncastError {})
        }
    }
}

impl MaybeSized for Array {
    fn maybe_size_of(&self) -> Option<usize> {
        match *self {
            Array::Primitive(ref a) => a.maybe_size_of(),
            Array::Dynamic(ref a) => a.maybe_size_of(),
        }
    }
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

impl MaybeSized for Field {
    fn maybe_size_of(&self) -> Option<usize> {
        self.dtype.maybe_size_of()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Format {
    pub name: Identifier,
    pub fields: Vec<Field>,
}

impl Format {
    pub fn try_get_field_type_offset_and_size(
        &self,
        field_name: &Identifier,
    ) -> Option<(Array, usize, usize)> {
        let mut acc: usize = 0;

        for field in &self.fields {
            let size = field.dtype.maybe_size_of().unwrap();

            if &field.name == field_name {
                return Some((field.dtype.clone(), acc, size));
            } else {
                acc += size;
            }
        }

        None
    }

    pub fn try_get_field_by_name(&self, field_name: &Identifier) -> Option<Field> {
        self.fields.iter().find(|&x| x.name == field_name.clone()).cloned()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AbstractFormat {
    pub format: Format,
}

impl AbstractFormat {
    pub fn get_dynamic_arrays(&self) -> Vec<Identifier> {
        self.format
            .fields
            .iter()
            .filter_map(|f| f.maybe_size_of().is_none().then(|| f.name.clone()))
            .collect()
    }

    /// Sizes is a vector of (id, size) pairs where the size is given in bytes.
    /// This function will calculate the number of elements required to equal that number of bytes.
    pub fn concretize(mut self, sizes: &Vec<(Identifier, usize)>) -> ConcreteFormat {
        for (id, size) in sizes {
            for field in self.format.fields.iter_mut() {
                if id == &field.name {
                    if let Array::Dynamic(_) = &field.dtype {
                        field.dtype =
                            PrimitiveArray(PrimitiveType::Numeric(NumericType::U8), *size).into()
                    }
                }
            }
        }
        self.format.try_into().unwrap()
    }

    pub fn into_inner(self) -> Format {
        self.format
    }
}

impl ConcreteFormat {
    pub fn into_inner(self) -> Format {
        self.format
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConcreteFormat {
    pub format: Format,
}

impl MaybeSized for AbstractFormat {
    fn maybe_size_of(&self) -> Option<usize> {
        self.format.maybe_size_of()
    }
}

impl StaticallySized for ConcreteFormat {
    fn size_of(&self) -> usize {
        self.format.maybe_size_of().unwrap()
    }
}

impl From<Format> for AbstractFormat {
    fn from(item: Format) -> AbstractFormat {
        AbstractFormat { format: item }
    }
}

impl TryFrom<Format> for ConcreteFormat {
    type Error = ConversionError;

    fn try_from(value: Format) -> Result<Self, Self::Error> {
        match value.maybe_size_of() {
            Some(_) => Ok(ConcreteFormat { format: value }),
            None => Err(Self::Error {}),
        }
    }
}

impl MaybeSized for Format {
    fn maybe_size_of(&self) -> Option<usize> {
        self.fields.iter().fold(Some(0), |acc, field| {
            if let Some(x) = field.maybe_size_of() {
                acc.map(|y| x + y)
            } else {
                None
            }
        })
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum FieldSemantic {
    Payload,
    Padding,
    Length,
}

impl FromStr for FieldSemantic {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, ParseError> {
        match s {
            "PAYLOAD" => Ok(FieldSemantic::Payload),
            "PADDING" => Ok(FieldSemantic::Padding),
            "LENGTH" => Ok(FieldSemantic::Length),
            _ => Err(ParseError {}),
        }
    }
}

impl FromStr for Role {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, ParseError> {
        match s {
            "CLIENT" => Ok(Role::Client),
            "SERVER" => Ok(Role::Server),
            _ => Err(ParseError {}),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Phase {
    Handshake,
    Data,
}

impl FromStr for Phase {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, ParseError> {
        match s {
            "HANDSHAKE" => Ok(Phase::Handshake),
            "DATA" => Ok(Phase::Data),
            _ => Err(ParseError {}),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SemanticBinding {
    pub format: Identifier,
    pub field: Identifier,
    pub semantic: FieldSemantic,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Semantics {
    semantics: HashMap<Identifier, FieldSemantic>
}

impl Semantics {
    pub fn new(semantics: HashMap<Identifier, FieldSemantic>) -> Self {
        Semantics { semantics }
    }

    pub fn as_mut_ref(&mut self) -> &mut HashMap<Identifier, FieldSemantic> {
        &mut self.semantics
    }

    pub fn find_field_id(&self, semantic: FieldSemantic) -> Option<Identifier> {
        if let Some(e) = self.semantics.iter().find(|&e| *e.1 == semantic) {
            Some(e.0.clone())
        } else {
            None
        }
    }
}

impl From<HashMap<Identifier, FieldSemantic>> for Semantics {
    fn from(value: HashMap<Identifier, FieldSemantic>) -> Self {
        Self::new(value)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AbstractFormatAndSemantics {
    pub format: AbstractFormat,
    pub semantics: Semantics,
}

impl From<AbstractFormat> for AbstractFormatAndSemantics {
    fn from(item: AbstractFormat) -> AbstractFormatAndSemantics {
        AbstractFormatAndSemantics {
            format: item,
            semantics: Default::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SequenceSpecifier {
    pub role: Role,
    pub phase: Phase,
    pub format: Identifier,
}

#[derive(Debug)]
pub struct PSF {
    pub formats: HashMap<Identifier, AbstractFormatAndSemantics>,
    pub sequence: Vec<SequenceSpecifier>,
}

impl PSF {
    fn validate_seqs(&self) -> bool {
        for s in &self.sequence[..] {
            if !self.formats.contains_key(&s.format) {
                return false;
            }
        }

        true
    }

    /// Run checks to ensure that the PSF is semantically valid
    pub fn is_valid(&self) -> bool {
        self.validate_seqs()
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn make_sized_format() -> ConcreteFormat {
        Format {
            name: "Handshake".parse().unwrap(),
            fields: vec![
                Field {
                    name: "Foo".parse().unwrap(),
                    dtype: PrimitiveArray(NumericType::U8.into(), 1).into(),
                },
                Field {
                    name: "Bar".parse().unwrap(),
                    dtype: PrimitiveArray(NumericType::U32.into(), 10).into(),
                },
            ],
        }
        .try_into()
        .unwrap()
    }

    pub fn make_unsized_format() -> AbstractFormat {
        Format {
            name: "Handshake".parse().unwrap(),
            fields: vec![
                Field {
                    name: "Foo".parse().unwrap(),
                    dtype: PrimitiveArray(NumericType::U8.into(), 1).into(),
                },
                Field {
                    name: "Bar".parse().unwrap(),
                    dtype: DynamicArray(UnaryOp::SizeOf("Foo".parse().unwrap())).into(),
                },
            ],
        }
        .into()
    }

    #[test]
    fn test_sized_format() {
        let format = make_sized_format();
        assert_eq!(format.maybe_size_of().unwrap(), 41);
    }

    #[test]
    #[should_panic]
    fn test_unsized_format() {
        let format = make_unsized_format();
        format.maybe_size_of().unwrap();
    }

    #[test]
    fn test_dynamic_arrays() {
        let format = make_unsized_format();
        let v = format.get_dynamic_arrays();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0], "Bar".parse().unwrap())
    }

    #[test]
    fn test_concretize() {
        let mut con_fmt = make_sized_format();
        con_fmt.format.fields[1].dtype = PrimitiveArray(NumericType::U8.into(), 40).into();
        let abs_fmt = make_unsized_format();
        let sizes = vec![("Bar".parse().unwrap(), 40)];
        let abs_fmt_conretized = abs_fmt.concretize(&sizes);
        assert_eq!(
            abs_fmt_conretized.maybe_size_of().unwrap(),
            con_fmt.maybe_size_of().unwrap()
        );
        assert_eq!(abs_fmt_conretized, con_fmt);
    }

    #[test]
    #[should_panic]
    fn test_concretize_panic() {
        let format = make_unsized_format();
        let sizes = vec![("Foo".parse().unwrap(), 40)];
        format.concretize(&sizes);
    }
}
