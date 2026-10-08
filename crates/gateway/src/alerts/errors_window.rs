//! How many calls ended in an error, per subject, over the last minutes.
//!
//! Per subject a ring of 60 one-minute buckets of `(requests, errors)`. The
//! calls themselves feed it from [`Scope::emit`](crate::telemetry::Scope);
//! the engine reads it every 30 seconds. Memory is bounded: subjects beyond
//! the cap of their scope are not tracked, and subjects idle for a full hour are
//! forgotten.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// Buckets in a ring: the longest window there is, in minutes.
pub const BUCKETS: usize = 60;
/// Subjects tracked per scope besides the gateway as a whole. Route and
/// provider names are those of the catalog (a call that names anything else
/// is never counted), so these are safety nets; keys are one per virtual key.
pub const MAX_ROUTES: usize = 5_000;
pub const MAX_PROVIDERS: usize = 1_000;
pub const MAX_KEYS: usize = 10_000;

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
    /// The configured route the call resolved to; none for a direct
    /// `provider/model` call.
    pub route: Option<&'a str>,
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
    routes: HashMap<String, Ring>,
    providers: HashMap<String, Ring>,
    keys: HashMap<i64, Ring>,
    /// Which scopes already warned about their cap.
    warned: [bool; 3],
}

/// Adds to the ring of `name`, making it if there is room.
fn add_named(
    rings: &mut HashMap<String, Ring>,
    cap: usize,
    warned: &mut bool,
    scope: &str,
    name: &str,
    bucket: u32,
    error: bool,
) {
    if let Some(ring) = rings.get_mut(name) {
        ring.add(bucket, error);
    } else if rings.len() < cap {
        let mut ring = Ring::new();
        ring.add(bucket, error);
        rings.insert(name.to_string(), ring);
    } else if !*warned {
        *warned = true;
        tracing::warn!(
            scope,
            limit = cap,
            "error rates are not tracked for more subjects"
        );
    }
}

/// The windows of every subject. Shared by the calls that feed it and the
/// engine that reads it.
pub struct ErrorWindows {
    inner: Mutex<Inner>,
    /// How long a bucket is: a minute, except in tests.
    bucket: Duration,
    origin: Instant,
    /// Whether any enabled error-rate rule exists. While none does, calls
    /// are not counted at all.
    active: AtomicBool,
}

impl ErrorWindows {
    pub fn new(bucket: Duration) -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
            bucket: bucket.max(Duration::from_millis(1)),
            origin: Instant::now(),
            active: AtomicBool::new(true),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // Counters only: usable after a panic elsewhere.
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether calls are counted now.
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    /// Turns counting on or off. Turning it off forgets what was counted: a
    /// rule that comes later starts with empty windows.
    pub fn set_active(&self, active: bool) {
        if self.active.swap(active, Ordering::Relaxed) && !active {
            *self.lock() = Inner::default();
        }
    }

    /// The bucket the clock is in.
    pub fn current(&self) -> u32 {
        let n = self.origin.elapsed().as_nanos() / self.bucket.as_nanos();
        u32::try_from(n).unwrap_or(u32::MAX - 1)
    }

    /// Counts a call now, if counting is on.
    pub fn record(&self, sample: &Sample<'_>) {
        if self.is_active() {
            self.record_at(self.current(), sample);
        }
    }

