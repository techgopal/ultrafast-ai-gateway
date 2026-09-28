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

fn record(failures: &mut Failures, key: String, limit: usize, now: Instant) {
    let times = failures.entry(key).or_default();
    times.push_back(now);
    // More than the limit never needs to be remembered.
    while times.len() > limit {
        times.pop_front();
    }
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

    /// True when this email or address has used up its failures.
    pub fn is_blocked(&self, email: &str, addr: IpAddr, now: Instant) -> bool {
        let mut failures = self.lock();
        prune(&mut failures, now);
        let count = |key: String| failures.get(&key).map_or(0, VecDeque::len);
        count(email_key(email)) >= MAX_PER_EMAIL || count(address_key(addr)) >= MAX_PER_ADDRESS
    }

    pub fn record_failure(&self, email: &str, addr: IpAddr, now: Instant) {
        let mut failures = self.lock();
        prune(&mut failures, now);
        record(&mut failures, email_key(email), MAX_PER_EMAIL, now);
        record(&mut failures, address_key(addr), MAX_PER_ADDRESS, now);
    }

    /// Clears the email's failures only: the address keeps its count.
    pub fn record_success(&self, email: &str) {
        self.lock().remove(&email_key(email));
    }

    #[cfg(test)]
    fn entries(&self) -> usize {
        self.lock().len()
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
            // Each failure from its own address, so only the email counts.
            assert!(!limiter.is_blocked(EMAIL, addr(n), t0), "after {n}");
            limiter.record_failure(EMAIL, addr(n), t0);
        }
        assert!(limiter.is_blocked(EMAIL, addr(99), t0));
        assert!(limiter.is_blocked(EMAIL, addr(99), t0 + minutes(14)));
        assert!(!limiter.is_blocked(EMAIL, addr(99), t0 + minutes(15)));
        assert!(!limiter.is_blocked(EMAIL, addr(99), t0 + minutes(16)));
        assert!(!limiter.is_blocked("omar@example.com", addr(99), t0));
    }

    #[test]
    fn the_window_slides() {
        let limiter = LoginLimiter::new();
        let t0 = Instant::now();
        for n in 0..4u8 {
            limiter.record_failure(EMAIL, addr(n), t0);
        }
        limiter.record_failure(EMAIL, addr(4), t0 + minutes(10));
        assert!(limiter.is_blocked(EMAIL, addr(99), t0 + minutes(14)));
        // The first four have left the window; the fifth has not.
        assert!(!limiter.is_blocked(EMAIL, addr(99), t0 + minutes(16)));
        for n in 0..4u8 {
            limiter.record_failure(EMAIL, addr(n), t0 + minutes(16));
        }
        assert!(limiter.is_blocked(EMAIL, addr(99), t0 + minutes(16)));
    }

    #[test]
    fn twenty_failures_block_an_address_across_emails() {
        let limiter = LoginLimiter::new();
        let t0 = Instant::now();
        for n in 0..20 {
            let email = format!("user{n}@example.com");
            assert!(!limiter.is_blocked(&email, addr(1), t0), "after {n}");
            limiter.record_failure(&email, addr(1), t0);
        }
        assert!(limiter.is_blocked("fresh@example.com", addr(1), t0));
        assert!(!limiter.is_blocked("fresh@example.com", addr(2), t0));
        assert!(limiter.is_blocked("fresh@example.com", addr(1), t0 + minutes(14)));
        assert!(!limiter.is_blocked("fresh@example.com", addr(1), t0 + minutes(16)));
    }

    #[test]
    fn success_clears_the_email_but_not_the_address() {
        let limiter = LoginLimiter::new();
        let t0 = Instant::now();
        for n in 0..19 {
            limiter.record_failure(&format!("user{n}@example.com"), addr(1), t0);
        }
        for _ in 0..4 {
            limiter.record_failure(EMAIL, addr(2), t0);
        }
        limiter.record_success(EMAIL);
        for _ in 0..4 {
            limiter.record_failure(EMAIL, addr(2), t0);
        }
        assert!(!limiter.is_blocked(EMAIL, addr(3), t0));
        limiter.record_failure(EMAIL, addr(2), t0);
        assert!(limiter.is_blocked(EMAIL, addr(3), t0));

        // Address 1 has 19 failures; a success does not take any away.
        limiter.record_success("user0@example.com");
        limiter.record_failure("user0@example.com", addr(1), t0);
        assert!(limiter.is_blocked("other@example.com", addr(1), t0));
    }

    #[test]
    fn an_email_and_an_address_with_the_same_text_are_separate() {
        let limiter = LoginLimiter::new();
        let t0 = Instant::now();
        for _ in 0..5 {
            limiter.record_failure("10.0.0.9", addr(1), t0);
        }
        assert!(limiter.is_blocked("10.0.0.9", addr(2), t0));
        assert!(!limiter.is_blocked("other@example.com", addr(9), t0));
    }

    #[test]
    fn the_map_does_not_grow_without_bound() {
        let limiter = LoginLimiter::new();
        let t0 = Instant::now();
        for n in 0..100u8 {
            limiter.record_failure(&format!("user{n}@example.com"), addr(n), t0);
        }
        assert_eq!(limiter.entries(), 200);
        // Repeated failures keep no more than the limit per entry.
        for _ in 0..50 {
            limiter.record_failure(EMAIL, addr(200), t0);
        }
        assert_eq!(limiter.lock()[&email_key(EMAIL)].len(), MAX_PER_EMAIL);
        assert_eq!(
            limiter.lock()[&address_key(addr(200))].len(),
            MAX_PER_ADDRESS
        );

        assert!(!limiter.is_blocked("new@example.com", addr(250), t0 + minutes(16)));
        assert_eq!(limiter.entries(), 0);

        limiter.record_failure(EMAIL, addr(1), t0 + minutes(20));
        limiter.record_failure("late@example.com", addr(2), t0 + minutes(40));
        assert_eq!(limiter.entries(), 2);
    }
}
