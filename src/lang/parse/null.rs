use crate::lang::{
    command::{Command, NetCmdIn, ReadCmdArgs},
    common::Role,
    field::{LengthField, LengthFieldKind},
    parse::{self, Parse},
    spec::proteus::{ProteusSpec, ProteusSpecBuilder},
};

use crate::lang::field::{Field, FieldKind};

pub struct NullParser {}

impl NullParser {
    pub fn new() -> Self {
        Self {}
    }
}

impl Parse for NullParser {
    fn parse(&mut self, psf_filename: &str, role: Role) -> Result<ProteusSpec, parse::Error> {
        // Set up a minimal functional protocol.
        let mut builder = ProteusSpecBuilder::new();

        let len = LengthField::new(LengthFieldKind::Payload, 2u8);
        let field = Field::new(FieldKind::Length(len));
        builder.add_in_field(field.clone());
        builder.add_out_field(field);

        builder.add_in_field(Field::new(FieldKind::Payload));
        builder.add_out_field(Field::new(FieldKind::Payload));

        Ok(builder.into())
    }
}
