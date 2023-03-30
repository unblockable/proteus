#![allow(dead_code)]

use crate::lang::types::*;
use pest::iterators::{Pair, Pairs};
use pest_derive::Parser;

#[derive(Parser)]
#[grammar = "lang/parse/proteus_lite.pest"]
pub struct ProteusLiteParser;

type RulePair<'a> = Pair<'a, Rule>;
type RulePairs<'a> = Pairs<'a, Rule>;

fn print_type_of<T>(_: &T) {
    println!("{}", std::any::type_name::<T>())
}

fn parse_numeric_type(p: &mut RulePair) -> NumericType {
    match p.as_str() {
        "u8" => NumericType::U8,
        "u16" => NumericType::U16,
        "u32" => NumericType::U32,
        "u64" => NumericType::U64,
        "i8" => NumericType::I8,
        "i16" => NumericType::I16,
        "i32" => NumericType::I32,
        "i64" => NumericType::I64,
        _ => panic!(),
    }
}

fn parse_primitive_type(p: &mut RulePair) -> PrimitiveType {
    match p.as_str() {
        "bool" => PrimitiveType::Bool,
        "char" => PrimitiveType::Char,
        _ => {
            // Numeric type
            PrimitiveType::Numeric(parse_numeric_type(
                &mut p.clone().into_inner().next().unwrap(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    // Note this useful idiom: importing names from outer (for mod tests) scope.
    use super::*;
    use pest::Parser;

    #[test]
    fn test_parse_numeric_type() {
        let test_cases = vec![
            ("u8", NumericType::U8),
            ("u16", NumericType::U16),
            ("u32", NumericType::U32),
            ("u64", NumericType::U64),
            ("i8", NumericType::I8),
            ("i16", NumericType::I16),
            ("i32", NumericType::I32),
            ("i64", NumericType::I64),
        ];

        for test_case in test_cases {
            let (input, expected_output) = test_case;

            let mut p =
                ProteusLiteParser::parse(Rule::numeric_type, input).expect("unsuccessful parse");

            let mut pair = p.next().unwrap();

            let output = parse_numeric_type(&mut pair);
            assert_eq!(output, expected_output);
        }
    }

    #[test]
    fn test_parse_primitive_type() {
        let test_cases = vec![
            ("u8", PrimitiveType::Numeric(NumericType::U8)),
            ("bool", PrimitiveType::Bool),
            ("char", PrimitiveType::Char),
        ];

        for test_case in test_cases {
            let (input, expected_output) = test_case;

            let mut p =
                ProteusLiteParser::parse(Rule::primitive_type, input).expect("unsuccessful parse");

            let mut pair = p.next().unwrap();

            let output = parse_primitive_type(&mut pair);

            assert_eq!(output, expected_output);
        }
    }
}
