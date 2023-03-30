use crate::lang::{
    common::Role,
    parse::{self, Parse},
    spec::proteus::ProteusSpecification,
};

use crate::lang::{
    action::Action,
    format::{Field, FieldKind, Format},
    spec::{crypto::{CryptoSpec, CryptoOperation, CryptoOperationKind}, message::MessageSpec}
};

pub struct NullParser {
}

impl NullParser {
    pub fn new() -> Self {
        Self {}
    }
}

impl Parse for NullParser {
    fn parse(
        &mut self,
        psf_filename: &str,
        role: Role,
    ) -> Result<ProteusSpecification, parse::Error> {
        let mut format = Format::new();
        format.push_field(Field::new(FieldKind::Fixed(Bytes::from(
            "Proteus v0.0.1 Default",
        ))));
        format.push_field(Field::new(FieldKind::Length(2)));
        format.push_field(Field::new(FieldKind::Payload));

        let mut crypto = CryptoSpec::new(format.clone(), format);
        // Encrypt the payload field.
        crypto.add_operation(CryptoOperation{kind: CryptoOperationKind::Encrypt, input_fmt_range: 2..3, output_fmt_range: 2..3});

        // let send = Action::new(ActionKind::NetworkAction(NetworkAction{kind: NetworkActionKind::}), format.clone());
        // let recv = Action::new(ActionKind::Receive, format);

        // MessageSpec is as follows, contains a MessageFormat which is just a chain of Byte objects
        // Copy(Bytes), field_num=0
        // ReadApp(Range), field_num=2
        // Encrypt(CryptoSpec), field_range(2..3)
        // WriteLength, field_num=1
        // 

        // Self { actions: vec![] }
        todo!()
    }
}
