mod common;

use std::time::{Duration, Instant};

use axum::http::StatusCode;
use common::{allow_model, hanging_upstream, harness, post_chat, Harness};
use serde_json::{json, Value};
use ultrafast_gateway::routing::{BreakerSettings, TargetRef, TargetState};
use ultrafast_gateway::store::{RouteSettings, TargetsInput};
use ultrafast_gateway::telemetry::{AttemptOutcome, RequestRecord};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

const DEFAULTS: RouteSettings = RouteSettings {
    retries: 2,
    first_token_timeout_ms: 30_000,
    total_timeout_ms: 300_000,
    breaker_failures: 5,
    breaker_window_s: 60,
    breaker_open_s: 30,
};

const NO_PROVIDER: &str = "No provider could serve this request.";

fn ok(text: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "c1", "model": "m",
        "choices": [{ "message": { "role": "assistant", "content": text }, "finish_reason": "stop" }],
        "usage": { "prompt_tokens": 1, "completion_tokens": 2 }
    }))
}

fn sse(text: &str, done: bool) -> ResponseTemplate {
    let mut body = format!(
        "data: {{\"choices\":[{{\"delta\":{{\"content\":\"{text}\"}},\"finish_reason\":null}}]}}\n\n"
    );
    if done {
        body.push_str("data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1}}\n\ndata: [DONE]\n\n");
    }
    ResponseTemplate::new(200).set_body_raw(body, "text/event-stream")
}

fn chat(model: &str, stream: bool) -> String {
    json!({ "model": model, "stream": stream, "messages": [{ "role": "user", "content": "hi" }] })
        .to_string()
}

/// A provider of this name on a mock server of its own, with a model "m".
/// Returns the server and the id of the model.
async fn provider(h: &Harness, name: &str) -> (MockServer, i64) {
    let server = MockServer::start().await;
    h.store
        .insert_provider(name, "openai", &server.uri(), None)
        .await
        .unwrap();
    let id = allow_model(&h.store, name, "m").await;
    (server, id)
}

/// A route over model ids; every primary has weight 1.
async fn route(
    h: &Harness,
    name: &str,
    primaries: &[i64],
    fallbacks: &[i64],
    settings: RouteSettings,
) {
    let mut tx = h.store.begin().await.unwrap();
    let id = tx.insert_route(name, &settings, true).await.unwrap();
    tx.replace_targets(
        id,
        &TargetsInput {
            primaries: primaries.iter().map(|m| (*m, 1)).collect(),
            fallbacks: fallbacks.to_vec(),
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    h.state.refresh().await.unwrap();
}

async fn hits(server: &MockServer) -> usize {
    server.received_requests().await.unwrap().len()
}

fn message(body: &str) -> String {
    let v: Value = serde_json::from_str(body).unwrap();
    v["error"]["message"].as_str().unwrap().to_string()
}

fn seen(r: &RequestRecord) -> Vec<(String, AttemptOutcome, Option<u16>)> {
    r.attempts
        .iter()
        .map(|a| (a.provider.clone(), a.outcome, a.status))
        .collect()
}

fn target(provider: &str) -> TargetRef {
    TargetRef {
        provider: provider.into(),
        model: "m".into(),
        model_id: 0,
    }
}

fn state_of(h: &Harness, provider: &str) -> Option<TargetState> {
    h.state
        .health
        .view()
        .into_iter()
        .find(|t| t.provider == provider)
        .map(|t| t.state)
}

/// The text of the `content` deltas of a stream body.
fn text(body: &str) -> String {
    body.lines()
        .filter_map(|l| l.strip_prefix("data: "))
        .filter_map(|d| serde_json::from_str::<Value>(d).ok())
        .filter_map(|v| {
            v["choices"][0]["delta"]["content"]
                .as_str()
                .map(str::to_string)
        })
        .collect()
}

#[tokio::test]
async fn retries_on_429_and_5xx_not_on_400() {
    for (status, expect_hits) in [(429, 2), (500, 2), (503, 2), (408, 2)] {
        let h = harness("openai").await;
        let (a, m) = provider(&h, "a").await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(status).set_body_string("no"))
            .up_to_n_times(1)
            .mount(&a)
            .await;
        Mock::given(method("POST"))
            .respond_with(ok("fine"))
            .mount(&a)
            .await;
        route(&h, "r", &[m], &[], DEFAULTS).await;
        let (s, body) = post_chat(&h.app, Some(&h.key), &chat("r", false)).await;
        assert_eq!(s, StatusCode::OK, "{status}: {body}");
        assert_eq!(hits(&a).await, expect_hits, "{status}");
        let r = &h.sink.records()[0];
        assert_eq!(
            seen(r),
            [
                ("a".into(), AttemptOutcome::Retryable, Some(status)),
                ("a".into(), AttemptOutcome::Ok, Some(200)),
            ]
        );
    }

    // A 400 is the caller's to fix: the provider's answer, at once.
    for status in [400, 404, 422] {
        let h = harness("openai").await;
        let (a, m) = provider(&h, "a").await;
        let (b, mb) = provider(&h, "b").await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(status)
                    .set_body_json(json!({ "error": { "message": "bad input" } })),
            )
            .mount(&a)
            .await;
        Mock::given(method("POST"))
            .respond_with(ok("b"))
            .mount(&b)
            .await;
        route(&h, "r", &[m], &[mb], DEFAULTS).await;
        let (s, body) = post_chat(&h.app, Some(&h.key), &chat("r", false)).await;
        assert_eq!(s.as_u16(), status);
        assert_eq!(message(&body), "bad input");
        assert_eq!(hits(&a).await, 1, "{status} is not retried");
        assert_eq!(hits(&b).await, 0, "{status} does not fail over");
        let r = &h.sink.records()[0];
        assert_eq!(r.attempts[0].outcome, AttemptOutcome::Fatal);
        assert_eq!(r.attempts[1].outcome, AttemptOutcome::Skipped);
    }
}

