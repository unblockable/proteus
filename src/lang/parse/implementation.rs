#![allow(dead_code)]

use crate::lang::common::Role;
use crate::lang::types::*;
use core::str::FromStr;
use pest::iterators::{Pair, Pairs};
use pest::Parser;
use pest_derive::Parser;
use std::collections::hash_map::HashMap;
use std::fmt::Debug;

#[derive(Parser)]
#[grammar = "lang/parse/proteus_lite.pest"]
pub struct ProteusLiteParser;

type RulePair<'a> = Pair<'a, Rule>;
type RulePairs<'a> = Pairs<'a, Rule>;

fn print_type_of<T>(_: &T) {
    println!("{}", std::any::type_name::<T>())
}

fn parse_simple<T: FromStr>(p: &RulePair) -> T
where
    <T as std::str::FromStr>::Err: Debug,
{
    p.as_str().parse().unwrap()
}

fn parse_numeric_type(p: &RulePair) -> NumericType {
    assert!(p.as_rule() == Rule::numeric_type);
    parse_simple(p)
}

fn parse_primitive_type(p: &RulePair) -> PrimitiveType {
    assert!(p.as_rule() == Rule::primitive_type);
    parse_simple(p)
}

fn parse_positive_numeric_literal(p: &RulePair) -> usize {
    assert!(p.as_rule() == Rule::positive_numeric_literal);
    p.as_str().parse::<usize>().unwrap()
}

fn parse_identifier(p: &RulePair) -> Identifier {
    assert!(p.as_rule() == Rule::identifier);
    parse_simple(p)
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

    for f in p {
        fields.push(parse_field(&f));
    }

    Format { name: id, fields }
}

fn parse_fixed_string_semantic(p: &RulePair) -> FieldSemantic {
    assert!(p.as_rule() == Rule::fixed_string_semantic);

    let p = p
        .clone()
        .into_inner()
        .next()
        .unwrap()
        .into_inner()
        .next()
        .unwrap();

    FieldSemantic::FixedString(p.as_str().to_string())
}

fn parse_field_semantic(p: &RulePair) -> FieldSemantic {
    assert!(p.as_rule() == Rule::field_semantic);

    let maybe_inner_p = p.clone().into_inner().next();

    if let Some(ref inner_p) = maybe_inner_p {
        if inner_p.as_rule() == Rule::fixed_string_semantic {
            parse_fixed_string_semantic(&inner_p)
        } else {
            panic!();
        }
    } else {
        parse_simple(p)
    }
}

fn parse_semantic_binding(p: &RulePair) -> SemanticBinding {
    assert!(p.as_rule() == Rule::semantic_binding);

    let mut p = p.clone().into_inner();

    let format = parse_identifier(&p.next().unwrap());
    let field = parse_identifier(&p.next().unwrap());
    let semantic = parse_field_semantic(&p.next().unwrap());

    SemanticBinding {
        format,
        field,
        semantic,
    }
}

fn parse_role(p: &RulePair) -> Role {
    assert!(p.as_rule() == Rule::role);
    parse_simple(p)
}

fn parse_phase(p: &RulePair) -> Phase {
    assert!(p.as_rule() == Rule::phase);
    parse_simple(p)
}

fn parse_sequence_specifier(p: &RulePair) -> SequenceSpecifier {
    assert!(p.as_rule() == Rule::sequence_specifier);

    let mut p = p.clone().into_inner();

    let role = parse_role(&p.next().unwrap());
    let phase = parse_phase(&p.next().unwrap());
    let format = parse_identifier(&p.next().unwrap());

    SequenceSpecifier {
        role,
        phase,
        format,
    }
}

fn parse_password_assignment(p: &RulePair) -> Password {
    assert!(p.as_rule() == Rule::password_assignment);

    let p = p
        .clone()
        .into_inner()
        .next()
        .unwrap()
        .into_inner()
        .next()
        .unwrap();

    Password(p.as_str().to_string())
}

