//! Limits failed sign-in attempts per email and client address together,
//! and per client address.
//!
//! An email is never locked out as a whole: failures from many addresses
//! count for each of them, so nobody can keep its owner from signing in by
//! failing on purpose. Counts are kept in memory: they are lost on restart
//! and not shared between processes. The map holds at most `MAX_ENTRIES`
//! names, the oldest dropped first, and is pruned by a timer
//! (`LoginLimiter::prune`), not on every attempt.

use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Failures older than this no longer count.
pub const WINDOW: Duration = Duration::from_secs(15 * 60);
/// Failures one email may have from one address inside the window.
pub const MAX_PER_EMAIL: usize = 5;
/// Failures one address may have inside the window.
pub const MAX_PER_ADDRESS: usize = 20;
/// The most names (an email with an address, or an address) kept at once.
pub const MAX_ENTRIES: usize = 100_000;

#[derive(Debug)]
struct Entry {
    times: VecDeque<Instant>,
    /// Which insertion this is, so a name in `order` from before the entry
    /// was dropped and made again does not drop the new one.
    made: u64,
}

#[derive(Debug, Default)]
struct Failures {
    entries: HashMap<String, Entry>,
    /// The names in the order they were made, oldest first, with `made`.
    order: VecDeque<(String, u64)>,
    next: u64,
}

/// Records failed sign-ins. Holds emails and addresses, never a password.
#[derive(Debug)]
pub struct LoginLimiter {
    failures: Mutex<Failures>,
    capacity: usize,
}

impl Default for LoginLimiter {
    fn default() -> Self {
        Self::with_capacity(MAX_ENTRIES)
    }
}

fn pair_key(email: &str, addr: IpAddr) -> String {
    format!("pair:{addr} {email}")
}

fn address_key(addr: IpAddr) -> String {
    format!("addr:{addr}")
}

/// Drops the failures of one entry that left the window.
fn expire(times: &mut VecDeque<Instant>, now: Instant) {
    while times
        .front()
        .is_some_and(|at| now.saturating_duration_since(*at) >= WINDOW)
    {
        times.pop_front();
    }
}

impl Failures {
    /// How many failures the name has inside the window.
    fn count(&mut self, key: &str, now: Instant) -> usize {
        self.entries.get_mut(key).map_or(0, |e| {
            expire(&mut e.times, now);
            e.times.len()
        })
    }

    fn push(&mut self, key: String, now: Instant, capacity: usize) {
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.times.push_back(now);
            return;
        }
        while self.entries.len() >= capacity {
            let Some((old, made)) = self.order.pop_front() else {
                break;
            };
            if self.entries.get(&old).is_some_and(|e| e.made == made) {
                self.entries.remove(&old);
            }
        }
        let made = self.next;
        self.next += 1;
        self.order.push_back((key.clone(), made));
        self.entries.insert(
            key,
            Entry {
                times: VecDeque::from([now]),
                made,
            },
        );
    }
}