#[tokio::test]
async fn a_direct_call_retries_and_then_answers_503_naming_no_provider() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(502).set_body_string("<html>secret-detail</html>"))
        .mount(&h.upstream)
        .await;
    let (s, body) = post_chat(&h.app, Some(&h.key), &chat("p/gpt-4o", false)).await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(message(&body), NO_PROVIDER);
    assert!(!body.contains("secret-detail") && !body.contains("provider-secret"));
    // 1 + 2 retries.
    assert_eq!(hits(&h.upstream).await, 3);
    let r = &h.sink.records()[0];
    assert_eq!(r.status, 503);
    assert_eq!(r.attempts.len(), 3);
    assert!(r
        .attempts
        .iter()
        .all(|a| a.outcome == AttemptOutcome::Retryable));
}

#[tokio::test]
async fn an_unreachable_provider_is_retried_and_fails_over() {
    let h = harness("openai").await;
    h.store
        .insert_provider("dead", "openai", "http://127.0.0.1:1", None)
        .await
        .unwrap();
    let dead = allow_model(&h.store, "dead", "m").await;
    let (b, mb) = provider(&h, "b").await;
    Mock::given(method("POST"))
        .respond_with(ok("b"))
        .mount(&b)
        .await;
    route(&h, "r", &[dead], &[mb], DEFAULTS).await;
    let (s, _) = post_chat(&h.app, Some(&h.key), &chat("r", false)).await;
    assert_eq!(s, StatusCode::OK);
    let r = &h.sink.records()[0];
    assert_eq!(
        seen(r),
        [
            ("dead".into(), AttemptOutcome::Retryable, None),
            ("dead".into(), AttemptOutcome::Retryable, None),
            ("dead".into(), AttemptOutcome::Retryable, None),
            ("b".into(), AttemptOutcome::Ok, Some(200)),
        ]
    );
}

#[tokio::test]
async fn fallbacks_are_tried_in_order_after_the_primaries() {
    let h = harness("openai").await;
    let (a, ma) = provider(&h, "a").await;
    let (b, mb) = provider(&h, "b").await;
    let (c, mc) = provider(&h, "c").await;
    let (d, md) = provider(&h, "d").await;
    for down in [&a, &b] {
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500))
            .mount(down)
            .await;
    }
    Mock::given(method("POST"))
        .respond_with(ok("c"))
        .mount(&c)
        .await;
    Mock::given(method("POST"))
        .respond_with(ok("d"))
        .mount(&d)
        .await;
    let settings = RouteSettings {
        retries: 0,
        ..DEFAULTS
    };
    route(&h, "r", &[ma], &[mb, mc, md], settings).await;
    let (s, body) = post_chat(&h.app, Some(&h.key), &chat("r", false)).await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.contains("\"c\""));
    let r = &h.sink.records()[0];
    assert_eq!(
        seen(r),
        [
            ("a".into(), AttemptOutcome::Retryable, Some(500)),
            ("b".into(), AttemptOutcome::Retryable, Some(500)),
            ("c".into(), AttemptOutcome::Ok, Some(200)),
            ("d".into(), AttemptOutcome::Skipped, None),
        ]
    );
    assert_eq!(hits(&d).await, 0);
}

