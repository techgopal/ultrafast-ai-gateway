//! How many calls ended in an error, per subject, over the last minutes.
//!
//! Per subject a ring of 60 one-minute buckets of `(requests, errors)`. The
//! calls themselves feed it from [`Scope::emit`](crate::telemetry::Scope);
//! the engine reads it every 30 seconds. Memory is bounded: subjects beyond
//! [`MAX_SUBJECTS`] are not tracked, and subjects idle for a full hour are
//! forgotten.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// Buckets in a ring: the longest window there is, in minutes.
pub const BUCKETS: usize = 60;
/// Subjects tracked besides the gateway as a whole.
pub const MAX_SUBJECTS: usize = 10_000;

/// What a window counts calls by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    Gateway,
    Route,
    Provider,
    Key,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Gateway => "gateway",
            Scope::Route => "route",
            Scope::Provider => "provider",
            Scope::Key => "key",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "gateway" => Some(Scope::Gateway),
            "route" => Some(Scope::Route),
            "provider" => Some(Scope::Provider),
            "key" => Some(Scope::Key),
            _ => None,
        }
    }
}

/// One finished call, as the windows see it.
#[derive(Debug, Clone, Copy)]
pub struct Sample<'a> {
    /// The route or model name the caller asked for; empty if unknown.
    pub route: &'a str,
    /// The provider of the final attempt, if a provider was called.
    pub provider: Option<&'a str>,
    pub key_id: Option<i64>,
    pub error: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Totals {
    pub requests: u64,
    pub errors: u64,
}

impl Totals {
    /// Whether `requests >= min_requests` and the error share is at least
    /// `percent`.
    pub fn breaches(&self, percent: u8, min_requests: u32) -> bool {
        self.requests >= u64::from(min_requests)
            && self.requests > 0
            && self.errors * 100 >= u64::from(percent) * self.requests
    }

    /// The error share, in percent, for a summary.
    pub fn percent(&self) -> f64 {
        if self.requests == 0 {
            0.0
        } else {
            self.errors as f64 * 100.0 / self.requests as f64
        }
    }
}

#[derive(Clone, Copy)]
struct Slot {
    bucket: u32,
    requests: u32,
    errors: u32,
}

const EMPTY: Slot = Slot {
    bucket: u32::MAX,
    requests: 0,
    errors: 0,
};

struct Ring {
    slots: [Slot; BUCKETS],
    /// The newest bucket written.
    last: u32,
}

impl Ring {
    fn new() -> Self {
        Self {
            slots: [EMPTY; BUCKETS],
            last: 0,
        }
    }

    fn add(&mut self, bucket: u32, error: bool) {
        let slot = &mut self.slots[bucket as usize % BUCKETS];
        if slot.bucket != bucket {
            // A bucket from an older lap, or a call that took more than an
            // hour to end: the slot belongs to the newer one.
            if slot.bucket != u32::MAX && slot.bucket > bucket {
                return;
            }
            *slot = Slot {
                bucket,
                requests: 0,
                errors: 0,
            };
        }
        slot.requests = slot.requests.saturating_add(1);
        slot.errors = slot.errors.saturating_add(u32::from(error));
        self.last = self.last.max(bucket);
    }

    fn sum(&self, now: u32, window: u32) -> Totals {
        let window = window.clamp(1, BUCKETS as u32);
        let first = now.saturating_sub(window - 1);
        let mut totals = Totals::default();
        for b in first..=now {
            let slot = &self.slots[b as usize % BUCKETS];
            if slot.bucket == b {
                totals.requests += u64::from(slot.requests);
                totals.errors += u64::from(slot.errors);
            }
        }
        totals
    }
}

#[derive(Default)]
struct Inner {
    gateway: Option<Ring>,
    /// Every other subject.
    rings: HashMap<(Scope, String), Ring>,
    warned: bool,
}

/// The windows of every subject. Shared by the calls that feed it and the
/// engine that reads it.
pub struct ErrorWindows {
    inner: Mutex<Inner>,
    /// How long a bucket is: a minute, except in tests.
    bucket: Duration,
    origin: Instant,
}