impl LoginLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    /// A limiter that keeps at most `capacity` names.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            failures: Mutex::default(),
            capacity: capacity.max(2),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Failures> {
        // The map stays usable if a holder of the lock panicked.
        self.failures
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Checks the limits and counts the attempt in one step, so attempts
    /// made at the same time cannot all pass the check. Returns `false`,
    /// counting nothing, when the email from this address, or the address,
    /// has used up its failures. Otherwise the attempt is counted as a
    /// failure: call `record_success` and `forgive` if it succeeds.
    pub fn try_begin(&self, email: &str, addr: IpAddr, now: Instant) -> bool {
        let mut failures = self.lock();
        let (pair, addr) = (pair_key(email, addr), address_key(addr));
        if failures.count(&pair, now) >= MAX_PER_EMAIL
            || failures.count(&addr, now) >= MAX_PER_ADDRESS
        {
            return false;
        }
        failures.push(pair, now, self.capacity);
        failures.push(addr, now, self.capacity);
        true
    }

    /// Clears the failures of the email from this address: the address
    /// keeps its count.
    pub fn record_success(&self, email: &str, addr: IpAddr) {
        self.lock().entries.remove(&pair_key(email, addr));
    }

    /// Takes back the one attempt that `try_begin` counted for the address.
    pub fn forgive(&self, addr: IpAddr) {
        let mut failures = self.lock();
        let key = address_key(addr);
        if let Some(entry) = failures.entries.get_mut(&key) {
            entry.times.pop_back();
            if entry.times.is_empty() {
                failures.entries.remove(&key);
            }
        }
    }

    /// Drops failures that left the window, and names that have none left.
    /// Called by a timer.
    pub fn prune(&self, now: Instant) {
        let mut failures = self.lock();
        failures.entries.retain(|_, entry| {
            expire(&mut entry.times, now);
            !entry.times.is_empty()
        });
        let Failures { entries, order, .. } = &mut *failures;
        order.retain(|(key, made)| entries.get(key).is_some_and(|e| e.made == *made));
    }

    /// How many names are held.
    pub fn entries(&self) -> usize {
        self.lock().entries.len()
    }

    #[cfg(test)]
    fn count(&self, key: &str) -> usize {
        self.lock().entries.get(key).map_or(0, |e| e.times.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EMAIL: &str = "maya@example.com";

    fn addr(last: u8) -> IpAddr {
        IpAddr::from([10, 0, 0, last])
    }

    fn minutes(n: u64) -> Duration {
        Duration::from_secs(n * 60)
    }

    #[test]
    fn limits_are_the_fixed_values() {
        assert_eq!(WINDOW, minutes(15));
        assert_eq!(MAX_PER_EMAIL, 5);
        assert_eq!(MAX_PER_ADDRESS, 20);
        assert_eq!(MAX_ENTRIES, 100_000);
    }

    #[test]
    fn five_failures_block_an_email_from_that_address_until_the_window_passes() {
        let limiter = LoginLimiter::new();
        let t0 = Instant::now();
        for n in 0..5 {
            assert!(limiter.try_begin(EMAIL, addr(1), t0), "attempt {n}");
        }
        assert!(!limiter.try_begin(EMAIL, addr(1), t0));
        assert!(!limiter.try_begin(EMAIL, addr(1), t0 + minutes(14)));
        // A refused attempt counts nothing.
        assert_eq!(limiter.count(&pair_key(EMAIL, addr(1))), 5);
        assert_eq!(limiter.count(&address_key(addr(1))), 5);
        // Another email from the address, and the email from another address, go on.
        assert!(limiter.try_begin("omar@example.com", addr(1), t0));
        assert!(limiter.try_begin(EMAIL, addr(2), t0));
        assert!(limiter.try_begin(EMAIL, addr(1), t0 + minutes(16)));
    }

    #[test]
    fn many_addresses_do_not_lock_an_email_out() {
        let limiter = LoginLimiter::new();
        let t0 = Instant::now();
        for n in 0..100u8 {
            for _ in 0..5 {
                assert!(limiter.try_begin(EMAIL, addr(n), t0));
            }
            assert!(!limiter.try_begin(EMAIL, addr(n), t0));
        }
        assert!(limiter.try_begin(EMAIL, addr(200), t0));
    }

    #[test]
    fn the_window_ends_at_fifteen_minutes() {
        let limiter = LoginLimiter::new();
        let t0 = Instant::now();
        for _ in 0..5 {
            assert!(limiter.try_begin(EMAIL, addr(1), t0));
        }
        assert!(!limiter.try_begin(EMAIL, addr(1), t0 + minutes(15) - Duration::from_secs(1)));
        assert!(limiter.try_begin(EMAIL, addr(1), t0 + minutes(15)));
    }

    #[test]
    fn the_window_slides() {
        let limiter = LoginLimiter::new();
        let t0 = Instant::now();
        for _ in 0..4 {
            assert!(limiter.try_begin(EMAIL, addr(1), t0));
        }
        assert!(limiter.try_begin(EMAIL, addr(1), t0 + minutes(10)));
        assert!(!limiter.try_begin(EMAIL, addr(1), t0 + minutes(14)));
        // The first four have left the window; the fifth has not.
        for _ in 0..4 {
            assert!(limiter.try_begin(EMAIL, addr(1), t0 + minutes(16)));
        }
        assert!(!limiter.try_begin(EMAIL, addr(1), t0 + minutes(16)));
    }

    #[test]
    fn twenty_failures_block_an_address_across_emails() {
        let limiter = LoginLimiter::new();
        let t0 = Instant::now();
        for n in 0..20 {
            let email = format!("user{n}@example.com");
            assert!(limiter.try_begin(&email, addr(1), t0), "attempt {n}");
        }
        assert!(!limiter.try_begin("fresh@example.com", addr(1), t0));
        assert!(!limiter.try_begin("fresh@example.com", addr(1), t0 + minutes(14)));
        // Refused attempts counted nothing for the email.
        assert_eq!(limiter.count(&pair_key("fresh@example.com", addr(1))), 0);
        assert!(limiter.try_begin("fresh@example.com", addr(2), t0));
        assert!(limiter.try_begin("fresh@example.com", addr(1), t0 + minutes(16)));
    }

    #[test]
    fn success_clears_the_email_from_that_address_but_not_the_address() {
        let limiter = LoginLimiter::new();
        let t0 = Instant::now();
        for _ in 0..4 {
            assert!(limiter.try_begin(EMAIL, addr(2), t0));
            assert!(limiter.try_begin(EMAIL, addr(3), t0));
        }
        limiter.record_success(EMAIL, addr(2));
        assert_eq!(limiter.count(&pair_key(EMAIL, addr(2))), 0);
        assert_eq!(limiter.count(&pair_key(EMAIL, addr(3))), 4);
        assert_eq!(limiter.count(&address_key(addr(2))), 4);
        for _ in 0..5 {
            assert!(limiter.try_begin(EMAIL, addr(2), t0));
        }
        assert!(!limiter.try_begin(EMAIL, addr(2), t0));
    }

    #[test]
    fn a_success_after_four_failures_costs_nothing() {
        let limiter = LoginLimiter::new();
        let t0 = Instant::now();
        for n in 0..19 {
            assert!(limiter.try_begin(&format!("user{n}@example.com"), addr(1), t0));
        }
        let quiet = LoginLimiter::new();
        for _ in 0..4 {
            assert!(quiet.try_begin(EMAIL, addr(7), t0));
        }
        // The fifth attempt is the right password.
        assert!(quiet.try_begin(EMAIL, addr(7), t0));
        quiet.record_success(EMAIL, addr(7));
        quiet.forgive(addr(7));
        assert_eq!(quiet.count(&pair_key(EMAIL, addr(7))), 0);
        assert_eq!(quiet.count(&address_key(addr(7))), 4);

        // On an address with one slot left, successes do not take it.
        for _ in 0..3 {
            assert!(limiter.try_begin(EMAIL, addr(1), t0));
            limiter.record_success(EMAIL, addr(1));
            limiter.forgive(addr(1));
        }
        assert_eq!(limiter.count(&address_key(addr(1))), 19);
        assert_eq!(limiter.count(&pair_key(EMAIL, addr(1))), 0);
        assert!(limiter.try_begin("last@example.com", addr(1), t0));
        assert!(!limiter.try_begin(EMAIL, addr(1), t0));
    }

    #[test]
    fn forgive_removes_one_attempt_and_empty_entries() {
        let limiter = LoginLimiter::new();
        let t0 = Instant::now();
        limiter.forgive(addr(1));
        assert_eq!(limiter.entries(), 0);
        assert!(limiter.try_begin(EMAIL, addr(1), t0));
        limiter.record_success(EMAIL, addr(1));
        limiter.forgive(addr(1));
        assert_eq!(limiter.entries(), 0);
    }

    #[test]
    fn attempts_at_the_same_time_cannot_pass_the_limit() {
        let limiter = std::sync::Arc::new(LoginLimiter::new());
        let t0 = Instant::now();
        let threads: Vec<_> = (0..32)
            .map(|_| {
                let limiter = limiter.clone();
                std::thread::spawn(move || limiter.try_begin(EMAIL, addr(1), t0))
            })
            .collect();
        let allowed = threads
            .into_iter()
            .map(|t| t.join().unwrap())
            .filter(|ok| *ok)
            .count();
        assert_eq!(allowed, MAX_PER_EMAIL);
    }

    #[test]
    fn an_email_and_an_address_with_the_same_text_are_separate() {
        let limiter = LoginLimiter::new();
        let t0 = Instant::now();
        for _ in 0..5 {
            assert!(limiter.try_begin("10.0.0.1", addr(1), t0));
        }
        assert!(!limiter.try_begin("10.0.0.1", addr(1), t0));
        assert!(limiter.try_begin("other@example.com", addr(1), t0));
    }

    #[test]
    fn old_failures_go_when_the_timer_prunes_not_on_an_attempt() {
        let limiter = LoginLimiter::new();
        let t0 = Instant::now();
        for n in 0..100u8 {
            assert!(limiter.try_begin(&format!("user{n}@example.com"), addr(n), t0));
        }
        assert_eq!(limiter.entries(), 200);
        // Repeated attempts keep no more than the limit per entry.
        for _ in 0..50 {
            limiter.try_begin(EMAIL, addr(200), t0);
        }
        assert_eq!(limiter.count(&pair_key(EMAIL, addr(200))), MAX_PER_EMAIL);
        assert_eq!(limiter.count(&address_key(addr(200))), MAX_PER_EMAIL);
        // An attempt later on leaves the other names alone.
        assert!(limiter.try_begin("late@example.com", addr(250), t0 + minutes(16)));
        assert_eq!(limiter.entries(), 204);
        limiter.prune(t0 + minutes(16));
        assert_eq!(limiter.entries(), 2);
        limiter.prune(t0 + minutes(40));
        assert_eq!(limiter.entries(), 0);
    }

    #[test]
    fn the_map_holds_at_most_its_capacity_and_drops_the_oldest() {
        let limiter = LoginLimiter::with_capacity(10);
        let t0 = Instant::now();
        // The first address fails five times for one email: two names.
        for _ in 0..5 {
            assert!(limiter.try_begin(EMAIL, addr(1), t0));
        }
        assert!(!limiter.try_begin(EMAIL, addr(1), t0));
        for n in 2..=60u8 {
            assert!(limiter.try_begin(EMAIL, addr(n), t0 + Duration::from_secs(u64::from(n))));
            assert!(limiter.entries() <= 10);
        }
        // The oldest names were dropped: the first address may try again.
        assert!(limiter.try_begin(EMAIL, addr(1), t0 + minutes(1)));
        // The newest are kept.
        assert_eq!(limiter.count(&address_key(addr(60))), 1);
    }

    #[test]
    fn a_name_made_again_after_a_prune_is_not_dropped_for_its_old_place() {
        let limiter = LoginLimiter::with_capacity(4);
        let t0 = Instant::now();
        assert!(limiter.try_begin(EMAIL, addr(1), t0));
        limiter.prune(t0 + minutes(16));
        assert_eq!(limiter.entries(), 0);
        let t1 = t0 + minutes(17);
        assert!(limiter.try_begin("a@example.com", addr(2), t1));
        assert!(limiter.try_begin(EMAIL, addr(1), t1));
        assert_eq!(limiter.entries(), 4);
        // Room is made by the oldest of what is held: the names of address 2.
        assert!(limiter.try_begin("b@example.com", addr(3), t1));
        assert_eq!(limiter.count(&pair_key(EMAIL, addr(1))), 1);
        assert_eq!(limiter.count(&address_key(addr(2))), 0);
    }
}