#[tokio::test]
async fn every_primary_is_tried_before_any_fallback() {
    let h = harness("openai").await;
    let (a, ma) = provider(&h, "a").await;
    let (b, mb) = provider(&h, "b").await;
    let (f, mf) = provider(&h, "f").await;
    for down in [&a, &b] {
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500))
            .mount(down)
            .await;
    }
    Mock::given(method("POST"))
        .respond_with(ok("f"))
        .mount(&f)
        .await;
    let settings = RouteSettings {
        retries: 0,
        ..DEFAULTS
    };
    route(&h, "r", &[ma, mb], &[mf], settings).await;
    for _ in 0..4 {
        let (s, _) = post_chat(&h.app, Some(&h.key), &chat("r", false)).await;
        assert_eq!(s, StatusCode::OK);
    }
    for r in h.sink.records() {
        let order: Vec<_> = r.attempts.iter().map(|a| a.provider.as_str()).collect();
        assert_eq!(order[2], "f", "{order:?}");
        let mut primaries = order[..2].to_vec();
        primaries.sort();
        assert_eq!(primaries, ["a", "b"]);
    }
}

#[tokio::test]
async fn a_route_is_spread_over_its_primaries() {
    let h = harness("openai").await;
    let (a, ma) = provider(&h, "a").await;
    let (b, mb) = provider(&h, "b").await;
    Mock::given(method("POST"))
        .respond_with(ok("a"))
        .mount(&a)
        .await;
    Mock::given(method("POST"))
        .respond_with(ok("b"))
        .mount(&b)
        .await;
    route(&h, "r", &[ma, mb], &[], DEFAULTS).await;
    for _ in 0..40 {
        post_chat(&h.app, Some(&h.key), &chat("r", false)).await;
    }
    let (na, nb) = (hits(&a).await, hits(&b).await);
    assert_eq!(na + nb, 40);
    assert!(na >= 5 && nb >= 5, "{na} / {nb}");
}

#[tokio::test]
async fn first_token_timeout_fails_over() {
    let h = harness("openai").await;
    let (a, ma) = provider(&h, "a").await;
    let (b, mb) = provider(&h, "b").await;
    Mock::given(method("POST"))
        .respond_with(ok("slow").set_delay(Duration::from_secs(5)))
        .mount(&a)
        .await;
    Mock::given(method("POST"))
        .respond_with(ok("fast"))
        .mount(&b)
        .await;
    let settings = RouteSettings {
        retries: 0,
        first_token_timeout_ms: 300,
        ..DEFAULTS
    };
    route(&h, "r", &[ma], &[mb], settings).await;
    let started = Instant::now();
    let (s, body) = post_chat(&h.app, Some(&h.key), &chat("r", false)).await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.contains("fast"));
    assert!(started.elapsed() < Duration::from_secs(3));
    let r = &h.sink.records()[0];
    assert_eq!(
        seen(r),
        [
            ("a".into(), AttemptOutcome::Retryable, None),
            ("b".into(), AttemptOutcome::Ok, Some(200)),
        ]
    );
}

