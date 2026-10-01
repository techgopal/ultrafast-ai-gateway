//! The routing engine: which targets a call tries and in what order,
//! retries with backoff, timeouts and the circuit breaker of each target.

pub mod breaker;
pub mod health;
pub mod select;

use std::time::Duration;

pub use breaker::{BreakerSettings, TargetState};
pub use health::{HealthStore, InMemoryHealth, TargetHealth};
pub use select::plan;

/// A model of a provider, by name, with the id of the catalog row.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TargetRef {
    pub provider: String,
    pub model: String,
    pub model_id: i64,
}

/// How a call is tried: used for a route, and by default for a direct call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    /// Extra tries of a target after the first, on a retryable failure.
    pub retries: u32,
    /// How long until the first byte (a stream: the first event).
    pub first_token_timeout: Duration,
    /// How long the whole request may take, every try included.
    pub total_timeout: Duration,
    pub breaker: BreakerSettings,
}

impl Settings {
    /// What a direct `provider/model` call uses.
    pub const DIRECT: Settings = Settings {
        retries: 2,
        first_token_timeout: Duration::from_secs(30),
        total_timeout: Duration::from_secs(300),
        breaker: BreakerSettings::DEFAULT,
    };
}

use std::future::Future;
use std::time::Duration as StdDuration;

use rand::{Rng, RngExt};
use tokio::time::{sleep, timeout_at, Instant};

use crate::telemetry::{AttemptOutcome, Scope};

/// A target of a plan, and whether this caller may call it now.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub target: TargetRef,
    /// False when the caller may not call it or it is disabled.
    pub callable: bool,
}

/// What a try of a target is held to.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// How long until the first byte (a stream: the first event).
    pub first_token: StdDuration,
    /// When the whole request is out of time.
    pub deadline: Instant,
}

/// A try that worked. `status` is what the provider answered.
pub struct Success<T> {
    pub value: T,
    pub status: Option<u16>,
}

/// A try that did not work. `status` is what the provider answered, if it did.
pub enum Failure<E> {
    /// Another try, or another target, may do better. `retry_after` is how
    /// long the provider asked to be left alone.
    Retryable {
        error: E,
        status: Option<u16>,
        retry_after: Option<StdDuration>,
    },
    /// This target cannot serve the call (the gateway's credential for it is
    /// refused): the next target is tried, with no retry of this one. It
    /// counts against the target's breaker.
    Failover { error: E, status: Option<u16> },
    /// The request itself or the answer cannot be helped by trying again.
    Fatal { error: E, status: Option<u16> },
}

/// Why no target served the call.
#[derive(Debug, PartialEq, Eq)]
pub enum Stop<E> {
    /// A try failed in a way no other try could help. Its error is the answer.
    Fatal(E),
    /// Every target was skipped, refused by its breaker or failed.
    Exhausted(Exhausted<E>),
}

/// What the tries of a call that no target served came to.
#[derive(Debug, PartialEq, Eq)]
pub struct Exhausted<E> {
    /// Tries made, retries included.
    pub attempts: u32,
    /// Of those, the ones the provider answered 429.
    pub rate_limited: u32,
    /// The largest wait a provider asked for.
    pub retry_after: Option<StdDuration>,
    /// The last refusal, when every try was a refusal of the credential.
    pub refused: Option<E>,
}

/// The wait before retry number `retry` (0 for the first): up to 250 ms,
/// doubling each time up to 4 s, with full jitter.
pub fn backoff(retry: u32, rng: &mut impl Rng) -> StdDuration {
    let cap_ms = BACKOFF_BASE_MS
        .saturating_mul(1u64 << retry.min(16))
        .min(BACKOFF_MAX_MS);
    StdDuration::from_millis(rng.random_range(0..=cap_ms))
}

const BACKOFF_BASE_MS: u64 = 250;
const BACKOFF_MAX_MS: u64 = 4_000;

