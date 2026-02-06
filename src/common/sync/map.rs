use std::collections::HashMap;
use std::sync::Arc;

use rand::RngCore;
use rand::rngs::ThreadRng;
use tokio::sync::Mutex;

pub struct AsyncMap<T> {
    inner: Arc<Mutex<HashMap<u64, T>>>,
}

impl<T> AsyncMap<T> {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn insert(&mut self, item: T) -> u64 {
        let mut map = self.inner.lock().await;
        let vacant_id = generate_vacant_id(&map);
        map.insert(vacant_id, item);
        vacant_id
    }

    pub async fn contains(&mut self, id: &u64) -> bool {
        self.inner.lock().await.contains_key(id)
    }

    pub async fn remove(&mut self, id: u64) -> Option<T> {
        self.inner.lock().await.remove(&id)
    }
}

impl<T> Clone for AsyncMap<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

fn generate_vacant_id<T>(map: &HashMap<u64, T>) -> u64 {
    let mut rng = ThreadRng::default();
    loop {
        let id = rng.next_u64();
        if id > 0 && !map.contains_key(&id) {
            return id;
        }
    }
}

// Disabled PollMap code, for reference purposes.
#[cfg(any())]
mod poll_map {
    use std::collections::HashMap;
    use std::hash::Hash;
    use std::task::{Context, Poll, Waker, ready};

    use crate::common::sync::PollMutex;

    #[derive(Clone)]
    pub struct PollMap<K, V> {
        inner: PollMutex<InnerMap<K, V>>,
    }

    #[derive(Clone)]
    struct InnerMap<K, V> {
        data: HashMap<K, V>,
        wakers: HashMap<K, Vec<Waker>>, // Multiple tasks might wait on the same ID
    }

    impl<K: Eq + Hash + Clone, V: Clone> PollMap<K, V> {
        pub fn new() -> Self {
            Self {
                inner: PollMutex::new(InnerMap {
                    data: HashMap::new(),
                    wakers: HashMap::new(),
                }),
            }
        }

        /// See description for `fn insert_cloned()`.
        pub fn poll_insert_cloned(&mut self, cx: &mut Context, id: &K, value: &V) -> Poll<()> {
            let mut inner = ready!(self.inner.poll_lock(cx));

            inner.data.insert(id.clone(), value.clone());

            // Notify all tasks waiting for this ID
            if let Some(wakers) = inner.wakers.remove(&id) {
                for waker in wakers {
                    waker.wake();
                }
            }

            Poll::Ready(())
        }

        /// Inserts a value and wakes up any tasks waiting for the given ID.
        ///
        /// Equivalent to:
        ///   `async fn insert_cloned(&mut self, id: &K, value: &V) -> ()`
        pub fn insert_cloned(&mut self, id: &K, value: &V) -> impl Future<Output = ()> {
            std::future::poll_fn(move |cx| self.poll_insert_cloned(cx, id, value))
        }

        /// See description for `fn get()`.
        pub fn poll_get(&mut self, cx: &mut Context, id: &K) -> Poll<Option<V>> {
            let mut inner = ready!(self.inner.poll_lock(cx));
            Poll::Ready(inner.data.get(id).map(|v| v.clone()))
        }

        /// Gets a clone of the value at the given ID if it exists.
        ///
        /// Equivalent to:
        ///   `async fn get_wait(&mut self, id: &K) -> Option<V>`
        pub fn get(&mut self, id: &K) -> impl Future<Output = Option<V>> {
            std::future::poll_fn(move |cx| self.poll_get(cx, id))
        }

        /// See description for `fn get_wait()`.
        pub fn poll_get_wait(&mut self, cx: &mut Context, id: &K) -> Poll<V> {
            let mut inner = ready!(self.inner.poll_lock(cx));

            if let Some(val) = inner.data.get(id) {
                return Poll::Ready(val.clone());
            }

            // Key not found: register the current task's waker to be notified on insert.
            inner
                .wakers
                .entry(id.clone())
                .or_insert_with(Vec::new)
                .push(cx.waker().clone());

            Poll::Pending
        }

        /// Gets a clone of the value at the given ID, waiting until it is inserted.
        ///
        /// Use `tokio::time::timeout` if you want to cancel a wait after a timeout:
        ///   `tokio::time::timeout(duration, pollmap.get_wait(id))`
        ///
        /// Equivalent to:
        ///   `async fn get_wait(&mut self, id: &K) -> V`
        pub fn get_wait(&mut self, id: &K) -> impl Future<Output = V> {
            std::future::poll_fn(move |cx| self.poll_get_wait(cx, id))
        }

        /// See description for `fn remove()`.
        pub fn poll_remove(&mut self, cx: &mut Context, id: &K) -> Poll<Option<V>> {
            let mut inner = ready!(self.inner.poll_lock(cx));
            inner.wakers.remove(id);
            Poll::Ready(inner.data.remove(id))
        }

        /// Tries to remove a value at the given ID and returns it if it exists.
        ///
        /// Equivalent to:
        ///   `async fn remove(&mut self, id: &K) -> Option<V>`
        pub fn remove(&mut self, id: &K) -> impl Future<Output = Option<V>> {
            std::future::poll_fn(move |cx| self.poll_remove(cx, id))
        }
    }

    #[cfg(test)]
    mod tests {
        use std::time::Duration;

        use crate::common::sync::PollMap;

        #[tokio::test]
        async fn insert() {
            let mut map = PollMap::<u64, u64>::new();
            map.insert_cloned(&0, &0).await;
            let inner = map.inner.lock().await;
            assert_eq!(inner.data.get(&0), Some(&0));
        }

        #[tokio::test]
        async fn remove_vacant() {
            let mut map = PollMap::<u64, u64>::new();
            assert_eq!(map.remove(&0).await, None);
        }

        #[tokio::test]
        async fn remove_occupied() {
            let mut map = PollMap::<u64, u64>::new();
            map.insert_cloned(&0, &0).await;
            assert_eq!(map.remove(&0).await, Some(0));
            assert_eq!(map.remove(&0).await, None);
        }

        #[tokio::test]
        async fn get_vacant() {
            let mut map = PollMap::<u64, u64>::new();
            assert_eq!(map.get(&0).await, None);
        }

        #[tokio::test]
        async fn get_occupied() {
            let mut map = PollMap::<u64, u64>::new();
            map.insert_cloned(&0, &0).await;
            assert_eq!(map.get(&0).await, Some(0));
        }

        #[tokio::test]
        async fn get_wait_vacant() {
            let mut map = PollMap::<u64, u64>::new();
            let result = tokio::time::timeout(Duration::from_millis(3), map.get_wait(&0)).await;
            assert!(result.is_err());
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn get_wait_occupied() {
            let mut map = PollMap::<u64, u64>::new();
            let mut map_cloned = map.clone();

            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(10)).await;
                map_cloned.insert_cloned(&0, &0).await;
            });

            let result = tokio::time::timeout(Duration::from_millis(20), map.get_wait(&0)).await;

            assert_eq!(result, Ok(0));
            assert_eq!(map.get(&0).await, Some(0));
        }
    }
}
