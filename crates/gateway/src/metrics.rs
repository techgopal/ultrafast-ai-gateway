//! Prometheus metrics: the counters of the gateway and `GET /metrics`.
//!
//! One [`Metrics`] lives in the state. The proxy and the telemetry scope
//! update it on the request path with relaxed atomics (the upstream
//! histogram takes a short lock); the counters other parts already keep (the
//! log queue's, the circuit breakers') are read when a scrape comes. The
//! text is written by hand in the exposition format 0.0.4: the format is a
//! page long and a client library would add a dependency and a global
//! registry for nothing.
//!
//! No label holds a key, user, team or prompt: only the endpoint, the
//! status class, the provider and model, and the kind of limit.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use axum::extract::State;
use axum::http::header::{AUTHORIZATION, CACHE_CONTROL, CONTENT_TYPE, WWW_AUTHENTICATE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use sha2::{Digest, Sha256};

use crate::app::AppState;
use crate::errors::error_response;
use crate::logs::LogStats;
use crate::routing::health::TargetHealth;
use crate::routing::TargetState;
use crate::telemetry::{AttemptOutcome, RequestRecord, CALLER_GONE};

/// The content type of the text format.
pub const CONTENT_TYPE_TEXT: &str = "text/plain; version=0.0.4; charset=utf-8";

const ENDPOINTS: [&str; 3] = ["chat", "messages", "embeddings"];
/// `499` is a caller that went away: no answer was sent, so it is no 4xx.
const CLASSES: [&str; 5] = ["2xx", "4xx", "499", "5xx", "other"];
const LIMITS: [&str; 3] = ["requests_per_minute", "tokens_per_minute", "concurrent"];
/// Upper bounds of the upstream duration buckets, in milliseconds.
const BUCKETS_MS: [u64; 12] = [
    50, 100, 250, 500, 1_000, 2_500, 5_000, 10_000, 30_000, 60_000, 120_000, 300_000,
];

#[derive(Default)]
struct Histogram {
    /// Not cumulative: the calls whose duration fell in each bucket.
    buckets: [u64; BUCKETS_MS.len()],
    count: u64,
    sum_ms: u64,
}

impl Histogram {
    fn observe(&mut self, ms: u64) {
        self.count += 1;
        self.sum_ms = self.sum_ms.saturating_add(ms);
        if let Some(i) = BUCKETS_MS.iter().position(|le| ms <= *le) {
            self.buckets[i] += 1;
        }
    }
}

#[derive(Default)]
pub struct Metrics {
    requests: [[AtomicU64; CLASSES.len()]; ENDPOINTS.len()],
    tokens_in: AtomicU64,
    tokens_out: AtomicU64,
    cache_hits: AtomicU64,
    cache_misses: AtomicU64,
    rate_limited: [AtomicU64; LIMITS.len()],
    budget_blocked: AtomicU64,
    /// Per provider.
    upstream: Mutex<BTreeMap<String, Histogram>>,
    /// The counters of the log pipeline, once it exists.
    logs: OnceLock<Arc<LogStats>>,
}

fn class_index(status: u16) -> usize {
    match status {
        CALLER_GONE => 2,
        200..=299 => 0,
        400..=498 => 1,
        500..=599 => 3,
        _ => 4,
    }
}

impl Metrics {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reads the log pipeline's counters (dropped, failed, cost) at scrape
    /// time. Only the first call counts.
    pub fn attach_logs(&self, stats: Arc<LogStats>) {
        let _ = self.logs.set(stats);
    }

    /// Counts one finished call. Called when its scope is emitted.
    pub fn record(&self, record: &RequestRecord) {
        if let Some(e) = ENDPOINTS.iter().position(|e| *e == record.endpoint) {
            self.requests[e][class_index(record.status)].fetch_add(1, Ordering::Relaxed);
        }
        // A cache hit used no provider tokens.
        if let (Some(u), false) = (record.usage, record.cached) {
            self.tokens_in
                .fetch_add(u64::from(u.input_tokens), Ordering::Relaxed);
            self.tokens_out
                .fetch_add(u64::from(u.output_tokens), Ordering::Relaxed);
        }
        // Calls that reached a provider; one that never settled (the caller
        // went away) has neither an answer nor a time.
        let mut calls = record.attempts.iter().filter(|a| {
            matches!(
                a.outcome,
                AttemptOutcome::Ok | AttemptOutcome::Retryable | AttemptOutcome::Fatal
            ) && (a.status.is_some() || a.duration_ms > 0)
        });
        if let Some(first) = calls.next() {
            let mut upstream = self
                .upstream
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for a in std::iter::once(first).chain(calls) {
                upstream
                    .entry(a.provider.clone())
                    .or_default()
                    .observe(a.duration_ms);
            }
        }
    }

    pub fn cache_hit(&self) {
        self.cache_hits.fetch_add(1, Ordering::Relaxed);
    }

    pub fn cache_miss(&self) {
        self.cache_misses.fetch_add(1, Ordering::Relaxed);
    }

    /// A call refused by a rate limit; `limit_name` is the limiter's
    /// `Refusal::limit_name`.
    pub fn rate_limited(&self, limit_name: &str) {
        let i = match limit_name {
            "requests per minute" => 0,
            "tokens per minute" => 1,
            _ => 2,
        };
        self.rate_limited[i].fetch_add(1, Ordering::Relaxed);
    }

    pub fn budget_blocked(&self) {
        self.budget_blocked.fetch_add(1, Ordering::Relaxed);
    }

    /// The exposition text. `health` is what the circuit breakers show.
    pub fn render(&self, health: &[TargetHealth]) -> String {
        let mut out = String::with_capacity(4096);
        let n = |a: &AtomicU64| a.load(Ordering::Relaxed);

        header(&mut out, "uf_requests_total", "counter", "Calls to /v1 that passed authentication, by endpoint and the class of the status the caller got (499: the caller went away).");
        for (e, endpoint) in ENDPOINTS.iter().enumerate() {
            for (c, class) in CLASSES.iter().enumerate() {
                let _ = writeln!(
                    out,
                    "uf_requests_total{{endpoint=\"{endpoint}\",status_class=\"{class}\"}} {}",
                    n(&self.requests[e][c])
                );
            }
        }

        header(
            &mut out,
            "uf_tokens_total",
            "counter",
            "Tokens the providers reported, by direction. Answers from the cache are not counted.",
        );
        let _ = writeln!(
            out,
            "uf_tokens_total{{direction=\"input\"}} {}",
            n(&self.tokens_in)
        );
        let _ = writeln!(
            out,
            "uf_tokens_total{{direction=\"output\"}} {}",
            n(&self.tokens_out)
        );

        header(
            &mut out,
            "uf_cost_micros_total",
            "counter",
            "Cost of priced calls in millionths of a dollar, as written to the request logs.",
        );
        let cost = self.logs.get().map_or(0, |l| n(&l.cost_micros));
        let _ = writeln!(out, "uf_cost_micros_total {cost}");

        header(
            &mut out,
            "uf_upstream_duration_seconds",
            "histogram",
            "Time of each call to a provider, by provider.",
        );
        {
            let upstream = self
                .upstream
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for (provider, h) in upstream.iter() {
                let provider = escape_label(provider);
                let mut cumulative = 0;
                for (i, le) in BUCKETS_MS.iter().enumerate() {
                    cumulative += h.buckets[i];
                    let _ = writeln!(
                        out,
                        "uf_upstream_duration_seconds_bucket{{provider=\"{provider}\",le=\"{}\"}} {cumulative}",
                        seconds(*le)
                    );
                }
                let _ = writeln!(
                    out,
                    "uf_upstream_duration_seconds_bucket{{provider=\"{provider}\",le=\"+Inf\"}} {}",
                    h.count
                );
                let _ = writeln!(
                    out,
                    "uf_upstream_duration_seconds_sum{{provider=\"{provider}\"}} {}",
                    seconds(h.sum_ms)
                );
                let _ = writeln!(
                    out,
                    "uf_upstream_duration_seconds_count{{provider=\"{provider}\"}} {}",
                    h.count
                );
            }
        }

        header(
            &mut out,
            "uf_log_records_dropped_total",
            "counter",
            "Request records the log queue did not take because it was full.",
        );
        let _ = writeln!(
            out,
            "uf_log_records_dropped_total {}",
            self.logs.get().map_or(0, |l| n(&l.dropped))
        );
        header(
            &mut out,
            "uf_log_write_failures_total",
            "counter",
            "Request records lost because the database refused their batch twice.",
        );
        let _ = writeln!(
            out,
            "uf_log_write_failures_total {}",
            self.logs.get().map_or(0, |l| n(&l.write_failures))
        );

        header(
            &mut out,
            "uf_cache_hits_total",
            "counter",
            "Calls answered from the response cache.",
        );
        let _ = writeln!(out, "uf_cache_hits_total {}", n(&self.cache_hits));
        header(
            &mut out,
            "uf_cache_misses_total",
            "counter",
            "Calls on a route with the cache on that the cache could not answer.",
        );
        let _ = writeln!(out, "uf_cache_misses_total {}", n(&self.cache_misses));

        header(
            &mut out,
            "uf_rate_limited_total",
            "counter",
            "Calls refused by a rate limit, by the kind of limit reached.",
        );
        for (i, limit) in LIMITS.iter().enumerate() {
            let _ = writeln!(
                out,
                "uf_rate_limited_total{{limit=\"{limit}\"}} {}",
                n(&self.rate_limited[i])
            );
        }

        header(
            &mut out,
            "uf_budget_blocked_total",
            "counter",
            "Calls refused because a budget was spent.",
        );
        let _ = writeln!(out, "uf_budget_blocked_total {}", n(&self.budget_blocked));

        header(&mut out, "uf_circuit_open", "gauge", "1 while the circuit breaker of a target refuses calls, else 0. Targets that were never called are not listed.");
        for t in health {
            let _ = writeln!(
                out,
                "uf_circuit_open{{provider=\"{}\",model=\"{}\"}} {}",
                escape_label(&t.provider),
                escape_label(&t.model),
                u8::from(t.state == TargetState::Open)
            );
        }
        out
    }
}

fn header(out: &mut String, name: &str, kind: &str, help: &str) {
    let _ = writeln!(out, "# HELP {name} {help}");
    let _ = writeln!(out, "# TYPE {name} {kind}");
}

/// Milliseconds as seconds, without trailing zeros: `0.05`, `1`, `2.5`.
fn seconds(ms: u64) -> String {
    let whole = ms / 1000;
    let frac = ms % 1000;
    if frac == 0 {
        whole.to_string()
    } else {
        format!("{whole}.{frac:03}")
            .trim_end_matches('0')
            .to_string()
    }
}

/// A label value as the format wants it: `\`, `"` and newline escaped.
pub fn escape_label(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out
}

/// Whether `presented` is `token`, without the time telling how much of it
/// matched or how long it is: both are hashed and the digests compared
/// byte by byte without stopping early.
fn token_matches(token: &str, presented: &str) -> bool {
    let a = Sha256::digest(token.as_bytes());
    let b = Sha256::digest(presented.as_bytes());
    a.iter().zip(b.iter()).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    scheme
        .eq_ignore_ascii_case("bearer")
        .then_some(token.trim())
}

/// `GET /metrics`. Without a configured token the path does not exist; with
/// one, the caller must present it as a bearer token.
pub async fn serve(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let Some(token) = state.metrics_token.as_deref() else {
        return error_response(
            StatusCode::NOT_FOUND,
            "invalid_request_error",
            "Unknown path.",
        );
    };
    let allowed = bearer(&headers).is_some_and(|presented| token_matches(token, presented));
    if !allowed {
        let mut response = error_response(
            StatusCode::UNAUTHORIZED,
            "authentication_error",
            "A metrics token is required.",
        );
        response
            .headers_mut()
            .insert(WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        return response;
    }
    let body = state.metrics.render(&state.health.view());
    let mut response = body.into_response();
    let h = response.headers_mut();
    h.insert(CONTENT_TYPE, HeaderValue::from_static(CONTENT_TYPE_TEXT));
    h.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seconds_are_written_without_trailing_zeros() {
        assert_eq!(seconds(50), "0.05");
        assert_eq!(seconds(250), "0.25");
        assert_eq!(seconds(1_000), "1");
        assert_eq!(seconds(2_500), "2.5");
        assert_eq!(seconds(1_234), "1.234");
        assert_eq!(seconds(0), "0");
    }

    #[test]
    fn statuses_fall_in_their_class() {
        assert_eq!(CLASSES[class_index(200)], "2xx");
        assert_eq!(CLASSES[class_index(404)], "4xx");
        assert_eq!(CLASSES[class_index(499)], "499");
        assert_eq!(CLASSES[class_index(503)], "5xx");
        assert_eq!(CLASSES[class_index(302)], "other");
    }

    #[test]
    fn a_duration_over_the_last_bucket_counts_only_in_inf() {
        let mut h = Histogram::default();
        h.observe(400_000);
        h.observe(10);
        assert_eq!(h.count, 2);
        assert_eq!(h.buckets.iter().sum::<u64>(), 1);
    }
}
