use std::cell::UnsafeCell;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::task::{Context, Poll};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::PollSemaphore;

pub struct PollMutex<T> {
    mutex: PollSemaphore,
    data: Arc<UnsafeCell<T>>,
}

pub struct PollMutexGuard<T> {
    // RAII: as long as we hold this permit, we are allowed exclusive access to
    // the data. On drop, the permit is released back to the semaphore.
    #[allow(unused)]
    permit: OwnedSemaphorePermit,
    data: Arc<UnsafeCell<T>>,
}

impl<T> PollMutex<T> {
    pub fn new(data: T) -> Self {
        Self {
            mutex: PollSemaphore::new(Arc::new(Semaphore::new(1))),
            data: Arc::new(UnsafeCell::new(data)),
        }
    }

    pub fn lock(&mut self) -> impl Future<Output = PollMutexGuard<T>> {
        std::future::poll_fn(move |cx| self.poll_lock(cx))
    }

    /// Acquire the mutex lock asynchronously.
    ///
    /// When this method returns Poll::Pending, the current task is scheduled to
    /// receive a wakeup when the mutex is released. Note that on multiple calls
    /// to poll_lock, only the Waker from the Context passed to the most recent
    /// call is scheduled to receive a wakeup.
    pub fn poll_lock(&mut self, cx: &mut Context<'_>) -> Poll<PollMutexGuard<T>> {
        match self.mutex.poll_acquire(cx) {
            Poll::Ready(Some(permit)) => Poll::Ready(PollMutexGuard {
                permit,
                data: self.data.clone(),
            }),
            // We never close the semaphore, and self has a ref to it so it
            // could not have closed itself on drop yet.
            Poll::Ready(None) => unreachable!(),
            Poll::Pending => Poll::Pending,
        }
    }
}

// Allow PollMutex to be sent across threads if T is Send
unsafe impl<T: Send> Send for PollMutex<T> {}
unsafe impl<T: Send> Sync for PollMutex<T> {}
unsafe impl<T: Send + Sync> Sync for PollMutexGuard<T> {}

impl<T> From<T> for PollMutex<T> {
    fn from(data: T) -> Self {
        Self::new(data)
    }
}

impl<T> Clone for PollMutex<T> {
    fn clone(&self) -> Self {
        Self {
            mutex: self.mutex.clone(),
            data: self.data.clone(),
        }
    }
}

impl<T: Default> Default for PollMutex<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<T> Deref for PollMutexGuard<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        // SAFETY: The existence of the PollMutexGuard guarantees we have
        // exclusive access, so we can safely get a shared reference to the
        // data.
        unsafe { &*self.data.get() }
    }
}

impl<T> DerefMut for PollMutexGuard<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: The existence of the PollMutexGuard guarantees we have
        // exclusive access, so we can safely get a mutable reference to the
        // data.
        unsafe { &mut *self.data.get() }
    }
}

#[cfg(test)]
mod tests {
    use crate::common::sync::PollMutex;

    async fn push_one(mut mutex: PollMutex<Vec<usize>>) {
        mutex.lock().await.push(1);
    }

    // #[tokio::test]
    #[tokio::test(flavor = "multi_thread", worker_threads = 1000)]
    async fn poll_mutex_parallel_lock() {
        let n = 1000;
        let mut mutex = PollMutex::new(vec![]);

        let mut handles = vec![];

        for _ in 0..n {
            handles.push(tokio::spawn(push_one(mutex.clone())));
        }

        assert_eq!(handles.len(), n);

        for handle in handles {
            let _ = handle.await;
        }

        let inner = mutex.lock().await;
        assert_eq!(inner.len(), n);
        let sum: usize = inner.iter().sum();
        assert_eq!(sum, n);
    }
}
