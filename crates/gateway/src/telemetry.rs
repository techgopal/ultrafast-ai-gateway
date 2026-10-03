//! The record of a `/v1` call and where it goes.
//!
//! The proxy makes one [`RequestRecord`] per call that passed authentication
//! and hands it to a [`RequestSink`]. Plan 6's request logs implement the
//! sink. A record holds no prompt, no answer and no credential.

use std::sync::Arc;
use std::time::Instant;

use ultrafast_translate::types::Usage;

use crate::limits::Permit;
use crate::metrics::Metrics;
use crate::store;

/// How one try at a target ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptOutcome {
    Ok,
    /// Another try, or another target, may do better.
    Retryable,
    /// The request itself or the provider's answer cannot be helped by a retry.
    Fatal,
    /// The target was not called because its breaker is open.
    CircuitOpen,
    /// The target was passed over: an earlier one answered.
    Skipped,
    /// The answer came from the response cache; no provider was called.
    /// The target is the one that gave the answer that was kept.
    Cached,
}

/// One target of a call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    pub provider: String,
    pub model: String,
    pub outcome: AttemptOutcome,
    /// What the provider answered, when it did.
    pub status: Option<u16>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RequestRecord {
    pub key_id: i64,
    pub user_id: Option<i64>,
    pub team_id: Option<i64>,
    /// The model or route name the caller asked for; empty when the body
    /// could not be read far enough to tell.
    pub requested: String,
    /// `"chat"`, `"messages"` or `"embeddings"`.
    pub endpoint: &'static str,
    pub stream: bool,
    /// What the caller was answered. A caller that went away before the
    /// answer ended is recorded as 499.
    pub status: u16,
    pub usage: Option<Usage>,
    pub attempts: Vec<Attempt>,
    /// Answered from the response cache: the usage is that of the answer
    /// that was kept, and the call cost nothing.
    pub cached: bool,
    pub started_at: String,
    pub duration_ms: u64,
}

/// Receives the records. `record` is called on the request path and must not
/// block: a sink that does work hands it to a task or a channel.
pub trait RequestSink: Send + Sync {
    fn record(&self, record: RequestRecord);
}

/// The default sink: drops every record.
pub struct NoopSink;

impl RequestSink for NoopSink {
    fn record(&self, _record: RequestRecord) {}
}

/// The status of a call whose caller went away before it was answered.
pub const CALLER_GONE: u16 = 499;

/// A record being built. It is emitted by [`finish`](Self::finish), or by
/// being dropped, which is how a call the caller abandons is still recorded.
pub struct Scope {
    sink: Arc<dyn RequestSink>,
    record: Option<RequestRecord>,
    /// Targets that were to be tried, in order; the ones with no attempt
    /// when the record is emitted are recorded as skipped.
    targets: Vec<(String, String)>,
    started: Instant,
    /// The rate-limit permit of the call. It goes with the scope: released
    /// when the call is recorded, whether it ended, failed or was dropped.
    permit: Option<Permit>,
    /// Counts the call when it is emitted.
    metrics: Option<Arc<Metrics>>,
}

impl Scope {
    pub fn begin(
        sink: Arc<dyn RequestSink>,
        key_id: i64,
        user_id: Option<i64>,
        team_id: Option<i64>,
        endpoint: &'static str,
    ) -> Self {
        Self {
            sink,
            targets: Vec::new(),
            started: Instant::now(),
            permit: None,
            metrics: None,
            record: Some(RequestRecord {
                key_id,
                user_id,
                team_id,
                requested: String::new(),
                endpoint,
                stream: false,
                status: CALLER_GONE,
                usage: None,
                attempts: Vec::new(),
                cached: false,
                started_at: store::now(),
                duration_ms: 0,
            }),
        }
    }

    /// The call is counted in `metrics` when it is recorded.
    pub fn metered(&mut self, metrics: Arc<Metrics>) {
        self.metrics = Some(metrics);
    }

    fn record_mut(&mut self) -> &mut RequestRecord {
        self.record.as_mut().expect("a scope is emitted only once")
    }

    pub fn requested(&mut self, name: &str, stream: bool) {
        let r = self.record_mut();
        r.requested = name.to_string();
        r.stream = stream;
    }

