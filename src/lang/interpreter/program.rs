use super::vm::VirtualMachine;
use crate::lang::Execute;
use crate::lang::ir::bridge::{Task, TaskID};
use crate::net::{Reader, Writer};

pub struct Program {
    task: Task,
    next_ins_index: usize,
}

impl Program {
    pub fn new(task: Task) -> Self {
        Self {
            task,
            next_ins_index: 0,
        }
    }

    pub fn task_id(&self) -> TaskID {
        self.task.id
    }

    pub async fn execute<R: Reader, W: Writer>(
        &mut self,
        vm: &mut VirtualMachine<R, W>,
    ) -> anyhow::Result<()> {
        while self.next_ins_index < self.task.ins.len() {
            self.task.ins[self.next_ins_index].execute(vm).await?;
            self.next_ins_index += 1;
        }
        vm.clear_heap();
        Ok(())
    }
}
