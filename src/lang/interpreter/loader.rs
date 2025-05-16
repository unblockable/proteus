use std::sync::{Arc, Mutex};

use anyhow::bail;
use tokio::sync::Notify;

use super::ForwardingDirection;
use crate::lang::interpreter::program::Program;
use crate::lang::task::{Task, TaskID, TaskProvider, TaskSet};

// Each forwarding direction concurrently runs a loader to step through the
// task graph and asynchronously load new programs to execute the tasks.
// Since sometimes only a single forwarding direction is active, we use the
// `tokio::sync::Notify` facility to make sure each forwarding direction is
// awoken when a new task for its direction becomes available in the graph.

#[derive(Clone)]
pub struct Loader<T: TaskProvider + Send> {
    spec: T,
    out_notify: Arc<Notify>,
    in_notify: Arc<Notify>,
    state_shared: Arc<Mutex<LoaderState>>,
}

struct LoaderState {
    out_loaded: Option<Task>,
    in_loaded: Option<Task>,
    last_unloaded: Option<TaskID>,
}

impl<T: TaskProvider + Send> Loader<T> {
    pub fn new(spec: T) -> Self {
        Self {
            spec,
            state_shared: Arc::new(Mutex::new(LoaderState {
                out_loaded: None,
                in_loaded: None,
                last_unloaded: None,
            })),
            out_notify: Arc::new(Notify::new()),
            in_notify: Arc::new(Notify::new()),
        }
    }

    pub async fn load(&mut self, direction: ForwardingDirection) -> anyhow::Result<Program> {
        loop {
            // Make sure we are synced with the current round.
            self.sync_tasks()?;

            // Wait for my direction to be ready.
            self.wait(direction).await;

            // Defensive: we expect a task to be here, but in case the other direction
            // raced us and the available task got unloaded, we just loop and wait again.
            if let Some(task) = self.take_next_task(direction)? {
                return Ok(Program::new(task));
            }
        }
    }

    /// Load the next round of tasks if we are currently in an unloaded state.
    fn sync_tasks(&mut self) -> anyhow::Result<()> {
        match self.state_shared.lock() {
            Ok(mut state) => {
                if state.out_loaded.is_none() && state.in_loaded.is_none() {
                    // Find the next taskset in the spec graph.
                    let task_set = match state.last_unloaded {
                        Some(id) => self.spec.get_next_tasks(&id),
                        // Run the initialization task on the outgoing handler.
                        None => TaskSet::OutTask(self.spec.get_init_task()),
                    };

                    // Store the resulting task(s) and notify permit(s).
                    match task_set {
                        TaskSet::InTask(t) => {
                            state.in_loaded = Some(t);
                            self.in_notify.notify_one();
                        }
                        TaskSet::OutTask(t) => {
                            state.out_loaded = Some(t);
                            self.out_notify.notify_one();
                        }
                        TaskSet::InAndOutTasks(pair) => {
                            state.in_loaded = Some(pair.in_task);
                            state.out_loaded = Some(pair.out_task);
                            self.in_notify.notify_one();
                            self.out_notify.notify_one();
                        }
                    };
                }
            }
            Err(e) => bail!("Loader mutex was poisoned during load: {}", e.to_string()),
        };
        Ok(())
    }

    /// Wait for the notify permit indicating a task is ready for `direction`.
    async fn wait(&mut self, direction: ForwardingDirection) {
        match direction {
            ForwardingDirection::AppToNet => {
                self.out_notify.notified().await;
            }
            ForwardingDirection::NetToApp => {
                self.in_notify.notified().await;
            }
        }
    }

    /// Take the next available task for `direction` out of shared state.
    fn take_next_task(&mut self, direction: ForwardingDirection) -> anyhow::Result<Option<Task>> {
        Ok(match self.state_shared.lock() {
            Ok(mut state) => match direction {
                ForwardingDirection::AppToNet => state.out_loaded.take(),
                ForwardingDirection::NetToApp => state.in_loaded.take(),
            },
            Err(e) => bail!("Loader mutex was poisoned during wait: {}", e.to_string()),
        })
    }

    pub fn unload(&mut self, program: Program) -> anyhow::Result<()> {
        // One direction finished its program. Store our current place in the task
        // graph and then unload both directions tasks to make sure we reload later.
        match self.state_shared.lock() {
            Ok(mut state) => {
                // Store the last task id so we can step forward.
                state.last_unloaded = Some(program.task_id());
                // Reset the tasks since they might change in the next round.
                state.out_loaded = None;
                state.in_loaded = None;
                // Wake up both sides to ensure that someone steps forward to resync.
                self.in_notify.notify_one();
                self.out_notify.notify_one();
            }
            Err(e) => bail!("Loader mutex was poisoned during unload: {}", e.to_string()),
        };
        Ok(())
    }
}