    pub fn record_at(&self, bucket: u32, sample: &Sample<'_>) {
        let mut inner = self.lock();
        let inner = &mut *inner;
        inner
            .gateway
            .get_or_insert_with(Ring::new)
            .add(bucket, sample.error);
        if let Some(route) = sample.route.filter(|r| !r.is_empty()) {
            add_named(
                &mut inner.routes,
                MAX_ROUTES,
                &mut inner.warned[0],
                "route",
                route,
                bucket,
                sample.error,
            );
        }
        if let Some(provider) = sample.provider {
            add_named(
                &mut inner.providers,
                MAX_PROVIDERS,
                &mut inner.warned[1],
                "provider",
                provider,
                bucket,
                sample.error,
            );
        }
        if let Some(key) = sample.key_id {
            if let Some(ring) = inner.keys.get_mut(&key) {
                ring.add(bucket, sample.error);
            } else if inner.keys.len() < MAX_KEYS {
                let mut ring = Ring::new();
                ring.add(bucket, sample.error);
                inner.keys.insert(key, ring);
            } else if !inner.warned[2] {
                inner.warned[2] = true;
                tracing::warn!(
                    scope = "key",
                    limit = MAX_KEYS,
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
            Scope::Route => inner.routes.get(subject),
            Scope::Provider => inner.providers.get(subject),
            Scope::Key => subject.parse::<i64>().ok().and_then(|k| inner.keys.get(&k)),
        };
        ring.map(|r| r.sum(now, window)).unwrap_or_default()
    }

    /// Every subject of a scope with calls in the window.
    pub fn totals_of(&self, scope: Scope, now: u32, window: u32) -> Vec<(String, Totals)> {
        let inner = self.lock();
        let live = |name: String, ring: &Ring| {
            let t = ring.sum(now, window);
            (t.requests > 0).then_some((name, t))
        };
        match scope {
            Scope::Gateway => inner
                .gateway
                .iter()
                .filter_map(|r| live("gateway".to_string(), r))
                .collect(),
            Scope::Route => inner
                .routes
                .iter()
                .filter_map(|(n, r)| live(n.clone(), r))
                .collect(),
            Scope::Provider => inner
                .providers
                .iter()
                .filter_map(|(n, r)| live(n.clone(), r))
                .collect(),
            Scope::Key => inner
                .keys
                .iter()
                .filter_map(|(k, r)| live(k.to_string(), r))
                .collect(),
        }
    }

    /// Forgets subjects with no call in a whole ring.
    pub fn prune(&self, now: u32) {
        let mut inner = self.lock();
        let idle = |r: &Ring| now.saturating_sub(r.last) >= BUCKETS as u32;
        inner.routes.retain(|_, r| !idle(r));
        inner.providers.retain(|_, r| !idle(r));
        inner.keys.retain(|_, r| !idle(r));
        let sizes = [
            inner.routes.len() < MAX_ROUTES,
            inner.providers.len() < MAX_PROVIDERS,
            inner.keys.len() < MAX_KEYS,
        ];
        for (w, room) in inner.warned.iter_mut().zip(sizes) {
            if room {
                *w = false;
            }
        }
    }

    /// How many subjects are tracked (the gateway not counted).
    pub fn tracked(&self) -> usize {
        let inner = self.lock();
        inner.routes.len() + inner.providers.len() + inner.keys.len()
    }

    /// How many subjects of a scope are tracked.
    pub fn tracked_in(&self, scope: Scope) -> usize {
        let inner = self.lock();
        match scope {
            Scope::Gateway => usize::from(inner.gateway.is_some()),
            Scope::Route => inner.routes.len(),
            Scope::Provider => inner.providers.len(),
            Scope::Key => inner.keys.len(),
        }
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
            route: (!route.is_empty()).then_some(route),
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
    fn each_scope_has_its_own_cap_and_idle_subjects_are_forgotten() {
        let w = windows();
        for i in 0..(MAX_ROUTES + 50) {
            w.record_at(1, &call(&format!("route-{i}"), None, None, false));
        }
        assert_eq!(w.tracked_in(Scope::Route), MAX_ROUTES);
        // A full route scope does not keep providers or keys out.
        w.record_at(1, &call("", Some("openai"), Some(7), true));
        assert_eq!(w.total_of(Scope::Provider, "openai", 1, 5).requests, 1);
        assert_eq!(w.total_of(Scope::Key, "7", 1, 5).requests, 1);
        // The gateway as a whole is still counted.
        assert_eq!(
            w.total_of(Scope::Gateway, "", 1, 5).requests,
            (MAX_ROUTES + 51) as u64
        );
        // A subject that was admitted keeps counting.
        w.record_at(2, &call("route-0", None, None, true));
        assert_eq!(w.total_of(Scope::Route, "route-0", 2, 5).requests, 2);
        assert_eq!(
            w.total_of(Scope::Route, &format!("route-{MAX_ROUTES}"), 2, 5),
            Totals::default()
        );
        // After an hour of silence they are forgotten and new ones fit.
        w.prune(2 + BUCKETS as u32);
        assert_eq!(w.tracked_in(Scope::Route), 0);
        w.record_at(70, &call("fresh", None, None, false));
        assert_eq!(w.tracked_in(Scope::Route), 1);
        for k in 0..(MAX_KEYS as i64 + 5) {
            w.record_at(70, &call("", None, Some(k), false));
        }
        assert_eq!(w.tracked_in(Scope::Key), MAX_KEYS);
    }

    #[test]
    fn nothing_is_counted_while_no_rule_wants_it_and_turning_off_forgets() {
        let w = windows();
        w.set_active(false);
        w.record(&call("chat", Some("p"), Some(1), true));
        assert_eq!(w.total_of(Scope::Gateway, "", w.current(), 5).requests, 0);
        assert_eq!(w.tracked(), 0);
        w.set_active(true);
        w.record(&call("chat", Some("p"), Some(1), true));
        assert_eq!(w.total_of(Scope::Gateway, "", w.current(), 5).requests, 1);
        w.set_active(false);
        w.set_active(true);
        assert_eq!(w.total_of(Scope::Gateway, "", w.current(), 5).requests, 0);
        assert_eq!(w.tracked(), 0);
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
