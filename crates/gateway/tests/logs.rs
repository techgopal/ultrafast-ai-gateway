//! Request logs: the queue, the batched writer, cost, retention.

mod common;

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{harness_with_sink, post_chat};
use serde_json::json;
use tokio::sync::watch;
use ultrafast_gateway::logs::retention::{self, purge, Purged, RetentionConfig};
use ultrafast_gateway::logs::writer::{spawn, WriterConfig};
use ultrafast_gateway::logs::{snapshot_prices, LogSink, Price, PriceLookup, QUEUE_CAPACITY};
use ultrafast_gateway::store::{NewLog, Store};
use ultrafast_gateway::telemetry::{Attempt, AttemptOutcome, RequestRecord, RequestSink};
use ultrafast_translate::types::Usage;
use wiremock::matchers::method;
use wiremock::{Mock, ResponseTemplate};

fn record(requested: &str) -> RequestRecord {
    RequestRecord {
        key_id: 1,
        user_id: Some(2),
        team_id: Some(3),
        requested: requested.into(),
        endpoint: "chat",
        stream: false,
        status: 200,
        usage: Some(Usage {
            input_tokens: 1_000,
            output_tokens: 500,
        }),
        attempts: vec![Attempt {
            provider: "p".into(),
            model: "m".into(),
            outcome: AttemptOutcome::Ok,
            status: Some(200),
            duration_ms: 12,
        }],
        started_at: "2999-01-01 00:00:00".into(),
        duration_ms: 20,
    }
}

fn prices(input: Option<i64>, output: Option<i64>) -> PriceLookup {
    Arc::new(move |provider, model| {
        (provider == "p" && model == "m").then_some(Price {
            input_micros: input,
            output_micros: output,
        })
    })
}

fn config(max_batch: usize, max_wait_ms: u64) -> WriterConfig {
    WriterConfig {
        max_batch,
        max_wait: Duration::from_millis(max_wait_ms),
    }
}

async fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    for _ in 0..400 {
        if done() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for {what}");
}