/// Tries the targets of `plan` in order and returns what the first that
/// works gives. Every try is written to `scope` before it is made.
pub async fn run<T, E, F, Fut>(
    health: &dyn HealthStore,
    scope: &mut Scope,
    settings: &Settings,
    plan: &[Candidate],
    rng: &mut impl Rng,
    mut call: F,
) -> Result<T, Stop<E>>
where
    F: FnMut(TargetRef, Limits) -> Fut,
    Fut: Future<Output = Result<Success<T>, Failure<E>>>,
{
    let deadline = Instant::now() + settings.total_timeout;
    let (mut attempts, mut rate_limited, mut refusals) = (0u32, 0u32, 0u32);
    let mut retry_after_max: Option<StdDuration> = None;
    let mut last_refusal: Option<E> = None;
    for candidate in plan {
        let t = &candidate.target;
        if !candidate.callable {
            scope.attempt(
                &t.provider,
                &t.model,
                AttemptOutcome::Skipped,
                None,
                std::time::Instant::now(),
            );
            continue;
        }
        if Instant::now() >= deadline {
            // Out of time: what is left is recorded as not tried.
            break;
        }
        if !health.allow(t, Instant::now(), &settings.breaker) {
            scope.attempt(
                &t.provider,
                &t.model,
                AttemptOutcome::CircuitOpen,
                None,
                std::time::Instant::now(),
            );
            continue;
        }
        let mut retry = 0;
        loop {
            // Written before the call, so a caller that goes away while it is
            // out leaves the target as one that was tried.
            scope.begin_attempt(&t.provider, &t.model);
            let started = std::time::Instant::now();
            let limits = Limits {
                first_token: settings.first_token_timeout,
                deadline,
            };
            attempts += 1;
            let tried = timeout_at(deadline, call(t.clone(), limits)).await;
            let mut asked = None;
            let status = match tried {
                Ok(Ok(done)) => {
                    scope.settle_attempt(AttemptOutcome::Ok, done.status, started);
                    health.report(
                        t,
                        true,
                        false,
                        done.status,
                        Instant::now(),
                        &settings.breaker,
                    );
                    return Ok(done.value);
                }
                Ok(Err(Failure::Fatal { error, status })) => {
                    scope.settle_attempt(AttemptOutcome::Fatal, status, started);
                    health.report(t, false, false, status, Instant::now(), &settings.breaker);
                    return Err(Stop::Fatal(error));
                }
                Ok(Err(Failure::Failover { error, status })) => {
                    scope.settle_attempt(AttemptOutcome::Retryable, status, started);
                    health.report(t, false, true, status, Instant::now(), &settings.breaker);
                    refusals += 1;
                    last_refusal = Some(error);
                    break;
                }
                Ok(Err(Failure::Retryable {
                    status,
                    retry_after,
                    ..
                })) => {
                    asked = retry_after;
                    status
                }
                // Out of time: a timeout is a retryable failure of the target.
                Err(_) => None,
            };
            if status == Some(429) {
                rate_limited += 1;
            }
            if let Some(a) = asked {
                retry_after_max = Some(retry_after_max.map_or(a, |m| m.max(a)));
            }
            scope.settle_attempt(AttemptOutcome::Retryable, status, started);
            health.report(t, false, true, status, Instant::now(), &settings.breaker);
            if retry >= settings.retries || !health.allow(t, Instant::now(), &settings.breaker) {
                break;
            }
            let mut wait = backoff(retry, rng);
            if let Some(a) = asked {
                wait = wait.max(a.min(StdDuration::from_millis(BACKOFF_MAX_MS)));
            }
            if Instant::now() + wait >= deadline {
                break;
            }
            sleep(wait).await;
            retry += 1;
        }
    }
    Err(Stop::Exhausted(Exhausted {
        attempts,
        rate_limited,
        retry_after: retry_after_max,
        refused: if attempts > 0 && refusals == attempts {
            last_refusal
        } else {
            None
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::{AttemptOutcome, RequestRecord, RequestSink, CALLER_GONE};
    use rand::rngs::StdRng;
    use rand::SeedableRng;
    use std::collections::{HashMap, VecDeque};
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Mem(Mutex<Vec<RequestRecord>>);
    impl RequestSink for Mem {
        fn record(&self, record: RequestRecord) {
            self.0.lock().unwrap().push(record);
        }
    }

    const SETTINGS: Settings = Settings {
        retries: 2,
        first_token_timeout: Duration::from_secs(30),
        total_timeout: Duration::from_secs(300),
        breaker: BreakerSettings {
            failures: 100,
            window: Duration::from_secs(60),
            open: Duration::from_secs(30),
        },
    };

    fn target(model: &str) -> TargetRef {
        TargetRef {
            provider: "p".into(),
            model: model.into(),
            model_id: 1,
        }
    }

    fn plan_of(models: &[&str]) -> Vec<Candidate> {
        models
            .iter()
            .map(|m| Candidate {
                target: target(m),
                callable: true,
            })
            .collect()
    }

    #[derive(Clone, Copy)]
    enum Step {
        Ok,
        Retry(Option<u16>),
        Fatal(u16),
        /// Never answers.
        Hang,
        /// Retryable, and the provider asked for this many seconds.
        RetryAfter(u16, u64),
        /// The target is refused for this call: another is tried, no retry.
        Failover(u16),
    }

    /// Answers each model from its own script; the last step repeats.
    struct Script {
        steps: Mutex<HashMap<String, VecDeque<Step>>>,
        calls: Mutex<Vec<String>>,
    }

    impl Script {
        fn new(steps: &[(&str, &[Step])]) -> Arc<Self> {
            Arc::new(Self {
                steps: Mutex::new(
                    steps
                        .iter()
                        .map(|(m, s)| (m.to_string(), s.iter().copied().collect()))
                        .collect(),
                ),
                calls: Mutex::new(Vec::new()),
            })
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }

        async fn answer(&self, t: TargetRef) -> Result<Success<String>, Failure<String>> {
            self.calls.lock().unwrap().push(t.model.clone());
            let step = {
                let mut steps = self.steps.lock().unwrap();
                let q = steps.get_mut(&t.model).expect("scripted");
                if q.len() > 1 {
                    q.pop_front().unwrap()
                } else {
                    *q.front().unwrap()
                }
            };
            match step {
                Step::Ok => Ok(Success {
                    value: t.model,
                    status: Some(200),
                }),
                Step::Retry(status) => Err(Failure::Retryable {
                    error: "retry".into(),
                    status,
                    retry_after: None,
                }),
                Step::RetryAfter(status, secs) => Err(Failure::Retryable {
                    error: "retry".into(),
                    status: Some(status),
                    retry_after: Some(Duration::from_secs(secs)),
                }),
                Step::Failover(status) => Err(Failure::Failover {
                    error: format!("refused {status}"),
                    status: Some(status),
                }),
                Step::Fatal(status) => Err(Failure::Fatal {
                    error: format!("fatal {status}"),
                    status: Some(status),
                }),
                Step::Hang => {
                    std::future::pending::<()>().await;
                    unreachable!()
                }
            }
        }
    }

    struct Ran {
        result: Result<String, Stop<String>>,
        record: RequestRecord,
        health: Arc<InMemoryHealth>,
    }

    async fn run_with(settings: Settings, plan: &[Candidate], script: &Arc<Script>) -> Ran {
        let health = Arc::new(InMemoryHealth::new());
        run_on(health, settings, plan, script).await
    }

    async fn run_on(
        health: Arc<InMemoryHealth>,
        settings: Settings,
        plan: &[Candidate],
        script: &Arc<Script>,
    ) -> Ran {
        let sink = Arc::new(Mem::default());
        let mut scope = Scope::begin(sink.clone(), 1, None, None, "chat");
        scope.targets(
            plan.iter()
                .map(|c| (c.target.provider.clone(), c.target.model.clone()))
                .collect(),
        );
        let mut rng = StdRng::seed_from_u64(9);
        let result = run(&*health, &mut scope, &settings, plan, &mut rng, |t, _| {
            let script = script.clone();
            async move { script.answer(t).await }
        })
        .await;
        scope.finish(200);
        let record = sink.0.lock().unwrap()[0].clone();
        Ran {
            result,
            record,
            health,
        }
    }

    fn seen(r: &RequestRecord) -> Vec<(String, AttemptOutcome, Option<u16>)> {
        r.attempts
            .iter()
            .map(|a| (a.model.clone(), a.outcome, a.status))
            .collect()
    }

    #[test]
    fn backoff_starts_at_250ms_doubles_and_stops_at_4s() {
        let mut rng = StdRng::seed_from_u64(1);
        for retry in 0..40 {
            let cap = Duration::from_millis((250u64 << retry.min(10)).min(4000));
            let mut longest = Duration::ZERO;
            for _ in 0..200 {
                let d = backoff(retry, &mut rng);
                assert!(d <= cap, "retry {retry}: {d:?} over {cap:?}");
                longest = longest.max(d);
            }
            // Full jitter reaches into the upper half of the range.
            assert!(longest > cap / 2, "retry {retry}: never above {longest:?}");
        }
    }

    #[test]
    fn backoff_is_jittered_from_zero() {
        let mut rng = StdRng::seed_from_u64(2);
        let shortest = (0..500).map(|_| backoff(3, &mut rng)).min().unwrap();
        assert!(shortest < Duration::from_millis(200));
    }

    #[tokio::test(start_paused = true)]
    async fn a_retryable_failure_is_tried_again_up_to_the_retries() {
        let script = Script::new(&[("a", &[Step::Retry(Some(503)), Step::Retry(None), Step::Ok])]);
        let ran = run_with(SETTINGS, &plan_of(&["a"]), &script).await;
        assert_eq!(ran.result, Ok("a".to_string()));
        assert_eq!(script.calls(), ["a", "a", "a"]);
        assert_eq!(
            seen(&ran.record),
            [
                ("a".into(), AttemptOutcome::Retryable, Some(503)),
                ("a".into(), AttemptOutcome::Retryable, None),
                ("a".into(), AttemptOutcome::Ok, Some(200)),
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_fatal_failure_is_returned_at_once_with_no_retry_and_no_fallback() {
        let script = Script::new(&[("a", &[Step::Fatal(400)]), ("b", &[Step::Ok])]);
        let ran = run_with(SETTINGS, &plan_of(&["a", "b"]), &script).await;
        assert_eq!(ran.result, Err(Stop::Fatal("fatal 400".to_string())));
        assert_eq!(script.calls(), ["a"]);
        assert_eq!(
            seen(&ran.record),
            [
                ("a".into(), AttemptOutcome::Fatal, Some(400)),
                ("b".into(), AttemptOutcome::Skipped, None),
            ]
        );
        // A rejected request is not the target's failure.
        assert_eq!(ran.health.view()[0].failures, 0);
    }

    #[tokio::test(start_paused = true)]
    async fn the_next_target_is_tried_when_the_retries_are_used_up() {
        let script = Script::new(&[("a", &[Step::Retry(Some(500))]), ("b", &[Step::Ok])]);
        let ran = run_with(SETTINGS, &plan_of(&["a", "b"]), &script).await;
        assert_eq!(ran.result, Ok("b".to_string()));
        assert_eq!(script.calls(), ["a", "a", "a", "b"]);
    }

    #[tokio::test(start_paused = true)]
    async fn with_no_retries_each_target_is_tried_once() {
        let settings = Settings {
            retries: 0,
            ..SETTINGS
        };
        let script = Script::new(&[
            ("a", &[Step::Retry(Some(500))]),
            ("b", &[Step::Retry(None)]),
        ]);
        let ran = run_with(settings, &plan_of(&["a", "b"]), &script).await;
        assert!(matches!(ran.result, Err(Stop::Exhausted(_))));
        assert_eq!(script.calls(), ["a", "b"]);
    }

    #[tokio::test(start_paused = true)]
    async fn backoff_bounded() {
        let settings = Settings {
            retries: 5,
            ..SETTINGS
        };
        let script = Script::new(&[("a", &[Step::Retry(Some(503))])]);
        let start = Instant::now();
        let ran = run_with(settings, &plan_of(&["a"]), &script).await;
        let waited = start.elapsed();
        assert!(matches!(ran.result, Err(Stop::Exhausted(_))));
        assert_eq!(script.calls().len(), 6);
        // Five waits of at most 250, 500, 1000, 2000 and 4000 ms.
        assert!(waited <= Duration::from_millis(7750), "waited {waited:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn a_retry_waits_at_least_the_retry_after_up_to_4s() {
        let settings = Settings {
            retries: 1,
            ..SETTINGS
        };
        for (asked, waited) in [(3u64, 3u64), (30, 4)] {
            let script = Script::new(&[("a", &[Step::RetryAfter(429, asked), Step::Ok])]);
            let start = Instant::now();
            let ran = run_with(settings, &plan_of(&["a"]), &script).await;
            assert_eq!(ran.result, Ok("a".to_string()));
            assert_eq!(
                start.elapsed(),
                Duration::from_secs(waited),
                "asked {asked}"
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_failover_moves_on_without_a_retry_and_counts_for_the_breaker() {
        let script = Script::new(&[("a", &[Step::Failover(401)]), ("b", &[Step::Ok])]);
        let ran = run_with(SETTINGS, &plan_of(&["a", "b"]), &script).await;
        assert_eq!(ran.result, Ok("b".to_string()));
        assert_eq!(script.calls(), ["a", "b"]);
        assert_eq!(
            seen(&ran.record)[0],
            ("a".into(), AttemptOutcome::Retryable, Some(401))
        );
        let a = &ran.health.view()[0];
        assert_eq!(
            (a.provider.as_str(), a.model.as_str(), a.failures),
            ("p", "a", 1)
        );
    }

    #[tokio::test(start_paused = true)]
    async fn exhausted_says_what_the_attempts_were() {
        let settings = Settings {
            retries: 0,
            ..SETTINGS
        };
        // Every attempt refused: the last refusal is the answer.
        let script = Script::new(&[("a", &[Step::Failover(401)]), ("b", &[Step::Failover(403)])]);
        let ran = run_with(settings, &plan_of(&["a", "b"]), &script).await;
        let Err(Stop::Exhausted(ex)) = ran.result else {
            panic!()
        };
        assert_eq!(ex.attempts, 2);
        assert_eq!(ex.refused, Some("refused 403".to_string()));
        assert_eq!(ex.rate_limited, 0);

        // Every attempt a 429; the largest Retry-After is kept.
        let script = Script::new(&[
            ("a", &[Step::RetryAfter(429, 7)]),
            ("b", &[Step::RetryAfter(429, 12)]),
            ("c", &[Step::Retry(Some(429))]),
        ]);
        let ran = run_with(settings, &plan_of(&["a", "b", "c"]), &script).await;
        let Err(Stop::Exhausted(ex)) = ran.result else {
            panic!()
        };
        assert_eq!((ex.attempts, ex.rate_limited), (3, 3));
        assert_eq!(ex.retry_after, Some(Duration::from_secs(12)));
        assert_eq!(ex.refused, None);

        // Mixed.
        let script = Script::new(&[
            ("a", &[Step::RetryAfter(429, 5)]),
            ("b", &[Step::Retry(Some(500))]),
        ]);
        let ran = run_with(settings, &plan_of(&["a", "b"]), &script).await;
        let Err(Stop::Exhausted(ex)) = ran.result else {
            panic!()
        };
        assert_eq!((ex.attempts, ex.rate_limited), (2, 1));
        assert_eq!(ex.retry_after, Some(Duration::from_secs(5)));
        assert_eq!(ex.refused, None);
    }

    #[tokio::test(start_paused = true)]
    async fn no_wait_between_targets() {
        let settings = Settings {
            retries: 0,
            ..SETTINGS
        };
        let script = Script::new(&[
            ("a", &[Step::Retry(None)]),
            ("b", &[Step::Retry(None)]),
            ("c", &[Step::Ok]),
        ]);
        let start = Instant::now();
        let ran = run_with(settings, &plan_of(&["a", "b", "c"]), &script).await;
        assert_eq!(ran.result, Ok("c".to_string()));
        assert_eq!(start.elapsed(), Duration::ZERO);
    }

    #[tokio::test(start_paused = true)]
    async fn targets_that_may_not_be_called_are_skipped_and_never_called() {
        let mut plan = plan_of(&["a", "b"]);
        plan[0].callable = false;
        let script = Script::new(&[("a", &[Step::Ok]), ("b", &[Step::Ok])]);
        let ran = run_with(SETTINGS, &plan, &script).await;
        assert_eq!(ran.result, Ok("b".to_string()));
        assert_eq!(script.calls(), ["b"]);
        assert_eq!(
            seen(&ran.record),
            [
                ("a".into(), AttemptOutcome::Skipped, None),
                ("b".into(), AttemptOutcome::Ok, Some(200)),
            ]
        );
    }

    fn open(health: &InMemoryHealth, model: &str, s: &BreakerSettings) {
        for _ in 0..s.failures {
            health.report(&target(model), false, true, Some(503), Instant::now(), s);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn all_open_is_fast_503() {
        let settings = Settings {
            breaker: BreakerSettings {
                failures: 2,
                ..SETTINGS.breaker
            },
            ..SETTINGS
        };
        let health = Arc::new(InMemoryHealth::new());
        open(&health, "a", &settings.breaker);
        open(&health, "b", &settings.breaker);
        let script = Script::new(&[("a", &[Step::Ok]), ("b", &[Step::Ok])]);
        let start = Instant::now();
        let ran = run_on(health, settings, &plan_of(&["a", "b"]), &script).await;
        assert!(start.elapsed() < Duration::from_millis(100));
        assert!(matches!(ran.result, Err(Stop::Exhausted(_))));
        assert!(script.calls().is_empty());
        assert_eq!(
            seen(&ran.record),
            [
                ("a".into(), AttemptOutcome::CircuitOpen, None),
                ("b".into(), AttemptOutcome::CircuitOpen, None),
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn an_open_breaker_sends_the_call_to_the_next_target() {
        let settings = Settings {
            breaker: BreakerSettings {
                failures: 1,
                ..SETTINGS.breaker
            },
            ..SETTINGS
        };
        let health = Arc::new(InMemoryHealth::new());
        open(&health, "a", &settings.breaker);
        let script = Script::new(&[("a", &[Step::Ok]), ("b", &[Step::Ok])]);
        let ran = run_on(health, settings, &plan_of(&["a", "b"]), &script).await;
        assert_eq!(ran.result, Ok("b".to_string()));
        assert_eq!(script.calls(), ["b"]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_breaker_that_opens_while_retrying_ends_the_retries() {
        let settings = Settings {
            retries: 5,
            breaker: BreakerSettings {
                failures: 2,
                ..SETTINGS.breaker
            },
            ..SETTINGS
        };
        let script = Script::new(&[("a", &[Step::Retry(Some(500))]), ("b", &[Step::Ok])]);
        let ran = run_with(settings, &plan_of(&["a", "b"]), &script).await;
        assert_eq!(ran.result, Ok("b".to_string()));
        assert_eq!(script.calls(), ["a", "a", "b"]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_success_is_reported_to_the_breaker() {
        let script = Script::new(&[("a", &[Step::Retry(Some(500)), Step::Ok])]);
        let ran = run_with(SETTINGS, &plan_of(&["a"]), &script).await;
        let h = &ran.health.view()[0];
        assert_eq!((h.successes, h.failures), (1, 1));
        assert_eq!(h.last_status, Some(500));
    }

    #[tokio::test(start_paused = true)]
    async fn the_total_timeout_ends_the_call_even_in_a_hung_try() {
        let settings = Settings {
            total_timeout: Duration::from_secs(5),
            ..SETTINGS
        };
        let script = Script::new(&[("a", &[Step::Hang]), ("b", &[Step::Ok])]);
        let start = Instant::now();
        let ran = run_with(settings, &plan_of(&["a", "b"]), &script).await;
        assert!(matches!(ran.result, Err(Stop::Exhausted(_))));
        assert_eq!(start.elapsed(), Duration::from_secs(5));
        assert_eq!(script.calls(), ["a"]);
        assert_eq!(
            seen(&ran.record),
            [
                ("a".into(), AttemptOutcome::Retryable, None),
                ("b".into(), AttemptOutcome::Skipped, None),
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn the_limits_given_to_a_try_are_the_settings() {
        let sink = Arc::new(Mem::default());
        let mut scope = Scope::begin(sink, 1, None, None, "chat");
        let health = InMemoryHealth::new();
        let settings = Settings {
            first_token_timeout: Duration::from_secs(7),
            total_timeout: Duration::from_secs(40),
            ..SETTINGS
        };
        let start = Instant::now();
        let got = std::sync::Mutex::new(None);
        let _ = run(
            &health,
            &mut scope,
            &settings,
            &plan_of(&["a"]),
            &mut StdRng::seed_from_u64(1),
            |_, limits| {
                *got.lock().unwrap() = Some(limits);
                async {
                    Ok::<_, Failure<()>>(Success {
                        value: (),
                        status: None,
                    })
                }
            },
        )
        .await;
        let limits = got.lock().unwrap().unwrap();
        assert_eq!(limits.first_token, Duration::from_secs(7));
        assert_eq!(limits.deadline, start + Duration::from_secs(40));
    }

    #[tokio::test(start_paused = true)]
    async fn a_caller_that_goes_away_mid_try_leaves_that_target_retryable() {
        let sink = Arc::new(Mem::default());
        let script = Script::new(&[("a", &[Step::Hang]), ("b", &[Step::Ok])]);
        let plan = plan_of(&["a", "b"]);
        {
            let mut scope = Scope::begin(sink.clone(), 1, None, None, "chat");
            scope.targets(vec![("p".into(), "a".into()), ("p".into(), "b".into())]);
            let health = InMemoryHealth::new();
            let mut rng = StdRng::seed_from_u64(1);
            let fut = run(&health, &mut scope, &SETTINGS, &plan, &mut rng, |t, _| {
                let script = script.clone();
                async move { script.answer(t).await }
            });
            // The caller gives up after a second: the future is dropped.
            let _ = tokio::time::timeout(Duration::from_secs(1), fut).await;
        }
        let r = sink.0.lock().unwrap()[0].clone();
        assert_eq!(r.status, CALLER_GONE);
        assert_eq!(
            seen(&r),
            [
                ("a".into(), AttemptOutcome::Retryable, None),
                ("b".into(), AttemptOutcome::Skipped, None),
            ]
        );
    }
}
