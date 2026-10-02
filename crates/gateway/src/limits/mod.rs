//! Rate limits of `/v1`: requests per minute, tokens per minute and
//! concurrent requests, set for the gateway, a team, a user or a key.
//!
//! A call must pass every limit that applies to it, so the strictest wins.
//! The counters live behind [`Limiter`] so a shared store can replace the
//! in-memory one; the limits themselves come from the snapshot with every
//! call.

mod window;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use window::Window;

/// What a limit is set on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LimitScope {
    Gateway,
    Team,
    User,
    Key,
}

impl LimitScope {
    pub fn as_str(self) -> &'static str {
        match self {
            LimitScope::Gateway => "gateway",
            LimitScope::Team => "team",
            LimitScope::User => "user",
            LimitScope::Key => "key",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "gateway" => Some(LimitScope::Gateway),
            "team" => Some(LimitScope::Team),
            "user" => Some(LimitScope::User),
            "key" => Some(LimitScope::Key),
            _ => None,
        }
    }

    /// How a message names the subject: `gateway`, `team 'Platform'`,
    /// `user 'lena@example.com'`, `key 'ci'`. `name` is the team's or key's
    /// name or the user's email.
    pub fn label(self, name: &str) -> String {
        match self {
            LimitScope::Gateway => "gateway".to_string(),
            other => format!("{} '{name}'", other.as_str()),
        }
    }
}

/// The limits of one subject. `None` is no limit of that kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RateLimit {
    pub requests_per_minute: Option<u64>,
    pub tokens_per_minute: Option<u64>,
    pub concurrent: Option<u64>,
}

impl RateLimit {
    pub fn is_none(&self) -> bool {
        self.requests_per_minute.is_none()
            && self.tokens_per_minute.is_none()
            && self.concurrent.is_none()
    }
}

/// Something with limits: the gateway, a team, a user or a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subject {
    pub scope: LimitScope,
    /// 0 for the gateway.
    pub id: i64,
    /// What a refusal calls it, as [`LimitScope::label`] writes it.
    pub label: String,
    pub limit: RateLimit,
}

/// Everything with a limit that applies to one call. A subject without a
/// limit is left out.
#[derive(Debug, Clone, Default)]
pub struct Subjects {
    pub key: Option<Arc<Subject>>,
    pub user: Option<Arc<Subject>>,
    pub teams: Vec<Arc<Subject>>,
    pub gateway: Option<Arc<Subject>>,
}

impl Subjects {
    pub fn is_empty(&self) -> bool {
        self.iter().next().is_none()
    }

    /// In the order they are checked: key, user, teams, gateway.
    pub fn iter(&self) -> impl Iterator<Item = &Arc<Subject>> {
        self.key
            .iter()
            .chain(self.user.iter())
            .chain(self.teams.iter())
            .chain(self.gateway.iter())
    }
}

/// Why a call was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// `requests per minute`, `tokens per minute` or `concurrent requests`.
    pub limit_name: &'static str,
    /// The subject whose limit was reached, as its label.
    pub scope_label: String,
    /// When to come back: at least a second.
    pub retry_after: Duration,
}

impl Refusal {
    pub fn message(&self) -> String {
        format!(
            "rate limit '{}' of {} reached",
            self.limit_name, self.scope_label
        )
    }

    /// For the `Retry-After` header: whole seconds, at least one.
    pub fn retry_after_seconds(&self) -> u64 {
        let d = self.retry_after;
        (d.as_secs() + u64::from(d.subsec_nanos() > 0)).max(1)
    }
}

/// What a running call holds: its concurrency slots and the tokens it was
/// charged. Dropping it gives the slots back.
pub struct Permit(Option<Box<dyn Held>>);

pub trait Held: Send {
    fn settle(&mut self, actual_tokens: u64);
}

impl Permit {
    /// A permit for a call that no limit applies to.
    pub fn none() -> Self {
        Permit(None)
    }

    pub fn new(held: Box<dyn Held>) -> Self {
        Permit(Some(held))
    }

    /// Replaces the estimate the call was charged with what it used. Only
    /// the first call counts.
    pub fn settle(&mut self, actual_tokens: u64) {
        if let Some(held) = self.0.as_mut() {
            held.settle(actual_tokens);
        }
    }
}

impl std::fmt::Debug for Permit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Permit").finish_non_exhaustive()
    }
}

/// Decides whether a call may run. `acquire` is atomic: a refused call
/// changes nothing.
pub trait Limiter: Send + Sync {
    /// `estimate_tokens` is what the call is expected to use; settle the
    /// permit with the real number when it is known.
    fn acquire(
        &self,
        who: &Subjects,
        estimate_tokens: u64,
        now: Instant,
    ) -> Result<Permit, Refusal>;
}

type SubjectKey = (LimitScope, i64);

#[derive(Default)]
struct State {
    window: Window,
    in_flight: u64,
}

struct Inner {
    /// Seconds are counted from here.
    origin: Instant,
    state: Mutex<Shared>,
}

