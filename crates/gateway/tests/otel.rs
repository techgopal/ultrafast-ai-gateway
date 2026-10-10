//! OTLP trace export: the shape of what a collector receives, parentage from
//! `traceparent`, sampling, and a collector that never answers.

mod common;

use std::time::{Duration, Instant};

use common::{harness, harness_with_state, post_chat, post_to, Harness};
use serde_json::{json, Value};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use ultrafast_gateway::otel::{Exporter, OtelConfig};
use ultrafast_gateway::telemetry::{Attempt, AttemptOutcome, RequestRecord};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const BODY: &str = r#"{"model":"p/gpt-4o","max_tokens":10,"messages":[{"role":"user","content":"hi SECRET-PROMPT"}]}"#;
const PARENT: &str = "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01";

fn ok_upstream() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "c1", "model": "gpt-4o",
        "choices": [{ "message": { "role": "assistant", "content": "hello" }, "finish_reason": "stop" }],
        "usage": { "prompt_tokens": 90, "completion_tokens": 20 }
    }))
}

fn config(endpoint: String, ratio: f64) -> OtelConfig {
    OtelConfig {
        endpoint,
        headers: vec![("x-collector-key".into(), "c0llector".into())],
        service_name: "ultrafast-test".into(),
        sample_ratio: ratio,
    }
}

struct Traced {
    h: Harness,
    collector: MockServer,
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}

async fn traced(ratio: f64, delay: Option<Duration>) -> Traced {
    let collector = MockServer::start().await;
    let mut ok = ResponseTemplate::new(200).set_body_json(json!({}));
    if let Some(d) = delay {
        ok = ok.set_delay(d);
    }
    Mock::given(method("POST"))
        .and(path("/v1/traces"))
        .respond_with(ok)
        .mount(&collector)
        .await;
    let (stop, stopped) = watch::channel(false);
    let endpoint = collector.uri();
    let slot = std::sync::Arc::new(std::sync::Mutex::new(None));
    let slot2 = slot.clone();
    let h = harness_with_state("openai", move |state| {
        let (exporter, task) = Exporter::spawn(
            config(endpoint, ratio),
            state.http.clone(),
            state.metrics.clone(),
            stopped,
        );
        state.otel = Some(exporter);
        *slot2.lock().unwrap() = Some(task);
    })
    .await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ok_upstream())
        .mount(&h.upstream)
        .await;
    let task = slot.lock().unwrap().take().unwrap();
    Traced {
        h,
        collector,
        stop,
        task,
    }
}

impl Traced {
    /// Stops the exporter, which flushes what it holds, and returns the
    /// export requests the collector got.
    async fn exports(self) -> Vec<wiremock::Request> {
        self.stop.send(true).unwrap();
        self.task.await.unwrap();
        self.collector.received_requests().await.unwrap()
    }
}

fn spans(req: &wiremock::Request) -> Vec<Value> {
    let v: Value = serde_json::from_slice(&req.body).unwrap();
    v["resourceSpans"][0]["scopeSpans"][0]["spans"]
        .as_array()
        .unwrap()
        .clone()
}

