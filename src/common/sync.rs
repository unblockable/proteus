use std::cell::UnsafeCell;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::task::{Context, Poll, ready};

use futures::FutureExt;
use tokio::sync::{Mutex, OwnedMutexGuard};
use tokio_util::sync::ReusableBoxFuture;

pub struct PollMutex<T> {
    mutex: Arc<Mutex<Nothing>>,
    data: Arc<UnsafeCell<T>>,
    future: Option<ReusableBoxFuture<'static, OwnedMutexGuard<Nothing>>>,
}

pub struct PollMutexGuard<T> {
    // RAII: as long as we hold this inner guard, we are allowed exclusive
    // access to the data.
    #[allow(unused)]
    guard: OwnedMutexGuard<Nothing>,
    data: Arc<UnsafeCell<T>>,
}

struct Nothing;

impl<T> PollMutex<T> {
    pub fn new(data: T) -> Self {
        Self {
            mutex: Arc::new(Mutex::new(Nothing)),
            data: Arc::new(UnsafeCell::new(data)),
            future: None,
        }
    }

    pub fn lock(&mut self) -> impl Future<Output = PollMutexGuard<T>> {
        std::future::poll_fn(move |cx| self.poll_lock(cx))
    }

    /// Acquire the mutex lock asynchronously. If the result is `Poll::Pending`,
    /// a wakeup will be registered with the context waker.
    pub fn poll_lock(&mut self, cx: &mut Context<'_>) -> Poll<PollMutexGuard<T>> {
        let boxed_future = match self.future.as_mut() {
            Some(boxed_future) => boxed_future,
            None => {
                // Avoid allocation if we can get the lock immediately.
                match self.mutex.clone().try_lock_owned() {
                    Ok(guard) => {
                        return Poll::Ready(PollMutexGuard {
                            guard,
                            data: self.data.clone(),
                        });
                    }
                    Err(_) => {}
                }

                // Lock not ready, set up a future we can poll.
                let lock_fut = self.mutex.clone().lock_owned();
                &mut self.future.get_or_insert(ReusableBoxFuture::new(lock_fut))
            }
        };

        // Poll until its ready.
        let guard = ready!(boxed_future.poll_unpin(cx));

        // Replace the future so we are ready for the next lock request.
        boxed_future.set(self.mutex.clone().lock_owned());

        Poll::Ready(PollMutexGuard {
            guard,
            data: self.data.clone(),
        })
    }
}

// Allow PollMutex to be sent across threads if T is Send
unsafe impl<T: Send> Send for PollMutex<T> {}
unsafe impl<T: Send> Sync for PollMutex<T> {}

impl<T> Clone for PollMutex<T> {
    fn clone(&self) -> Self {
        Self {
            mutex: self.mutex.clone(),
            data: self.data.clone(),
            future: None,
        }
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