#[tokio::test]
async fn a_full_batch_is_written_in_one_transaction_without_waiting() {
    let store = Store::open_in_memory().await.unwrap();
    let (sink, rx) = LogSink::channel(100);
    let stats = sink.stats();
    let (_stop, stopped) = watch::channel(false);
    // The wait is long: only the size of the batch can trigger a write.
    let _writer = spawn(
        store.clone(),
        rx,
        prices(Some(2_500_000), Some(10_000_000)),
        stats.clone(),
        config(3, 60_000),
        stopped,
    );
    for i in 0..6 {
        sink.record(record(&format!("r{i}")));
    }
    wait_until("six rows", || stats.written.load(Ordering::Relaxed) == 6).await;
    assert_eq!(stats.batches.load(Ordering::Relaxed), 2);
    let rows = store.recent_logs(10).await.unwrap();
    assert_eq!(rows.len(), 6);
    assert_eq!(rows[0].requested, "r5");
    assert_eq!(stats.dropped.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn a_partial_batch_is_written_after_the_wait() {
    let store = Store::open_in_memory().await.unwrap();
    let (sink, rx) = LogSink::channel(100);
    let stats = sink.stats();
    let (_stop, stopped) = watch::channel(false);
    let _writer = spawn(
        store.clone(),
        rx,
        prices(None, None),
        stats.clone(),
        config(500, 50),
        stopped,
    );
    sink.record(record("one"));
    wait_until("the row", || stats.written.load(Ordering::Relaxed) == 1).await;
    assert_eq!(store.recent_logs(10).await.unwrap().len(), 1);
}

#[tokio::test]
async fn cost_is_tokens_times_price_per_million_rounded_half_up() {
    let store = Store::open_in_memory().await.unwrap();
    let (sink, rx) = LogSink::channel(100);
    let stats = sink.stats();
    let (stop, stopped) = watch::channel(false);
    let writer = spawn(
        store.clone(),
        rx,
        // 1000 in at 2.5 USD and 500 out at 10 USD per million:
        // 2_500 + 5_000 micro-dollars.
        prices(Some(2_500_000), Some(10_000_000)),
        stats,
        config(500, 20),
        stopped,
    );
    sink.record(record("priced"));
    // 1 token at 2_500_000 per million is 2.5 micro: half up is 3.
    let mut half = record("half");
    half.usage = Some(Usage {
        input_tokens: 1,
        output_tokens: 0,
    });
    sink.record(half);
    stop.send(true).unwrap();
    writer.await.unwrap();
    let rows = store.recent_logs(10).await.unwrap();
    let by = |name: &str| rows.iter().find(|r| r.requested == name).unwrap().clone();
    assert_eq!(by("priced").cost_micros, 7_500);
    assert!(by("priced").priced);
    assert_eq!(by("half").cost_micros, 3);
}

#[tokio::test]
async fn an_unknown_price_costs_nothing_and_says_so() {
    let store = Store::open_in_memory().await.unwrap();
    let (sink, rx) = LogSink::channel(100);
    let stats = sink.stats();
    let (stop, stopped) = watch::channel(false);
    let writer = spawn(
        store.clone(),
        rx,
        prices(None, None),
        stats,
        config(500, 20),
        stopped,
    );
    sink.record(record("no price"));
    // No usage: nothing to price.
    let mut none = record("no usage");
    none.usage = None;
    sink.record(none);
    // A model the catalog does not know.
    let mut unknown = record("unknown model");
    unknown.attempts[0].model = "gone".into();
    sink.record(unknown);
    // Refused before any attempt: no provider, no model.
    let mut refused = record("refused");
    refused.status = 429;
    refused.usage = None;
    refused.attempts.clear();
    sink.record(refused);
    stop.send(true).unwrap();
    writer.await.unwrap();
    let rows = store.recent_logs(10).await.unwrap();
    assert_eq!(rows.len(), 4);
    for r in &rows {
        assert_eq!(r.cost_micros, 0, "{}", r.requested);
        assert!(!r.priced, "{}", r.requested);
    }
    let refused = rows.iter().find(|r| r.requested == "refused").unwrap();
    assert_eq!(
        (refused.provider.clone(), refused.model.clone()),
        (None, None)
    );
    assert_eq!(refused.input_tokens, None);
    assert_eq!(refused.status, 429);
}

#[tokio::test]
async fn only_a_side_with_tokens_needs_a_price() {
    let store = Store::open_in_memory().await.unwrap();
    let (sink, rx) = LogSink::channel(100);
    let (stop, stopped) = watch::channel(false);
    let stats = sink.stats();
    // An embeddings model: an input price and no output price.
    let writer = spawn(
        store.clone(),
        rx,
        prices(Some(100_000), None),
        stats,
        config(500, 20),
        stopped,
    );
    let mut embedding = record("embedding");
    embedding.usage = Some(Usage {
        input_tokens: 10_000,
        output_tokens: 0,
    });
    sink.record(embedding);
    sink.record(record("chat"));
    stop.send(true).unwrap();
    writer.await.unwrap();
    let rows = store.recent_logs(10).await.unwrap();
    let embedding = rows.iter().find(|r| r.requested == "embedding").unwrap();
    assert!(embedding.priced);
    assert_eq!(embedding.cost_micros, 1_000);
    let chat = rows.iter().find(|r| r.requested == "chat").unwrap();
    assert!(!chat.priced, "output tokens without an output price");
    assert_eq!(chat.cost_micros, 0);
}

#[tokio::test]
async fn the_answering_attempt_names_the_provider_and_model() {
    let store = Store::open_in_memory().await.unwrap();
    let (sink, rx) = LogSink::channel(100);
    let (stop, stopped) = watch::channel(false);
    let stats = sink.stats();
    let writer = spawn(
        store.clone(),
        rx,
        prices(None, None),
        stats,
        config(500, 20),
        stopped,
    );
    let attempt = |provider: &str, outcome| Attempt {
        provider: provider.into(),
        model: "m".into(),
        outcome,
        status: Some(200),
        duration_ms: 1,
    };
    let mut fell_back = record("fell back");
    fell_back.attempts = vec![
        attempt("a", AttemptOutcome::Retryable),
        attempt("b", AttemptOutcome::Ok),
        attempt("c", AttemptOutcome::Skipped),
    ];
    sink.record(fell_back);
    let mut failed = record("failed");
    failed.status = 502;
    failed.usage = None;
    failed.attempts = vec![
        attempt("a", AttemptOutcome::Retryable),
        attempt("b", AttemptOutcome::Fatal),
    ];
    sink.record(failed);
    stop.send(true).unwrap();
    writer.await.unwrap();
    let rows = store.recent_logs(10).await.unwrap();
    let by = |name: &str| rows.iter().find(|r| r.requested == name).unwrap().clone();
    assert_eq!(by("fell back").provider.as_deref(), Some("b"));
    assert_eq!(by("failed").provider.as_deref(), Some("b"));
    // The attempts are kept, in order.
    let attempts: serde_json::Value = serde_json::from_str(&by("fell back").attempts).unwrap();
    let providers: Vec<&str> = attempts
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["provider"].as_str().unwrap())
        .collect();
    assert_eq!(providers, ["a", "b", "c"]);
    assert_eq!(attempts[1]["outcome"], "ok");
    assert_eq!(attempts[2]["outcome"], "skipped");
    assert_eq!(attempts[0]["status"], 200);
}

#[tokio::test]
async fn a_full_queue_drops_and_counts_without_blocking() {
    // Review focus 3: the writer is stalled (nothing reads the queue) and
    // every call still returns at once.
    let (sink, _stalled) = LogSink::channel(1);
    let started = Instant::now();
    for i in 0..1_000 {
        sink.record(record(&format!("r{i}")));
    }
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(sink.stats().dropped.load(Ordering::Relaxed), 999);
    assert_eq!(QUEUE_CAPACITY, 10_000);
}

#[tokio::test]
async fn calls_answer_while_the_log_queue_is_full() {
    let (sink, _stalled) = LogSink::channel(1);
    let stats = sink.stats();
    let h = harness_with_sink("openai", Arc::new(sink)).await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "c1", "model": "x",
            "choices": [{ "message": { "role": "assistant", "content": "hi" }, "finish_reason": "stop" }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 2 }
        })))
        .mount(&h.upstream)
        .await;
    let body =
        json!({ "model": "p/m", "messages": [{ "role": "user", "content": "hi" }] }).to_string();
    let started = Instant::now();
    for _ in 0..20 {
        let (status, _) = post_chat(&h.app, Some(&h.key), &body).await;
        assert_eq!(status, 200);
    }
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(stats.dropped.load(Ordering::Relaxed), 19);
}