#[tokio::test]
async fn a_chat_call_is_exported_with_the_expected_shape_and_headers() {
    let t = traced(1.0, None).await;
    let (status, _) = post_chat(&t.h.app, Some(&t.h.key), BODY).await;
    assert_eq!(status, 200);
    let sink_key = t.h.key.clone();
    let reqs = t.exports().await;
    assert_eq!(reqs.len(), 1);
    let r = &reqs[0];
    assert_eq!(r.url.path(), "/v1/traces");
    assert_eq!(r.headers.get("x-collector-key").unwrap(), "c0llector");
    assert_eq!(r.headers.get("content-type").unwrap(), "application/json");
    let v: Value = serde_json::from_slice(&r.body).unwrap();
    let res = &v["resourceSpans"][0]["resource"]["attributes"];
    assert!(res
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["key"] == "service.name" && a["value"]["stringValue"] == "ultrafast-test"));
    assert!(res
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["key"] == "service.version"));
    let spans = spans(r);
    assert_eq!(spans.len(), 2);
    assert_eq!(spans[0]["name"], "uf.chat");
    assert_eq!(spans[0]["kind"], 2);
    assert_eq!(spans[1]["name"], "uf.attempt p");
    assert_eq!(spans[1]["kind"], 3);
    assert_eq!(spans[1]["parentSpanId"], spans[0]["spanId"]);
    assert_eq!(spans[0]["traceId"], spans[1]["traceId"]);
    assert_eq!(spans[0]["traceId"].as_str().unwrap().len(), 32);
    let body = String::from_utf8_lossy(&r.body).to_string();
    for secret in [
        "SECRET-PROMPT",
        "provider-secret",
        sink_key.as_str(),
        "hello",
    ] {
        assert!(!body.contains(secret), "{secret} leaked");
    }
}

#[tokio::test]
async fn a_batch_is_sent_on_its_own_within_the_flush_interval() {
    let t = traced(1.0, None).await;
    assert_eq!(post_chat(&t.h.app, Some(&t.h.key), BODY).await.0, 200);
    let mut seen = 0;
    for _ in 0..40 {
        seen = t.collector.received_requests().await.unwrap().len();
        if seen > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(seen, 1);
}

#[tokio::test]
async fn a_full_traces_url_is_used_as_it_is() {
    let collector = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/traces"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&collector)
        .await;
    let metrics = std::sync::Arc::new(ultrafast_gateway::metrics::Metrics::new());
    let (stop, stopped) = watch::channel(false);
    let (exporter, task) = Exporter::spawn(
        config(format!("{}/v1/traces", collector.uri()), 1.0),
        reqwest::Client::new(),
        metrics,
        stopped,
    );
    exporter.offer(&record());
    stop.send(true).unwrap();
    task.await.unwrap();
    assert_eq!(collector.received_requests().await.unwrap().len(), 1);
    assert_eq!(
        collector.received_requests().await.unwrap()[0].url.path(),
        "/v1/traces"
    );
}

#[tokio::test]
async fn an_incoming_traceparent_becomes_the_parent() {
    let t = traced(1.0, None).await;
    let (status, _, _) = post_to(
        &t.h.app,
        "/v1/chat/completions",
        &[
            ("authorization", &format!("Bearer {}", t.h.key)),
            ("traceparent", PARENT),
        ],
        BODY,
    )
    .await;
    assert_eq!(status, 200);
    let reqs = t.exports().await;
    let spans = spans(&reqs[0]);
    assert_eq!(spans[0]["traceId"], "0af7651916cd43dd8448eb211c80319c");
    assert_eq!(spans[0]["parentSpanId"], "b7ad6b7169203331");
}

#[tokio::test]
async fn sample_ratio_zero_exports_nothing_unless_the_caller_sampled() {
    let t = traced(0.0, None).await;
    let key = t.h.key.clone();
    assert_eq!(post_chat(&t.h.app, Some(&key), BODY).await.0, 200);
    // An unsampled parent is dropped.
    let unsampled = PARENT.replace("-01", "-00");
    post_to(
        &t.h.app,
        "/v1/chat/completions",
        &[
            ("authorization", &format!("Bearer {key}")),
            ("traceparent", &unsampled),
        ],
        BODY,
    )
    .await;
    // A sampled parent is always kept.
    post_to(
        &t.h.app,
        "/v1/chat/completions",
        &[
            ("authorization", &format!("Bearer {key}")),
            ("traceparent", PARENT),
        ],
        BODY,
    )
    .await;
    let reqs = t.exports().await;
    assert_eq!(reqs.len(), 1);
    let spans = spans(&reqs[0]);
    assert_eq!(spans.len(), 2);
    assert_eq!(spans[0]["traceId"], "0af7651916cd43dd8448eb211c80319c");
}

#[tokio::test]
async fn the_playground_has_no_trace_parent_and_ratio_zero_exports_nothing() {
    let t = traced(0.0, None).await;
    assert_eq!(post_chat(&t.h.app, Some(&t.h.key), BODY).await.0, 200);
    assert!(t.exports().await.is_empty());
}

async fn calls(h: &Harness, n: usize) -> Duration {
    let started = Instant::now();
    for _ in 0..n {
        assert_eq!(post_chat(&h.app, Some(&h.key), BODY).await.0, 200);
    }
    started.elapsed()
}

#[tokio::test]
async fn collector_down_never_slows_calls() {
    // The same calls without an exporter set the normal time.
    let plain = harness("openai").await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ok_upstream())
        .mount(&plain.upstream)
        .await;
    let normal = calls(&plain, 2_000).await;

    let t = traced(1.0, Some(Duration::from_secs(30))).await;
    let slow = calls(&t.h, 2_000).await;
    assert!(
        slow < normal * 3 + Duration::from_secs(5),
        "with a hanging collector {slow:?}, without {normal:?}"
    );
    let exporter = t.h.state.otel.as_ref().unwrap();
    assert!(exporter.queued() <= 4096);
    // The full-queue path, on top of the calls: more records than it holds.
    let one = t.h.sink.records().remove(0);
    for _ in 0..5_000 {
        exporter.offer(&one);
    }
    assert!(exporter.queued() <= 4096);
    assert!(
        t.h.state
            .metrics
            .render(&[])
            .contains("uf_otel_spans_dropped_total ")
            && !t
                .h
                .state
                .metrics
                .render(&[])
                .contains("uf_otel_spans_dropped_total 0\n")
    );
    let _ = (&t.stop, &t.task, &t.collector);
}

