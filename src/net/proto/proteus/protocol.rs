use crate::lang::ProteusSpecification;
use crate::net::proto::proteus::{
    Role,
    action::Action,
    message::{CovertMessage, OvertMessage},
};

pub struct ProteusProtocol {
    // An instantiation of a proteus protocol currently running on a connection.
    role: Role
}

impl ProteusProtocol {
    pub fn new(role: Role) -> Self {
        Self {role}
    }

    // probably need some functions here to build up the action graph

    pub fn get_next_action(&mut self, spec: &ProteusSpecification) -> Vec<Action> {
        todo!()
    }

    pub fn pack_message(
        &mut self,
        spec: &ProteusSpecification,
        payload: CovertMessage,
    ) -> OvertMessage {
        todo!()
    }

    pub fn unpack_message(
        &mut self,
        spec: &ProteusSpecification,
        payload: OvertMessage,
    ) -> Result<CovertMessage, u64> {
        todo!()
    }
}
