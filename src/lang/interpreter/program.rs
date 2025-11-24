use tokio::io::{AsyncRead, AsyncWrite};

use super::vm::VirtualMachine;
use crate::lang::ir::bridge::{Task, TaskID};
use crate::lang::{Execute, ExecuteOk};

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

    pub async fn execute<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
        &mut self,
        vm: &mut VirtualMachine<R, W>,
    ) -> anyhow::Result<ExecuteOk> {
        while self.next_ins_index < self.task.ins.len() {
            match self.task.ins[self.next_ins_index].execute(vm).await {
                Ok(ExecuteOk::Ok) => {
                    self.next_ins_index += 1;
                }
                Ok(ExecuteOk::ReadEof) => {
                    vm.clear_heap();
                    return Ok(ExecuteOk::ReadEof);
                }
                Err(e) => {
                    vm.clear_heap();
                    return Err(e);
                }
            }
        }
        vm.clear_heap();
        Ok(ExecuteOk::Ok)
    }
}
