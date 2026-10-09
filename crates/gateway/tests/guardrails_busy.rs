//! A large input finds every scan slot taken: the call is refused as busy
//! (503, nothing sent to the provider) and gives its rate-limit permit back.
//! This file is its own test binary, so using up the process-wide scan slots
//! cannot slow down any other test.

mod common;

use axum::http::StatusCode;
use common::{harness, post_chat};
use serde_json::{json, Value};
use ultrafast_gateway::guardrails::run::{scan_slots, INLINE_LIMIT_BYTES};
use ultrafast_gateway::limits::{LimitScope, RateLimit};
use ultrafast_gateway::store::NewGuardrail;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

fn chat_body(user_text: &str) -> String {
    json!({ "model": "p/m", "max_tokens": 50,
            "messages": [{ "role": "user", "content": user_text }] })
    .to_string()
}

#[tokio::test]
async fn a_large_input_with_no_free_scan_slot_is_refused_as_busy_and_gives_its_permit_back() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "c1", "model": "m",
            "choices": [{ "message": { "role": "assistant", "content": "hello" }, "finish_reason": "stop" }],
            "usage": { "prompt_tokens": 9, "completion_tokens": 4 }
        })))
        .mount(&h.upstream)
        .await;
    let rules = json!([{ "id": "email", "matcher": { "pii": ["EMAIL"] },
                         "action": "redact", "directions": "input" }])
    .to_string();
    let mut tx = h.store.begin().await.unwrap();
    tx.insert_guardrail(NewGuardrail {
        name: "g",
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
    // one request a minute and one at a time: a permit that is not given
    // back refuses the next call
    tx.upsert_limit(
        LimitScope::Gateway,
        None,
        &RateLimit {
            requests_per_minute: Some(1),
            tokens_per_minute: None,
            concurrent: Some(1),
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    h.state.refresh().await.unwrap();

    let pool = scan_slots();
    let all = u32::try_from(pool.available_permits()).unwrap();
    let held = pool.clone().acquire_many_owned(all).await.unwrap();

    let big = "word ".repeat(INLINE_LIMIT_BYTES / 4);
    let (status, body) = post_chat(&h.app, Some(&h.key), &chat_body(&big)).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"]["type"], "upstream_error");
    assert_eq!(
        v["error"]["message"],
        "The guardrails are busy checking other requests. Try again shortly."
    );
    assert!(
        h.upstream.received_requests().await.unwrap().is_empty(),
        "a refused call reached the provider"
    );

    // The slots come back; the minute's one request and the one concurrent
    // slot are still there.
    drop(held);
    let (status, body) = post_chat(&h.app, Some(&h.key), &chat_body("fine")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}
