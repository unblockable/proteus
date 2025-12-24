use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::task::{Context, Poll};

use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::{Notify, mpsc};
use tokio_util::sync::{PollSender, ReusableBoxFuture};

pub struct PollTaskChannel;

impl PollTaskChannel {
    pub fn channel<T: Send>(buf_size: usize) -> (PollTaskSender<T>, PollTaskReceiver<T>) {
        let (task_tx, task_rx) = mpsc::channel(buf_size);
        (PollTaskSender::new(task_tx), PollTaskReceiver::new(task_rx))
    }
}

pub struct PollTaskSender<T> {
    sender: PollSender<PollTask<T>>,
    task_future: ReusableBoxFuture<'static, ()>,
    task_armed: bool,
}

impl<T: Send> PollTaskSender<T> {
    fn new(sender: Sender<PollTask<T>>) -> Self {
        Self {
            sender: PollSender::new(sender),
            task_future: ReusableBoxFuture::new(async move { unreachable!() }),
            task_armed: false,
        }
    }

    pub fn _reserve(&mut self) -> impl Future<Output = Result<(), ()>> {
        std::future::poll_fn(move |cx| self.poll_reserve(cx))
    }

    pub fn poll_reserve(&mut self, cx: &mut Context) -> Poll<Result<(), ()>> {
        match self.sender.poll_reserve(cx) {
            Poll::Ready(Ok(_)) => Poll::Ready(Ok(())),
            Poll::Ready(Err(_)) => Poll::Ready(Err(())),
            Poll::Pending => Poll::Pending,
        }
    }

    pub fn send(&mut self, data: T) -> Result<(), ()> {
        let task = PollTask::new(data);
        let task_not = task.notify.clone();

        match self.sender.send_item(task) {
            Ok(_) => {
                let fut = async move { task_not.notified().await };
                self.task_future.set(fut);
                self.task_armed = true;
                Ok(())
            }
            Err(_) => Err(()),
        }
    }

    pub fn _wait(&mut self) -> impl Future<Output = ()> {
        std::future::poll_fn(move |cx| self.poll_wait(cx))
    }

    pub fn poll_wait(&mut self, cx: &mut Context) -> Poll<()> {
        if self.task_armed {
            match self.task_future.poll(cx) {
                Poll::Ready(_) => {
                    self.task_armed = false;
                    Poll::Ready(())
                }
                Poll::Pending => Poll::Pending,
            }
        } else {
            Poll::Ready(())
        }
    }
}

pub struct PollTaskReceiver<T> {
    receiver: Receiver<PollTask<T>>,
}

impl<T> PollTaskReceiver<T> {
    fn new(receiver: Receiver<PollTask<T>>) -> Self {
        Self { receiver }
    }
}

impl<T> Deref for PollTaskReceiver<T> {
    type Target = Receiver<PollTask<T>>;
    fn deref(&self) -> &Self::Target {
        &self.receiver
    }
}

impl<T> DerefMut for PollTaskReceiver<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.receiver
    }
}

pub struct PollTask<T> {
    data: T,
    notify: Arc<Notify>,
}

impl<T> PollTask<T> {
    fn new(data: T) -> Self {
        Self {
            data,
            notify: Arc::new(Notify::new()),
        }
    }
}

impl<T> Drop for PollTask<T> {
    fn drop(&mut self) {
        self.notify.notify_one();
    }
}

impl<T> Deref for PollTask<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl<T> DerefMut for PollTask<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Deref;

    use crate::common::sync::PollTaskChannel;
    use crate::common::sync::task::PollTask;

    struct TestData {
        value: usize,
    }
    const N_TASKS: usize = 100;
    const CHAN_BUF_SIZE: usize = 10;

    async fn poll_channel_tasks_inner(do_wait: bool) {
        let (mut sender, mut receiver) = PollTaskChannel::channel(CHAN_BUF_SIZE);

        let r_handle = tokio::spawn(async move {
            for i in 1..=N_TASKS {
                let task: PollTask<TestData> = receiver.recv().await.unwrap();
                log::debug!("Receive task {i}/{N_TASKS}, value = {}", task.deref().value)
            }
        });

        let s_handle = tokio::spawn(async move {
            for i in 1..=N_TASKS {
                let data = TestData { value: 10 * i };
                sender._reserve().await.unwrap();
                log::debug!("Reserve task {i}/{N_TASKS}");
                sender.send(data).unwrap();
                log::debug!("Send task {i}/{N_TASKS}");
                if do_wait {
                    sender._wait().await;
                    log::debug!("Wait task {i}/{N_TASKS}");
                }
            }
        });

        r_handle.await.unwrap();
        s_handle.await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn poll_channel_tasks_wait() {
        // let _ = env_logger::try_init();
        poll_channel_tasks_inner(true).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn poll_channel_tasks_nowait() {
        // let _ = env_logger::try_init();
        poll_channel_tasks_inner(false).await;
    }
}