#[tokio::test]
async fn stream_fails_over_before_first_byte_not_after() {
    // Before: the primary says nothing for too long; the caller sees only
    // the fallback's text.
    let h = harness("openai").await;
    let (a, ma) = provider(&h, "a").await;
    let (b, mb) = provider(&h, "b").await;
    Mock::given(method("POST"))
        .respond_with(sse("from-a", true).set_delay(Duration::from_secs(5)))
        .mount(&a)
        .await;
    Mock::given(method("POST"))
        .respond_with(sse("from-b", true))
        .mount(&b)
        .await;
    let settings = RouteSettings {
        retries: 0,
        first_token_timeout_ms: 300,
        ..DEFAULTS
    };
    route(&h, "r", &[ma], &[mb], settings).await;
    let (s, body) = post_chat(&h.app, Some(&h.key), &chat("r", true)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(text(&body), "from-b");
    assert!(!body.contains("from-a"));
    assert!(body.contains("[DONE]"));
    let r = &h.sink.wait_for(1).await[0];
    assert_eq!(r.attempts[0].outcome, AttemptOutcome::Retryable);
    assert_eq!(r.attempts[1].outcome, AttemptOutcome::Ok);

    // After: the primary sends one event and then breaks off. The caller
    // gets that event and an error; the fallback is never called, so no text
    // is sent twice.
    let h = harness("openai").await;
    let (a, ma) = provider(&h, "a").await;
    let (b, mb) = provider(&h, "b").await;
    Mock::given(method("POST"))
        .respond_with(sse("from-a", false))
        .mount(&a)
        .await;
    Mock::given(method("POST"))
        .respond_with(sse("from-b", true))
        .mount(&b)
        .await;
    route(&h, "r", &[ma], &[mb], DEFAULTS).await;
    let (s, body) = post_chat(&h.app, Some(&h.key), &chat("r", true)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(text(&body), "from-a");
    assert!(body.contains("\"error\""), "{body}");
    assert!(!body.contains("[DONE]"));
    assert_eq!(hits(&a).await, 1, "no retry of a stream that began");
    assert_eq!(hits(&b).await, 0);
    let r = &h.sink.wait_for(1).await[0];
    assert_eq!(r.status, 200);
    assert_eq!(
        seen(r),
        [
            ("a".into(), AttemptOutcome::Retryable, Some(200)),
            ("b".into(), AttemptOutcome::Skipped, None),
        ]
    );
    // The break counts against the target.
    let health = h.state.health.view();
    let a_health = health.iter().find(|t| t.provider == "a").unwrap();
    assert_eq!((a_health.successes, a_health.failures), (1, 1));
}

#[tokio::test]
async fn a_stream_that_starts_with_an_error_event_fails_over() {
    let h = harness("openai").await;
    let (a, ma) = provider(&h, "a").await;
    let (b, mb) = provider(&h, "b").await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            "data: {\"error\":{\"message\":\"overloaded\",\"type\":\"server_error\"}}\n\n",
            "text/event-stream",
        ))
        .mount(&a)
        .await;
    Mock::given(method("POST"))
        .respond_with(sse("from-b", true))
        .mount(&b)
        .await;
    route(
        &h,
        "r",
        &[ma],
        &[mb],
        RouteSettings {
            retries: 0,
            ..DEFAULTS
        },
    )
    .await;
    let (s, body) = post_chat(&h.app, Some(&h.key), &chat("r", true)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(text(&body), "from-b");
}

#[tokio::test]
async fn the_total_timeout_ends_a_stream_with_an_error_event() {
    let h = harness("openai").await;
    let (uri, _closed) = hanging_upstream().await;
    h.store
        .insert_provider("hang", "openai", &uri, None)
        .await
        .unwrap();
    let m = allow_model(&h.store, "hang", "m").await;
    let settings = RouteSettings {
        total_timeout_ms: 700,
        ..DEFAULTS
    };
    route(&h, "r", &[m], &[], settings).await;
    let started = Instant::now();
    let (s, body) = post_chat(&h.app, Some(&h.key), &chat("r", true)).await;
    assert_eq!(s, StatusCode::OK);
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(text(&body), "one");
    assert!(body.contains("The request timed out."), "{body}");
    assert!(!body.contains("[DONE]"));
    let r = &h.sink.wait_for(1).await[0];
    assert_eq!(r.status, 200);
    assert_eq!(r.attempts[0].outcome, AttemptOutcome::Retryable);
    assert_eq!(state_of(&h, "hang"), Some(TargetState::Closed));
    assert_eq!(h.state.health.view()[0].failures, 1);
}

#[tokio::test]
async fn the_total_timeout_bounds_the_retries_of_a_route() {
    let h = harness("openai").await;
    let (a, ma) = provider(&h, "a").await;
    Mock::given(method("POST"))
        .respond_with(ok("x").set_delay(Duration::from_secs(5)))
        .mount(&a)
        .await;
    let settings = RouteSettings {
        total_timeout_ms: 500,
        ..DEFAULTS
    };
    route(&h, "r", &[ma], &[], settings).await;
    let started = Instant::now();
    let (s, body) = post_chat(&h.app, Some(&h.key), &chat("r", false)).await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(message(&body), NO_PROVIDER);
    assert!(started.elapsed() < Duration::from_secs(3));
}

fn flaky(failures: u32, open_s: i64) -> RouteSettings {
    RouteSettings {
        retries: 0,
        breaker_failures: i64::from(failures),
        breaker_window_s: 60,
        breaker_open_s: open_s,
        ..DEFAULTS
    }
}

#[tokio::test]
async fn breaker_opens_half_opens_closes() {
    let h = harness("openai").await;
    let (a, ma) = provider(&h, "a").await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .up_to_n_times(2)
        .mount(&a)
        .await;
    Mock::given(method("POST"))
        .respond_with(ok("back"))
        .mount(&a)
        .await;
    route(&h, "r", &[ma], &[], flaky(2, 1)).await;
    for _ in 0..2 {
        let (s, _) = post_chat(&h.app, Some(&h.key), &chat("r", false)).await;
        assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
    }
    assert_eq!(state_of(&h, "a"), Some(TargetState::Open));
    // Open: refused without a call.
    let (s, body) = post_chat(&h.app, Some(&h.key), &chat("r", false)).await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(message(&body), NO_PROVIDER);
    assert_eq!(hits(&a).await, 2);
    assert_eq!(
        h.sink.records()[2].attempts[0].outcome,
        AttemptOutcome::CircuitOpen
    );
    // After the open time one trial goes through, and its success closes it.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let (s, body) = post_chat(&h.app, Some(&h.key), &chat("r", false)).await;
    assert_eq!(s, StatusCode::OK, "{body}");
    assert_eq!(state_of(&h, "a"), Some(TargetState::Closed));
    assert_eq!(hits(&a).await, 3);
    let (s, _) = post_chat(&h.app, Some(&h.key), &chat("r", false)).await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn a_failed_trial_opens_the_breaker_again() {
    let h = harness("openai").await;
    let (a, ma) = provider(&h, "a").await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&a)
        .await;
    route(&h, "r", &[ma], &[], flaky(1, 1)).await;
    post_chat(&h.app, Some(&h.key), &chat("r", false)).await;
    assert_eq!(state_of(&h, "a"), Some(TargetState::Open));
    tokio::time::sleep(Duration::from_millis(1100)).await;
    post_chat(&h.app, Some(&h.key), &chat("r", false)).await;
    assert_eq!(hits(&a).await, 2);
    assert_eq!(state_of(&h, "a"), Some(TargetState::Open));
    post_chat(&h.app, Some(&h.key), &chat("r", false)).await;
    assert_eq!(hits(&a).await, 2, "refused again without a call");
}

#[tokio::test]
async fn bad_requests_do_not_open_breaker() {
    let h = harness("openai").await;
    let (a, ma) = provider(&h, "a").await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(400))
        .mount(&a)
        .await;
    route(&h, "r", &[ma], &[], flaky(2, 30)).await;
    for _ in 0..6 {
        let (s, _) = post_chat(&h.app, Some(&h.key), &chat("r", false)).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }
    assert_eq!(hits(&a).await, 6, "every call reached the provider");
    assert_eq!(state_of(&h, "a"), Some(TargetState::Closed));
    assert_eq!(h.state.health.view()[0].failures, 0);
}

