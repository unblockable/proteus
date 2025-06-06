#![allow(dead_code)]

use std::convert::From;

use crate::lang::Role;
use crate::lang::ir::v1::InstructionV1;

pub trait TaskProvider {
    fn get_init_task(&self) -> Task;
    fn get_next_tasks(&self, last_task: &TaskID) -> TaskSet;
}

pub trait OldCompile {
    fn parse_path(psf_filename: &str, role: Role) -> anyhow::Result<impl TaskProvider>;
    fn parse_content(psf_content: &str, role: Role) -> anyhow::Result<impl TaskProvider>;
}

#[derive(Debug)]
pub struct TaskPair {
    pub in_task: Task,
    pub out_task: Task,
}

#[derive(Debug)]
pub enum TaskSet {
    InTask(Task),
    OutTask(Task),
    InAndOutTasks(TaskPair),
}

#[derive(Eq, PartialEq, Clone, Copy, Debug, Default)]
pub struct TaskID {
    id: usize,
}

impl TaskID {
    pub fn into_inner(self) -> usize {
        self.id
    }
}

impl From<TaskID> for usize {
    fn from(value: TaskID) -> Self {
        value.id
    }
}

impl From<usize> for TaskID {
    fn from(value: usize) -> Self {
        TaskID { id: value }
    }
}

#[derive(Debug)]
pub struct Task {
    pub ins: Vec<InstructionV1>,
    pub id: TaskID,
}
