//! The circuit breaker of one target.

use std::time::Duration;

/// When a target is cut off and for how long.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BreakerSettings {
    /// Retryable failures within `window` that open the breaker.
    pub failures: u32,
    pub window: Duration,
    /// How long it stays open before one trial call is let through.
    pub open: Duration,
}

impl BreakerSettings {
    /// 5 failures in 60 s, open for 30 s.
    pub const DEFAULT: BreakerSettings = BreakerSettings {
        failures: 5,
        window: Duration::from_secs(60),
        open: Duration::from_secs(30),
    };
}

use std::collections::VecDeque;

use serde::Serialize;
use tokio::time::Instant;
use utoipa::ToSchema;

use crate::store;

/// What the breaker of a target allows now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum TargetState {
    /// Calls pass.
    Closed,
    /// Calls are refused until the time to try again comes.
    Open,
    /// One trial call is let through; its answer decides.
    HalfOpen,
}

impl TargetState {
    pub fn as_str(self) -> &'static str {
        match self {
            TargetState::Closed => "closed",
            TargetState::Open => "open",
            TargetState::HalfOpen => "half_open",
        }
    }
}

/// A change of the breaker that alerts care about. Open to half open is
/// not one: nothing was learned by waiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    /// Closed or half open to open.
    Opened,
    /// Half open to closed: the trial call succeeded.
    Closed,
}

#[derive(Debug, Clone, Copy)]
enum Phase {
    Closed,
    Open {
        /// When one trial call is let through.
        until: Instant,
    },
    HalfOpen {
        /// While a trial call is out, until when it counts as out: a call
        /// that is never reported (its caller went away) must not hold the
        /// breaker half open for ever.
        trial_until: Option<Instant>,
    },
}

/// The circuit breaker of one target. Time is passed in, never read.
#[derive(Debug)]
pub struct Breaker {
    phase: Phase,
    /// When the retryable failures that count toward opening happened.
    window: VecDeque<Instant>,
    pub successes: u64,
    pub failures: u64,
    pub last_failure_at: Option<String>,
    pub last_status: Option<u16>,
}

impl Breaker {
    pub fn new() -> Self {
        Self {
            phase: Phase::Closed,
            window: VecDeque::new(),
            successes: 0,
            failures: 0,
            last_failure_at: None,
            last_status: None,
        }
    }

    /// Whether a call may go to the target now. In the half-open state this
    /// takes the trial: the next caller is refused until it is reported.
    pub fn allow(&mut self, now: Instant, s: &BreakerSettings) -> bool {
        match self.phase {
            Phase::Closed => true,
            Phase::Open { until } if now >= until => {
                self.phase = Phase::HalfOpen {
                    trial_until: Some(now + s.open),
                };
                true
            }
            Phase::Open { .. } => false,
            Phase::HalfOpen { trial_until } => {
                if trial_until.is_none_or(|t| now >= t) {
                    self.phase = Phase::HalfOpen {
                        trial_until: Some(now + s.open),
                    };
                    true
                } else {
                    false
                }
            }
        }
    }

    /// What a call came to. A retryable failure counts toward opening; a
    /// failure that is not retryable (a rejected request) counts for
    /// nothing, as the target did answer.
    pub fn report(
        &mut self,
        ok: bool,
        retryable_failure: bool,
        status: Option<u16>,
        now: Instant,
        s: &BreakerSettings,
    ) -> Option<Transition> {
        if ok {
            self.successes += 1;
            if matches!(self.phase, Phase::HalfOpen { .. }) {
                self.phase = Phase::Closed;
                self.window.clear();
                return Some(Transition::Closed);
            }
            return None;
        }
        if !retryable_failure {
            // A trial that got a rejection of the request was answered, but
            // proved nothing: the next call is the trial.
            if matches!(self.phase, Phase::HalfOpen { .. }) {
                self.phase = Phase::HalfOpen { trial_until: None };
            }
            return None;
        }
        self.failures += 1;
        self.last_failure_at = Some(store::now());
        self.last_status = status;
        match self.phase {
            Phase::HalfOpen { .. } => {
                self.phase = Phase::Open {
                    until: now + s.open,
                };
                Some(Transition::Opened)
            }
            Phase::Closed => {
                self.window.push_back(now);
                while self
                    .window
                    .front()
                    .is_some_and(|t| now.saturating_duration_since(*t) >= s.window)
                {
                    self.window.pop_front();
                }
                if self.window.len() >= s.failures.max(1) as usize {
                    self.phase = Phase::Open {
                        until: now + s.open,
                    };
                    self.window.clear();
                    return Some(Transition::Opened);
                }
                None
            }
            // A late answer of a call that began before it opened.
            Phase::Open { .. } => None,
        }
    }

