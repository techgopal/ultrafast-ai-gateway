//! Limits failed sign-in attempts per email and per client address.
//!
//! Counts are kept in memory: they are lost on restart and not shared
//! between processes.

use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Failures older than this no longer count.
pub const WINDOW: Duration = Duration::from_secs(15 * 60);
/// Failures one email may have inside the window.
pub const MAX_PER_EMAIL: usize = 5;
/// Failures one address may have inside the window.
pub const MAX_PER_ADDRESS: usize = 20;

type Failures = HashMap<String, VecDeque<Instant>>;

/// Records failed sign-ins. Holds emails and addresses, never a password.
#[derive(Debug, Default)]
pub struct LoginLimiter {
    failures: Mutex<Failures>,
}

fn email_key(email: &str) -> String {
    format!("email:{email}")
}

fn address_key(addr: IpAddr) -> String {
    format!("addr:{addr}")
}

/// Drops failures that left the window, and entries that have none left.
fn prune(failures: &mut Failures, now: Instant) {
    failures.retain(|_, times| {
        while times
            .front()
            .is_some_and(|at| now.saturating_duration_since(*at) >= WINDOW)
        {
            times.pop_front();
        }
        !times.is_empty()
    });
}

impl LoginLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, Failures> {
        // The map stays usable if a holder of the lock panicked.
        self.failures
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Checks the limits and counts the attempt in one step, so attempts
    /// made at the same time cannot all pass the check. Returns `false`,
    /// counting nothing, when the email or the address has used up its
    /// failures. Otherwise the attempt is counted as a failure: call
    /// `record_success` and `forgive` if it turns out to succeed.
    pub fn try_begin(&self, email: &str, addr: IpAddr, now: Instant) -> bool {
        let mut failures = self.lock();
        prune(&mut failures, now);
        let (email, addr) = (email_key(email), address_key(addr));
        let count = |key: &str| failures.get(key).map_or(0, VecDeque::len);
        if count(&email) >= MAX_PER_EMAIL || count(&addr) >= MAX_PER_ADDRESS {
            return false;
        }
        failures.entry(email).or_default().push_back(now);
        failures.entry(addr).or_default().push_back(now);
        true
    }

    /// Clears the email's failures only: the address keeps its count.
    pub fn record_success(&self, email: &str) {
        self.lock().remove(&email_key(email));
    }

    /// Takes back the one attempt that `try_begin` counted for the address.
    pub fn forgive(&self, addr: IpAddr, now: Instant) {
        let mut failures = self.lock();
        let key = address_key(addr);
        if let Some(times) = failures.get_mut(&key) {
            times.pop_back();
        }
        prune(&mut failures, now);
    }

    #[cfg(test)]
    fn entries(&self) -> usize {
        self.lock().len()
    }

    #[cfg(test)]
    fn count(&self, key: &str) -> usize {
        self.lock().get(key).map_or(0, VecDeque::len)
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
    }

    #[test]
    fn five_failures_block_an_email_until_the_window_passes() {
        let limiter = LoginLimiter::new();
        let t0 = Instant::now();
        for n in 0..5u8 {
            // Each attempt from its own address, so only the email counts.
            assert!(limiter.try_begin(EMAIL, addr(n), t0), "attempt {n}");
        }
        assert!(!limiter.try_begin(EMAIL, addr(99), t0));
        assert!(!limiter.try_begin(EMAIL, addr(99), t0 + minutes(14)));
        // A refused attempt counts nothing, for the email or the address.
        assert_eq!(limiter.count(&email_key(EMAIL)), 5);
        assert_eq!(limiter.count(&address_key(addr(99))), 0);
        assert!(limiter.try_begin("omar@example.com", addr(99), t0));
        assert!(limiter.try_begin(EMAIL, addr(99), t0 + minutes(16)));
    }

    #[test]
    fn the_window_ends_at_fifteen_minutes() {
        let limiter = LoginLimiter::new();
        let t0 = Instant::now();
        for n in 0..5u8 {
            assert!(limiter.try_begin(EMAIL, addr(n), t0));
        }
        assert!(!limiter.try_begin(EMAIL, addr(9), t0 + minutes(15) - Duration::from_secs(1)));
        assert!(limiter.try_begin(EMAIL, addr(9), t0 + minutes(15)));
    }

