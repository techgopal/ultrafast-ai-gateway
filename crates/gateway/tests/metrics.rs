//! `GET /metrics`: the token, the text format, and counters that move with
//! traffic, with nothing about who made the calls.

mod common;

use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use common::{
    harness_with_metrics_token, harness_with_rate, harness_with_sink, post_chat, post_to, Harness,
};
use serde_json::json;
use time::OffsetDateTime;
use tokio::sync::watch;
use tower::ServiceExt;
use ultrafast_gateway::budgets::{account, BudgetAction, Period};
use ultrafast_gateway::cache::{CacheScope, RouteCache};
use ultrafast_gateway::identity::{Role, TeamRole};
use ultrafast_gateway::limits::{LimitScope, Limiter, Permit, Refusal, Subjects};
use ultrafast_gateway::logs::writer::{spawn, WriterConfig};
use ultrafast_gateway::logs::{LogSink, Price, PriceLookup};
use ultrafast_gateway::metrics::escape_label;
use ultrafast_gateway::routing::{BreakerSettings, TargetRef};
use ultrafast_gateway::secrets::generate_key;
use ultrafast_gateway::store::{RouteSettings, TargetsInput};
use ultrafast_gateway::telemetry::{NoopSink, RequestRecord, Scope};
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

const TOKEN: &str = "scrape-token-0123456789";
const BODY: &str =
    r#"{"model":"p/gpt-4o","max_tokens":10,"messages":[{"role":"user","content":"hi"}]}"#;

async fn scrape(
    app: &Router,
    authorization: Option<&str>,
) -> (StatusCode, axum::http::HeaderMap, String) {
    let mut req = Request::builder().method("GET").uri("/metrics");
    if let Some(a) = authorization {
        req = req.header("authorization", a);
    }
    let resp = app
        .clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let (parts, body) = resp.into_parts();
    let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
    (
        parts.status,
        parts.headers,
        String::from_utf8(bytes.to_vec()).unwrap(),
    )
}

async fn metrics(h: &Harness) -> String {
    let (status, _, text) = scrape(&h.app, Some(&format!("Bearer {TOKEN}"))).await;
    assert_eq!(status, StatusCode::OK);
    text
}

/// The value of the sample whose name and labels are exactly `series`.
fn sample(text: &str, series: &str) -> f64 {
    text.lines()
        .find_map(|l| l.strip_prefix(series).and_then(|r| r.strip_prefix(' ')))
        .unwrap_or_else(|| panic!("no sample {series} in:\n{text}"))
        .parse()
        .unwrap()
}

fn ok_upstream() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "c1", "model": "gpt-4o",
        "choices": [{ "message": { "role": "assistant", "content": "hello" }, "finish_reason": "stop" }],
        "usage": { "prompt_tokens": 90, "completion_tokens": 20 }
    }))
}

async fn mount_ok(h: &Harness) {
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ok_upstream())
        .mount(&h.upstream)
        .await;
}

#[tokio::test]
async fn without_a_token_configured_metrics_is_any_other_unknown_path() {
    let h = harness_with_metrics_token("openai", None).await;
    let get = |uri: &'static str, auth: Option<&'static str>| {
        let app = h.app.clone();
        async move {
            let mut req = Request::builder().method("GET").uri(uri);
            if let Some(a) = auth {
                req = req.header("authorization", a);
            }
            let resp = app.oneshot(req.body(Body::empty()).unwrap()).await.unwrap();
            let (parts, body) = resp.into_parts();
            let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
            // The page carries a nonce that differs on every response.
            let page = String::from_utf8_lossy(&bytes)
                .lines()
                .filter(|l| !l.contains("csp-nonce"))
                .collect::<Vec<_>>()
                .join("\n");
            (
                parts.status,
                parts.headers.get("content-type").cloned(),
                page,
            )
        }
    };
    let other = get("/no-such-page", None).await;
    for auth in [None, Some("Bearer anything")] {
        let metrics = get("/metrics", auth).await;
        assert_eq!(metrics.0, other.0);
        assert_eq!(metrics.1, other.1);
        assert_eq!(metrics.2, other.2);
        assert!(!metrics.2.contains("uf_requests_total"));
    }
}

