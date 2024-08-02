use crate::lang::interpreter::program::Program;
use crate::lang::task::{TaskID, TaskProvider, TaskSet};

use super::ForwardingDirection;

pub enum LoaderResult {
    Ready(Program),
    Pending,
}

pub struct Loader<T: TaskProvider + Send> {
    spec: T,
    current_out: Option<TaskID>,
    previous_out: Option<TaskID>,
    current_in: Option<TaskID>,
    previous_in: Option<TaskID>,
}

impl<T: TaskProvider + Send> Loader<T> {
    pub fn new(spec: T) -> Self {
        Self {
            spec,
            current_out: None,
            previous_out: None,
            current_in: None,
            previous_in: None,
        }
    }

    pub fn next(&mut self, direction: ForwardingDirection) -> LoaderResult {
        match direction {
            ForwardingDirection::AppToNet => self.next_app_to_net(),
            ForwardingDirection::NetToApp => self.next_net_to_app(),
        }
    }

    fn next_app_to_net(&mut self) -> LoaderResult {
        if self.current_out.is_some() {
            self.previous_out = self.current_out.take();
        }

        let task = match self.previous_out {
            Some(id) => match self.spec.get_next_tasks(&id) {
                TaskSet::InTask(_) => None,
                TaskSet::OutTask(task) => Some(task),
                TaskSet::InAndOutTasks(pair) => Some(pair.out_task),
            },
            None => Some(self.spec.get_init_task()),
        };

        if let Some(t) = task {
            self.current_out = Some(t.id);
            LoaderResult::Ready(Program::new(t))
        } else {
            LoaderResult::Pending
        }
    }

    fn next_net_to_app(&mut self) -> LoaderResult {
        if self.current_in.is_some() {
            self.previous_in = self.current_in.take();
        }

        let task = match self.previous_in {
            Some(id) => match self.spec.get_next_tasks(&id) {
                TaskSet::InTask(task) => Some(task),
                TaskSet::OutTask(_) => None,
                TaskSet::InAndOutTasks(pair) => Some(pair.in_task),
            },
            None => None,
        };

        if let Some(t) = task {
            self.current_in = Some(t.id);
            LoaderResult::Ready(Program::new(t))
        } else {
            LoaderResult::Pending
        }
    }
}
