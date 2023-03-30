#![allow(dead_code)]

extern crate pest;
#[macro_use]
extern crate pest_derive;

use pest::Parser;

use core::alloc::{GlobalAlloc, Layout};
use std::fs;

#[derive(Parser)]
#[grammar = "proteus_lite.pest"]
pub struct ProteusLiteParser;

use mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

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

// fn primitive_type_to_size(data_type: PrimitiveType) -> usize {
//   match datatype {
//     PrimitiveType::U8 => std::mem::size_of::<u8>(),
//     _ => panic!("No size determined for given datatype!")
//   }
// }
//
// struct Allocation {
//   datatype: PrimitiveType,
//   nelems: usize,
//   data: *mut u8,
// }
//
// impl Allocation {
//   pub fn new(
//     datatype: PrimitiveType,
//     nelems: usize,
//     data: *mut u8,
//   ) -> Allocation {
//     Allocation{ datatype, nelems, data }
//   }
// }
//
// fn get_alignment_bytes(size_nbytes: usize) -> usize {
//   if size_nbytes <= 2 { 2 }
//   else if size_nbytes <= 4 { 4 }
//   else { 8 }
// }
//
// fn allocate(datatype: PrimitiveType, nelems: usize) -> Allocation {
//   let datatype_size_nbytes = primitive_type_to_size(datatype) * nelems;
//   let layout = core::alloc::Layout::from_size_align(datatype_size_nbytes, get_alignment_bytes(datatype_size_nbytes)).expect("");
//   let data = unsafe { MiMalloc.alloc(layout) };
//   Allocation::new(datatype, nelems, data)
// }

// fn parse_numeric_type(

fn main() {
    let n = ArrayType(DataType::NumericType(NumericType::U16), 10);

    println!("{:?}", n);

    //let a = allocate(PrimitiveType::U8, 1);
    let elem = "DEFINE F1 { NAME: Hello; TYPE: u8; };";

    let frame_spec = ProteusLiteParser::parse(Rule::frame_spec, elem)
        .expect("")
        .next()
        .unwrap();

    for elem in frame_spec.into_inner() {
        println!("{:?}", elem.into_inner());
    }
    // println!("{:?}", successful_parse);

    //let field = "{ NAME: foo, TYPE: int[], SIZE: 3 + 3 + x.sizeof }";
    //let unparsed_file = fs::read_to_string("src/test.protodesc").expect("cannot read file");
    // let field = "3 + 3 + 3 - x.sizeof";
    //let successful_parse = ProtoDescParser::parse(Rule::protodesc, &unparsed_file);
    //println!("{:?}", successful_parse);

    /*
    let successful_parse = CSVParser::parse(Rule::field, "-273.15");
    println!("{:?}", successful_parse);

    let unsuccessful_parse = CSVParser::parse(Rule::field, "this is not a number");
    println!("{:?}", unsuccessful_parse);


    let file = CSVParser::parse(Rule::file, &unparsed_file)
    .expect("unsuccessful parse")
    .next().unwrap();

    let mut field_sum: f64 = 0.0;
    let mut record_count: u64 = 0;

    for record in file.into_inner() {
      match record.as_rule() {
        Rule::record => { record_count += 1;
          for field in record.into_inner() {
            field_sum += field.as_str().parse::<f64>().unwrap();
          }
        }
        Rule::EOI => (),
        _ => unreachable!(),
      }
    }

    println!("Sum of fields: {}", field_sum);
    println!("Number of records: {}", record_count);
    */
}
