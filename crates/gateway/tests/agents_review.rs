//! Pass-through under the production harness (pass-through on): what the
//! final review of the coding-agents work found, and the guarantees that
//! stay with it. Input guardrails read the agent's own body whatever the
//! translator thinks of it, a pass-through stream is read by a tolerant
//! reader, a Responses stream is redacted in its `.done` events too, and
//! nothing that reads or writes the provider account's stored state goes
//! through.

mod common;

use axum::http::StatusCode;
use common::{harness, harness_with_rate, post_to, Harness};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};
use ufaigw_gateway::limits::{Limiter, Permit, Refusal, Subjects};
use ufaigw_gateway::store::NewGuardrail;
use ufaigw_gateway::telemetry::AttemptOutcome;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

const MAIL: &str = "ada@example.com";

async fn guardrail(h: &Harness, rules: Value) {
    let rules = rules.to_string();
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
    tx.commit().await.unwrap();
    h.state.refresh().await.unwrap();
}

async fn block_mail_in(h: &Harness) {
    guardrail(
        h,
        json!([{ "id": "email", "matcher": { "pii": ["EMAIL"] },
                 "action": "block", "directions": "input" }]),
    )
    .await;
}

async fn redact_mail_in(h: &Harness) {
    guardrail(
        h,
        json!([{ "id": "email", "matcher": { "pii": ["EMAIL"] },
                 "action": "redact", "directions": "input" }]),
    )
    .await;
}

async fn redact_mail_out(h: &Harness) {
    guardrail(
        h,
        json!([{ "id": "email", "matcher": { "pii": ["EMAIL"] },
                 "action": "redact", "directions": "output" }]),
    )
    .await;
}

async fn post(h: &Harness, uri: &str, body: &Value) -> (StatusCode, String) {
    let bearer = format!("Bearer {}", h.key);
    let (s, _, b) = post_to(
        &h.app,
        uri,
        &[
            ("authorization", bearer.as_str()),
            ("anthropic-version", "2023-06-01"),
        ],
        &body.to_string(),
    )
    .await;
    (s, b)
}

const ANSWER: &str = r#"{"id":"m","type":"message","role":"assistant","model":"m","content":[{"type":"text","text":"ok"}],"stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":1}}"#;
const RESPONSES_ANSWER: &str = r#"{"id":"r","object":"response","status":"completed","model":"m","output":[],"usage":{"input_tokens":1,"output_tokens":1}}"#;

async fn mock_ok(h: &Harness, p: &str, body: &str, ct: &str) {
    Mock::given(method("POST"))
        .and(path(p.to_string()))
        .respond_with(ResponseTemplate::new(200).set_body_raw(body.to_string(), ct))
        .mount(&h.upstream)
        .await;
}

async fn sent(h: &Harness) -> usize {
    h.upstream.received_requests().await.unwrap().len()
}

fn user(text: &str) -> Value {
    json!({"role":"user","content":text})
}

// ------------------------------------------------------------------ C1

/// A body the translator refuses (one malformed known field) is still
/// scanned: it goes to the provider as it came.
#[tokio::test]
async fn c1_a_body_the_translator_refuses_is_still_blocked() {
    let h = harness("anthropic").await;
    mock_ok(&h, "/v1/messages", ANSWER, "application/json").await;
    block_mail_in(&h).await;
    let body = json!({"model":"p/m","max_tokens":5,"thinking":"yes",
        "messages":[user(&format!("mail {MAIL}"))]});
    let (s, b) = post(&h, "/v1/messages", &body).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{b}");
    assert_eq!(sent(&h).await, 0, "guarded text reached the provider");
}

#[tokio::test]
async fn c1_a_body_the_translator_refuses_is_still_redacted() {
    let h = harness("anthropic").await;
    mock_ok(&h, "/v1/messages", ANSWER, "application/json").await;
    redact_mail_in(&h).await;
    let body = json!({"model":"p/m","max_tokens":5,"thinking":"yes",
        "messages":[user(&format!("mail {MAIL}"))]});
    let (s, b) = post(&h, "/v1/messages", &body).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let got = String::from_utf8(
        h.upstream.received_requests().await.unwrap()[0]
            .body
            .clone(),
    )
    .unwrap();
    assert!(!got.contains(MAIL), "{got}");
    assert!(got.contains("\"thinking\":\"yes\""), "{got}");
}

#[tokio::test]
async fn c1_responses_store_true_is_blocked_by_the_guardrail_and_never_sent() {
    let h = harness("openai").await;
    mock_ok(&h, "/responses", RESPONSES_ANSWER, "application/json").await;
    block_mail_in(&h).await;
    let body = json!({"model":"p/m","store":true,"input":format!("mail {MAIL}")});
    let (s, b) = post(&h, "/v1/responses", &body).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{b}");
    assert!(b.contains("guardrail"), "{b}");
    assert_eq!(sent(&h).await, 0);
}