impl ErrorWindows {
    pub fn new(bucket: Duration) -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
            bucket: bucket.max(Duration::from_millis(1)),
            origin: Instant::now(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // Counters only: usable after a panic elsewhere.
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The bucket the clock is in.
    pub fn current(&self) -> u32 {
        let n = self.origin.elapsed().as_nanos() / self.bucket.as_nanos();
        u32::try_from(n).unwrap_or(u32::MAX - 1)
    }

    pub fn record(&self, sample: &Sample<'_>) {
        self.record_at(self.current(), sample);
    }

    pub fn record_at(&self, bucket: u32, sample: &Sample<'_>) {
        let mut inner = self.lock();
        inner
            .gateway
            .get_or_insert_with(Ring::new)
            .add(bucket, sample.error);
        let route = (!sample.route.is_empty()).then_some((Scope::Route, sample.route));
        let provider = sample.provider.map(|p| (Scope::Provider, p));
        let key = sample.key_id.map(|k| (Scope::Key, k.to_string()));
        let key_ref = key.as_ref().map(|(s, k)| (*s, k.as_str()));
        for (scope, subject) in [route, provider, key_ref].into_iter().flatten() {
            let id = (scope, subject.to_string());
            if let Some(ring) = inner.rings.get_mut(&id) {
                ring.add(bucket, sample.error);
            } else if inner.rings.len() < MAX_SUBJECTS {
                let mut ring = Ring::new();
                ring.add(bucket, sample.error);
                inner.rings.insert(id, ring);
            } else if !inner.warned {
                inner.warned = true;
                tracing::warn!(
                    limit = MAX_SUBJECTS,
                    "error rates are not tracked for more subjects"
                );
            }
        }
    }

    /// The totals of one subject over the last `window` buckets up to and
    /// including `now`. `subject` is ignored for the gateway.
    pub fn total_of(&self, scope: Scope, subject: &str, now: u32, window: u32) -> Totals {
        let inner = self.lock();
        let ring = match scope {
            Scope::Gateway => inner.gateway.as_ref(),
            _ => inner.rings.get(&(scope, subject.to_string())),
        };
        ring.map(|r| r.sum(now, window)).unwrap_or_default()
    }

    /// Every subject of a scope with calls in the window.
    pub fn totals_of(&self, scope: Scope, now: u32, window: u32) -> Vec<(String, Totals)> {
        let inner = self.lock();
        if scope == Scope::Gateway {
            return inner
                .gateway
                .as_ref()
                .map(|r| r.sum(now, window))
                .filter(|t| t.requests > 0)
                .map(|t| vec![("gateway".to_string(), t)])
                .unwrap_or_default();
        }
        inner
            .rings
            .iter()
            .filter(|((s, _), _)| *s == scope)
            .map(|((_, subject), ring)| (subject.clone(), ring.sum(now, window)))
            .filter(|(_, t)| t.requests > 0)
            .collect()
    }

    /// Forgets subjects with no call in a whole ring.
    pub fn prune(&self, now: u32) {
        let mut inner = self.lock();
        inner
            .rings
            .retain(|_, ring| now.saturating_sub(ring.last) < BUCKETS as u32);
        if inner.rings.len() < MAX_SUBJECTS {
            inner.warned = false;
        }
    }

    /// How many subjects are tracked (the gateway not counted).
    pub fn tracked(&self) -> usize {
        self.lock().rings.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn windows() -> ErrorWindows {
        ErrorWindows::new(Duration::from_secs(60))
    }

    fn call<'a>(
        route: &'a str,
        provider: Option<&'a str>,
        key: Option<i64>,
        error: bool,
    ) -> Sample<'a> {
        Sample {
            route,
            provider,
            key_id: key,
            error,
        }
    }

    #[test]
    fn a_call_counts_for_the_gateway_its_route_provider_and_key() {
        let w = windows();
        w.record_at(10, &call("chat", Some("openai"), Some(7), true));
        w.record_at(10, &call("chat", Some("openai"), Some(7), false));
        let want = Totals {
            requests: 2,
            errors: 1,
        };
        assert_eq!(w.total_of(Scope::Gateway, "", 10, 5), want);
        assert_eq!(w.total_of(Scope::Route, "chat", 10, 5), want);
        assert_eq!(w.total_of(Scope::Provider, "openai", 10, 5), want);
        assert_eq!(w.total_of(Scope::Key, "7", 10, 5), want);
        assert_eq!(w.total_of(Scope::Route, "other", 10, 5), Totals::default());
    }

