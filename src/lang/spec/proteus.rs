use crate::lang::{
    compiler::*,
    task::{Task, TaskID, TaskProvider, TaskSet},
};

// Holds the immutable part of a proteus protocol as parsed from a PSF. This is
// used as input to a ProteusProtocol, which is newly created for each
// connection and holds mutable state.
#[derive(Clone)]
pub struct ProteusSpec {
    task_graph: TaskGraphImpl,
}

impl ProteusSpec {
    pub fn new(task_graph: TaskGraphImpl) -> ProteusSpec {
        ProteusSpec { task_graph }
    }
}

impl TaskProvider for ProteusSpec {
    fn get_init_task(&self) -> Task {
        self.task_graph.init_task()
    }

    fn get_next_tasks(&self, last_task: &TaskID) -> TaskSet {
        self.task_graph.next(*last_task)
    }
}