// ------------------------------------------------------------------ C2

fn server_tool_stream() -> String {
    concat!(
        "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"m\",\"content\":[],\"usage\":{\"input_tokens\":7,\"output_tokens\":1,\"cache_read_input_tokens\":100}}}\n\n",
        "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"server_tool_use\",\"id\":\"s1\",\"name\":\"web_search\",\"input\":{}}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"query\\\":\\\"x\\\"}\"}}\n\n",
        "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"web_search_tool_result\",\"tool_use_id\":\"s1\",\"content\":[]}}\n\n",
        "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
        "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":2,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"text_delta\",\"text\":\"done\"}}\n\n",
        "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":2}\n\n",
        "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":5}}\n\n",
        "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
    )
    .to_string()
}

#[tokio::test]
async fn c2_a_server_tool_stream_is_relayed_whole_and_recorded_ok_with_its_usage() {
    let h = harness("anthropic").await;
    let stream = server_tool_stream();
    mock_ok(&h, "/v1/messages", &stream, "text/event-stream").await;
    let body = json!({"model":"p/m","max_tokens":5,"stream":true,"messages":[user("search")]});
    let (s, b) = post(&h, "/v1/messages", &body).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(b, stream, "no event may follow the provider's own last one");
    let records = h.sink.wait_for(1).await;
    let r = &records[0];
    assert_eq!(r.attempts.last().unwrap().outcome, AttemptOutcome::Ok);
    assert!(!r.estimated);
    let u = r.usage.unwrap();
    assert_eq!(
        (u.input_tokens, u.output_tokens, u.cache_read_input_tokens),
        (7, 5, 100)
    );
}

#[tokio::test]
async fn c2_the_same_stream_under_a_guardrail_scanner_is_scanned_and_clean() {
    let h = harness("anthropic").await;
    let stream = server_tool_stream();
    mock_ok(&h, "/v1/messages", &stream, "text/event-stream").await;
    redact_mail_out(&h).await;
    let body = json!({"model":"p/m","max_tokens":5,"stream":true,"messages":[user("search")]});
    let (_, b) = post(&h, "/v1/messages", &body).await;
    assert!(!b.contains("event: error"), "{b}");
    assert!(b.ends_with("event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"));
    let records = h.sink.wait_for(1).await;
    assert_eq!(
        records[0].attempts.last().unwrap().outcome,
        AttemptOutcome::Ok
    );
    assert_eq!(records[0].usage.unwrap().cache_read_input_tokens, 100);
}

// ------------------------------------------------------------------ I1

fn mail_responses_stream() -> String {
    concat!(
        "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"sequence_number\":1,\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"delta\":\"mail ada@example.com now\"}\n\n",
        "event: response.output_text.done\ndata: {\"type\":\"response.output_text.done\",\"sequence_number\":2,\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"text\":\"mail ada@example.com now\"}\n\n",
        "event: response.content_part.done\ndata: {\"type\":\"response.content_part.done\",\"sequence_number\":3,\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"part\":{\"type\":\"output_text\",\"text\":\"mail ada@example.com now\"}}\n\n",
        "event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"sequence_number\":4,\"output_index\":0,\"item\":{\"type\":\"message\",\"id\":\"msg_1\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"mail ada@example.com now\"}]}}\n\n",
        "event: response.completed\ndata: {\"type\":\"response.completed\",\"sequence_number\":5,\"response\":{\"id\":\"r\",\"status\":\"completed\",\"output\":[{\"type\":\"message\",\"id\":\"msg_1\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"mail ada@example.com now\"}]}],\"usage\":{\"input_tokens\":3,\"output_tokens\":2}}}\n\n",
    )
    .to_string()
}

/// Every `data:` JSON of an SSE body, in order.
fn events(stream: &str) -> Vec<Value> {
    stream
        .split("\n\n")
        .filter_map(|e| e.lines().find_map(|l| l.strip_prefix("data: ")))
        .filter_map(|d| serde_json::from_str(d).ok())
        .collect()
}

#[tokio::test]
async fn i1_a_redacted_responses_stream_holds_the_address_nowhere() {
    let h = harness("openai").await;
    mock_ok(
        &h,
        "/responses",
        &mail_responses_stream(),
        "text/event-stream",
    )
    .await;
    redact_mail_out(&h).await;
    let body = json!({"model":"p/m","stream":true,"store":false,"input":"hi"});
    let (s, b) = post(&h, "/v1/responses", &body).await;
    assert_eq!(s, StatusCode::OK);
    assert!(!b.contains(MAIL), "{b}");
    // The shape of the stream is as sent: the same events, in order (text
    // the scanner held back is let out as a delta before the first .done).
    let got: Vec<String> = events(&b)
        .iter()
        .map(|e| e["type"].as_str().unwrap().to_string())
        .filter(|t| t != "response.output_text.delta")
        .collect();
    assert_eq!(
        got,
        [
            "response.output_text.done",
            "response.content_part.done",
            "response.output_item.done",
            "response.completed"
        ]
    );
    let text: String = events(&b)
        .iter()
        .filter_map(|e| e["delta"].as_str().map(str::to_string))
        .collect();
    assert!(text.contains("[REDACTED"), "{text}");
    assert!(b.contains("\"text\":\"mail [REDACTED"), "{b}");
    let records = h.sink.wait_for(1).await;
    assert_eq!(records[0].usage.unwrap().input_tokens, 3);
}