#[tokio::test]
async fn shutdown_drains_the_queue() {
    let store = Store::open_in_memory().await.unwrap();
    let (sink, rx) = LogSink::channel(100);
    let stats = sink.stats();
    let (stop, stopped) = watch::channel(false);
    // A long wait and a big batch: only the shutdown can write these.
    let writer = spawn(
        store.clone(),
        rx,
        prices(None, None),
        stats.clone(),
        config(500, 60_000),
        stopped,
    );
    for i in 0..7 {
        sink.record(record(&format!("r{i}")));
    }
    stop.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(5), writer)
        .await
        .expect("the writer ends")
        .unwrap();
    assert_eq!(store.recent_logs(100).await.unwrap().len(), 7);
    assert_eq!(stats.written.load(Ordering::Relaxed), 7);
    // After the drain nothing is accepted any more, and nothing blocks.
    sink.record(record("late"));
    assert_eq!(stats.dropped.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn a_call_is_logged_with_its_price_from_the_snapshot() {
    let store_of;
    let (sink, rx) = LogSink::channel(100);
    let stats = sink.stats();
    let h = harness_with_sink("openai", Arc::new(sink)).await;
    store_of = h.store.clone();
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "c1", "model": "x",
            "choices": [{ "message": { "role": "assistant", "content": "hi" }, "finish_reason": "stop" }],
            "usage": { "prompt_tokens": 1000, "completion_tokens": 500 }
        })))
        .mount(&h.upstream)
        .await;
    let model_id = h
        .store
        .list_models()
        .await
        .unwrap()
        .into_iter()
        .find(|m| m.name == "m")
        .unwrap()
        .id;
    let mut tx = h.store.begin().await.unwrap();
    tx.set_model_input_price(model_id, Some(2_000_000))
        .await
        .unwrap();
    tx.set_model_output_price(model_id, Some(4_000_000))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    h.state.refresh().await.unwrap();
    let (stop, stopped) = watch::channel(false);
    let writer = spawn(
        store_of.clone(),
        rx,
        snapshot_prices(h.state.clone()),
        stats,
        config(500, 20),
        stopped,
    );
    let body =
        json!({ "model": "p/m", "messages": [{ "role": "user", "content": "hi" }] }).to_string();
    let (status, _) = post_chat(&h.app, Some(&h.key), &body).await;
    assert_eq!(status, 200);
    stop.send(true).unwrap();
    writer.await.unwrap();
    let rows = store_of.recent_logs(10).await.unwrap();
    assert_eq!(rows.len(), 1);
    let r = &rows[0];
    assert_eq!(
        (r.provider.as_deref(), r.model.as_deref()),
        (Some("p"), Some("m"))
    );
    assert_eq!((r.input_tokens, r.output_tokens), (Some(1000), Some(500)));
    // 1000 x 2 + 500 x 4 thousandths of a micro-dollar per token.
    assert_eq!(r.cost_micros, 4_000);
    assert!(r.priced);
    assert_eq!(
        (r.endpoint.as_str(), r.status, r.stream),
        ("chat", 200, false)
    );
    assert_eq!(r.requested, "p/m");
    assert!(!r.cached);
}

