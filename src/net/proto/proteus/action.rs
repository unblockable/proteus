use crate::net::proto::proteus::{Role, format::Format};

pub enum ActionKind {
    Send,
    Receive,
}

pub struct Action {
    role: Role,
    kind: ActionKind,
    format: Format,
}

impl Action {
    pub fn new(role: Role, kind: ActionKind, format: Format) -> Self {
        Self {role, kind, format}
    }
}
