//! The record of a `/v1` call and where it goes.
//!
//! The proxy makes one [`RequestRecord`] per call that passed authentication
//! and hands it to a [`RequestSink`]. Plan 6's request logs implement the
//! sink. A record holds no prompt, no answer and no credential.

use std::sync::Arc;
use std::time::Instant;

use ultrafast_translate::types::Usage;

use crate::alerts::errors_window::Sample;
use crate::alerts::EngineHandle;
use crate::guardrails::log::{GuardrailLog, SideLog};
use crate::guardrails::Direction;
use crate::limits::Permit;
use crate::metrics::Metrics;
use crate::otel::{Exporter, TraceParent};
use crate::store;
use crate::tags::Tags;

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
    /// Milliseconds from the start of the call to the start of the attempt.
    /// Only the trace export reads it; the request log does not store it.
    pub offset_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RequestRecord {
    /// `None` for a call made without a key (the console playground).
    pub key_id: Option<i64>,
    pub user_id: Option<i64>,
    pub team_id: Option<i64>,
    /// The model or route name the caller asked for; empty when the body
    /// could not be read far enough to tell.
    pub requested: String,
    /// `"chat"`, `"messages"`, `"embeddings"` or `"playground"`.
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
    /// `usage` is an estimate: a stream ended without the provider's report
    /// (the caller went away, or an error came after content was sent).
    pub estimated: bool,
    pub started_at: String,
    pub duration_ms: u64,
    /// What the call sent in `x-uf-tags`, overlaid by the tags of its key.
    pub tags: Tags,
    /// The incoming W3C `traceparent` of the call, when it had a valid one.
    /// Only the trace export reads it; the request log does not store it.
    pub trace_parent: Option<TraceParent>,
    /// The kind (`openai`, `anthropic`, ...) of each provider the call may
    /// try, by provider name, for the trace export.
    pub provider_kinds: Vec<(String, &'static str)>,
    /// Wall-clock start of the call, milliseconds since the epoch: the
    /// precise form of `started_at`, for the trace export only.
    pub started_unix_ms: u64,
    /// What the guardrail checks found, when they found anything. Counts and
    /// names only, never the text that matched.
    pub guardrails: Option<GuardrailLog>,
}

/// The longest `requested` a record keeps, in bytes. The name is whatever the
/// caller sent (also for models that do not exist); every queue that holds a
/// record would otherwise hold it whole.
pub const MAX_REQUESTED: usize = 256;

/// `text` cut to at most `max` bytes, on a char boundary.
fn truncated(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
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
    /// Set when a stream is handed to the caller: the input tokens it is
    /// estimated at, for a stream that ends without a usage report.
    stream_input: Option<u64>,
    /// Characters of answer streamed to the caller so far.
    streamed_chars: u64,
    /// Where a copy of the record goes to become a trace, when enabled.
    otel: Option<Exporter>,
    /// Counts the call toward error rates, when alert rules exist.
    alerts: Option<EngineHandle>,
    /// Set once the name the caller asked for resolved to something it may
    /// call: only such calls are counted toward error rates.
    resolved: bool,
    /// The configured route it resolved to, if it is a route.
    resolved_route: Option<String>,
    /// The answer was cut off by a guardrail while it streamed: the provider
    /// kept generating, so the call is charged an estimate if it has no usage.
    cut_short: bool,
}

impl Scope {
    pub fn begin(
        sink: Arc<dyn RequestSink>,
        key_id: Option<i64>,
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
            stream_input: None,
            streamed_chars: 0,
            otel: None,
            alerts: None,
            resolved: false,
            resolved_route: None,
            cut_short: false,
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
                estimated: false,
                started_at: store::now(),
                duration_ms: 0,
                tags: Tags::new(),
                trace_parent: None,
                provider_kinds: Vec::new(),
                started_unix_ms: unix_ms_now(),
                guardrails: None,
            }),
        }
    }

    /// The call is counted in `metrics` when it is recorded.
    pub fn metered(&mut self, metrics: Arc<Metrics>) {
        self.metrics = Some(metrics);
    }

    /// The call is exported as a trace by `exporter` when it is recorded.
    pub fn traced(&mut self, exporter: Option<Exporter>) {
        self.otel = exporter;
    }

    /// The call is counted toward error rates by `engine` when it is recorded.
    pub fn watched(&mut self, engine: Option<EngineHandle>) {
        self.alerts = engine;
    }

    /// The name the caller asked for resolved: to the configured route
    /// `route`, or (none) to a `provider/model`. Calls that never get here
    /// (unknown names, refused ones) are not counted toward error rates, so a
    /// caller cannot make subjects up.
    pub fn resolved(&mut self, route: Option<&str>) {
        self.resolved = true;
        // The name is kept only while a rule reads it: no copy per call
        // otherwise.
        self.resolved_route = route
            .filter(|_| self.alerts.as_ref().is_some_and(EngineHandle::is_active))
            .map(str::to_string);
    }

    /// The `traceparent` the caller sent: the call's trace continues it.
    pub fn parented(&mut self, parent: TraceParent) {
        self.record_mut().trace_parent = Some(parent);
    }

    /// What the guardrails found in one direction of the call (nothing found
    /// leaves the record as it is).
    pub fn guardrails_found(&mut self, dir: Direction, side: Option<SideLog>) {
        let r = self.record_mut();
        r.guardrails = GuardrailLog::with(r.guardrails.take(), dir, side);
    }

    /// What the guardrails found in the output of the call so far.
    pub fn output_guardrails(&self) -> Option<SideLog> {
        self.record.as_ref()?.guardrails.as_ref()?.output.clone()
    }

    /// A guardrail ended the answer while it streamed.
    pub fn cut_short(&mut self) {
        self.cut_short = true;
    }

    /// The kind of each provider the call may try, by provider name.
    pub fn provider_kinds(&mut self, kinds: Vec<(String, &'static str)>) {
        self.record_mut().provider_kinds = kinds;
    }

    fn offset_of(&self, started: Instant) -> u64 {
        u64::try_from(started.saturating_duration_since(self.started).as_millis())
            .unwrap_or(u64::MAX)
    }

    fn record_mut(&mut self) -> &mut RequestRecord {
        self.record.as_mut().expect("a scope is emitted only once")
    }

    /// The tags the call is recorded with.
    pub fn tagged(&mut self, tags: Tags) {
        self.record_mut().tags = tags;
    }

    pub fn requested(&mut self, name: &str, stream: bool) {
        let r = self.record_mut();
        r.requested = truncated(name, MAX_REQUESTED).to_string();
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
        let offset_ms = self.offset_of(started);
        self.record_mut().attempts.push(Attempt {
            provider: provider.to_string(),
            model: model.to_string(),
            outcome,
            status,
            duration_ms: elapsed_ms(started),
            offset_ms,
        });
    }

    /// Starts an attempt that has not settled: a call that is out. Until
    /// [`settle_attempt`](Self::settle_attempt) it reads as retryable with no
    /// status, which is what a caller that goes away leaves behind.
    pub fn begin_attempt(&mut self, provider: &str, model: &str) {
        let offset_ms = self.offset_of(Instant::now());
        self.record_mut().attempts.push(Attempt {
            provider: provider.to_string(),
            model: model.to_string(),
            outcome: AttemptOutcome::Retryable,
            status: None,
            duration_ms: 0,
            offset_ms,
        });
    }

    /// What the attempt that began last came to.
    pub fn settle_attempt(
        &mut self,
        outcome: AttemptOutcome,
        status: Option<u16>,
        started: Instant,
    ) {
        let offset_ms = self.offset_of(started);
        if let Some(a) = self.record_mut().attempts.last_mut() {
            a.outcome = outcome;
            a.status = status;
            a.duration_ms = elapsed_ms(started);
            a.offset_ms = offset_ms;
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

    /// A stream is handed to the caller. If it ends without the provider's
    /// usage after content, or the caller leaves, the call is charged an
    /// estimate: `input_tokens` in, and the streamed characters / 4 out.
    pub fn begin_stream(&mut self, input_tokens: u64) {
        self.stream_input = Some(input_tokens);
    }

    /// `chars` characters of the answer were sent to the caller.
    pub fn streamed(&mut self, chars: usize) {
        self.streamed_chars = self.streamed_chars.saturating_add(chars as u64);
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
            offset_ms: 0,
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
                        // Passed over when the call ended.
                        offset_ms: elapsed_ms(self.started),
                    });
                }
            }
            record.status = status;
            record.duration_ms = elapsed_ms(self.started);
            // A stream that ended without the provider's report, because the
            // caller left or an error came after content, was still paid for:
            // it is charged an estimate, marked as one.
            if let (None, Some(input)) = (record.usage, self.stream_input) {
                let ended_early = status == CALLER_GONE
                    || self.cut_short
                    || record
                        .attempts
                        .last()
                        .is_some_and(|a| a.outcome != AttemptOutcome::Ok);
                if ended_early
                    && (status == CALLER_GONE || self.cut_short || self.streamed_chars > 0)
                {
                    let to_u32 = |n: u64| u32::try_from(n).unwrap_or(u32::MAX);
                    let usage = Usage {
                        input_tokens: to_u32(input),
                        output_tokens: to_u32(self.streamed_chars.div_ceil(4)),
                    };
                    record.usage = Some(usage);
                    record.estimated = true;
                    if let Some(permit) = self.permit.as_mut() {
                        permit
                            .settle(u64::from(usage.input_tokens) + u64::from(usage.output_tokens));
                    }
                }
            }
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
            // Offered before the sink takes the record; never blocks.
            if let Some(otel) = &self.otel {
                otel.offer(&record);
            }
            if let Some(engine) = self
                .alerts
                .as_ref()
                .filter(|e| self.resolved && e.is_active())
            {
                if let Some(sample) = alert_sample(&record, self.resolved_route.as_deref()) {
                    engine.observe(&sample);
                }
            }
            self.sink.record(record);
        }
        self.permit = None;
    }
}