    pub fn attempt(
        &mut self,
        provider: &str,
        model: &str,
        outcome: AttemptOutcome,
        status: Option<u16>,
        started: Instant,
    ) {
        self.record_mut().attempts.push(Attempt {
            provider: provider.to_string(),
            model: model.to_string(),
            outcome,
            status,
            duration_ms: elapsed_ms(started),
        });
    }

    /// Starts an attempt that has not settled: a call that is out. Until
    /// [`settle_attempt`](Self::settle_attempt) it reads as retryable with no
    /// status, which is what a caller that goes away leaves behind.
    pub fn begin_attempt(&mut self, provider: &str, model: &str) {
        self.record_mut().attempts.push(Attempt {
            provider: provider.to_string(),
            model: model.to_string(),
            outcome: AttemptOutcome::Retryable,
            status: None,
            duration_ms: 0,
        });
    }

    /// What the attempt that began last came to.
    pub fn settle_attempt(
        &mut self,
        outcome: AttemptOutcome,
        status: Option<u16>,
        started: Instant,
    ) {
        if let Some(a) = self.record_mut().attempts.last_mut() {
            a.outcome = outcome;
            a.status = status;
            a.duration_ms = elapsed_ms(started);
        }
    }

    /// The `(provider, model)` of every target the call may try, in order.
    pub fn targets(&mut self, targets: Vec<(String, String)>) {
        self.targets = targets;
    }

    /// Makes the call hold a rate-limit permit until it is recorded.
    pub fn hold(&mut self, permit: Permit) {
        self.permit = Some(permit);
    }

    /// The call is refused before any provider is called: its request and
    /// token estimate are given back to the rate limits.
    pub fn refund_permit(&mut self) {
        if let Some(permit) = self.permit.as_mut() {
            permit.refund();
        }
    }

    pub fn usage(&mut self, usage: Option<Usage>) {
        // The tokens the call used replace the estimate it was charged.
        if let (Some(permit), Some(u)) = (self.permit.as_mut(), usage) {
            permit.settle(u64::from(u.input_tokens) + u64::from(u.output_tokens));
        }
        self.record_mut().usage = usage;
    }

    /// The target that answered the call: the last attempt that was `Ok`.
    pub fn answered_by(&self) -> Option<(String, String)> {
        self.record
            .as_ref()?
            .attempts
            .iter()
            .rev()
            .find(|a| a.outcome == AttemptOutcome::Ok)
            .map(|a| (a.provider.clone(), a.model.clone()))
    }

    /// The call was answered from the cache by the answer that `provider`
    /// and `model` gave. A hit used no provider tokens: the tokens the call
    /// was charged against the rate limit are given back, and `usage` is
    /// that of the answer, for the record.
    pub fn cache_hit(&mut self, provider: &str, model: &str, usage: Option<Usage>) {
        if let Some(permit) = self.permit.as_mut() {
            permit.settle(0);
        }
        let r = self.record_mut();
        r.cached = true;
        r.usage = usage;
        r.attempts.push(Attempt {
            provider: provider.to_string(),
            model: model.to_string(),
            outcome: AttemptOutcome::Cached,
            status: None,
            duration_ms: 0,
        });
    }

    /// Changes the outcome of the last attempt: a stream whose end is not
    /// known when it begins.
    pub fn set_last_outcome(&mut self, outcome: AttemptOutcome) {
        if let Some(a) = self.record_mut().attempts.last_mut() {
            a.outcome = outcome;
        }
    }

    /// Fixes the duration of the last attempt to now.
    pub fn end_last_attempt(&mut self, started: Instant) {
        if let Some(a) = self.record_mut().attempts.last_mut() {
            a.duration_ms = elapsed_ms(started);
        }
    }

    pub fn finish(mut self, status: u16) {
        self.emit(status);
    }