fn record() -> RequestRecord {
    RequestRecord {
        key_id: Some(1),
        user_id: None,
        team_id: None,
        requested: "p/m".into(),
        endpoint: "chat",
        stream: false,
        status: 200,
        usage: None,
        attempts: vec![Attempt {
            provider: "p".into(),
            model: "m".into(),
            outcome: AttemptOutcome::Ok,
            status: Some(200),
            duration_ms: 1,
            offset_ms: 0,
            skipped: None,
        }],
        cached: false,
        estimated: false,
        started_at: "2999-01-01 00:00:00".into(),
        duration_ms: 2,
        tags: Default::default(),
        trace_parent: None,
        provider_kinds: Vec::new(),
        started_unix_ms: 0,
        guardrails: None,
        prompt: None,
    }
}

#[tokio::test]
async fn a_full_queue_drops_and_counts_and_stays_bounded() {
    let collector = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(30)))
        .mount(&collector)
        .await;
    let metrics = std::sync::Arc::new(ultrafast_gateway::metrics::Metrics::new());
    let (_stop, stopped) = watch::channel(false);
    let (exporter, _task) = Exporter::spawn(
        config(collector.uri(), 1.0),
        reqwest::Client::new(),
        metrics.clone(),
        stopped,
    );
    let r = record();
    let started = Instant::now();
    for _ in 0..10_000 {
        exporter.offer(&r);
    }
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(exporter.queued() <= 4096);
    let text = metrics.render(&[]);
    let dropped: u64 = text
        .lines()
        .find_map(|l| l.strip_prefix("uf_otel_spans_dropped_total "))
        .unwrap()
        .parse()
        .unwrap();
    assert!(dropped > 0, "{text}");
}

