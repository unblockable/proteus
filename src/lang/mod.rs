use bytes::Bytes;

use self::{action::{Action, ActionId, ActionKind}, format::{Format, Field, FieldKind}};

pub mod action;
pub mod format;
pub mod proteus;

#[derive(Clone, PartialEq)]
pub enum Role {
    Client,
    Server,
}

#[derive(Clone)]
pub struct ProteusSpecification {
    // Holds the immutable part of a proteus protocol as parsed from
    // a psf. This is used as input to a ProteusProtocol, which is
    // newly created for each connection and holds mutable state.
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

        let send = Action::new(0, Role::Client, ActionKind::Send, format.clone());
        let recv = Action::new(1, Role::Client, ActionKind::Receive, format);

        Self {
            actions: vec![send, recv],
        }
    }

    pub fn start(&self) -> &Vec<Action> {
        self.actions.as_ref()
    }

    pub fn current(&self, id: ActionId) -> &Vec<Action> {
        self.actions.as_ref()
    }

    pub fn next(&self, id: ActionId) -> &Vec<Action> {
        self.actions.as_ref()
    }
}