#[derive(Default)]
struct Shared {
    subjects: HashMap<SubjectKey, State>,
    /// The latest second any call named, so a permit settled in a test's
    /// made-up time lands in the same time.
    latest: u64,
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, Shared> {
        // The counters stay usable after a panic elsewhere.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn second(&self, now: Instant) -> u64 {
        now.saturating_duration_since(self.origin).as_secs()
    }
}

/// Counters in the memory of this process. They start empty on every start
/// and are not shared between processes.
pub struct MemoryLimiter {
    inner: Arc<Inner>,
}

impl Default for MemoryLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryLimiter {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                origin: Instant::now(),
                state: Mutex::default(),
            }),
        }
    }
}

/// What one subject holds for a call.
struct Hold {
    key: SubjectKey,
    /// A concurrency slot was taken.
    slot: bool,
    /// The second and amount of the tokens charged.
    charged: Option<(u64, u64)>,
}

struct MemoryPermit {
    inner: Arc<Inner>,
    holds: Vec<Hold>,
    settled: bool,
}

impl Held for MemoryPermit {
    fn settle(&mut self, actual: u64) {
        if std::mem::replace(&mut self.settled, true) {
            return;
        }
        let mut shared = self.inner.lock();
        let now = shared.latest.max(self.inner.second(Instant::now()));
        for hold in &self.holds {
            let Some((at, charged)) = hold.charged else {
                continue;
            };
            if let Some(state) = shared.subjects.get_mut(&hold.key) {
                state.window.correct_tokens(at, charged, actual, now);
            }
        }
    }
}

impl Drop for MemoryPermit {
    fn drop(&mut self) {
        let mut shared = self.inner.lock();
        for hold in self.holds.iter().filter(|h| h.slot) {
            if let Some(state) = shared.subjects.get_mut(&hold.key) {
                state.in_flight = state.in_flight.saturating_sub(1);
            }
        }
    }
}

impl Limiter for MemoryLimiter {
    fn acquire(
        &self,
        who: &Subjects,
        estimate_tokens: u64,
        now: Instant,
    ) -> Result<Permit, Refusal> {
        let limited: Vec<&Arc<Subject>> = who.iter().filter(|s| !s.limit.is_none()).collect();
        if limited.is_empty() {
            return Ok(Permit::none());
        }
        let second = self.inner.second(now);
        let mut shared = self.inner.lock();
        shared.latest = shared.latest.max(second);

        // Every limit is checked before anything is counted, so a refused
        // call leaves no trace. Of several refusals the longest wait is told.
        let mut worst: Option<Refusal> = None;
        let mut consider = |refusal: Refusal| {
            if worst
                .as_ref()
                .is_none_or(|w| refusal.retry_after > w.retry_after)
            {
                worst = Some(refusal);
            }
        };
        let empty = State::default();
        for subject in &limited {
            let state = shared
                .subjects
                .get(&(subject.scope, subject.id))
                .unwrap_or(&empty);
            let limit = &subject.limit;
            let refuse = |limit_name: &'static str, retry_after: Duration| Refusal {
                limit_name,
                scope_label: subject.label.clone(),
                retry_after,
            };
            if let Some(max) = limit.concurrent {
                if state.in_flight >= max {
                    consider(refuse("concurrent requests", Duration::from_secs(1)));
                }
            }
            let (requests, tokens) = state.window.used(second);
            if let Some(max) = limit.requests_per_minute {
                if requests.saturating_add(1) > max {
                    let wait = state.window.wait(second, max, 1, false);
                    consider(refuse("requests per minute", wait));
                }
            }
            if let Some(max) = limit.tokens_per_minute {
                // A call larger than the whole limit would never be let
                // through; it is charged the limit and so runs alone.
                let charge = estimate_tokens.min(max);
                if tokens.saturating_add(charge) > max {
                    let wait = state.window.wait(second, max, charge, true);
                    consider(refuse("tokens per minute", wait));
                }
            }
        }
        if let Some(refusal) = worst {
            return Err(refusal);
        }

        let mut holds = Vec::with_capacity(limited.len());
        for subject in limited {
            let key = (subject.scope, subject.id);
            let state = shared.subjects.entry(key).or_default();
            let limit = &subject.limit;
            let mut hold = Hold {
                key,
                slot: false,
                charged: None,
            };
            if limit.concurrent.is_some() {
                state.in_flight += 1;
                hold.slot = true;
            }
            if limit.requests_per_minute.is_some() {
                state.window.add(second, 1, 0);
            }
            if let Some(max) = limit.tokens_per_minute {
                let charge = estimate_tokens.min(max);
                state.window.add(second, 0, charge);
                hold.charged = Some((second, charge));
            }
            holds.push(hold);
        }
        drop(shared);
        Ok(Permit::new(Box::new(MemoryPermit {
            inner: self.inner.clone(),
            holds,
            settled: false,
        })))
    }
}