fn parse_cipher(p: &RulePair) -> Cipher {
    assert!(p.as_rule() == Rule::cipher);
    parse_simple(p)
}

fn parse_cipher_assignment(p: &RulePair) -> Cipher {
    assert!(p.as_rule() == Rule::cipher_assignment);
    let p = p.clone().into_inner().next().unwrap();
    parse_cipher(&p)
}

fn parse_encryption_format_binding(p: &RulePair) -> EncryptionFormatBinding {
    assert!(p.as_rule() == Rule::encryption_format_binding);

    let mut p = p.clone().into_inner();

    let to_format_name = parse_identifier(&p.next().unwrap());
    let from_format_name = parse_identifier(&p.next().unwrap());

    EncryptionFormatBinding {
        to_format_name,
        from_format_name,
    }
}

fn parse_encryption_field_directive(p: &RulePair) -> EncryptionFieldDirective {
    assert!(p.as_rule() == Rule::encryption_field_directive);

    let mut p = p.clone().into_inner();

    let ptext_name: Identifier = parse_identifier(&p.next().unwrap());
    let ctext_name: Identifier = parse_identifier(&p.next().unwrap());
    let mac_name: Identifier = parse_identifier(&p.next().unwrap());

    EncryptionFieldDirective {
        ptext_name,
        ctext_name,
        mac_name,
    }
}

fn parse_encryption_directives(p: &RulePair) -> EncryptionDirectives {
    assert!(p.as_rule() == Rule::encryption_directives);

    let mut p = p.clone().into_inner();

    let a = p.next().unwrap();
    let enc_fmt_bnd = parse_encryption_format_binding(&a);

    let mut enc_field_dirs = vec![];

    for x in p {
        enc_field_dirs.push(parse_encryption_field_directive(&x));
    }

    EncryptionDirectives {
        enc_fmt_bnd,
        enc_field_dirs,
    }
}

pub fn parse_crypto_segment(p: &RulePair) -> CryptoSpec {
    assert!(p.as_rule() == Rule::crypto_segment);

    let mut password: Option<Password> = None;
    let mut cipher: Option<Cipher> = None;
    let mut encryption_directives = vec![];

    for e in p.clone().into_inner() {
        match e.as_rule() {
            Rule::password_assignment => {
                password = Some(parse_password_assignment(&e));
            }
            Rule::cipher_assignment => {
                cipher = Some(parse_cipher_assignment(&e));
            }
            Rule::encryption_directives => {
                encryption_directives.push(parse_encryption_directives(&e));
            }
            _ => panic!(),
        }
    }

    CryptoSpec::new(password, cipher.unwrap(), encryption_directives.iter())
}

pub fn parse_psf_impl(p: &RulePair) -> PSF {
    assert!(p.as_rule() == Rule::psf);

    let mut formats: HashMap<Identifier, AbstractFormatAndSemantics> = Default::default();
    let mut sequence: Vec<SequenceSpecifier> = vec![];
    let mut crypto_spec: Option<CryptoSpec> = None;

    let p = p.clone().into_inner();

    for x in p {
        match x.as_rule() {
            Rule::format => {
                let format: AbstractFormatAndSemantics =
                    Into::<AbstractFormat>::into(parse_format(&x)).into();
                formats.insert(format.format.format.name.clone(), format);
            }
            Rule::semantic_binding => {
                let sem_binding = parse_semantic_binding(&x);
                formats
                    .get_mut(&sem_binding.format)
                    .unwrap()
                    .semantics
                    .as_mut_ref()
                    .insert(sem_binding.field.clone(), sem_binding.semantic);
            }
            Rule::sequence_specifier => {
                let seqspec = parse_sequence_specifier(&x);
                sequence.push(seqspec);
            }
            Rule::crypto_segment => {
                crypto_spec = Some(parse_crypto_segment(&x));
            }
            _ => {}
        }
    }

    PSF {
        formats,
        sequence,
        crypto_spec,
    }
}