#[tokio::test]
async fn i1_a_custom_tool_input_delta_is_scanned_and_its_done_event_redacted() {
    let h = harness("openai").await;
    let stream = concat!(
        "event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"sequence_number\":1,\"output_index\":0,\"item\":{\"type\":\"custom_tool_call\",\"id\":\"c1\",\"call_id\":\"k1\",\"name\":\"apply_patch\",\"input\":\"\"}}\n\n",
        "event: response.custom_tool_call_input.delta\ndata: {\"type\":\"response.custom_tool_call_input.delta\",\"sequence_number\":2,\"output_index\":0,\"item_id\":\"c1\",\"delta\":\"+ ada@example.com\"}\n\n",
        "event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"sequence_number\":3,\"output_index\":0,\"item\":{\"type\":\"custom_tool_call\",\"id\":\"c1\",\"call_id\":\"k1\",\"name\":\"apply_patch\",\"input\":\"+ ada@example.com\"}}\n\n",
        "event: response.completed\ndata: {\"type\":\"response.completed\",\"sequence_number\":4,\"response\":{\"id\":\"r\",\"status\":\"completed\",\"output\":[{\"type\":\"custom_tool_call\",\"id\":\"c1\",\"call_id\":\"k1\",\"name\":\"apply_patch\",\"input\":\"+ ada@example.com\"}],\"usage\":{\"input_tokens\":3,\"output_tokens\":2}}}\n\n",
    );
    mock_ok(&h, "/responses", stream, "text/event-stream").await;
    redact_mail_out(&h).await;
    let body = json!({"model":"p/m","stream":true,"store":false,"input":"hi"});
    let (_, b) = post(&h, "/v1/responses", &body).await;
    assert!(!b.contains(MAIL), "{b}");
    assert!(b.contains("apply_patch"), "{b}");
}

#[tokio::test]
async fn i1_a_blocked_responses_stream_is_cut_before_its_done_events() {
    let h = harness("openai").await;
    mock_ok(
        &h,
        "/responses",
        &mail_responses_stream(),
        "text/event-stream",
    )
    .await;
    guardrail(
        &h,
        json!([{ "id": "email", "matcher": { "pii": ["EMAIL"] },
                 "action": "block", "directions": "output" }]),
    )
    .await;
    let body = json!({"model":"p/m","stream":true,"store":false,"input":"hi"});
    let (_, b) = post(&h, "/v1/responses", &body).await;
    assert!(!b.contains(MAIL), "{b}");
    assert!(b.contains("event: error\n"), "{b}");
    assert!(!b.contains("response.completed"), "{b}");
}

