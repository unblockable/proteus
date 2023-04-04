use crate::lang::{
    field::Field,
    spec::message::MessageSpec,
    task::{Task, TaskID},
};

use super::message::MessageSpecBuilder;

// Holds the immutable part of a proteus protocol as parsed from a PSF. This is
// used as input to a ProteusProtocol, which is newly created for each
// connection and holds mutable state.
pub struct ProteusSpec {
    // state_machine: RyanGraph,
    // crypto: CryptoSpec,
    // message: MessageSpec,
    in_fmt: MessageSpec,
    out_fmt: MessageSpec,
}

impl ProteusSpec {
    pub fn get_in_fmt(&self) -> &MessageSpec {
        &self.in_fmt
    }

    pub fn get_out_fmt(&self) -> &MessageSpec {
        &self.out_fmt
    }

    pub fn get_next_tasks(&self, last_task: &TaskID) -> Vec<Task> {
        todo!()
    }
}

pub struct ProteusSpecBuilder {
    in_fmt: MessageSpecBuilder,
    out_fmt: MessageSpecBuilder,
}

impl ProteusSpecBuilder {
    pub fn new() -> Self {
        Self {
            out_fmt: MessageSpecBuilder::new(),
            in_fmt: MessageSpecBuilder::new(),
        }
    }

    pub fn add_in_field(&mut self, field: Field) {
        self.in_fmt.add_field(field);
    }

    pub fn add_out_field(&mut self, field: Field) {
        self.out_fmt.add_field(field);
    }
}

impl From<ProteusSpecBuilder> for ProteusSpec {
    fn from(builder: ProteusSpecBuilder) -> Self {
        ProteusSpec {
            in_fmt: builder.in_fmt.into(),
            out_fmt: builder.out_fmt.into(),
        }
    }
}