/// How a finished call counts toward error rates: `None` for a caller that
/// went away (it says nothing about the gateway). It is an error when the
/// caller was answered with a server error (5xx), or with a 429 that the
/// provider gave (the last attempt that reached a provider answered 429); a
/// 429 of the gateway's own limits and budgets, and any other 4xx, are the
/// caller's.
pub fn alert_sample<'a>(record: &'a RequestRecord, route: Option<&'a str>) -> Option<Sample<'a>> {
    if record.status == CALLER_GONE {
        return None;
    }
    let last = record.attempts.iter().rev().find(|a| {
        !matches!(
            a.outcome,
            AttemptOutcome::Skipped | AttemptOutcome::Cached | AttemptOutcome::CircuitOpen
        )
    });
    let error = record.status >= 500
        || (record.status == 429 && last.is_some_and(|a| a.status == Some(429)));
    Some(Sample {
        route,
        provider: last.map(|a| a.provider.as_str()),
        key_id: record.key_id,
        error,
    })
}

impl Drop for Scope {
    fn drop(&mut self) {
        self.emit(CALLER_GONE);
    }
}

fn unix_ms_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
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
        Scope::begin(sink.clone(), Some(7), Some(1), None, "chat")
    }

    fn attempt(provider: &str, outcome: AttemptOutcome, status: Option<u16>) -> Attempt {
        Attempt {
            provider: provider.into(),
            model: "m".into(),
            outcome,
            status,
            duration_ms: 1,
            offset_ms: 0,
        }
    }

    fn finished(status: u16, attempts: Vec<Attempt>) -> RequestRecord {
        let sink = Arc::new(Mem::default());
        let mut s = scope(&sink);
        s.requested("r", false);
        for a in attempts {
            s.attempt(&a.provider, &a.model, a.outcome, a.status, Instant::now());
        }
        s.finish(status);
        let record = sink.0.lock().unwrap().remove(0);
        record
    }

    #[test]
    fn what_counts_as_an_error_for_alerts() {
        let sample = |r: &RequestRecord| alert_sample(r, Some("r")).map(|s| s.error);
        // Server errors are errors.
        for status in [500, 502, 503, 504] {
            let r = finished(
                status,
                vec![attempt("p", AttemptOutcome::Retryable, Some(500))],
            );
            assert_eq!(sample(&r), Some(true), "{status}");
        }
        // A 429 the provider gave is one; the gateway's own 429 is not.
        let r = finished(
            429,
            vec![attempt("p", AttemptOutcome::Retryable, Some(429))],
        );
        assert_eq!(sample(&r), Some(true));
        assert_eq!(alert_sample(&r, Some("r")).unwrap().provider, Some("p"));
        let r = finished(429, vec![]);
        assert_eq!(
            sample(&r),
            Some(false),
            "a rate limit or budget of the gateway"
        );
        let r = finished(
            429,
            vec![
                attempt("p", AttemptOutcome::Retryable, Some(503)),
                attempt("q", AttemptOutcome::Skipped, None),
            ],
        );
        assert_eq!(
            sample(&r),
            Some(false),
            "the last call to a provider was not a 429"
        );
        // The caller's mistakes and successes are not.
        for status in [200, 400, 401, 403, 404, 413, 422] {
            let r = finished(status, vec![attempt("p", AttemptOutcome::Ok, Some(200))]);
            assert_eq!(sample(&r), Some(false), "{status}");
        }
        // A caller that went away is not counted at all.
        assert!(alert_sample(&finished(CALLER_GONE, vec![]), None).is_none());
    }

    #[test]
    fn the_provider_is_that_of_the_last_attempt_that_reached_one() {
        let r = finished(
            503,
            vec![
                attempt("a", AttemptOutcome::Retryable, Some(500)),
                attempt("b", AttemptOutcome::Retryable, Some(503)),
                attempt("c", AttemptOutcome::CircuitOpen, None),
            ],
        );
        assert_eq!(alert_sample(&r, Some("r")).unwrap().provider, Some("b"));
        let r = finished(200, vec![attempt("a", AttemptOutcome::Cached, None)]);
        assert_eq!(alert_sample(&r, Some("r")).unwrap().provider, None);
        assert_eq!(alert_sample(&r, Some("r")).unwrap().key_id, Some(7));
        assert_eq!(alert_sample(&r, Some("r")).unwrap().route, Some("r"));
        assert_eq!(alert_sample(&r, None).unwrap().route, None);
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
        assert_eq!(records[0].key_id, Some(7));
        assert_eq!(records[0].requested, "p/m");
        assert!(records[0].stream);
    }

    #[test]
    fn a_huge_requested_name_is_cut_on_a_char_boundary() {
        let sink = Arc::new(Mem::default());
        // 1 MiB of a two-byte char: the cut must not split one.
        let mut s = scope(&sink);
        s.requested(&"\u{e9}".repeat(512 * 1024), false);
        s.finish(404);
        let mut s = scope(&sink);
        s.requested(&format!("a{}", "\u{e9}".repeat(512 * 1024)), false);
        s.finish(404);
        let mut s = scope(&sink);
        s.requested(&"x".repeat(1024 * 1024), false);
        s.finish(404);
        let mut s = scope(&sink);
        s.requested("p/m", false);
        s.finish(200);
        let records = sink.0.lock().unwrap();
        for r in &records[..3] {
            assert!(r.requested.len() <= 256, "{}", r.requested.len());
            assert!(r.requested.len() >= 254);
        }
        assert_eq!(records[3].requested, "p/m");
    }

    #[test]
    fn the_route_name_is_kept_only_while_an_error_rate_rule_reads_it() {
        let sink = Arc::new(Mem::default());
        let (engine, _rx) = EngineHandle::unread(4);
        engine.windows().set_active(false);
        let mut s = scope(&sink);
        s.watched(Some(engine.clone()));
        s.resolved(Some("chat"));
        assert_eq!(s.resolved_route, None, "no rule: nothing is copied");
        engine.windows().set_active(true);
        s.resolved(Some("chat"));
        assert_eq!(s.resolved_route.as_deref(), Some("chat"));
        s.resolved(None);
        assert_eq!(s.resolved_route, None);
        s.finish(200);
        let mut no_engine = scope(&sink);
        no_engine.resolved(Some("chat"));
        assert_eq!(no_engine.resolved_route, None);
        no_engine.finish(200);
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
    fn attempts_record_their_offset_from_the_start_of_the_call() {
        let sink = Arc::new(Mem::default());
        let mut s = scope(&sink);
        std::thread::sleep(std::time::Duration::from_millis(30));
        s.begin_attempt("a", "m1");
        let started = Instant::now();
        s.settle_attempt(AttemptOutcome::Ok, Some(200), started);
        s.finish(200);
        let r = sink.0.lock().unwrap()[0].clone();
        assert!(r.attempts[0].offset_ms >= 30, "{}", r.attempts[0].offset_ms);
        assert!(r.attempts[0].offset_ms < 5_000);
    }

    #[test]
    fn a_target_passed_over_is_stamped_when_the_call_ended() {
        let sink = Arc::new(Mem::default());
        let mut s = scope(&sink);
        s.targets(vec![("b".into(), "m2".into())]);
        std::thread::sleep(std::time::Duration::from_millis(30));
        s.finish(200);
        let r = sink.0.lock().unwrap()[0].clone();
        assert_eq!(r.attempts[0].outcome, AttemptOutcome::Skipped);
        assert!(r.attempts[0].offset_ms >= 30);
    }

    #[test]
    fn the_noop_sink_accepts_records() {
        drop(Scope::begin(
            Arc::new(NoopSink),
            Some(1),
            None,
            None,
            "chat",
        ));
    }
}