#[tokio::test]
async fn a_failing_collector_counts_failures_and_drops_the_batch() {
    let collector = MockServer::start().await;
    Mock::given(method("POST"))
        .and(header("x-collector-key", "c0llector"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&collector)
        .await;
    let metrics = std::sync::Arc::new(ultrafast_gateway::metrics::Metrics::new());
    let (stop, stopped) = watch::channel(false);
    let (exporter, task) = Exporter::spawn(
        config(collector.uri(), 1.0),
        reqwest::Client::new(),
        metrics.clone(),
        stopped,
    );
    exporter.offer(&record());
    stop.send(true).unwrap();
    task.await.unwrap();
    let text = metrics.render(&[]);
    assert!(text.contains("uf_otel_export_failures_total 1"), "{text}");
    assert!(text.contains("uf_otel_spans_exported_total 0"), "{text}");
    assert_eq!(collector.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn shutdown_ends_within_the_cap_even_with_a_send_in_flight() {
    let collector = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(60)))
        .mount(&collector)
        .await;
    let metrics = std::sync::Arc::new(ultrafast_gateway::metrics::Metrics::new());
    let (stop, stopped) = watch::channel(false);
    let (exporter, task) = Exporter::spawn(
        config(collector.uri(), 1.0),
        reqwest::Client::new(),
        metrics.clone(),
        stopped,
    );
    // 256 records of two spans make a full batch, which is sent at once.
    let r = record();
    for _ in 0..300 {
        exporter.offer(&r);
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    let started = Instant::now();
    stop.send(true).unwrap();
    task.await.unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(7),
        "{:?}",
        started.elapsed()
    );
    let text = metrics.render(&[]);
    assert!(!text.contains("uf_otel_spans_dropped_total 0\n"), "{text}");
}

#[tokio::test]
async fn a_query_string_stays_after_the_traces_path() {
    let collector = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/traces"))
        .and(wiremock::matchers::query_param("token", "t"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&collector)
        .await;
    let metrics = std::sync::Arc::new(ultrafast_gateway::metrics::Metrics::new());
    let (stop, stopped) = watch::channel(false);
    let (exporter, task) = Exporter::spawn(
        config(format!("{}/?token=t", collector.uri()), 1.0),
        reqwest::Client::new(),
        metrics.clone(),
        stopped,
    );
    exporter.offer(&record());
    stop.send(true).unwrap();
    task.await.unwrap();
    assert!(metrics
        .render(&[])
        .contains("uf_otel_spans_exported_total 2"));
}

#[tokio::test]
async fn rejected_spans_of_a_partial_success_are_counted_as_dropped() {
    let collector = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({ "partialSuccess": { "rejectedSpans": "1", "errorMessage": "no" } }),
        ))
        .mount(&collector)
        .await;
    let metrics = std::sync::Arc::new(ultrafast_gateway::metrics::Metrics::new());
    let (stop, stopped) = watch::channel(false);
    let (exporter, task) = Exporter::spawn(
        config(collector.uri(), 1.0),
        reqwest::Client::new(),
        metrics.clone(),
        stopped,
    );
    exporter.offer(&record()); // two spans
    stop.send(true).unwrap();
    task.await.unwrap();
    let text = metrics.render(&[]);
    assert!(text.contains("uf_otel_spans_exported_total 1"), "{text}");
    assert!(text.contains("uf_otel_spans_dropped_total 1"), "{text}");
}

#[tokio::test]
async fn an_oversized_answer_to_a_good_export_is_not_read_to_the_end() {
    // The refusal is in the first bytes, but the answer runs on past 64 KiB:
    // only 64 KiB are read, and what does not parse is taken as no refusal.
    let collector = MockServer::start().await;
    let mut body = String::from(r#"{"partialSuccess":{"rejectedSpans":"1"}"#);
    body.push_str(&" ".repeat(200 * 1024));
    body.push('}');
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .mount(&collector)
        .await;
    let metrics = std::sync::Arc::new(ultrafast_gateway::metrics::Metrics::new());
    let (stop, stopped) = watch::channel(false);
    let (exporter, task) = Exporter::spawn(
        config(collector.uri(), 1.0),
        reqwest::Client::new(),
        metrics.clone(),
        stopped,
    );
    exporter.offer(&record()); // two spans
    stop.send(true).unwrap();
    task.await.unwrap();
    let text = metrics.render(&[]);
    assert!(text.contains("uf_otel_spans_exported_total 2"), "{text}");
    assert!(text.contains("uf_otel_spans_dropped_total 0"), "{text}");
}
