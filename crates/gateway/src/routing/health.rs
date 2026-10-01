//! What is known of the health of each target.
//!
//! The trait is the seam for a store shared between gateway instances; the
//! default keeps it in memory.

use std::collections::HashMap;
use std::sync::Mutex;

use tokio::time::Instant;

use super::breaker::{Breaker, BreakerSettings, TargetState};
use super::TargetRef;

/// The health of one target, as `GET /api/routing/health` shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetHealth {
    pub provider: String,
    pub model: String,
    pub state: TargetState,
    pub successes: u64,
    /// Retryable failures. A request the provider rejected is not one.
    pub failures: u64,
    /// When the last of those happened (UTC, `YYYY-MM-DD HH:MM:SS`).
    pub last_failure_at: Option<String>,
    /// What the provider answered to the last of them; none when it did not
    /// answer.
    pub last_status: Option<u16>,
}

pub trait HealthStore: Send + Sync {
    /// Whether a call may go to the target. When the breaker is half open
    /// this takes its one trial, so it is followed by a `report`.
    fn allow(&self, t: &TargetRef, now: Instant, s: &BreakerSettings) -> bool;

    /// What a call came to. `retryable_failure` is a failure another try or
    /// target may do better than; a rejected request is neither `ok` nor that.
    fn report(
        &self,
        t: &TargetRef,
        ok: bool,
        retryable_failure: bool,
        status: Option<u16>,
        now: Instant,
        s: &BreakerSettings,
    );

    /// Every target that was called, by provider then model.
    fn view(&self) -> Vec<TargetHealth>;
}

#[derive(Default)]
pub struct InMemoryHealth {
    targets: Mutex<HashMap<(String, String), Breaker>>,
}

impl InMemoryHealth {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<(String, String), Breaker>> {
        // The state is counters and times: a panic elsewhere leaves them usable.
        self.targets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn key(t: &TargetRef) -> (String, String) {
    (t.provider.clone(), t.model.clone())
}

impl HealthStore for InMemoryHealth {
    fn allow(&self, t: &TargetRef, now: Instant, s: &BreakerSettings) -> bool {
        self.lock().entry(key(t)).or_default().allow(now, s)
    }

    fn report(
        &self,
        t: &TargetRef,
        ok: bool,
        retryable_failure: bool,
        status: Option<u16>,
        now: Instant,
        s: &BreakerSettings,
    ) {
        self.lock()
            .entry(key(t))
            .or_default()
            .report(ok, retryable_failure, status, now, s);
    }

    fn view(&self) -> Vec<TargetHealth> {
        let now = Instant::now();
        let targets = self.lock();
        let mut out: Vec<TargetHealth> = targets
            .iter()
            .map(|((provider, model), b)| TargetHealth {
                provider: provider.clone(),
                model: model.clone(),
                state: b.state(now),
                successes: b.successes,
                failures: b.failures,
                last_failure_at: b.last_failure_at.clone(),
                last_status: b.last_status,
            })
            .collect();
        out.sort_by(|a, b| (&a.provider, &a.model).cmp(&(&b.provider, &b.model)));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    const S: BreakerSettings = BreakerSettings {
        failures: 2,
        window: Duration::from_secs(60),
        open: Duration::from_secs(30),
    };

    fn target(provider: &str, model: &str) -> TargetRef {
        TargetRef {
            provider: provider.into(),
            model: model.into(),
            model_id: 1,
        }
    }

    #[test]
    fn each_provider_and_model_has_its_own_breaker() {
        let h = InMemoryHealth::new();
        let now = Instant::now();
        let (a, b, c) = (target("p", "a"), target("p", "b"), target("q", "a"));
        for _ in 0..2 {
            h.report(&a, false, true, Some(500), now, &S);
        }
        assert!(!h.allow(&a, now, &S));
        assert!(h.allow(&b, now, &S));
        assert!(h.allow(&c, now, &S));
    }

    #[test]
    fn the_view_is_sorted_and_shows_state_and_counts() {
        let h = InMemoryHealth::new();
        let now = Instant::now();
        let (a, b) = (target("q", "a"), target("p", "z"));
        h.report(&a, true, false, Some(200), now, &S);
        for _ in 0..2 {
            h.report(&b, false, true, Some(503), now, &S);
        }
        let v = h.view();
        let names: Vec<_> = v
            .iter()
            .map(|t| (t.provider.as_str(), t.model.as_str()))
            .collect();
        assert_eq!(names, [("p", "z"), ("q", "a")]);
        assert_eq!(v[0].state, TargetState::Open);
        assert_eq!((v[0].successes, v[0].failures), (0, 2));
        assert_eq!(v[0].last_status, Some(503));
        assert!(v[0].last_failure_at.is_some());
        assert_eq!(v[1].state, TargetState::Closed);
        assert_eq!((v[1].successes, v[1].failures), (1, 0));
        assert_eq!(v[1].last_failure_at, None);
    }

    #[test]
    fn a_target_never_called_is_not_in_the_view() {
        assert!(InMemoryHealth::new().view().is_empty());
    }

    #[test]
    fn it_is_shared_between_threads() {
        let h = Arc::new(InMemoryHealth::new());
        let t = target("p", "m");
        let now = Instant::now();
        let threads: Vec<_> = (0..4)
            .map(|_| {
                let (h, t) = (h.clone(), t.clone());
                std::thread::spawn(move || {
                    for _ in 0..100 {
                        h.report(&t, true, false, Some(200), now, &S);
                    }
                })
            })
            .collect();
        for th in threads {
            th.join().unwrap();
        }
        assert_eq!(h.view()[0].successes, 400);
    }
}