pub fn parse_psf(psf_contents: &String) -> PSF {
    let rule = Rule::psf;
    let mut p = ProteusLiteParser::parse(rule, psf_contents).expect("Unsuccessful parse");
    let mut pair = p.next().unwrap();
    let psf = parse_psf_impl(&mut pair);
    assert!(psf.is_valid());
    psf
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use pest::Parser;
    use std::fs;
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
            "DEFINE Handshake \
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

    #[test]
    fn test_parse_fixed_string_semantic() {
        let test_cases = vec![(
            "FIXED_STRING(\"foo\")",
            FieldSemantic::FixedString("foo".to_string()),
        )];

        test_rule_pair(
            test_cases.iter(),
            Rule::fixed_string_semantic,
            parse_fixed_string_semantic,
        );
    }

    #[test]
    fn test_parse_field_semantic() {
        let test_cases = vec![
            ("PAYLOAD", FieldSemantic::Payload),
            ("PADDING", FieldSemantic::Padding),
            ("LENGTH", FieldSemantic::Length),
            (
                "FIXED_STRING(\"foo\")",
                FieldSemantic::FixedString("foo".to_string()),
            ),
        ];

        test_rule_pair(
            test_cases.iter(),
            Rule::field_semantic,
            parse_field_semantic,
        );
    }

    #[test]
    fn test_parse_semantic_binding() {
        let s = "{ FORMAT: Foo; FIELD: Bar; SEMANTIC: PAYLOAD };";

        let sb = SemanticBinding {
            format: "Foo".id(),
            field: "Bar".id(),
            semantic: FieldSemantic::Payload,
        };

        let test_cases = vec![(s, sb)];

        test_rule_pair(
            test_cases.iter(),
            Rule::semantic_binding,
            parse_semantic_binding,
        );
    }

    #[test]
    fn test_parse_role() {
        let test_cases = vec![("CLIENT", Role::Client), ("SERVER", Role::Server)];
        test_rule_pair(test_cases.iter(), Rule::role, parse_role);
    }

    #[test]
    fn test_parse_phase() {
        let test_cases = vec![("HANDSHAKE", Phase::Handshake), ("DATA", Phase::Data)];
        test_rule_pair(test_cases.iter(), Rule::phase, parse_phase);
    }

    #[test]
    fn test_parse_sequence_specifier() {
        let s = "{ ROLE: CLIENT; PHASE: HANDSHAKE; FORMAT: Foo };";

        let ss = SequenceSpecifier {
            role: Role::Client,
            phase: Phase::Handshake,
            format: "Foo".id(),
        };

        let test_cases = vec![(s, ss)];

        test_rule_pair(
            test_cases.iter(),
            Rule::sequence_specifier,
            parse_sequence_specifier,
        );
    }

    #[test]
    fn test_parse_password() {
        let input = "PASSWORD = \"hunter2\";";
        let output: Password = "hunter2".parse().unwrap();

        let test_cases = vec![(input, output)];

        test_rule_pair(
            test_cases.iter(),
            Rule::password_assignment,
            parse_password_assignment,
        );
    }

    #[test]
    fn test_parse_cipher_assignment() {
        let input = "CIPHER = CHACHA20-POLY1305;";
        let output = Cipher::ChaCha20Poly1305;

        let test_cases = vec![(input, output)];

        test_rule_pair(
            test_cases.iter(),
            Rule::cipher_assignment,
            parse_cipher_assignment,
        );
    }

    #[test]
    fn test_parse_encryption_format_binding() {
        let input = "ENCRYPT Foo FROM Bar";
        let output = EncryptionFormatBinding {
            to_format_name: "Foo".id(),
            from_format_name: "Bar".id(),
        };

        let test_cases = vec![(input, output)];

        test_rule_pair(
            test_cases.iter(),
            Rule::encryption_format_binding,
            parse_encryption_format_binding,
        );
    }

    #[test]
    fn test_parse_encryption_field_directive() {
        let input = "{PTEXT: length;  CTEXT: enc_length;  MAC: length_mac}";
        let output = EncryptionFieldDirective {
            ptext_name: "length".id(),
            ctext_name: "enc_length".id(),
            mac_name: "length_mac".id(),
        };

        let test_cases = vec![(input, output)];

        test_rule_pair(
            test_cases.iter(),
            Rule::encryption_field_directive,
            parse_encryption_field_directive,
        );
    }

    #[test]
    fn test_parse_encryption_directives() {
        let input = "\
        ENCRYPT EncDataMsg FROM DataMsg\
        { PTEXT: length;  CTEXT: enc_length;  MAC: length_mac },\
        { PTEXT: payload; CTEXT: enc_payload; MAC: payload_mac };";

        let enc_fmt_bnd = EncryptionFormatBinding {
            to_format_name: "EncDataMsg".id(),
            from_format_name: "DataMsg".id(),
        };

        let enc_field_dirs = vec![
            EncryptionFieldDirective {
                ptext_name: "length".id(),
                ctext_name: "enc_length".id(),
                mac_name: "length_mac".id(),
            },
            EncryptionFieldDirective {
                ptext_name: "payload".id(),
                ctext_name: "enc_payload".id(),
                mac_name: "payload_mac".id(),
            },
        ];

        let output = EncryptionDirectives {
            enc_fmt_bnd,
            enc_field_dirs,
        };

        let test_cases = vec![(input, output)];

        test_rule_pair(
            test_cases.iter(),
            Rule::encryption_directives,
            parse_encryption_directives,
        );
    }

    #[test]
    fn test_parse_crypto_segment() {
        let input = "@SEGMENT.CRYPTO\
            PASSWORD = \"hunter2\";\
            CIPHER   = CHACHA20-POLY1305;\
            ENCRYPT EncDataMsg FROM DataMsg\
            { PTEXT: length;  CTEXT: enc_length;  MAC: length_mac },\
            { PTEXT: payload; CTEXT: enc_payload; MAC: payload_mac };";

        let enc_fmt_bnd = EncryptionFormatBinding {
            to_format_name: "EncDataMsg".id(),
            from_format_name: "DataMsg".id(),
        };

        let password = Some(Password("hunter2".to_string()));
        let cipher = Cipher::ChaCha20Poly1305;

        let enc_field_dirs = vec![
            EncryptionFieldDirective {
                ptext_name: "length".id(),
                ctext_name: "enc_length".id(),
                mac_name: "length_mac".id(),
            },
            EncryptionFieldDirective {
                ptext_name: "payload".id(),
                ctext_name: "enc_payload".id(),
                mac_name: "payload_mac".id(),
            },
        ];

        let directives = vec![EncryptionDirectives {
            enc_fmt_bnd,
            enc_field_dirs,
        }];

        let output = CryptoSpec::new(password, cipher, directives.iter());

        let test_cases = vec![(input, output)];

        test_rule_pair(
            test_cases.iter(),
            Rule::crypto_segment,
            parse_crypto_segment,
        );
    }

    pub fn parse_example_psf() -> PSF {
        let filepath = "src/lang/parse/examples/example.psf";
        let input = fs::read_to_string(filepath).expect("cannot read example file");
        parse_psf(&input)
    }

    #[test]
    fn test_parse_psf() {
        parse_example_psf();
    }

    pub fn parse_shadowsocks_psf() -> PSF {
        let filepath = "src/lang/parse/examples/shadowsocks.psf";
        let input = fs::read_to_string(filepath).expect("cannot read shadowsocks file");
        parse_psf(&input)
    }

    #[test]
    fn test_parse_shadowsocks_psf() {
        let psf = parse_shadowsocks_psf();
    }
}