#[tokio::test]
async fn the_breaker_of_a_model_is_shared_by_routes_and_direct_calls() {
    let h = harness("openai").await;
    let (a, ma) = provider(&h, "a").await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&a)
        .await;
    route(&h, "r", &[ma], &[], flaky(1, 30)).await;
    post_chat(&h.app, Some(&h.key), &chat("r", false)).await;
    let before = hits(&a).await;
    let (s, _) = post_chat(&h.app, Some(&h.key), &chat("a/m", false)).await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(hits(&a).await, before);
    assert_eq!(
        h.sink.records()[1].attempts[0].outcome,
        AttemptOutcome::CircuitOpen
    );
}

#[tokio::test]
async fn all_open_is_fast_503() {
    let h = harness("openai").await;
    let (_a, ma) = provider(&h, "a").await;
    let (_b, mb) = provider(&h, "b").await;
    route(&h, "r", &[ma], &[mb], DEFAULTS).await;
    let settings = BreakerSettings::DEFAULT;
    for name in ["a", "b"] {
        for _ in 0..settings.failures {
            h.state.health.report(
                &target(name),
                false,
                true,
                Some(500),
                tokio::time::Instant::now(),
                &settings,
            );
        }
    }
    tokio::time::pause();
    let virtual_start = tokio::time::Instant::now();
    let real_start = Instant::now();
    let (s, body) = post_chat(&h.app, Some(&h.key), &chat("r", false)).await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(message(&body), NO_PROVIDER);
    assert!(virtual_start.elapsed() < Duration::from_millis(100));
    assert!(real_start.elapsed() < Duration::from_millis(100));
    let r = &h.sink.records()[0];
    assert_eq!(
        seen(r),
        [
            ("a".into(), AttemptOutcome::CircuitOpen, None),
            ("b".into(), AttemptOutcome::CircuitOpen, None),
        ]
    );
}

