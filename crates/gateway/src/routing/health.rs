//! What is known of the health of each target.
//!
//! The trait is the seam for a store shared between gateway instances; the
//! default keeps it in memory.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use serde::Serialize;
use tokio::sync::mpsc;
use tokio::time::Instant;
use utoipa::ToSchema;

use super::breaker::{Breaker, BreakerSettings, TargetState, Transition};
use super::TargetRef;

/// The health of one target, as `GET /api/routing/health` shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct TargetHealth {
    pub provider: String,
    pub model: String,
    pub state: TargetState,
    pub successes: u64,
    /// Retryable failures. A request the provider rejected is not one.
    pub failures: u64,
    /// When the last of those happened (UTC, `YYYY-MM-DD HH:MM:SS`).
    #[schema(required)]
    pub last_failure_at: Option<String>,
    /// What the provider answered to the last of them; none when it did not
    /// answer.
    #[schema(required)]
    pub last_status: Option<u16>,
}

/// A breaker of a target changed in a way alerts care about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthEvent {
    Opened { provider: String, model: String },
    Closed { provider: String, model: String },
}

pub trait HealthStore: Send + Sync {
    /// Sends every [`HealthEvent`] to `events` from now on. Set once at
    /// start; a full channel drops the event (a report never waits). The
    /// default store that keeps nothing to tell ignores it.
    fn watch(&self, _events: mpsc::Sender<HealthEvent>) {}

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

    /// Forgets the targets `keep` does not name (given provider and model):
    /// those that left the catalog.
    fn retain(&self, keep: &dyn Fn(&str, &str) -> bool);
}

#[derive(Default)]
pub struct InMemoryHealth {
    targets: Mutex<HashMap<(String, String), Breaker>>,
    events: OnceLock<mpsc::Sender<HealthEvent>>,
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
    fn watch(&self, events: mpsc::Sender<HealthEvent>) {
        let _ = self.events.set(events);
    }

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
        let transition =
            self.lock()
                .entry(key(t))
                .or_default()
                .report(ok, retryable_failure, status, now, s);
        // After the lock is released; never waits.
        if let (Some(transition), Some(events)) = (transition, self.events.get()) {
            let (provider, model) = key(t);
            let _ = events.try_send(match transition {
                Transition::Opened => HealthEvent::Opened { provider, model },
                Transition::Closed => HealthEvent::Closed { provider, model },
            });
        }
    }

    fn retain(&self, keep: &dyn Fn(&str, &str) -> bool) {
        self.lock()
            .retain(|(provider, model), _| keep(provider, model));
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
    fn parallel_callers_of_a_half_open_breaker_admit_exactly_one_trial() {
        let h = Arc::new(InMemoryHealth::new());
        let t = target("p", "m");
        let start = Instant::now();
        for _ in 0..2 {
            h.report(&t, false, true, Some(500), start, &S);
        }
        let later = start + Duration::from_secs(31);
        let barrier = Arc::new(std::sync::Barrier::new(64));
        let threads: Vec<_> = (0..64)
            .map(|_| {
                let (h, t, barrier) = (h.clone(), t.clone(), barrier.clone());
                std::thread::spawn(move || {
                    barrier.wait();
                    h.allow(&t, later, &S)
                })
            })
            .collect();
        let admitted = threads
            .into_iter()
            .map(|th| th.join().unwrap())
            .filter(|ok| *ok)
            .count();
        assert_eq!(admitted, 1);
    }

    #[test]
    fn retain_forgets_the_targets_not_kept() {
        let h = InMemoryHealth::new();
        let now = Instant::now();
        h.report(&target("p", "a"), true, false, Some(200), now, &S);
        h.report(&target("p", "b"), true, false, Some(200), now, &S);
        h.retain(&|_, model| model == "a");
        let v = h.view();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].model, "a");
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

    #[test]
    fn a_watcher_hears_each_open_and_close_once() {
        let h = InMemoryHealth::new();
        let (tx, mut rx) = mpsc::channel(16);
        h.watch(tx);
        let t = target("p", "m");
        let start = Instant::now();
        h.report(&t, false, true, Some(500), start, &S);
        assert!(rx.try_recv().is_err(), "one failure changes nothing");
        h.report(&t, false, true, Some(500), start, &S);
        h.report(&t, false, true, Some(500), start, &S);
        let later = start + Duration::from_secs(31);
        assert!(h.allow(&t, later, &S));
        h.report(&t, true, false, Some(200), later, &S);
        h.report(&t, true, false, Some(200), later, &S);
        let heard: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        let (provider, model) = ("p".to_string(), "m".to_string());
        assert_eq!(
            heard,
            [
                HealthEvent::Opened {
                    provider: provider.clone(),
                    model: model.clone()
                },
                HealthEvent::Closed { provider, model },
            ]
        );
    }

    #[test]
    fn a_full_watcher_channel_never_blocks_a_report() {
        let h = InMemoryHealth::new();
        let (tx, _rx) = mpsc::channel(1);
        h.watch(tx);
        let start = Instant::now();
        // 20 targets open; the channel holds one.
        for i in 0..20 {
            let t = target("p", &format!("m{i}"));
            for _ in 0..2 {
                h.report(&t, false, true, Some(500), start, &S);
            }
        }
        assert_eq!(h.view().len(), 20);
    }
}