    pub fn state(&self, now: Instant) -> TargetState {
        match self.phase {
            Phase::Closed => TargetState::Closed,
            Phase::Open { until } if now >= until => TargetState::HalfOpen,
            Phase::Open { .. } => TargetState::Open,
            Phase::HalfOpen { .. } => TargetState::HalfOpen,
        }
    }
}

impl Default for Breaker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const S: BreakerSettings = BreakerSettings {
        failures: 3,
        window: Duration::from_secs(10),
        open: Duration::from_secs(30),
    };

    fn secs(base: Instant, n: u64) -> Instant {
        base + Duration::from_secs(n)
    }

    fn fail(b: &mut Breaker, at: Instant) {
        b.report(false, true, Some(503), at, &S);
    }

    #[test]
    fn opens_at_the_threshold_within_the_window() {
        let t0 = Instant::now();
        let mut b = Breaker::new();
        fail(&mut b, t0);
        fail(&mut b, secs(t0, 1));
        assert!(b.allow(secs(t0, 2), &S), "two failures: still closed");
        fail(&mut b, secs(t0, 2));
        assert_eq!(b.state(secs(t0, 2)), TargetState::Open);
        assert!(!b.allow(secs(t0, 3), &S));
    }

    #[test]
    fn failures_outside_the_window_do_not_add_up() {
        let t0 = Instant::now();
        let mut b = Breaker::new();
        fail(&mut b, t0);
        fail(&mut b, secs(t0, 5));
        // The first one is 10 s old now: out of the window.
        fail(&mut b, secs(t0, 10));
        assert_eq!(b.state(secs(t0, 10)), TargetState::Closed);
        // At 16 s the one of 5 s is out too: two in the window.
        fail(&mut b, secs(t0, 16));
        assert_eq!(b.state(secs(t0, 16)), TargetState::Closed);
        fail(&mut b, secs(t0, 17));
        assert_eq!(b.state(secs(t0, 17)), TargetState::Open);
    }

    #[test]
    fn half_opens_after_the_open_period_with_one_trial() {
        let t0 = Instant::now();
        let mut b = Breaker::new();
        for i in 0..3 {
            fail(&mut b, secs(t0, i));
        }
        let opened = secs(t0, 2);
        assert!(!b.allow(opened + Duration::from_secs(29), &S));
        let later = opened + Duration::from_secs(30);
        assert_eq!(b.state(later), TargetState::HalfOpen);
        assert!(b.allow(later, &S), "the trial");
        assert!(!b.allow(later, &S), "only one");
        assert!(!b.allow(later + Duration::from_secs(5), &S));
    }

    #[test]
    fn a_successful_trial_closes_and_a_failed_one_reopens() {
        let t0 = Instant::now();
        let mut b = Breaker::new();
        for i in 0..3 {
            fail(&mut b, secs(t0, i));
        }
        let trial = secs(t0, 40);
        assert!(b.allow(trial, &S));
        b.report(true, false, Some(200), trial, &S);
        assert_eq!(b.state(trial), TargetState::Closed);
        assert!(b.allow(trial, &S) && b.allow(trial, &S));
        // The failures from before do not count any more.
        fail(&mut b, trial);
        assert_eq!(b.state(trial), TargetState::Closed);

        let mut b = Breaker::new();
        for i in 0..3 {
            fail(&mut b, secs(t0, i));
        }
        assert!(b.allow(trial, &S));
        fail(&mut b, trial);
        assert_eq!(b.state(trial), TargetState::Open);
        assert!(!b.allow(secs(t0, 69), &S));
        assert!(
            b.allow(secs(t0, 70), &S),
            "a new trial 30 s after reopening"
        );
    }

    #[test]
    fn a_failure_that_is_not_retryable_counts_for_nothing() {
        let t0 = Instant::now();
        let mut b = Breaker::new();
        for i in 0..10 {
            b.report(false, false, Some(400), secs(t0, i), &S);
        }
        assert_eq!(b.state(secs(t0, 10)), TargetState::Closed);
        assert_eq!(b.failures, 0);
        assert_eq!(b.last_status, None);
    }

    #[test]
    fn a_rejected_trial_lets_the_next_call_be_the_trial() {
        let t0 = Instant::now();
        let mut b = Breaker::new();
        for i in 0..3 {
            fail(&mut b, secs(t0, i));
        }
        let trial = secs(t0, 40);
        assert!(b.allow(trial, &S));
        b.report(false, false, Some(400), trial, &S);
        assert_eq!(b.state(trial), TargetState::HalfOpen);
        assert!(b.allow(trial, &S));
    }

    #[test]
    fn a_trial_that_is_never_reported_does_not_hold_the_breaker() {
        let t0 = Instant::now();
        let mut b = Breaker::new();
        for i in 0..3 {
            fail(&mut b, secs(t0, i));
        }
        let trial = secs(t0, 40);
        assert!(b.allow(trial, &S));
        assert!(!b.allow(secs(t0, 69), &S));
        assert!(b.allow(secs(t0, 70), &S));
    }

    #[test]
    fn counts_and_the_last_failure_are_kept() {
        let t0 = Instant::now();
        let mut b = Breaker::new();
        b.report(true, false, Some(200), t0, &S);
        b.report(false, true, None, t0, &S);
        assert_eq!((b.successes, b.failures), (1, 1));
        assert_eq!(b.last_status, None);
        assert!(b.last_failure_at.is_some());
        b.report(false, true, Some(429), t0, &S);
        assert_eq!(b.last_status, Some(429));
    }

    #[test]
    fn each_change_is_reported_exactly_once() {
        let t0 = Instant::now();
        let mut b = Breaker::new();
        assert_eq!(b.report(true, false, Some(200), t0, &S), None);
        assert_eq!(b.report(false, true, Some(500), t0, &S), None);
        assert_eq!(b.report(false, true, Some(500), secs(t0, 1), &S), None);
        assert_eq!(
            b.report(false, true, Some(500), secs(t0, 2), &S),
            Some(Transition::Opened),
            "the third failure opens it"
        );
        // A late answer of a call that began before it opened: no change.
        assert_eq!(b.report(false, true, Some(500), secs(t0, 3), &S), None);
        assert_eq!(b.report(true, false, Some(200), secs(t0, 3), &S), None);
        // Waiting is not a change that matters.
        assert!(b.allow(secs(t0, 40), &S));
        // A failed trial opens it again: a change.
        assert_eq!(
            b.report(false, true, Some(500), secs(t0, 40), &S),
            Some(Transition::Opened)
        );
        assert!(b.allow(secs(t0, 80), &S));
        // A rejected trial proves nothing.
        assert_eq!(b.report(false, false, Some(400), secs(t0, 80), &S), None);
        assert!(b.allow(secs(t0, 80), &S));
        assert_eq!(
            b.report(true, false, Some(200), secs(t0, 81), &S),
            Some(Transition::Closed)
        );
        assert_eq!(b.report(true, false, Some(200), secs(t0, 82), &S), None);
    }
}