    #[test]
    fn the_window_slides() {
        let limiter = LoginLimiter::new();
        let t0 = Instant::now();
        for n in 0..4u8 {
            assert!(limiter.try_begin(EMAIL, addr(n), t0));
        }
        assert!(limiter.try_begin(EMAIL, addr(4), t0 + minutes(10)));
        assert!(!limiter.try_begin(EMAIL, addr(99), t0 + minutes(14)));
        // The first four have left the window; the fifth has not.
        for n in 0..4u8 {
            assert!(limiter.try_begin(EMAIL, addr(n), t0 + minutes(16)));
        }
        assert!(!limiter.try_begin(EMAIL, addr(99), t0 + minutes(16)));
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
        assert_eq!(limiter.count(&email_key("fresh@example.com")), 0);
        assert!(limiter.try_begin("fresh@example.com", addr(2), t0));
        assert!(limiter.try_begin("fresh@example.com", addr(1), t0 + minutes(16)));
    }

    #[test]
    fn success_clears_the_email_but_not_the_address() {
        let limiter = LoginLimiter::new();
        let t0 = Instant::now();
        for _ in 0..4 {
            assert!(limiter.try_begin(EMAIL, addr(2), t0));
        }
        limiter.record_success(EMAIL);
        assert_eq!(limiter.count(&email_key(EMAIL)), 0);
        assert_eq!(limiter.count(&address_key(addr(2))), 4);
        for _ in 0..5 {
            assert!(limiter.try_begin(EMAIL, addr(2), t0));
        }
        assert!(!limiter.try_begin(EMAIL, addr(3), t0));
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
        quiet.record_success(EMAIL);
        quiet.forgive(addr(7), t0);
        assert_eq!(quiet.count(&email_key(EMAIL)), 0);
        assert_eq!(quiet.count(&address_key(addr(7))), 4);

        // On an address with one slot left, successes do not take it.
        for _ in 0..3 {
            assert!(limiter.try_begin(EMAIL, addr(1), t0));
            limiter.record_success(EMAIL);
            limiter.forgive(addr(1), t0);
        }
        assert_eq!(limiter.count(&address_key(addr(1))), 19);
        assert_eq!(limiter.count(&email_key(EMAIL)), 0);
        assert!(limiter.try_begin("last@example.com", addr(1), t0));
        assert!(!limiter.try_begin(EMAIL, addr(1), t0));
    }

    #[test]
    fn forgive_removes_one_attempt_and_empty_entries() {
        let limiter = LoginLimiter::new();
        let t0 = Instant::now();
        limiter.forgive(addr(1), t0);
        assert_eq!(limiter.entries(), 0);
        assert!(limiter.try_begin(EMAIL, addr(1), t0));
        limiter.record_success(EMAIL);
        limiter.forgive(addr(1), t0);
        assert_eq!(limiter.entries(), 0);
    }

    #[test]
    fn attempts_at_the_same_time_cannot_pass_the_limit() {
        let limiter = std::sync::Arc::new(LoginLimiter::new());
        let t0 = Instant::now();
        let threads: Vec<_> = (0..32u8)
            .map(|n| {
                let limiter = limiter.clone();
                std::thread::spawn(move || limiter.try_begin(EMAIL, addr(n), t0))
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
            assert!(limiter.try_begin("10.0.0.9", addr(1), t0));
        }
        assert!(!limiter.try_begin("10.0.0.9", addr(2), t0));
        assert!(limiter.try_begin("other@example.com", addr(9), t0));
    }

    #[test]
    fn the_map_does_not_grow_without_bound() {
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
        assert_eq!(limiter.count(&email_key(EMAIL)), MAX_PER_EMAIL);
        assert_eq!(limiter.count(&address_key(addr(200))), MAX_PER_EMAIL);

        limiter.forgive(addr(250), t0 + minutes(16));
        assert_eq!(limiter.entries(), 0);

        assert!(limiter.try_begin(EMAIL, addr(1), t0 + minutes(20)));
        assert!(limiter.try_begin("late@example.com", addr(2), t0 + minutes(40)));
        assert_eq!(limiter.entries(), 2);
    }
}
