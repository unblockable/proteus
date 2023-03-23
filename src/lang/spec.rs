use bytes::Bytes;

use crate::lang::{action::{Action}, format::{Format, Field, FieldKind}};

// Holds the immutable part of a proteus protocol as parsed from a PSF. This is
// used as input to a ProteusProtocol, which is newly created for each
// connection and holds mutable state.
pub struct ProteusSpecification {
    actions: Vec<Action>
}

impl ProteusSpecification {
    pub fn new() -> Self {
        todo!()
    }

    pub fn default() -> Self {
        let mut format = Format::new();
        format.push_field(Field::new(FieldKind::Fixed(Bytes::from("Proteus v0.0.1 Default"))));
        format.push_field(Field::new(FieldKind::Length(2)));
        format.push_field(Field::new(FieldKind::Payload));

        // let send = Action::new(ActionKind::NetworkAction(NetworkAction{kind: NetworkActionKind::}), format.clone());
        // let recv = Action::new(ActionKind::Receive, format);

        Self {
            actions: vec![],
        }
    }

    pub fn start(&self) -> &Vec<Action> {
        self.actions.as_ref()
    }

    pub fn state(&self) -> &Vec<Action> {
        self.actions.as_ref()
    }

    pub fn next(&self) -> &Vec<Action> {
        self.actions.as_ref()
    }
}