#[tokio::test]
async fn i1_a_whole_answer_has_its_tool_inputs_redacted() {
    // Anthropic: the input of a tool_use block.
    let h = harness("anthropic").await;
    let answer = json!({"id":"m","type":"message","role":"assistant","model":"m",
        "content":[{"type":"tool_use","id":"t1","name":"Write",
                    "input":{"path":"a.txt","text":format!("mail {MAIL}")}}],
        "stop_reason":"tool_use","usage":{"input_tokens":1,"output_tokens":1}})
    .to_string();
    mock_ok(&h, "/v1/messages", &answer, "application/json").await;
    redact_mail_out(&h).await;
    let (s, b) = post(
        &h,
        "/v1/messages",
        &json!({"model":"p/m","max_tokens":5,"messages":[user("go")]}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert!(!b.contains(MAIL), "{b}");
    let v: Value = serde_json::from_str(&b).unwrap();
    assert_eq!(v["content"][0]["input"]["path"], "a.txt");

    // Responses: the input of a custom tool call and a function call.
    let h = harness("openai").await;
    let answer = json!({"id":"r","object":"response","status":"completed","model":"m",
        "output":[
            {"type":"custom_tool_call","id":"c","call_id":"k","name":"apply_patch",
             "input":format!("+ {MAIL}")},
            {"type":"function_call","id":"f","call_id":"k2","name":"sh",
             "arguments":format!("{{\"cmd\":\"echo {MAIL}\"}}")}],
        "usage":{"input_tokens":1,"output_tokens":1}})
    .to_string();
    mock_ok(&h, "/responses", &answer, "application/json").await;
    redact_mail_out(&h).await;
    let (s, b) = post(
        &h,
        "/v1/responses",
        &json!({"model":"p/m","store":false,"input":"go"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert!(!b.contains(MAIL), "{b}");
}

#[tokio::test]
async fn i1_a_whole_answer_tool_input_can_be_blocked() {
    let h = harness("anthropic").await;
    let answer = json!({"id":"m","type":"message","role":"assistant","model":"m",
        "content":[{"type":"tool_use","id":"t1","name":"Write","input":{"text":MAIL}}],
        "stop_reason":"tool_use","usage":{"input_tokens":1,"output_tokens":1}})
    .to_string();
    mock_ok(&h, "/v1/messages", &answer, "application/json").await;
    guardrail(
        &h,
        json!([{ "id": "email", "matcher": { "pii": ["EMAIL"] },
                 "action": "block", "directions": "output" }]),
    )
    .await;
    let (s, b) = post(
        &h,
        "/v1/messages",
        &json!({"model":"p/m","max_tokens":5,"messages":[user("go")]}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{b}");
    assert!(!b.contains(MAIL), "{b}");
}

// ------------------------------------------------------------------ I2

async fn assert_blocked(h: &Harness, uri: &str, body: Value, what: &str) {
    let (s, b) = post(h, uri, &body).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{what}: {b}");
    assert!(b.contains("guardrail"), "{what}: {b}");
    assert_eq!(sent(h).await, 0, "{what}: reached the provider");
}

#[tokio::test]
async fn i2_every_place_3_0_scanned_is_still_scanned_on_messages() {
    let m = format!("mail {MAIL}");
    let cases = [
        (
            "an assistant turn",
            json!([user("hi"),
                {"role":"assistant","content":[{"type":"text","text":m}]},
                user("go on")]),
        ),
        (
            "an assistant string turn",
            json!([user("hi"), {"role":"assistant","content":m}, user("go on")]),
        ),
        (
            "a tool_use input",
            json!([user("hi"),
                {"role":"assistant","content":[{"type":"tool_use","id":"t","name":"W",
                    "input":{"a":{"b":[m]}}}]},
                user("go on")]),
        ),
        (
            "a tool_result string",
            json!([user("hi"),
                {"role":"user","content":[{"type":"tool_result","tool_use_id":"t","content":m}]}]),
        ),
        (
            "a system-role message",
            json!([user("hi"), {"role":"system","content":[{"type":"text","text":m}]}]),
        ),
        (
            "a document with a text source",
            json!([{"role":"user","content":[{"type":"document",
                "source":{"type":"text","media_type":"text/plain","data":m}}]}]),
        ),
    ];
    for (what, messages) in cases {
        for kind in ["anthropic", "openai"] {
            let h = harness(kind).await;
            mock_ok(&h, "/v1/messages", ANSWER, "application/json").await;
            mock_ok(
                &h,
                "/chat/completions",
                r#"{"id":"c","model":"m","choices":[{"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1}}"#,
                "application/json",
            )
            .await;
            block_mail_in(&h).await;
            let body = json!({"model":"p/m","max_tokens":5,"messages":messages});
            assert_blocked(&h, "/v1/messages", body, &format!("{what} ({kind})")).await;
        }
    }
}

#[tokio::test]
async fn i2_every_place_3_0_scanned_is_still_scanned_on_responses() {
    let m = format!("mail {MAIL}");
    let cases = [
        (
            "an assistant output_text",
            json!([{"type":"message","role":"assistant","content":[{"type":"output_text","text":m}]},
                {"role":"user","content":"go on"}]),
        ),
        (
            "a function_call's arguments",
            json!([{"type":"function_call","call_id":"c","name":"sh","arguments":format!("{{\"a\":\"{MAIL}\"}}")},
                {"type":"function_call_output","call_id":"c","output":"ok"}]),
        ),
        (
            "a custom tool call's input",
            json!([{"type":"custom_tool_call","call_id":"c","name":"apply_patch","input":m},
                {"type":"custom_tool_call_output","call_id":"c","output":"ok"}]),
        ),
        (
            "a function_call_output in parts",
            json!([{"type":"function_call","call_id":"c","name":"sh","arguments":"{}"},
                {"type":"function_call_output","call_id":"c","output":[{"type":"input_text","text":m}]}]),
        ),
        (
            "a system message",
            json!([{"type":"message","role":"system","content":[{"type":"input_text","text":m}]}]),
        ),
    ];
    for (what, input) in cases {
        for kind in ["openai", "anthropic"] {
            let h = harness(kind).await;
            mock_ok(&h, "/responses", RESPONSES_ANSWER, "application/json").await;
            mock_ok(&h, "/v1/messages", ANSWER, "application/json").await;
            block_mail_in(&h).await;
            let body = json!({"model":"p/m","store":false,"input":input});
            assert_blocked(&h, "/v1/responses", body, &format!("{what} ({kind})")).await;
        }
    }
}

#[tokio::test]
async fn i2_redaction_reaches_assistant_text_and_leaves_signed_thinking_alone() {
    let h = harness("anthropic").await;
    mock_ok(&h, "/v1/messages", ANSWER, "application/json").await;
    redact_mail_in(&h).await;
    let body = json!({"model":"p/m","max_tokens":5,"messages":[
        user("hi"),
        {"role":"assistant","content":[
            {"type":"thinking","thinking":format!("think {MAIL}"),"signature":"sig"},
            {"type":"text","text":format!("mail {MAIL}")}]},
        user("go on")]});
    let (s, b) = post(&h, "/v1/messages", &body).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let got: Value =
        serde_json::from_slice(&h.upstream.received_requests().await.unwrap()[0].body).unwrap();
    assert!(!got["messages"][1]["content"][1]["text"]
        .as_str()
        .unwrap()
        .contains(MAIL));
    // The signed block is not ours to edit.
    assert_eq!(
        got["messages"][1]["content"][0]["thinking"],
        format!("think {MAIL}")
    );
}

// ------------------------------------------------------------------ I3

struct Refuses;

impl Limiter for Refuses {
    fn acquire(&self, _: &Subjects, _: u64, _: Instant) -> Result<Permit, Refusal> {
        Err(Refusal {
            limit_name: "requests per minute",
            scope_label: "key 'ci'".into(),
            retry_after: Duration::from_secs(1),
        })
    }
}

#[tokio::test]
async fn i3_count_tokens_takes_a_request_permit() {
    let h = harness_with_rate("anthropic", Arc::new(Refuses)).await;
    mock_ok(
        &h,
        "/v1/messages/count_tokens",
        r#"{"input_tokens":3}"#,
        "application/json",
    )
    .await;
    let (s, b) = post(
        &h,
        "/v1/messages/count_tokens",
        &json!({"model":"p/m","messages":[user("hi")]}),
    )
    .await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS, "{b}");
    let v: Value = serde_json::from_str(&b).unwrap();
    assert_eq!(v["type"], "error");
    assert_eq!(v["error"]["type"], "rate_limit_error");
    assert_eq!(sent(&h).await, 0);
    let text = h.state.metrics.render(&h.state.health.view());
    assert!(text.contains("uf_rate_limited_total"), "{text}");
}

#[tokio::test]
async fn i3_count_tokens_runs_the_input_guardrails() {
    let h = harness("anthropic").await;
    mock_ok(
        &h,
        "/v1/messages/count_tokens",
        r#"{"input_tokens":3}"#,
        "application/json",
    )
    .await;
    block_mail_in(&h).await;
    let (s, b) = post(
        &h,
        "/v1/messages/count_tokens",
        &json!({"model":"p/m","messages":[user(&format!("mail {MAIL}"))]}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{b}");
    let v: Value = serde_json::from_str(&b).unwrap();
    assert_eq!(v["type"], "error");
    assert_eq!(v["error"]["type"], "invalid_request_error");
    assert!(b.contains("guardrail"), "{b}");
    assert_eq!(sent(&h).await, 0, "count_tokens forwarded guarded text");
}

#[tokio::test]
async fn i3_count_tokens_forwards_the_redacted_body() {
    let h = harness("anthropic").await;
    mock_ok(
        &h,
        "/v1/messages/count_tokens",
        r#"{"input_tokens":3}"#,
        "application/json",
    )
    .await;
    redact_mail_in(&h).await;
    let (s, b) = post(
        &h,
        "/v1/messages/count_tokens",
        &json!({"model":"p/m","messages":[user(&format!("mail {MAIL}"))]}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let got = String::from_utf8(
        h.upstream.received_requests().await.unwrap()[0]
            .body
            .clone(),
    )
    .unwrap();
    assert!(!got.contains(MAIL), "{got}");
}

// ------------------------------------------------------------------ I4

async fn refused(h: &Harness, uri: &str, body: Value, field: &str) {
    let (s, b) = post(h, uri, &body).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{field}: {b}");
    let v: Value = serde_json::from_str(&b).unwrap();
    let message = v["error"]["message"].as_str().unwrap();
    assert!(message.contains(field), "{field}: {b}");
    if uri == "/v1/messages" {
        assert_eq!(v["type"], "error", "{b}");
    }
    assert_eq!(v["error"]["type"], "invalid_request_error", "{b}");
    assert_eq!(sent(h).await, 0, "{field}: reached the provider");
}

#[tokio::test]
async fn i4_responses_state_and_hosted_tools_are_refused() {
    let cases: Vec<(&str, Value)> = vec![
        ("store", json!({"store":true})),
        (
            "previous_response_id",
            json!({"previous_response_id":"resp_1"}),
        ),
        ("conversation", json!({"conversation":"conv_1"})),
        ("background", json!({"background":true})),
        (
            "file_id",
            json!({"input":[{"role":"user","content":[
                {"type":"input_file","file_id":"file_1"}]}]}),
        ),
        (
            "file_id",
            json!({"input":[{"role":"user","content":[
                {"type":"input_image","file_id":"file_1"}]}]}),
        ),
        (
            "vector_store_ids",
            json!({"tools":[{"type":"file_search","vector_store_ids":["vs_1"]}]}),
        ),
        (
            "code_interpreter",
            json!({"tools":[{"type":"code_interpreter"}]}),
        ),
        (
            "mcp",
            json!({"tools":[{"type":"mcp","server_label":"x","server_url":"https://x.example"}]}),
        ),
        (
            "computer",
            json!({"tools":[{"type":"computer_use_preview"}]}),
        ),
        (
            "image_generation",
            json!({"tools":[{"type":"image_generation"}]}),
        ),
    ];
    for kind in ["openai", "azure"] {
        for (field, extra) in &cases {
            let h = harness(kind).await;
            mock_ok(&h, "/responses", RESPONSES_ANSWER, "application/json").await;
            let mut body = json!({"model":"p/m","input":"x"});
            for (k, v) in extra.as_object().unwrap() {
                body[k] = v.clone();
            }
            refused(&h, "/v1/responses", body, field).await;
        }
    }
}

#[tokio::test]
async fn i4_anthropic_state_is_refused() {
    let cases: Vec<(&str, Value)> = vec![
        (
            "file_id",
            json!({"messages":[{"role":"user","content":[{"type":"document",
                "source":{"type":"file","file_id":"file_1"}}]}]}),
        ),
        (
            "file_id",
            json!({"messages":[{"role":"user","content":[{"type":"image",
                "source":{"type":"file","file_id":"file_1"}}]}]}),
        ),
        ("container", json!({"container":"cont_1"})),
        (
            "mcp_servers",
            json!({"mcp_servers":[{"type":"url","url":"https://x.example","name":"x"}]}),
        ),
    ];
    for (field, extra) in cases {
        let h = harness("anthropic").await;
        mock_ok(&h, "/v1/messages", ANSWER, "application/json").await;
        let mut body = json!({"model":"p/m","max_tokens":5,"messages":[user("x")]});
        for (k, v) in extra.as_object().unwrap() {
            body[k] = v.clone();
        }
        refused(&h, "/v1/messages", body, field).await;
    }
}

#[tokio::test]
async fn i4_count_tokens_refuses_a_file_reference_too() {
    let h = harness("anthropic").await;
    mock_ok(
        &h,
        "/v1/messages/count_tokens",
        r#"{"input_tokens":3}"#,
        "application/json",
    )
    .await;
    let body = json!({"model":"p/m","messages":[{"role":"user","content":[{"type":"document",
        "source":{"type":"file","file_id":"file_1"}}]}]});
    refused(&h, "/v1/messages/count_tokens", body, "file_id").await;
}

#[tokio::test]
async fn i4_what_the_agents_send_still_goes_through() {
    // Codex: store false, the encrypted reasoning, the hosted web search,
    // tool_search, a custom tool and functions.
    let h = harness("openai").await;
    mock_ok(&h, "/responses", RESPONSES_ANSWER, "application/json").await;
    let body = json!({"model":"p/m","store":false,"include":["reasoning.encrypted_content"],
        "background":false,"previous_response_id":null,"conversation":null,
        "tools":[{"type":"function","name":"f","parameters":{"type":"object",
                    "properties":{"file_id":{"type":"string"}}}},
                 {"type":"custom","name":"apply_patch","format":{"type":"grammar"}},
                 {"type":"tool_search"},
                 {"type":"web_search","external_web_access":true}],
        "input":"hi"});
    let (s, b) = post(&h, "/v1/responses", &body).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(sent(&h).await, 1);

    // Claude Code: the web search and web fetch server tools.
    let h = harness("anthropic").await;
    mock_ok(&h, "/v1/messages", ANSWER, "application/json").await;
    let body = json!({"model":"p/m","max_tokens":5,
        "tools":[{"type":"web_search_20250305","name":"web_search","max_uses":5},
                 {"type":"web_fetch_20250910","name":"web_fetch"},
                 {"name":"Read","input_schema":{"type":"object",
                    "properties":{"file_id":{"type":"string"}}}}],
        "messages":[user("hi")]});
    let (s, b) = post(&h, "/v1/messages", &body).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(sent(&h).await, 1);
}

// ------------------------------------------------------------------ M1

#[tokio::test]
async fn m1_a_provider_5xx_and_429_come_back_as_the_providers_own() {
    for (status, body) in [
        (
            503u16,
            r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
        ),
        (
            429,
            r#"{"type":"error","error":{"type":"rate_limit_error","message":"slow down"}}"#,
        ),
        (
            500,
            r#"{"type":"error","error":{"type":"api_error","message":"boom"}}"#,
        ),
    ] {
        let h = harness("anthropic").await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(
                ResponseTemplate::new(status)
                    .insert_header("retry-after", "7")
                    .insert_header("x-should-retry", "true")
                    .set_body_raw(body.to_string(), "application/json"),
            )
            .mount(&h.upstream)
            .await;
        let bearer = format!("Bearer {}", h.key);
        let (s, headers, b) = post_to(
            &h.app,
            "/v1/messages",
            &[
                ("authorization", &bearer),
                ("anthropic-version", "2023-06-01"),
            ],
            &json!({"model":"p/m","max_tokens":5,"messages":[user("x")]}).to_string(),
        )
        .await;
        assert_eq!(s.as_u16(), status);
        assert_eq!(b, body);
        assert_eq!(headers.get("x-should-retry").unwrap(), "true");
        assert_eq!(headers.get("retry-after").unwrap(), "7");
    }
}

#[tokio::test]
async fn m1_a_responses_provider_503_is_relayed_too() {
    let h = harness("openai").await;
    let body = r#"{"error":{"message":"The server had an error","type":"server_error"}}"#;
    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(ResponseTemplate::new(503).set_body_raw(body.to_string(), "application/json"))
        .mount(&h.upstream)
        .await;
    let (s, b) = post(
        &h,
        "/v1/responses",
        &json!({"model":"p/m","store":false,"input":"x"}),
    )
    .await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(b, body);
}

// ------------------------------------------------------------------ M5

#[tokio::test]
async fn m5_an_event_that_never_ends_is_not_held_without_limit() {
    let h = harness("anthropic").await;
    let mut stream = String::from(
        "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"m\",\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}}\n\n",
    );
    // One event of 3 MiB with no delimiter.
    stream.push_str("event: content_block_delta\ndata: {\"x\":\"");
    stream.push_str(&"a".repeat(3 * 1024 * 1024));
    mock_ok(&h, "/v1/messages", &stream, "text/event-stream").await;
    redact_mail_out(&h).await;
    let (s, b) = post(
        &h,
        "/v1/messages",
        &json!({"model":"p/m","max_tokens":5,"stream":true,"messages":[user("x")]}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    // Nothing unscanned was let out; the caller is told.
    assert!(b.len() < 2 * 1024 * 1024, "{} bytes", b.len());
    assert!(b.contains("event: error\n"), "{}", &b[..b.len().min(300)]);
}

// ------------------------------------------------------------------ M6

#[tokio::test]
async fn m6_flushed_held_text_never_repeats_a_sequence_number() {
    let h = harness("openai").await;
    // The address ends the text, so the scanner holds it until the end.
    let stream = concat!(
        "event: response.created\ndata: {\"type\":\"response.created\",\"sequence_number\":0,\"response\":{\"id\":\"r\",\"status\":\"in_progress\",\"output\":[]}}\n\n",
        "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"sequence_number\":1,\"item_id\":\"m\",\"output_index\":0,\"content_index\":0,\"delta\":\"mail ada@exam\"}\n\n",
        "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"sequence_number\":2,\"item_id\":\"m\",\"output_index\":0,\"content_index\":0,\"delta\":\"ple.com\"}\n\n",
        "event: response.output_text.done\ndata: {\"type\":\"response.output_text.done\",\"sequence_number\":3,\"item_id\":\"m\",\"output_index\":0,\"content_index\":0,\"text\":\"mail ada@example.com\"}\n\n",
        "event: response.output_item.done\ndata: {\"type\":\"response.output_item.done\",\"sequence_number\":4,\"output_index\":0,\"item\":{\"type\":\"message\",\"id\":\"m\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"mail ada@example.com\"}]}}\n\n",
        "event: response.completed\ndata: {\"type\":\"response.completed\",\"sequence_number\":5,\"response\":{\"id\":\"r\",\"status\":\"completed\",\"output\":[],\"usage\":{\"input_tokens\":3,\"output_tokens\":2}}}\n\n",
    );
    mock_ok(&h, "/responses", stream, "text/event-stream").await;
    redact_mail_out(&h).await;
    let (_, b) = post(
        &h,
        "/v1/responses",
        &json!({"model":"p/m","stream":true,"store":false,"input":"hi"}),
    )
    .await;
    assert!(!b.contains(MAIL), "{b}");
    let seqs: Vec<u64> = events(&b)
        .iter()
        .map(|e| e["sequence_number"].as_u64().unwrap())
        .collect();
    let mut sorted = seqs.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        sorted, seqs,
        "sequence numbers must rise without repeats: {b}"
    );
}

// ------------------------------------------------------- M2, blocked streams

fn words_block_out() -> Value {
    json!([{ "id": "w", "matcher": { "keywords": { "words": ["swordfish"] } },
             "action": "block", "directions": "output" }])
}

fn anthropic_stream_saying(text: &str) -> String {
    format!(
        concat!(
            "event: message_start\ndata: {{\"type\":\"message_start\",\"message\":{{\"id\":\"m\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"m\",\"content\":[],\"usage\":{{\"input_tokens\":7,\"output_tokens\":1,\"cache_read_input_tokens\":100}}}}}}\n\n",
            "event: content_block_start\ndata: {{\"type\":\"content_block_start\",\"index\":0,\"content_block\":{{\"type\":\"text\",\"text\":\"\"}}}}\n\n",
            "event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":{}}}}}\n\n",
            "event: content_block_stop\ndata: {{\"type\":\"content_block_stop\",\"index\":0}}\n\n",
            "event: message_delta\ndata: {{\"type\":\"message_delta\",\"delta\":{{\"stop_reason\":\"end_turn\"}},\"usage\":{{\"output_tokens\":5}}}}\n\n",
            "event: message_stop\ndata: {{\"type\":\"message_stop\"}}\n\n",
        ),
        json!(text)
    )
}

async fn blocked_stream_usage(h: &Harness) {
    mock_ok(
        h,
        "/v1/messages",
        &anthropic_stream_saying("the swordfish"),
        "text/event-stream",
    )
    .await;
    guardrail(h, words_block_out()).await;
    let (_, b) = post(
        h,
        "/v1/messages",
        &json!({"model":"p/m","max_tokens":5,"stream":true,"messages":[user("x")]}),
    )
    .await;
    assert!(!b.contains("swordfish"), "{b}");
    let records = h.sink.wait_for(1).await;
    let r = &records[0];
    assert!(r.guardrails.is_some());
    assert!(!r.estimated, "the usage read is not an estimate");
    let u = r.usage.expect("the usage read so far");
    assert_eq!((u.input_tokens, u.cache_read_input_tokens), (7, 100));
}

#[tokio::test]
async fn m2_a_blocked_pass_through_stream_records_the_usage_it_read() {
    blocked_stream_usage(&harness("anthropic").await).await;
}

#[tokio::test]
async fn m2_a_blocked_translated_stream_records_the_usage_it_read() {
    blocked_stream_usage(&common::harness_translating("anthropic").await).await;
}

// ------------------------------------------------------------------ M4

struct Counts(std::sync::Mutex<Vec<u64>>);

impl Limiter for Counts {
    fn acquire(&self, _: &Subjects, estimate: u64, _: Instant) -> Result<Permit, Refusal> {
        self.0.lock().unwrap().push(estimate);
        Ok(Permit::none())
    }
}

#[tokio::test]
async fn m4_the_estimate_counts_what_translation_drops() {
    let counts = Arc::new(Counts(Default::default()));
    let h = harness_with_rate("anthropic", counts.clone()).await;
    mock_ok(&h, "/v1/messages", ANSWER, "application/json").await;
    // 8 000 characters in a document, which the common request drops.
    let body = json!({"model":"p/m","max_tokens":10,"messages":[{"role":"user","content":[
        {"type":"document","source":{"type":"text","media_type":"text/plain","data":"x".repeat(8000)}}]}]});
    let (s, b) = post(&h, "/v1/messages", &body).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let estimate = counts.0.lock().unwrap()[0];
    assert!(estimate >= 2000 + 10, "{estimate}");
}

// ------------------------------------------------------------------ M9

#[tokio::test]
async fn m9_hosted_searches_are_read_from_the_providers_usage() {
    let h = harness("anthropic").await;
    let answer = r#"{"id":"m","type":"message","role":"assistant","model":"m","content":[{"type":"text","text":"ok"}],"stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":1,"server_tool_use":{"web_search_requests":2}}}"#;
    mock_ok(&h, "/v1/messages", answer, "application/json").await;
    let (s, _) = post(
        &h,
        "/v1/messages",
        &json!({"model":"p/m","max_tokens":5,"messages":[user("x")]}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(h.sink.wait_for(1).await[0].hosted_calls, 2);

    let h = harness("anthropic").await;
    let stream = server_tool_stream().replace(
        "\"usage\":{\"output_tokens\":5}",
        "\"usage\":{\"output_tokens\":5,\"server_tool_use\":{\"web_search_requests\":3}}",
    );
    mock_ok(&h, "/v1/messages", &stream, "text/event-stream").await;
    post(
        &h,
        "/v1/messages",
        &json!({"model":"p/m","max_tokens":5,"stream":true,"messages":[user("x")]}),
    )
    .await;
    assert_eq!(h.sink.wait_for(1).await[0].hosted_calls, 3);

    let h = harness("openai").await;
    let answer = r#"{"id":"r","object":"response","status":"completed","model":"m","output":[{"type":"web_search_call","id":"w1","status":"completed"},{"type":"web_search_call","id":"w2","status":"completed"}],"usage":{"input_tokens":1,"output_tokens":1}}"#;
    mock_ok(&h, "/responses", answer, "application/json").await;
    post(
        &h,
        "/v1/responses",
        &json!({"model":"p/m","store":false,"input":"x"}),
    )
    .await;
    assert_eq!(h.sink.wait_for(1).await[0].hosted_calls, 2);
}