#[tokio::test]
async fn the_token_is_required_and_must_match() {
    let h = harness_with_metrics_token("openai", Some(TOKEN)).await;
    for auth in [
        None,
        Some("Bearer wrong"),
        Some("Bearer "),
        Some(TOKEN),
        Some("Basic c2NyYXBl"),
        Some(&format!("Bearer {TOKEN}x")),
        Some(&format!("Bearer {}", &TOKEN[..TOKEN.len() - 1])),
    ] {
        let (status, headers, body) = scrape(&h.app, auth).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{auth:?}");
        assert!(!body.contains("uf_requests_total"));
        assert_eq!(headers["www-authenticate"], "Bearer");
    }
    let (status, headers, body) = scrape(&h.app, Some(&format!("Bearer {TOKEN}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        headers["content-type"],
        "text/plain; version=0.0.4; charset=utf-8"
    );
    assert_eq!(headers["cache-control"], "no-store");
    assert!(body.contains("uf_requests_total"));
}

#[test]
fn label_values_are_escaped_as_the_format_says() {
    assert_eq!(escape_label(r#"a\b"c"#), r#"a\\b\"c"#);
    assert_eq!(escape_label("line1\nline2"), r"line1\nline2");
    assert_eq!(escape_label("plain/model-1.5"), "plain/model-1.5");
}

/// Checks the text exposition format line by line.
fn check_format(text: &str) {
    assert!(text.ends_with('\n'));
    let mut typed: BTreeMap<String, String> = BTreeMap::new();
    let mut helped = HashSet::new();
    // Per histogram series: the bucket counts in order of appearance.
    let mut buckets: BTreeMap<String, Vec<(String, f64)>> = BTreeMap::new();
    let mut counts: BTreeMap<String, f64> = BTreeMap::new();
    for line in text.lines() {
        assert!(!line.is_empty(), "no blank lines");
        if let Some(rest) = line.strip_prefix("# HELP ") {
            let (name, help) = rest.split_once(' ').expect("HELP has a text");
            assert!(!help.is_empty());
            assert!(helped.insert(name.to_string()), "HELP twice for {name}");
        } else if let Some(rest) = line.strip_prefix("# TYPE ") {
            let (name, kind) = rest.split_once(' ').unwrap();
            assert!(["counter", "gauge", "histogram"].contains(&kind), "{line}");
            assert!(helped.contains(name), "HELP before TYPE for {name}");
            assert!(typed.insert(name.to_string(), kind.to_string()).is_none());
        } else {
            assert!(!line.starts_with('#'), "unknown comment {line}");
            let (series, value) = line.rsplit_once(' ').expect("name and value");
            let value: f64 = value.parse().unwrap_or_else(|_| panic!("number in {line}"));
            let (name, labels) = match series.split_once('{') {
                Some((n, l)) => (n, l.strip_suffix('}').expect("closing brace")),
                None => (series, ""),
            };
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':')
                    && !name.starts_with(|c: char| c.is_ascii_digit()),
                "metric name {name}"
            );
            // Labels: name="value" pairs, values escaped.
            let mut rest = labels;
            let mut le = None;
            let mut others = Vec::new();
            while !rest.is_empty() {
                let (lname, after) = rest.split_once("=\"").expect("label");
                assert!(lname.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
                let mut value_end = None;
                let mut chars = after.char_indices();
                while let Some((i, c)) = chars.next() {
                    match c {
                        '\\' => {
                            let (_, e) = chars.next().expect("escape");
                            assert!(['\\', '"', 'n'].contains(&e), "bad escape in {line}");
                        }
                        '"' => {
                            value_end = Some(i);
                            break;
                        }
                        '\n' => panic!("raw newline"),
                        _ => {}
                    }
                }
                let end = value_end.expect("closing quote");
                let lvalue = &after[..end];
                if lname == "le" {
                    le = Some(lvalue.to_string());
                } else {
                    others.push(format!("{lname}={lvalue}"));
                }
                rest = after[end + 1..].trim_start_matches(',');
            }
            let family = ["_bucket", "_sum", "_count"]
                .iter()
                .find_map(|s| {
                    name.strip_suffix(s)
                        .filter(|f| typed.get(*f).is_some_and(|t| t == "histogram"))
                })
                .unwrap_or(name);
            assert!(typed.contains_key(family), "TYPE before samples of {name}");
            if name.ends_with("_bucket") && family != name {
                buckets
                    .entry(format!("{family}{{{}}}", others.join(",")))
                    .or_default()
                    .push((le.expect("a bucket has le"), value));
            } else if name.ends_with("_count") && family != name {
                counts.insert(format!("{family}{{{}}}", others.join(",")), value);
            }
        }
    }
    assert!(!typed.is_empty());
    for (series, list) in &buckets {
        assert_eq!(list.last().unwrap().0, "+Inf", "{series}");
        let mut previous = 0.0;
        let mut les = Vec::new();
        for (le, v) in list {
            assert!(*v >= previous, "buckets of {series} are cumulative");
            previous = *v;
            if le != "+Inf" {
                les.push(le.parse::<f64>().unwrap());
            }
        }
        assert!(les.windows(2).all(|w| w[0] < w[1]), "le ascends");
        assert_eq!(counts[series], previous, "_count equals the +Inf bucket");
    }
}

#[tokio::test]
async fn the_output_is_valid_exposition_text_with_every_metric() {
    let h = harness_with_metrics_token("openai", Some(TOKEN)).await;
    mount_ok(&h).await;
    let (status, _) = post_chat(&h.app, Some(&h.key), BODY).await;
    assert_eq!(status, StatusCode::OK);
    let text = metrics(&h).await;
    check_format(&text);
    for (name, kind) in [
        ("uf_requests_total", "counter"),
        ("uf_tokens_total", "counter"),
        ("uf_cost_micros_total", "counter"),
        ("uf_upstream_duration_seconds", "histogram"),
        ("uf_log_records_dropped_total", "counter"),
        ("uf_log_write_failures_total", "counter"),
        ("uf_cache_hits_total", "counter"),
        ("uf_cache_misses_total", "counter"),
        ("uf_cache_flight_waits_total", "counter"),
        ("uf_rate_limited_total", "counter"),
        ("uf_budget_blocked_total", "counter"),
        ("uf_guardrail_actions_total", "counter"),
        ("uf_guardrail_external_errors_total", "counter"),
        ("uf_circuit_open", "gauge"),
    ] {
        assert!(text.contains(&format!("# TYPE {name} {kind}\n")), "{name}");
        assert!(text.contains(&format!("# HELP {name} ")), "{name}");
    }
}

#[tokio::test]
async fn requests_tokens_and_upstream_time_move_with_traffic() {
    let h = harness_with_metrics_token("openai", Some(TOKEN)).await;
    mount_ok(&h).await;
    let before = metrics(&h).await;
    assert_eq!(
        sample(
            &before,
            r#"uf_requests_total{endpoint="chat",status_class="2xx"}"#
        ),
        0.0
    );
    for _ in 0..2 {
        assert_eq!(
            post_chat(&h.app, Some(&h.key), BODY).await.0,
            StatusCode::OK
        );
    }
    // An unknown model is a 4xx answer; an unknown key is not recorded at all.
    let unknown =
        r#"{"model":"p/nope","max_tokens":10,"messages":[{"role":"user","content":"hi"}]}"#;
    assert_eq!(
        post_chat(&h.app, Some(&h.key), unknown).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        post_chat(&h.app, Some("uf-nope"), BODY).await.0,
        StatusCode::UNAUTHORIZED
    );
    let text = metrics(&h).await;
    check_format(&text);
    assert_eq!(
        sample(
            &text,
            r#"uf_requests_total{endpoint="chat",status_class="2xx"}"#
        ),
        2.0
    );
    assert_eq!(
        sample(
            &text,
            r#"uf_requests_total{endpoint="chat",status_class="4xx"}"#
        ),
        1.0
    );
    assert_eq!(
        sample(
            &text,
            r#"uf_requests_total{endpoint="messages",status_class="2xx"}"#
        ),
        0.0
    );
    assert_eq!(
        sample(&text, r#"uf_tokens_total{direction="input"}"#),
        180.0
    );
    assert_eq!(
        sample(&text, r#"uf_tokens_total{direction="output"}"#),
        40.0
    );
    assert_eq!(
        sample(&text, r#"uf_upstream_duration_seconds_count{provider="p"}"#),
        2.0
    );
    assert_eq!(
        sample(
            &text,
            r#"uf_upstream_duration_seconds_bucket{provider="p",le="+Inf"}"#
        ),
        2.0
    );
    assert!(sample(&text, r#"uf_upstream_duration_seconds_sum{provider="p"}"#) >= 0.0);
}

#[tokio::test]
async fn an_attempt_is_timed_by_its_outcome_even_at_zero_ms_without_a_status() {
    use ultrafast_gateway::telemetry::{Attempt, AttemptOutcome};
    let metrics = ultrafast_gateway::metrics::Metrics::new();
    let attempt = |outcome| Attempt {
        provider: "p".into(),
        model: "m".into(),
        outcome,
        status: None,
        duration_ms: 0,
        offset_ms: 0,
    };
    let mut record = RequestRecord {
        tags: Default::default(),
        key_id: Some(1),
        user_id: None,
        team_id: None,
        requested: "p/m".into(),
        endpoint: "chat",
        stream: false,
        status: 200,
        usage: None,
        attempts: vec![
            attempt(AttemptOutcome::Ok),
            attempt(AttemptOutcome::Fatal),
            attempt(AttemptOutcome::Skipped),
            attempt(AttemptOutcome::CircuitOpen),
            attempt(AttemptOutcome::Cached),
        ],
        cached: false,
        estimated: false,
        started_at: ultrafast_gateway::store::now(),
        duration_ms: 1,
        trace_parent: None,
        provider_kinds: Vec::new(),
        started_unix_ms: 0,
        guardrails: None,
    };
    record.attempts[1].status = Some(400);
    metrics.record(&record);
    let text = metrics.render(&[]);
    assert_eq!(
        sample(&text, r#"uf_upstream_duration_seconds_count{provider="p"}"#),
        2.0
    );
}

#[tokio::test]
async fn a_failing_provider_counts_as_5xx() {
    let h = harness_with_metrics_token("openai", Some(TOKEN)).await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&h.upstream)
        .await;
    assert!(post_chat(&h.app, Some(&h.key), BODY)
        .await
        .0
        .is_server_error());
    let text = metrics(&h).await;
    assert_eq!(
        sample(
            &text,
            r#"uf_requests_total{endpoint="chat",status_class="5xx"}"#
        ),
        1.0
    );
}

#[test]
fn a_call_the_caller_abandoned_counts_as_499_and_not_as_a_4xx() {
    let metrics = Arc::new(ultrafast_gateway::metrics::Metrics::new());
    let mut scope = Scope::begin(Arc::new(NoopSink), Some(1), None, None, "messages");
    scope.metered(metrics.clone());
    drop(scope);
    let text = metrics.render(&[]);
    assert_eq!(
        sample(
            &text,
            r#"uf_requests_total{endpoint="messages",status_class="499"}"#
        ),
        1.0
    );
    assert_eq!(
        sample(
            &text,
            r#"uf_requests_total{endpoint="messages",status_class="4xx"}"#
        ),
        0.0
    );
}

#[test]
fn a_playground_call_has_its_own_endpoint_in_the_counts() {
    let metrics = Arc::new(ultrafast_gateway::metrics::Metrics::new());
    // A call of a user, with no key.
    let mut scope = Scope::begin(Arc::new(NoopSink), None, Some(1), None, "playground");
    scope.metered(metrics.clone());
    scope.finish(200);
    let text = metrics.render(&[]);
    assert_eq!(
        sample(
            &text,
            r#"uf_requests_total{endpoint="playground",status_class="2xx"}"#
        ),
        1.0
    );
    assert_eq!(
        sample(
            &text,
            r#"uf_requests_total{endpoint="chat",status_class="2xx"}"#
        ),
        0.0
    );
}

#[tokio::test]
async fn the_circuit_gauge_shows_open_targets_per_provider_and_model() {
    let h = harness_with_metrics_token("openai", Some(TOKEN)).await;
    let settings = BreakerSettings {
        failures: 1,
        window: Duration::from_secs(60),
        open: Duration::from_secs(3600),
    };
    let now = tokio::time::Instant::now();
    let bad = TargetRef {
        provider: "p".into(),
        model: "bad".into(),
        model_id: 1,
    };
    let good = TargetRef {
        provider: "p".into(),
        model: "good".into(),
        model_id: 2,
    };
    h.state
        .health
        .report(&bad, false, true, Some(503), now, &settings);
    h.state
        .health
        .report(&good, true, false, Some(200), now, &settings);
    let text = metrics(&h).await;
    check_format(&text);
    assert_eq!(
        sample(&text, r#"uf_circuit_open{provider="p",model="bad"}"#),
        1.0
    );
    assert_eq!(
        sample(&text, r#"uf_circuit_open{provider="p",model="good"}"#),
        0.0
    );
}

struct Refuses(std::sync::atomic::AtomicUsize);

impl Limiter for Refuses {
    fn acquire(&self, _: &Subjects, _: u64, _: Instant) -> Result<Permit, Refusal> {
        let names = [
            "requests per minute",
            "tokens per minute",
            "concurrent requests",
        ];
        let i = self.0.fetch_add(1, Ordering::SeqCst);
        Err(Refusal {
            limit_name: names[i % 3],
            scope_label: "key 'ci'".into(),
            retry_after: Duration::from_secs(1),
        })
    }
}

#[tokio::test]
async fn refusals_are_counted_per_limit_kind() {
    let h2 = harness_with_rate("openai", Arc::new(Refuses(Default::default()))).await;
    for _ in 0..4 {
        assert_eq!(
            post_chat(&h2.app, Some(&h2.key), BODY).await.0,
            StatusCode::TOO_MANY_REQUESTS
        );
    }
    let text = h2.state.metrics.render(&h2.state.health.view());
    assert_eq!(
        sample(
            &text,
            r#"uf_rate_limited_total{limit="requests_per_minute"}"#
        ),
        2.0
    );
    assert_eq!(
        sample(&text, r#"uf_rate_limited_total{limit="tokens_per_minute"}"#),
        1.0
    );
    assert_eq!(
        sample(&text, r#"uf_rate_limited_total{limit="concurrent"}"#),
        1.0
    );
}

#[tokio::test]
async fn a_budget_refusal_is_counted() {
    let h = harness_with_metrics_token("openai", Some(TOKEN)).await;
    mount_ok(&h).await;
    let mut tx = h.store.begin().await.unwrap();
    tx.upsert_budget(
        LimitScope::Gateway,
        None,
        100,
        Period::Daily,
        BudgetAction::Block,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    h.state.refresh().await.unwrap();
    assert_eq!(
        post_chat(&h.app, Some(&h.key), BODY).await.0,
        StatusCode::OK
    );
    let record = RequestRecord {
        tags: Default::default(),
        key_id: Some(1),
        user_id: None,
        team_id: None,
        requested: "p/m".into(),
        endpoint: "chat",
        stream: false,
        status: 200,
        usage: None,
        attempts: Vec::new(),
        cached: false,
        estimated: false,
        started_at: ultrafast_gateway::store::now(),
        duration_ms: 1,
        trace_parent: None,
        provider_kinds: Vec::new(),
        started_unix_ms: 0,
        guardrails: None,
    };
    account(&h.state, &record, 500, OffsetDateTime::now_utc());
    assert_eq!(
        post_chat(&h.app, Some(&h.key), BODY).await.0,
        StatusCode::TOO_MANY_REQUESTS
    );
    let text = metrics(&h).await;
    assert_eq!(sample(&text, "uf_budget_blocked_total"), 1.0);
}

#[tokio::test]
async fn cache_hits_and_misses_are_counted() {
    let h = harness_with_metrics_token("openai", Some(TOKEN)).await;
    mount_ok(&h).await;
    let model = h
        .store
        .list_models()
        .await
        .unwrap()
        .iter()
        .find(|m| m.name == "gpt-4o")
        .unwrap()
        .id;
    let mut tx = h.store.begin().await.unwrap();
    let id = tx
        .insert_route(
            "r",
            &RouteSettings {
                retries: 0,
                first_token_timeout_ms: 30_000,
                total_timeout_ms: 300_000,
                breaker_failures: 5,
                breaker_window_s: 60,
                breaker_open_s: 30,
            },
            true,
        )
        .await
        .unwrap();
    tx.replace_targets(
        id,
        &TargetsInput {
            primaries: vec![(model, 1)],
            fallbacks: vec![],
        },
    )
    .await
    .unwrap();
    tx.set_route_cache(
        id,
        &RouteCache {
            enabled: true,
            ttl_s: 300,
            scope: CacheScope::Key,
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    h.state.refresh().await.unwrap();
    let route = r#"{"model":"r","max_tokens":10,"messages":[{"role":"user","content":"hi"}]}"#;
    for _ in 0..3 {
        assert_eq!(
            post_chat(&h.app, Some(&h.key), route).await.0,
            StatusCode::OK
        );
    }
    let text = metrics(&h).await;
    assert_eq!(sample(&text, "uf_cache_misses_total"), 1.0);
    assert_eq!(sample(&text, "uf_cache_hits_total"), 2.0);
    // A hit used no provider: one upstream call, tokens of one call only.
    assert_eq!(
        sample(&text, r#"uf_upstream_duration_seconds_count{provider="p"}"#),
        1.0
    );
    assert_eq!(sample(&text, r#"uf_tokens_total{direction="input"}"#), 90.0);
    assert_eq!(
        sample(
            &text,
            r#"uf_requests_total{endpoint="chat",status_class="2xx"}"#
        ),
        3.0
    );
}

#[tokio::test]
async fn cost_and_log_counters_come_from_the_log_pipeline() {
    let (sink, rx) = LogSink::channel(100);
    let stats = sink.stats();
    let h = harness_with_sink("openai", Arc::new(sink)).await;
    h.state.metrics.attach_logs(stats.clone());
    mount_ok(&h).await;
    let store = h.store.clone();
    let prices: PriceLookup = Arc::new(|p, m| {
        (p == "p" && m == "gpt-4o").then_some(Price {
            input_micros: Some(2_500_000),
            output_micros: Some(10_000_000),
        })
    });
    let (_stop, stopped) = watch::channel(false);
    let _writer = spawn(
        store,
        rx,
        prices,
        stats.clone(),
        WriterConfig {
            max_batch: 1,
            max_wait: Duration::from_millis(10),
            retry_delay: Duration::from_millis(10),
        },
        stopped,
    );
    assert_eq!(
        post_chat(&h.app, Some(&h.key), BODY).await.0,
        StatusCode::OK
    );
    // 90 * 2.5 + 20 * 10 = 425 micro-dollars.
    for _ in 0..400 {
        if stats.written.load(Ordering::Relaxed) == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    stats.dropped.store(3, Ordering::Relaxed);
    stats.write_failures.store(2, Ordering::Relaxed);
    let text = h.state.metrics.render(&h.state.health.view());
    check_format(&text);
    assert_eq!(sample(&text, "uf_cost_micros_total"), 425.0);
    assert_eq!(sample(&text, "uf_log_records_dropped_total"), 3.0);
    assert_eq!(sample(&text, "uf_log_write_failures_total"), 2.0);
}

#[tokio::test]
async fn no_key_name_email_or_secret_is_in_the_output() {
    let h = harness_with_metrics_token("openai", Some(TOKEN)).await;
    mount_ok(&h).await;
    let user = common::seed_user(
        &h.store,
        "zq-person@example.com",
        Role::Member,
        "correct horse battery",
    )
    .await;
    let team = common::seed_team(&h.store, "zq-team", &[(user, TeamRole::Member)]).await;
    let key = generate_key();
    let mut tx = h.store.begin().await.unwrap();
    tx.insert_key(
        "zq-key-name",
        &key.hash,
        &key.display,
        None,
        Some(user),
        Some(team),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    h.state.refresh().await.unwrap();
    let bearer = format!("Bearer {}", key.full);
    let (status, _, _) = post_to(
        &h.app,
        "/v1/chat/completions",
        &[("authorization", &bearer)],
        r#"{"model":"p/gpt-4o","max_tokens":10,"messages":[{"role":"user","content":"zq-prompt-text"}]}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let text = metrics(&h).await;
    for secret in [
        "zq-key-name",
        "zq-person",
        "example.com",
        "zq-team",
        "zq-prompt-text",
        key.full.as_str(),
        key.display.as_str(),
        TOKEN,
        "provider-secret",
    ] {
        assert!(!text.contains(secret), "{secret} leaked");
    }
}

#[tokio::test]
async fn guardrail_actions_are_counted_by_action_and_direction_with_no_names() {
    let h = harness_with_metrics_token("openai", Some(TOKEN)).await;
    mount_ok(&h).await;
    let rules = json!([
        { "id": "w", "matcher": { "keywords": { "words": ["swordfish"] } },
          "action": "block", "directions": "input" },
        { "id": "hello", "matcher": { "keywords": { "words": ["hello"] } },
          "action": "redact", "directions": "output" }
    ])
    .to_string();
    let mut tx = h.store.begin().await.unwrap();
    tx.insert_guardrail(ultrafast_gateway::store::NewGuardrail {
        name: "a-secret-name",
        description: "",
        kind: "rules",
        rules: &rules,
        url: None,
        secret_enc: None,
        timeout_ms: 3000,
        fail_mode: "open",
        directions: "both",
        enabled: true,
        is_default: true,
    })
    .await
    .unwrap();
    tx.commit().await.unwrap();
    h.state.refresh().await.unwrap();
    let blocked = BODY.replace("\"hi\"", "\"swordfish\"");
    assert_eq!(
        post_chat(&h.app, Some(&h.key), &blocked).await.0,
        StatusCode::BAD_REQUEST
    );
    for _ in 0..2 {
        assert_eq!(
            post_chat(&h.app, Some(&h.key), BODY).await.0,
            StatusCode::OK
        );
    }
    let text = metrics(&h).await;
    check_format(&text);
    let series = |a: &str, d: &str| {
        sample(
            &text,
            &format!("uf_guardrail_actions_total{{action=\"{a}\",direction=\"{d}\"}}"),
        )
    };
    assert_eq!(series("block", "input"), 1.0);
    assert_eq!(series("redact", "output"), 2.0);
    assert_eq!(series("block", "output"), 0.0);
    assert_eq!(series("flag", "input"), 0.0);
    assert!(!text.contains("a-secret-name"));
}
