//! Single-flight for the response cache: concurrent misses of one key take
//! turns, so the first goes to the provider and the others find its answer.
//!
//! The wait is in this process only; a shared cache store would need its own.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError, Weak};

use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

use super::CacheKey;

type Map = Mutex<HashMap<CacheKey, Weak<AsyncMutex<()>>>>;

/// Serializes concurrent misses of one cache key in this process.
#[derive(Default)]
pub struct Flights {
    map: Arc<Map>,
}

impl Flights {
    pub fn new() -> Self {
        Self::default()
    }

    /// Waits until no other caller holds `key`, then holds it until the
    /// guard is dropped. Entries of dropped guards are removed. Dropping the
    /// returned future while it waits gives up the place and leaves nothing
    /// behind.
    pub async fn hold(&self, key: CacheKey) -> FlightGuard {
        let lock = {
            let mut map = self.map.lock().unwrap_or_else(PoisonError::into_inner);
            match map.get(&key).and_then(Weak::upgrade) {
                Some(lock) => lock,
                None => {
                    let lock = Arc::new(AsyncMutex::new(()));
                    map.insert(key, Arc::downgrade(&lock));
                    lock
                }
            }
        };
        // From here the guard cleans up on every exit, a dropped future too.
        let mut flight = FlightGuard {
            guard: None,
            lock: Some(lock.clone()),
            key,
            map: self.map.clone(),
            waited: false,
        };
        match lock.clone().try_lock_owned() {
            Ok(guard) => flight.guard = Some(guard),
            Err(_) => {
                flight.waited = true;
                flight.guard = Some(lock.lock_owned().await);
            }
        }
        flight
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.map
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The hold of one key; dropping it lets the next caller of the key go.
pub struct FlightGuard {
    guard: Option<OwnedMutexGuard<()>>,
    lock: Option<Arc<AsyncMutex<()>>>,
    key: CacheKey,
    map: Arc<Map>,
    waited: bool,
}

impl FlightGuard {
    /// Whether another caller held the key when this one asked.
    pub fn waited(&self) -> bool {
        self.waited
    }
}

impl Drop for FlightGuard {
    fn drop(&mut self) {
        self.guard.take();
        self.lock.take();
        let mut map = self.map.lock().unwrap_or_else(PoisonError::into_inner);
        // Nobody holds or awaits the lock any more: the entry is dead. A
        // live entry of a newer holder of the same key stays.
        if map.get(&self.key).is_some_and(|w| w.strong_count() == 0) {
            map.remove(&self.key);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use super::*;

    fn key(n: &str) -> CacheKey {
        CacheKey::for_test(n)
    }

    #[tokio::test]
    async fn second_holder_waits_for_the_first() {
        let flights = Arc::new(Flights::new());
        let first = flights.hold(key("a")).await;
        assert!(!first.waited());
        let done = Arc::new(AtomicUsize::new(0));
        let waiter = {
            let (flights, done) = (flights.clone(), done.clone());
            tokio::spawn(async move {
                let g = flights.hold(key("a")).await;
                done.fetch_add(1, Ordering::SeqCst);
                g.waited()
            })
        };
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(done.load(Ordering::SeqCst), 0, "still waiting");
        drop(first);
        let waited = tokio::time::timeout(Duration::from_secs(10), waiter)
            .await
            .unwrap()
            .unwrap();
        assert!(waited);
        assert_eq!(done.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn different_keys_do_not_wait() {
        let flights = Flights::new();
        let _a = flights.hold(key("a")).await;
        let b = tokio::time::timeout(Duration::from_secs(10), flights.hold(key("b")))
            .await
            .unwrap();
        assert!(!b.waited());
    }

    #[tokio::test]
    async fn dropped_guard_releases_and_cleans_up() {
        let flights = Flights::new();
        let g = flights.hold(key("a")).await;
        assert_eq!(flights.len(), 1);
        drop(g);
        assert!(flights.is_empty());
        let again = flights.hold(key("a")).await;
        assert!(!again.waited());
    }

    #[tokio::test]
    async fn cancelled_waiter_does_not_block_others() {
        let flights = Arc::new(Flights::new());
        let first = flights.hold(key("a")).await;
        // A waiter whose future is dropped while it waits.
        let cancelled = tokio::time::timeout(Duration::from_millis(50), flights.hold(key("a")));
        assert!(cancelled.await.is_err());
        let third = {
            let flights = flights.clone();
            tokio::spawn(async move { flights.hold(key("a")).await.waited() })
        };
        tokio::time::sleep(Duration::from_millis(50)).await;
        drop(first);
        let waited = tokio::time::timeout(Duration::from_secs(10), third)
            .await
            .unwrap()
            .unwrap();
        assert!(waited);
        assert_eq!(flights.len(), 0, "nothing leaks");
    }

    #[tokio::test]
    async fn a_panicking_holder_releases_the_key() {
        let flights = Arc::new(Flights::new());
        let holder = {
            let flights = flights.clone();
            tokio::spawn(async move {
                let _guard = flights.hold(key("a")).await;
                tokio::time::sleep(Duration::from_millis(50)).await;
                panic!("the leader's call panicked");
            })
        };
        // A waiter that queued behind it.
        tokio::time::sleep(Duration::from_millis(10)).await;
        let waiter = {
            let flights = flights.clone();
            tokio::spawn(async move { flights.hold(key("a")).await.waited() })
        };
        assert!(holder.await.unwrap_err().is_panic());
        let waited = tokio::time::timeout(Duration::from_secs(10), waiter)
            .await
            .expect("the waiter hangs after the holder panicked")
            .unwrap();
        assert!(waited);
        assert!(flights.is_empty(), "nothing leaks");
        let again = flights.hold(key("a")).await;
        assert!(!again.waited());
    }

    #[tokio::test]
    async fn cancelled_waiter_after_the_holder_left_leaves_no_entry() {
        let flights = Flights::new();
        let first = flights.hold(key("a")).await;
        let mut waiter = Box::pin(flights.hold(key("a")));
        assert!(futures_poll_once(&mut waiter).await.is_none());
        drop(first);
        drop(waiter);
        assert_eq!(flights.len(), 0);
    }

    async fn futures_poll_once<F: std::future::Future + Unpin>(f: &mut F) -> Option<F::Output> {
        std::future::poll_fn(|cx| match std::pin::Pin::new(&mut *f).poll(cx) {
            std::task::Poll::Ready(v) => std::task::Poll::Ready(Some(v)),
            std::task::Poll::Pending => std::task::Poll::Ready(None),
        })
        .await
    }
}