    #[test]
    fn a_call_without_a_route_provider_or_key_counts_for_the_gateway_only() {
        let w = windows();
        w.record_at(1, &call("", None, None, true));
        assert_eq!(w.total_of(Scope::Gateway, "", 1, 5).requests, 1);
        assert_eq!(w.tracked(), 0);
    }

    #[test]
    fn the_window_sums_its_last_buckets_and_no_others() {
        let w = windows();
        for (bucket, n) in [(10, 1), (11, 2), (12, 3), (13, 4), (14, 5), (15, 6)] {
            for _ in 0..n {
                w.record_at(bucket, &call("r", None, None, bucket % 2 == 0));
            }
        }
        // Buckets 11..=15 are the window of 5 at 15.
        let t = w.total_of(Scope::Gateway, "", 15, 5);
        assert_eq!(t.requests, 2 + 3 + 4 + 5 + 6);
        assert_eq!(t.errors, 3 + 5);
        // At 12 the window reaches back before the first bucket: no wrap.
        assert_eq!(w.total_of(Scope::Gateway, "", 12, 5).requests, 1 + 2 + 3);
    }

    #[test]
    fn a_ring_rolls_over_and_forgets_the_old_lap() {
        let w = windows();
        w.record_at(3, &call("r", None, None, true));
        // 60 buckets later the same slot holds a new bucket.
        w.record_at(63, &call("r", None, None, false));
        let t = w.total_of(Scope::Gateway, "", 63, 60);
        assert_eq!(
            t,
            Totals {
                requests: 1,
                errors: 0
            }
        );
        // A call that ends very late does not overwrite a newer bucket.
        w.record_at(3, &call("r", None, None, true));
        assert_eq!(w.total_of(Scope::Gateway, "", 63, 60).requests, 1);
    }

    #[test]
    fn a_window_of_60_sees_every_bucket_of_the_ring() {
        let w = windows();
        for b in 100..160 {
            w.record_at(b, &call("r", None, None, false));
        }
        assert_eq!(w.total_of(Scope::Gateway, "", 159, 60).requests, 60);
        assert_eq!(w.total_of(Scope::Gateway, "", 159, 5).requests, 5);
    }

    #[test]
    fn subjects_beyond_the_cap_are_not_tracked_and_idle_ones_are_forgotten() {
        let w = windows();
        for i in 0..(MAX_SUBJECTS + 50) {
            w.record_at(1, &call(&format!("route-{i}"), None, None, false));
        }
        assert_eq!(w.tracked(), MAX_SUBJECTS);
        // The gateway as a whole is still counted.
        assert_eq!(
            w.total_of(Scope::Gateway, "", 1, 5).requests,
            (MAX_SUBJECTS + 50) as u64
        );
        // A subject that was admitted keeps counting.
        w.record_at(2, &call("route-0", None, None, true));
        assert_eq!(w.total_of(Scope::Route, "route-0", 2, 5).requests, 2);
        assert_eq!(
            w.total_of(Scope::Route, &format!("route-{MAX_SUBJECTS}"), 2, 5),
            Totals::default()
        );
        // After an hour of silence they are forgotten and new ones fit.
        w.prune(2 + BUCKETS as u32);
        assert_eq!(w.tracked(), 0);
        w.record_at(70, &call("fresh", None, None, false));
        assert_eq!(w.tracked(), 1);
    }

    #[test]
    fn totals_of_lists_the_subjects_with_calls() {
        let w = windows();
        w.record_at(5, &call("a", None, None, true));
        w.record_at(5, &call("b", None, None, false));
        let mut v = w.totals_of(Scope::Route, 5, 5);
        v.sort_by(|x, y| x.0.cmp(&y.0));
        let names: Vec<_> = v.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["a", "b"]);
        assert!(w.totals_of(Scope::Route, 500, 5).is_empty());
        assert_eq!(w.totals_of(Scope::Gateway, 5, 5).len(), 1);
    }

    #[test]
    fn breaches_needs_the_minimum_and_the_percent() {
        let t = Totals {
            requests: 20,
            errors: 2,
        };
        assert!(t.breaches(10, 20));
        assert!(!t.breaches(11, 20));
        assert!(!t.breaches(10, 21), "too few requests");
        assert!(!Totals::default().breaches(1, 1));
    }
}
