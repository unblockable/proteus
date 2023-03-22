use crate::lang::{format::Format, Role};

#[derive(Clone, PartialEq)]
pub enum ActionKind {
    Send,
    Receive,
}

#[derive(Clone, Copy)]
pub struct ActionId {
    value: u64,
}

#[derive(Clone)]
pub struct Action {
    id: ActionId,
    role: Role,
    kind: ActionKind,
    format: Format,
}

impl Action {
    pub fn new(id: u64, role: Role, kind: ActionKind, format: Format) -> Self {
        Self {
            id: ActionId { value: id },
            role,
            kind,
            format,
        }
    }

    pub fn get_kind(&self, for_role: Role) -> &ActionKind {
        match for_role == self.role {
            True => &self.kind,
            False => match self.kind {
                ActionKind::Send => &ActionKind::Receive,
                ActionKind::Receive => &ActionKind::Send
            }
        }
    }

    pub fn get_format(&self) -> &Format {
        &self.format
    }
}