fn log_at(at: &str, requested: &str) -> NewLog {
    NewLog {
        at: at.into(),
        key_id: None,
        user_id: None,
        team_id: None,
        requested: requested.into(),
        endpoint: "chat".into(),
        stream: false,
        status: 200,
        provider: None,
        model: None,
        input_tokens: None,
        output_tokens: None,
        cost_micros: 0,
        priced: false,
        cached: false,
        duration_ms: 1,
        attempts: "[]".into(),
    }
}

#[tokio::test]
async fn retention_deletes_only_old_rows_in_batches() {
    let store = Store::open_in_memory().await.unwrap();
    let mut rows = Vec::new();
    for i in 0..7 {
        rows.push(log_at("2000-01-01 00:00:00", &format!("old{i}")));
    }
    rows.push(log_at("2500-01-01 00:00:00", "boundary-after"));
    rows.push(log_at("2999-01-01 00:00:00", "new"));
    store.insert_logs(&rows).await.unwrap();

    let purged = purge(&store, "2500-01-01 00:00:00", 3, Duration::from_millis(1))
        .await
        .unwrap();
    assert_eq!(
        purged,
        Purged {
            rows: 7,
            batches: 3
        }
    );
    let left: Vec<String> = store
        .recent_logs(100)
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.requested)
        .collect();
    assert_eq!(left, ["new", "boundary-after"]);
    // Nothing old is left: a second pass deletes nothing.
    let again = purge(&store, "2500-01-01 00:00:00", 3, Duration::from_millis(1))
        .await
        .unwrap();
    assert_eq!(
        again,
        Purged {
            rows: 0,
            batches: 0
        }
    );
}

#[tokio::test]
async fn the_retention_task_deletes_by_the_setting_and_stops() {
    let store = Store::open_in_memory().await.unwrap();
    // The default is 30 days.
    assert_eq!(store.log_retention_days().await.unwrap(), 30);
    store
        .insert_logs(&[
            log_at("2000-01-01 00:00:00", "old"),
            log_at("2999-01-01 00:00:00", "new"),
        ])
        .await
        .unwrap();
    let (stop, stopped) = watch::channel(false);
    let task = retention::spawn(
        store.clone(),
        RetentionConfig {
            interval: Duration::from_millis(20),
            batch: 1_000,
            pause: Duration::from_millis(1),
        },
        stopped,
    );
    for _ in 0..400 {
        if store.recent_logs(10).await.unwrap().len() == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let left = store.recent_logs(10).await.unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].requested, "new");
    stop.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("the task ends")
        .unwrap();
}

#[tokio::test]
async fn the_retention_setting_is_stored_and_read_back() {
    let store = Store::open_in_memory().await.unwrap();
    assert_eq!(store.log_retention_days().await.unwrap(), 30);
    let mut tx = store.begin().await.unwrap();
    tx.set_log_retention_days(7).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(store.log_retention_days().await.unwrap(), 7);
}
