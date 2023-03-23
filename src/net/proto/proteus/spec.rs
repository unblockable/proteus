use crate::lang::{ProteusSpecification, Role, action::{Action, ActionId}};
use crate::net::proto::proteus::{
    message::{CovertPayload, OvertMessage},
};

pub struct ProteusProtocol {
    // An instantiation of a proteus protocol currently running on a connection.
    spec: ProteusSpecification,
    role: Role,
    action_id: Option<ActionId>,
    vec_id: Option<usize>,
}

impl ProteusProtocol {
    pub fn new(spec: ProteusSpecification, role: Role) -> Self {
        Self {spec, role, action_id: None, vec_id: None}
    }

    pub fn get_next_action(&mut self) -> &Action {
        match self.action_id {
            Some(aid) => match self.vec_id {
                Some(mut vid) => {
                    let actions = self.spec.current(aid);
                    if vid > actions.len() {
                        vid = 0;
                    }
                    &actions[vid]
                },
                None => {
                    self.vec_id = Some(0);
                    let vid = self.vec_id.unwrap();
                    &self.spec.next(aid)[vid]
                },
            },
            None => {
                self.vec_id = Some(0);
                let vid = self.vec_id.unwrap();
                &self.spec.start()[vid]
            }
        }
    }

    pub fn pack_message(
        &mut self,
        payload: CovertPayload,
    ) -> OvertMessage {
        // if success, set self.vec_id to None
        // if failed, increment vec_id
        todo!()
    }

    pub fn unpack_message(
        &mut self,
        message: OvertMessage,
    ) -> Result<CovertPayload, u64> {
        // if success, set self.vec_id to None
        // if failed, increment vec_id
        todo!()
    }
}
