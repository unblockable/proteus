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

fn parse_numeric_type(p: &RulePair) -> NumericType {
    assert!(p.as_rule() == Rule::numeric_type);
    p.as_str().parse().unwrap()
}

fn parse_primitive_type(p: &RulePair) -> PrimitiveType {
    assert!(p.as_rule() == Rule::primitive_type);
    p.as_str().parse().unwrap()
}

fn parse_positive_numeric_literal(p: &RulePair) -> usize {
    assert!(p.as_rule() == Rule::positive_numeric_literal);
    p.as_str().parse::<usize>().unwrap()
}

fn parse_identifier(p: &RulePair) -> Identifier {
    assert!(p.as_rule() == Rule::identifier);

    p.as_str().parse().unwrap()
}

fn parse_primitive_array(p: &RulePair) -> PrimitiveArray {
    assert!(p.as_rule() == Rule::primitive_array);

    let mut p = p.clone().into_inner();

    let pt = p.next().unwrap();
    let pt = parse_primitive_type(&pt);

    let pnl = p.next().unwrap();
    let pnl = parse_positive_numeric_literal(&pnl);

    PrimitiveArray(pt, pnl)
}

fn parse_sizeof_op(p: &RulePair) -> UnaryOp {
    assert!(p.as_rule() == Rule::size_of_op);

    UnaryOp::SizeOf(parse_identifier(&p.clone().into_inner().next().unwrap()))
}

fn parse_dynamic_array(p: &RulePair) -> DynamicArray {
    assert!(p.as_rule() == Rule::dynamic_array);

    let mut p = p.clone().into_inner();

    // TODO: ryan, fix the grammar and parser because dynamic arrays are now
    // always u8
    let pt = p.next().unwrap();
    let pt = parse_primitive_type(&pt);

    let soo = p.next().unwrap();
    let soo = parse_sizeof_op(&soo);

    DynamicArray(soo)
}

fn parse_array(p: &RulePair) -> Array {
    assert!(p.as_rule() == Rule::array);

    let p = p.clone().into_inner().next().unwrap();

    match p.as_rule() {
        Rule::primitive_array => parse_primitive_array(&p).into(),
        Rule::dynamic_array => parse_dynamic_array(&p).into(),
        _ => panic!(),
    }
}

fn parse_name_value(p: &RulePair) -> Identifier {
    assert!(p.as_rule() == Rule::name_value);
    parse_identifier(&p.clone().into_inner().next().unwrap())
}

fn parse_type_value(p: &RulePair) -> Array {
    assert!(p.as_rule() == Rule::type_value);

    let p = p.clone().into_inner().next().unwrap();

    match p.as_rule() {
        Rule::primitive_type => Array::Primitive(PrimitiveArray(parse_primitive_type(&p), 1)),
        Rule::array => parse_array(&p),
        _ => panic!(),
    }
}

fn parse_field(p: &RulePair) -> Field {
    assert!(p.as_rule() == Rule::field);

    let mut p = p.clone().into_inner();

    let nv = p.next().unwrap();
    let nv = parse_name_value(&nv);

    let tv = p.next().unwrap();
    let tv = parse_type_value(&tv);

    Field {
        name: nv,
        dtype: tv,
    }
}

fn parse_format(p: &RulePair) -> Format {
    assert!(p.as_rule() == Rule::format);

    let mut p = p.clone().into_inner();

    let id = p.next().unwrap();
    let id = parse_identifier(&id);

    let mut fields = vec![];

    let f = p.next().unwrap();
    fields.push(parse_field(&f));

    while let Some(f) = p.next() {
        fields.push(parse_field(&f));
    }

    Format { name: id, fields }
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
        parse_function: fn(&RulePair) -> V,
    ) {
        for test_case in test_cases {
            let (input, expected_output) = test_case;

            let mut p = ProteusLiteParser::parse(rule, &input).expect("Unsuccessful parse");

            let mut pair = p.next().unwrap();

            let output = parse_function(&mut pair);

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
            ("u8", NumericType::U8.into()),
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
        let test_cases = vec![("[u8; 10]", PrimitiveArray(NumericType::U8.into(), 10))];

        test_rule_pair(
            test_cases.iter(),
            Rule::primitive_array,
            parse_primitive_array,
        );
    }

    #[test]
    fn test_parse_size_of_op() {
        let test_cases = vec![("x.size_of", UnaryOp::SizeOf("x".parse().unwrap()))];

        test_rule_pair(test_cases.iter(), Rule::size_of_op, parse_sizeof_op);
    }

    #[test]
    fn test_parse_dynamic_array() {
        let test_cases = vec![(
            "[u8; x.size_of]",
            DynamicArray(UnaryOp::SizeOf("x".parse().unwrap())),
        )];

        test_rule_pair(test_cases.iter(), Rule::dynamic_array, parse_dynamic_array);
    }

    #[test]
    fn test_parse_array() {
        let test_cases = vec![
            (
                "[u8; 10]",
                PrimitiveArray(NumericType::U8.into(), 10).into(),
            ),
            (
                "[u8; x.size_of]",
                DynamicArray(UnaryOp::SizeOf("x".parse().unwrap())).into(),
            ),
        ];

        test_rule_pair(test_cases.iter(), Rule::array, parse_array);
    }

    #[test]
    fn test_name_value() {
        let test_cases = vec![("NAME: Foo", "Foo".parse().unwrap())];

        test_rule_pair(test_cases.iter(), Rule::name_value, parse_name_value);
    }

    #[test]
    fn test_type_value() {
        let test_cases = vec![
            ("TYPE: u8", PrimitiveArray(NumericType::U8.into(), 1).into()),
            (
                "TYPE: [i8; 10]",
                PrimitiveArray(NumericType::I8.into(), 10).into(),
            ),
        ];

        test_rule_pair(test_cases.iter(), Rule::type_value, parse_type_value);
    }

    #[test]
    fn test_parse_field() {
        let test_cases = vec![(
            "{ NAME: Foo; TYPE: u8 }",
            Field {
                name: "Foo".parse().unwrap(),
                dtype: PrimitiveArray(NumericType::U8.into(), 1).into(),
            },
        )];

        test_rule_pair(test_cases.iter(), Rule::field, parse_field);
    }

    #[test]
    fn test_format() {
        let test_cases = vec![(
            "DEFINE Handshake FIELDS \
            {NAME: Foo; TYPE: u8}, \
            {NAME: Bar; TYPE: [u32; 10]};",
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
            },
        )];

        println!("{:?}", test_cases[0].1.maybe_size_of().unwrap());

        test_rule_pair(test_cases.iter(), Rule::format, parse_format);
    }
}
