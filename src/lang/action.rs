use crate::lang::{
    common::{LengthBounds, VertexId},
    format::Format,
};

pub struct Action {
    pub kind: ActionKind,
    from: VertexId,
    to: VertexId,
}

pub enum ActionKind {
    NetworkAction(NetworkAction),
    CryptoAction(CryptoAction),
}

pub struct NetworkAction {
    pub kind: NetworkActionKind,
}

pub enum NetworkActionKind {
    SendApp(Format), // not sure if we need/want a format for this
    ReceiveApp(LengthBounds),
    SendNet(Format),
    ReceiveNet(LengthBounds),
}

pub struct CryptoAction {
    pub kind: CryptoActionKind,
}

pub enum CryptoActionKind {
    GenerateKey,
    Encrypt,
    Decrypt,
}