#[tokio::test]
async fn skips_targets_caller_may_not_call() {
    use ultrafast_gateway::store::Grants;
    let h = harness("openai").await;
    let (a, ma) = provider(&h, "a").await;
    let (b, mb) = provider(&h, "b").await;
    // The key has no owner, so it may call only what is granted to everyone.
    let mut tx = h.store.begin().await.unwrap();
    tx.replace_grants(ma, &Grants::default()).await.unwrap();
    tx.commit().await.unwrap();
    Mock::given(method("POST"))
        .respond_with(ok("a"))
        .mount(&a)
        .await;
    Mock::given(method("POST"))
        .respond_with(ok("b"))
        .mount(&b)
        .await;
    route(&h, "r", &[ma], &[mb], DEFAULTS).await;
    let (s, body) = post_chat(&h.app, Some(&h.key), &chat("r", false)).await;
    assert_eq!(s, StatusCode::OK);
    assert!(body.contains("\"b\""));
    assert_eq!(hits(&a).await, 0);
    let r = &h.sink.records()[0];
    assert_eq!(
        seen(r),
        [
            ("a".into(), AttemptOutcome::Skipped, None),
            ("b".into(), AttemptOutcome::Ok, Some(200)),
        ]
    );
    // A disabled model is skipped the same way.
    let mut tx = h.store.begin().await.unwrap();
    assert!(tx.set_model_enabled(mb, false).await.unwrap());
    tx.replace_grants(
        ma,
        &Grants {
            everyone: true,
            ..Grants::default()
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    h.state.refresh().await.unwrap();
    let (s, _) = post_chat(&h.app, Some(&h.key), &chat("r", false)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(hits(&a).await, 1);
    assert_eq!(
        h.sink.records()[1].attempts[1].outcome,
        AttemptOutcome::Skipped
    );
}

#[tokio::test]
async fn a_caller_that_goes_away_mid_call_leaves_that_target_retryable() {
    let h = harness("openai").await;
    let (a, ma) = provider(&h, "a").await;
    let (b, mb) = provider(&h, "b").await;
    Mock::given(method("POST"))
        .respond_with(ok("slow").set_delay(Duration::from_secs(10)))
        .mount(&a)
        .await;
    Mock::given(method("POST"))
        .respond_with(ok("b"))
        .mount(&b)
        .await;
    route(&h, "r", &[ma], &[mb], DEFAULTS).await;
    // The request future is dropped while the provider is still thinking.
    let gone = tokio::time::timeout(
        Duration::from_millis(400),
        post_chat(&h.app, Some(&h.key), &chat("r", false)),
    )
    .await;
    assert!(gone.is_err());
    let records = h.sink.wait_for(1).await;
    assert_eq!(records.len(), 1);
    let r = &records[0];
    assert_eq!(r.status, 499);
    assert_eq!(
        seen(r),
        [
            ("a".into(), AttemptOutcome::Retryable, None),
            ("b".into(), AttemptOutcome::Skipped, None),
        ]
    );
}

#[tokio::test]
async fn a_request_the_translator_refuses_is_recorded_as_an_attempt() {
    // Anthropic cannot carry the `name` of a message: building the request fails.
    let h = harness("openai").await;
    h.store
        .insert_provider("anth", "anthropic", "http://127.0.0.1:1", None)
        .await
        .unwrap();
    allow_model(&h.store, "anth", "m").await;
    h.state.refresh().await.unwrap();
    let body = json!({
        "model": "anth/m", "messages": [{ "role": "user", "name": "bob", "content": "hi" }]
    })
    .to_string();
    let (s, _) = post_chat(&h.app, Some(&h.key), &body).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let r = &h.sink.records()[0];
    assert_eq!(seen(r), [("anth".into(), AttemptOutcome::Fatal, None)]);
}

#[tokio::test]
async fn direct_calls_use_the_engine_with_default_settings() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429).set_body_string("slow down"))
        .up_to_n_times(1)
        .mount(&h.upstream)
        .await;
    Mock::given(method("POST"))
        .respond_with(ok("fine"))
        .mount(&h.upstream)
        .await;
    let (s, body) = post_chat(&h.app, Some(&h.key), &chat("p/gpt-4o", false)).await;
    assert_eq!(s, StatusCode::OK, "{body}");
    assert_eq!(hits(&h.upstream).await, 2);
}