    fn emit(&mut self, status: u16) {
        if let Some(mut record) = self.record.take() {
            for (provider, model) in std::mem::take(&mut self.targets) {
                let tried = record
                    .attempts
                    .iter()
                    .any(|a| a.provider == provider && a.model == model);
                if !tried {
                    record.attempts.push(Attempt {
                        provider,
                        model,
                        outcome: AttemptOutcome::Skipped,
                        status: None,
                        duration_ms: 0,
                    });
                }
            }
            record.status = status;
            record.duration_ms = elapsed_ms(self.started);
            // A call that failed before any tokens were used gives its
            // estimate back. A caller that went away may have used some.
            if let Some(permit) = self.permit.as_mut() {
                if record.usage.is_none() && status >= 400 && status != CALLER_GONE {
                    permit.settle(0);
                }
            }
            if let Some(metrics) = &self.metrics {
                metrics.record(&record);
            }
            self.sink.record(record);
        }
        self.permit = None;
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        self.emit(CALLER_GONE);
    }
}

pub fn elapsed_ms(since: Instant) -> u64 {
    u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Mem(Mutex<Vec<RequestRecord>>);
    impl RequestSink for Mem {
        fn record(&self, record: RequestRecord) {
            self.0.lock().unwrap().push(record);
        }
    }

    fn scope(sink: &Arc<Mem>) -> Scope {
        Scope::begin(sink.clone(), 7, Some(1), None, "chat")
    }

    #[test]
    fn finish_emits_once_with_the_status() {
        let sink = Arc::new(Mem::default());
        let mut s = scope(&sink);
        s.requested("p/m", true);
        s.finish(200);
        let records = sink.0.lock().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, 200);
        assert_eq!(records[0].key_id, 7);
        assert_eq!(records[0].requested, "p/m");
        assert!(records[0].stream);
    }

    #[test]
    fn dropping_emits_a_record_for_a_gone_caller() {
        let sink = Arc::new(Mem::default());
        drop(scope(&sink));
        let records = sink.0.lock().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, CALLER_GONE);
    }

    #[test]
    fn attempts_keep_their_order_and_skipped_has_no_status() {
        let sink = Arc::new(Mem::default());
        let mut s = scope(&sink);
        s.attempt(
            "a",
            "m1",
            AttemptOutcome::Retryable,
            Some(503),
            Instant::now(),
        );
        s.targets(vec![("a".into(), "m1".into()), ("b".into(), "m2".into())]);
        s.finish(502);
        let r = sink.0.lock().unwrap()[0].clone();
        assert_eq!(r.attempts[0].outcome, AttemptOutcome::Retryable);
        assert_eq!(r.attempts[1].outcome, AttemptOutcome::Skipped);
        assert_eq!(r.attempts[1].status, None);
    }

    #[test]
    fn an_attempt_that_never_settles_is_retryable_without_a_status() {
        let sink = Arc::new(Mem::default());
        let mut s = scope(&sink);
        s.targets(vec![("a".into(), "m1".into()), ("b".into(), "m2".into())]);
        s.begin_attempt("a", "m1");
        drop(s);
        let r = sink.0.lock().unwrap()[0].clone();
        assert_eq!(r.status, CALLER_GONE);
        assert_eq!(r.attempts.len(), 2);
        assert_eq!(r.attempts[0].outcome, AttemptOutcome::Retryable);
        assert_eq!(r.attempts[0].status, None);
        assert_eq!(r.attempts[1].outcome, AttemptOutcome::Skipped);
    }

    #[test]
    fn a_settled_attempt_keeps_what_it_came_to() {
        let sink = Arc::new(Mem::default());
        let mut s = scope(&sink);
        s.begin_attempt("a", "m1");
        s.settle_attempt(AttemptOutcome::Ok, Some(200), Instant::now());
        s.begin_attempt("a", "m1");
        s.settle_attempt(AttemptOutcome::Fatal, Some(400), Instant::now());
        s.finish(400);
        let r = sink.0.lock().unwrap()[0].clone();
        let seen: Vec<_> = r.attempts.iter().map(|a| (a.outcome, a.status)).collect();
        assert_eq!(
            seen,
            [
                (AttemptOutcome::Ok, Some(200)),
                (AttemptOutcome::Fatal, Some(400))
            ]
        );
    }

    #[test]
    fn the_noop_sink_accepts_records() {
        drop(Scope::begin(Arc::new(NoopSink), 1, None, None, "chat"));
    }
}
