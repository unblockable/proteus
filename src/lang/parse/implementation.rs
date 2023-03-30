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

fn parse_positive_numeric_literal(p: &mut RulePair) -> usize {
    p.as_str().parse::<usize>().unwrap()
}

fn parse_identifier(p: &mut RulePair) -> Identifier {
    Identifier(p.as_str().to_string())
}

fn parse_primitive_array(p: &mut RulePairs) -> PrimitiveArray {
    let mut p = p.next().unwrap().into_inner();

    let mut pt = p.next().unwrap();
    assert!(pt.as_rule() == Rule::primitive_type);
    let pt = parse_primitive_type(&mut pt);

    let mut pnl = p.next().unwrap();
    assert!(pnl.as_rule() == Rule::positive_numeric_literal);
    let pnl = parse_positive_numeric_literal(&mut pnl);

    PrimitiveArray(pt, pnl)
}

fn parse_sizeof_op(p: &mut RulePair) -> UnaryOp {
    UnaryOp::SizeOf(parse_identifier(
        &mut p.clone().into_inner().next().unwrap(),
    ))
}

fn parse_dynamic_array(p: &mut RulePairs) -> DynamicArray {
    let mut p = p.next().unwrap().into_inner();

    let mut pt = p.next().unwrap();
    assert!(pt.as_rule() == Rule::primitive_type);
    let pt = parse_primitive_type(&mut pt);

    let mut soo = p.next().unwrap();
    assert!(soo.as_rule() == Rule::size_of_op);
    let soo = parse_sizeof_op(&mut soo);

    DynamicArray(pt, soo)
}

fn parse_array(p: &mut RulePair) -> Array {
    match p.clone().into_inner().next().unwrap().as_rule() {
        Rule::primitive_array => Array::Primitive(parse_primitive_array(&mut p.clone().into_inner())),
        Rule::dynamic_array => Array::Dynamic(parse_dynamic_array(&mut p.clone().into_inner())),
        _ => panic!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pest::Parser;
    use std::iter::Iterator;

    fn test_rule_pair<
        'a,
        T: Iterator<Item = &'a (&'a str, V)>,
        V: std::fmt::Debug + std::cmp::PartialEq + 'a,
    >(
        test_cases: T,
        rule: Rule,
        parse_function: fn(&mut RulePair) -> V,
    ) {
        for test_case in test_cases {
            let (input, expected_output) = test_case;

            let mut p = ProteusLiteParser::parse(rule, &input).expect("Unsuccessful parse");

            let mut pair = p.next().unwrap();

            let output = parse_function(&mut pair);

            assert_eq!(&output, expected_output);
        }
    }

    fn test_rule_pairs<
        'a,
        T: Iterator<Item = &'a (&'a str, V)>,
        V: std::fmt::Debug + std::cmp::PartialEq + 'a,
    >(
        test_cases: T,
        rule: Rule,
        parse_function: fn(&mut RulePairs) -> V,
    ) {
        for test_case in test_cases {
            let (input, expected_output) = test_case;

            let mut p = ProteusLiteParser::parse(rule, &input).expect("Unsuccessful parse");

            let output = parse_function(&mut p);

            assert_eq!(&output, expected_output);
        }
    }

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

        test_rule_pair(test_cases.iter(), Rule::numeric_type, parse_numeric_type);
    }

    #[test]
    fn test_parse_primitive_type() {
        let test_cases = vec![
            ("u8", PrimitiveType::Numeric(NumericType::U8)),
            ("bool", PrimitiveType::Bool),
            ("char", PrimitiveType::Char),
        ];

        test_rule_pair(
            test_cases.iter(),
            Rule::primitive_type,
            parse_primitive_type,
        );
    }

    #[test]
    fn test_parse_positive_numeric_literal() {
        let test_cases = vec![("0123", 123)];

        test_rule_pair(
            test_cases.iter(),
            Rule::positive_numeric_literal,
            parse_positive_numeric_literal,
        );
    }

    #[test]
    fn test_parse_primitive_array() {
        let test_cases = vec![(
            "[u8; 10]",
            PrimitiveArray(PrimitiveType::Numeric(NumericType::U8), 10),
        )];

        test_rule_pairs(
            test_cases.iter(),
            Rule::primitive_array,
            parse_primitive_array,
        );
    }

    #[test]
    fn test_parse_size_of_op() {
        let test_cases = vec![("x.size_of", UnaryOp::SizeOf(Identifier("x".to_string())))];

        test_rule_pair(test_cases.iter(), Rule::size_of_op, parse_sizeof_op);
    }

    #[test]
    fn test_parse_dynamic_array() {
        let test_cases = vec![(
            "[u8; x.size_of]",
            DynamicArray(
                PrimitiveType::Numeric(NumericType::U8),
                UnaryOp::SizeOf(Identifier("x".to_string())),
            ),
        )];

        test_rule_pairs(test_cases.iter(), Rule::dynamic_array, parse_dynamic_array);
    }

    #[test]
    fn test_parse_array() {
        let test_cases = vec![
            (
                "[u8; 10]",
                Array::Primitive(PrimitiveArray(PrimitiveType::Numeric(NumericType::U8), 10)),
            ),
            (
                "[u8; x.size_of]",
                Array::Dynamic(DynamicArray(
                    PrimitiveType::Numeric(NumericType::U8),
                    UnaryOp::SizeOf(Identifier("x".to_string())),
                )),
            ),
        ];

        test_rule_pair(test_cases.iter(), Rule::array, parse_array);
    }
}
