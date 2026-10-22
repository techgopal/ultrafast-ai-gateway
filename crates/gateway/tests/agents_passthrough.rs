//! Claude Code and Codex through the gateway: a request whose API is the
//! provider's own is forwarded as it came; anything else is translated.

mod common;

use axum::http::StatusCode;
use common::{allow_model, harness, org_with_sink, post_to, Harness, MemorySink};
use serde_json::{json, Value};
use std::sync::Arc;
use ufaigw_gateway::cache::{CacheScope, RouteCache};
use ufaigw_gateway::secrets::generate_key;
use ufaigw_gateway::store::{NewGuardrail, RouteSettings, TargetsInput};
use ufaigw_gateway::telemetry::AttemptOutcome;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const CLAUDE_CODE: &str = include_str!("fixtures/agents/claude-code-2.1.305.json");
const CODEX: &str = include_str!("fixtures/agents/codex-0.160.1.json");

const NO_RETRY: RouteSettings = RouteSettings {
    retries: 0,
    first_token_timeout_ms: 30_000,
    total_timeout_ms: 300_000,
    breaker_failures: 5,
    breaker_window_s: 60,
    breaker_open_s: 30,
};

/// The recorded request of an agent, naming `model`, as a whole answer.
fn recorded(fixture: &str, model: &str) -> Value {
    let mut v: Value = serde_json::from_str(fixture).unwrap();
    v["model"] = json!(model);
    v["stream"] = json!(false);
    v
}

fn keys_of(v: &Value) -> Vec<String> {
    v.as_object().unwrap().keys().cloned().collect()
}

/// An answer written with spaces the gateway's own serialiser would not.
const ANTHROPIC_ANSWER: &str = r#"{"id": "msg_1",  "type": "message", "role": "assistant", "model": "m", "content": [{"type": "text", "text": "hi"}], "stop_reason": "end_turn", "usage": {"input_tokens": 7, "output_tokens": 3, "cache_read_input_tokens": 100, "cache_creation_input_tokens": 20}}"#;

const RESPONSES_ANSWER: &str = r#"{"id": "resp_1",  "object": "response", "status": "completed", "model": "m", "output": [{"type": "message", "id": "msg_1", "role": "assistant", "status": "completed", "content": [{"type": "output_text", "text": "hi", "annotations": []}]}], "usage": {"input_tokens": 10, "output_tokens": 2, "total_tokens": 12, "input_tokens_details": {"cached_tokens": 4}, "output_tokens_details": {"reasoning_tokens": 0}}}"#;

async fn mock_anthropic(h: &Harness, answer: ResponseTemplate) {
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(answer)
        .mount(&h.upstream)
        .await;
}

fn json_answer(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(body.to_string(), "application/json")
}

async fn claude_code(
    h: &Harness,
    uri: &str,
    extra: &[(&str, &str)],
    body: &Value,
) -> (StatusCode, axum::http::HeaderMap, String) {
    let bearer = format!("Bearer {}", h.key);
    let mut headers = vec![
        ("authorization", bearer.as_str()),
        ("anthropic-version", "2023-06-01"),
        ("x-claude-code-session-id", "s-1"),
        ("x-stainless-lang", "js"),
    ];
    headers.extend_from_slice(extra);
    post_to(&h.app, uri, &headers, &body.to_string()).await
}

async fn codex(
    h: &Harness,
    extra: &[(&str, &str)],
    body: &Value,
) -> (StatusCode, axum::http::HeaderMap, String) {
    let bearer = format!("Bearer {}", h.key);
    let mut headers = vec![
        ("authorization", bearer.as_str()),
        ("session-id", "c-1"),
        ("thread-id", "t-1"),
        ("originator", "codex_exec"),
        ("x-codex-turn-state", "x"),
    ];
    headers.extend_from_slice(extra);
    post_to(&h.app, "/v1/responses", &headers, &body.to_string()).await
}

async fn received(server: &MockServer) -> Vec<wiremock::Request> {
    server.received_requests().await.unwrap()
}

#[tokio::test]
async fn claude_code_request_reaches_anthropic_unchanged() {
    let h = harness("anthropic").await;
    mock_anthropic(&h, json_answer(ANTHROPIC_ANSWER)).await;
    let sent = recorded(CLAUDE_CODE, "p/m");
    let (status, _, body) = claude_code(
        &h,
        "/v1/messages?beta=true",
        &[("anthropic-beta", "a,b")],
        &sent,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // The caller gets the provider's bytes.
    assert_eq!(body, ANTHROPIC_ANSWER);
    let seen = received(&h.upstream).await;
    assert_eq!(seen.len(), 1);
    let req = &seen[0];
    assert_eq!(req.url.path(), "/v1/messages");
    assert_eq!(req.url.query(), Some("beta=true"));
    let got: Value = serde_json::from_slice(&req.body).unwrap();
    assert_eq!(got["model"], "m");
    let mut want = sent.clone();
    want["model"] = json!("m");
    assert_eq!(got, want);
    assert_eq!(keys_of(&got), keys_of(&want), "key order is kept");
    assert_eq!(req.headers.get("anthropic-beta").unwrap(), "a,b");
    assert_eq!(req.headers.get("anthropic-version").unwrap(), "2023-06-01");
    assert_eq!(req.headers.get("x-api-key").unwrap(), "provider-secret");
    for name in [
        "x-claude-code-session-id",
        "x-stainless-lang",
        "authorization",
    ] {
        assert!(
            req.headers.get(name).is_none(),
            "{name} must not be forwarded"
        );
    }
    // The call is recorded with what the provider reported.
    let records = h.sink.wait_for(1).await;
    let usage = records[0].usage.unwrap();
    assert_eq!(
        (
            usage.input_tokens,
            usage.output_tokens,
            usage.cache_read_input_tokens,
            usage.cache_write_input_tokens
        ),
        (7, 3, 100, 20)
    );
    assert_eq!(records[0].agent.session.as_deref(), Some("s-1"));
}

#[tokio::test]
async fn two_anthropic_beta_lines_both_arrive() {
    let h = harness("anthropic").await;
    mock_anthropic(&h, json_answer(ANTHROPIC_ANSWER)).await;
    let (status, _, body) = claude_code(
        &h,
        "/v1/messages",
        &[("anthropic-beta", "one"), ("anthropic-beta", "two,three")],
        &recorded(CLAUDE_CODE, "p/m"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let seen = received(&h.upstream).await;
    let lines: Vec<&str> = seen[0]
        .headers
        .get_all("anthropic-beta")
        .iter()
        .map(|v| v.to_str().unwrap())
        .collect();
    assert_eq!(lines, ["one", "two,three"]);
}

#[tokio::test]
async fn codex_request_reaches_openai_unchanged() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(json_answer(RESPONSES_ANSWER))
        .mount(&h.upstream)
        .await;
    let sent = recorded(CODEX, "p/m");
    let (status, _, body) = codex(
        &h,
        &[
            ("openai-beta", "responses=v1"),
            ("openai-organization", "org-x"),
            ("openai-project", "proj-x"),
        ],
        &sent,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, RESPONSES_ANSWER);
    let seen = received(&h.upstream).await;
    assert_eq!(seen.len(), 1);
    let req = &seen[0];
    assert_eq!(req.url.path(), "/responses");
    let got: Value = serde_json::from_slice(&req.body).unwrap();
    let mut want = sent.clone();
    want["model"] = json!("m");
    assert_eq!(got, want);
    assert_eq!(keys_of(&got), keys_of(&want));
    assert_eq!(req.headers.get("openai-beta").unwrap(), "responses=v1");
    assert_eq!(
        req.headers.get("authorization").unwrap(),
        "Bearer provider-secret"
    );
    // A3: an organization or project of the caller's is not the gateway's.
    for name in [
        "openai-organization",
        "openai-project",
        "session-id",
        "thread-id",
        "originator",
        "x-codex-turn-state",
    ] {
        assert!(
            req.headers.get(name).is_none(),
            "{name} must not be forwarded"
        );
    }
    let records = h.sink.wait_for(1).await;
    let usage = records[0].usage.unwrap();
    assert_eq!(
        (
            usage.input_tokens,
            usage.output_tokens,
            usage.cache_read_input_tokens
        ),
        (6, 2, 4)
    );
    assert_eq!(records[0].agent.client, Some("codex"));
}

#[tokio::test]
async fn provider_errors_pass_through_unchanged() {
    let h = harness("anthropic").await;
    let error = r#"{"type":"error","error":{"type":"invalid_request_error","message":"context_management: Extra inputs are not permitted"}}"#;
    mock_anthropic(
        &h,
        ResponseTemplate::new(400)
            .insert_header("retry-after", "3")
            .insert_header("x-should-retry", "false")
            .set_body_raw(error.to_string(), "application/json"),
    )
    .await;
    let (status, headers, body) = claude_code(
        &h,
        "/v1/messages?beta=true",
        &[],
        &recorded(CLAUDE_CODE, "p/m"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body, error);
    assert_eq!(headers.get("retry-after").unwrap(), "3");
    assert_eq!(headers.get("x-should-retry").unwrap(), "false");
    assert_eq!(received(&h.upstream).await.len(), 1);
}

async fn route(h: &Harness, name: &str, primaries: &[i64], fallbacks: &[i64]) -> i64 {
    let mut tx = h.store.begin().await.unwrap();
    let id = tx.insert_route(name, &NO_RETRY, true).await.unwrap();
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
    id
}

#[tokio::test]
async fn fallback_from_pass_through_to_translation() {
    let h = harness("anthropic").await;
    let other = MockServer::start().await;
    h.store
        .insert_provider("q", "openai", &other.uri(), None)
        .await
        .unwrap();
    let first = allow_model(&h.store, "p", "pm").await;
    let second = allow_model(&h.store, "q", "n").await;
    route(&h, "claude-opus-5-5", &[first], &[second]).await;
    mock_anthropic(
        &h,
        ResponseTemplate::new(529).set_body_raw(
            r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#
                .to_string(),
            "application/json",
        ),
    )
    .await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "c1", "model": "n",
            "choices": [{ "message": { "role": "assistant", "content": "from q" }, "finish_reason": "stop" }],
            "usage": { "prompt_tokens": 4, "completion_tokens": 2 }
        })))
        .mount(&other)
        .await;
    let (status, _, body) = claude_code(
        &h,
        "/v1/messages?beta=true",
        &[],
        &recorded(CLAUDE_CODE, "claude-opus-5-5"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["type"], "message");
    assert_eq!(v["content"][0]["text"], "from q");
    let records = h.sink.wait_for(1).await;
    let attempts: Vec<_> = records[0]
        .attempts
        .iter()
        .map(|a| (a.provider.clone(), a.outcome, a.status))
        .collect();
    assert_eq!(
        attempts,
        [
            ("p".to_string(), AttemptOutcome::Retryable, Some(529)),
            ("q".to_string(), AttemptOutcome::Ok, Some(200)),
        ]
    );
}

#[tokio::test]
async fn the_response_cache_is_not_used_on_pass_through() {
    let h = harness("anthropic").await;
    let model = allow_model(&h.store, "p", "pm").await;
    let id = route(&h, "r", &[model], &[]).await;
    let mut tx = h.store.begin().await.unwrap();
    tx.set_route_cache(
        id,
        &RouteCache {
            enabled: true,
            ttl_s: 300,
            scope: CacheScope::Team,
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    h.state.refresh().await.unwrap();
    mock_anthropic(&h, json_answer(ANTHROPIC_ANSWER)).await;
    let sent = recorded(CLAUDE_CODE, "r");
    for _ in 0..2 {
        let (status, _, body) = claude_code(&h, "/v1/messages", &[], &sent).await;
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    assert_eq!(received(&h.upstream).await.len(), 2);
}

#[tokio::test]
async fn translation_still_serves_cross_family() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "c1", "model": "m",
            "choices": [{ "message": { "role": "assistant", "content": "hello" }, "finish_reason": "stop" }],
            "usage": { "prompt_tokens": 4, "completion_tokens": 2 }
        })))
        .mount(&h.upstream)
        .await;
    let (status, _, body) = claude_code(
        &h,
        "/v1/messages?beta=true",
        &[("anthropic-beta", "a")],
        &recorded(CLAUDE_CODE, "p/m"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["content"][0]["text"], "hello");
    let seen = received(&h.upstream).await;
    assert_eq!(seen[0].url.path(), "/chat/completions");
    assert!(seen[0].headers.get("anthropic-beta").is_none());
}

#[tokio::test]
async fn a_prompt_template_is_refused_on_pass_through() {
    let sink = Arc::new(MemorySink::default());
    let org = org_with_sink(Some(sink.clone())).await;
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(json_answer(RESPONSES_ANSWER))
        .mount(&upstream)
        .await;
    let store = &org.api.store;
    store
        .insert_provider("p", "openai", &upstream.uri(), None)
        .await
        .unwrap();
    allow_model(store, "p", "m").await;
    let key = generate_key();
    store
        .insert_key("test", &key.hash, &key.display, None)
        .await
        .unwrap();
    org.api.state.refresh().await.unwrap();
    let maya = org.sign_in("maya").await;
    let (status, v) = org
        .call(
            Some(&maya),
            "POST",
            "/api/prompts",
            Some(json!({ "name": "greet", "model": "p/m",
                "messages": [{ "role": "user", "content": "Hello {{name}}" }] })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{v}");
    let bearer = format!("Bearer {}", key.full);
    let mut body = recorded(CODEX, "p/m");
    body["prompt"] = json!({ "id": "greet", "variables": { "name": "Ada" } });
    let (status, _, text) = post_to(
        &org.api.app,
        "/v1/responses",
        &[("authorization", &bearer)],
        &body.to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{text}");
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["error"]["type"], "invalid_request_error");
    assert_eq!(
        v["error"]["message"],
        "Prompt templates cannot be used with a pass-through call."
    );
    assert!(received(&upstream).await.is_empty());
}

/// A history with the blocks a translator may not know.
fn exotic_history() -> Value {
    let mut v = recorded(CLAUDE_CODE, "p/m");
    v["messages"] = json!([
        { "role": "user", "content": [{ "type": "text", "text": "look" }] },
        { "role": "assistant", "content": [
            { "type": "redacted_thinking", "data": "abc" },
            { "type": "server_tool_use", "id": "srv_1", "name": "web_search", "input": { "query": "q" } },
            { "type": "web_search_tool_result", "tool_use_id": "srv_1", "content": [
                { "type": "web_search_result", "url": "https://e.example", "title": "t", "encrypted_content": "x" }
            ] },
            { "type": "tool_use", "id": "tu_1", "name": "Read", "input": {} }
        ] },
        { "role": "user", "content": [
            { "type": "tool_result", "tool_use_id": "tu_1", "content": [
                { "type": "text", "text": "seen" },
                { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "aGk=" } }
            ] },
            { "type": "document", "source": { "type": "text", "media_type": "text/plain", "data": "doc" } }
        ] }
    ]);
    v
}

#[tokio::test]
async fn blocks_a_translator_does_not_know_reach_anthropic_unchanged() {
    let h = harness("anthropic").await;
    mock_anthropic(&h, json_answer(ANTHROPIC_ANSWER)).await;
    let sent = exotic_history();
    let (status, _, body) = claude_code(&h, "/v1/messages?beta=true", &[], &sent).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let got: Value = serde_json::from_slice(&received(&h.upstream).await[0].body).unwrap();
    assert_eq!(got["messages"], sent["messages"]);
}

#[tokio::test]
async fn the_same_blocks_are_translated_and_named_for_openai() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "c1", "model": "m",
            "choices": [{ "message": { "role": "assistant", "content": "ok" }, "finish_reason": "stop" }],
            "usage": { "prompt_tokens": 4, "completion_tokens": 2 }
        })))
        .mount(&h.upstream)
        .await;
    let (status, _, body) = claude_code(&h, "/v1/messages?beta=true", &[], &exotic_history()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let records = h.sink.wait_for(1).await;
    let dropped = records[0].dropped.join(",");
    for name in ["redacted_thinking", "server_tool_use", "document"] {
        assert!(dropped.contains(name), "{name} not named in {dropped}");
    }
}

async fn guardrail(h: &Harness, name: &str, rules: Value) {
    let rules = rules.to_string();
    let mut tx = h.store.begin().await.unwrap();
    tx.insert_guardrail(NewGuardrail {
        name,
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

#[tokio::test]
async fn output_redaction_rewrites_the_text_of_a_native_answer() {
    let h = harness("anthropic").await;
    let answer =
        ANTHROPIC_ANSWER.replace("\"text\": \"hi\"", "\"text\": \"mail ada@example.com now\"");
    mock_anthropic(&h, json_answer(&answer)).await;
    guardrail(
        &h,
        "pii",
        json!([{ "id": "email", "matcher": { "pii": ["EMAIL"] },
                 "action": "redact", "directions": "output" }]),
    )
    .await;
    let (status, _, body) =
        claude_code(&h, "/v1/messages", &[], &recorded(CLAUDE_CODE, "p/m")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!body.contains("ada@example.com"), "{body}");
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["usage"]["cache_read_input_tokens"], 100);
    assert_eq!(v["id"], "msg_1");
}

#[tokio::test]
async fn an_output_block_refuses_a_native_answer() {
    let h = harness("anthropic").await;
    let answer = ANTHROPIC_ANSWER.replace("\"text\": \"hi\"", "\"text\": \"the swordfish\"");
    mock_anthropic(&h, json_answer(&answer)).await;
    guardrail(
        &h,
        "words",
        json!([{ "id": "w", "matcher": { "keywords": { "words": ["swordfish"] } },
                 "action": "block", "directions": "output" }]),
    )
    .await;
    let (status, _, body) =
        claude_code(&h, "/v1/messages", &[], &recorded(CLAUDE_CODE, "p/m")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(!body.contains("swordfish"), "{body}");
}

// ------------------------------------------------------------------ streams

const ANTHROPIC_STREAM: &str = concat!(
    "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"m\",\"content\":[],\"usage\":{\"input_tokens\":7,\"output_tokens\":1,\"cache_read_input_tokens\":100,\"cache_creation_input_tokens\":20}}}\n\n",
    "event: ping\ndata: {\"type\": \"ping\"}\n\n",
    "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
    "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello \"}}\n\n",
    "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"world\"}}\n\n",
    "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
    "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":5}}\n\n",
    "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
);

fn anthropic_stream_of(texts: &[&str]) -> String {
    let mut s = String::new();
    s.push_str(
        ANTHROPIC_STREAM
            .split("event: content_block_delta")
            .next()
            .unwrap(),
    );
    for t in texts {
        s.push_str(&format!(
            "event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":{}}}}}\n\n",
            json!(t)
        ));
    }
    s.push_str(
        "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
    );
    s.push_str(
        ANTHROPIC_STREAM
            .split("event: content_block_stop\n")
            .nth(1)
            .unwrap()
            .split_once("\n\n")
            .unwrap()
            .1,
    );
    s
}

fn sse_answer(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(body.to_string(), "text/event-stream")
}

fn streaming(mut v: Value) -> Value {
    v["stream"] = json!(true);
    v
}

/// The text of every `text_delta` of an Anthropic stream.
fn delta_text(stream: &str) -> String {
    stream
        .split("\n\n")
        .filter_map(|e| e.lines().find_map(|l| l.strip_prefix("data: ")))
        .filter_map(|d| serde_json::from_str::<Value>(d).ok())
        .filter_map(|v| v["delta"]["text"].as_str().map(str::to_string))
        .collect()
}

#[tokio::test]
async fn anthropic_stream_is_relayed_as_sent() {
    let h = harness("anthropic").await;
    mock_anthropic(&h, sse_answer(ANTHROPIC_STREAM)).await;
    let (status, headers, body) = claude_code(
        &h,
        "/v1/messages?beta=true",
        &[],
        &streaming(recorded(CLAUDE_CODE, "p/m")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, ANTHROPIC_STREAM);
    assert!(headers["content-type"]
        .to_str()
        .unwrap()
        .starts_with("text/event-stream"));
    let records = h.sink.wait_for(1).await;
    let u = records[0].usage.unwrap();
    assert_eq!(
        (
            u.input_tokens,
            u.output_tokens,
            u.cache_read_input_tokens,
            u.cache_write_input_tokens
        ),
        (7, 5, 100, 20)
    );
    assert!(!records[0].estimated);
    assert!(records[0].stream);
}

const RESPONSES_STREAM: &str = concat!(
    "event: response.created\ndata: {\"type\":\"response.created\",\"sequence_number\":0,\"response\":{\"id\":\"resp_1\",\"object\":\"response\",\"status\":\"in_progress\",\"model\":\"m\",\"output\":[]}}\n\n",
    "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"sequence_number\":1,\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"delta\":\"Hi \"}\n\n",
    "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"sequence_number\":2,\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"delta\":\"there\"}\n\n",
    "event: response.completed\ndata: {\"type\":\"response.completed\",\"sequence_number\":3,\"response\":{\"id\":\"resp_1\",\"object\":\"response\",\"status\":\"completed\",\"model\":\"m\",\"output\":[],\"usage\":{\"input_tokens\":10,\"output_tokens\":2,\"total_tokens\":12,\"input_tokens_details\":{\"cached_tokens\":4},\"output_tokens_details\":{\"reasoning_tokens\":0}}}}\n\n",
);

#[tokio::test]
async fn responses_stream_is_relayed_as_sent() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(sse_answer(RESPONSES_STREAM))
        .mount(&h.upstream)
        .await;
    let (status, _, body) = codex(&h, &[], &streaming(recorded(CODEX, "p/m"))).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, RESPONSES_STREAM);
    let records = h.sink.wait_for(1).await;
    let u = records[0].usage.unwrap();
    assert_eq!(
        (u.input_tokens, u.output_tokens, u.cache_read_input_tokens),
        (6, 2, 4)
    );
    assert!(!records[0].estimated);
}

#[tokio::test]
async fn an_output_block_ends_a_native_stream_with_an_error_event() {
    let h = harness("anthropic").await;
    mock_anthropic(
        &h,
        sse_answer(&anthropic_stream_of(&[
            "the sword",
            "fish is here",
            " and more",
        ])),
    )
    .await;
    guardrail(
        &h,
        "words",
        json!([{ "id": "w", "matcher": { "keywords": { "words": ["swordfish"] } },
                 "action": "block", "directions": "output" }]),
    )
    .await;
    let (status, _, body) = claude_code(
        &h,
        "/v1/messages",
        &[],
        &streaming(recorded(CLAUDE_CODE, "p/m")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!body.contains("swordfish"), "{body}");
    assert!(body.contains("event: error\n"), "{body}");
    assert!(!body.contains("message_stop"), "{body}");
    assert!(body.ends_with("\n\n"));
    let records = h.sink.wait_for(1).await;
    assert!(records[0].guardrails.is_some());
}

#[tokio::test]
async fn output_redaction_rewrites_only_text_deltas() {
    let h = harness("anthropic").await;
    let upstream = anthropic_stream_of(&["mail ada@exa", "mple.com now", " thanks"]);
    mock_anthropic(&h, sse_answer(&upstream)).await;
    guardrail(
        &h,
        "pii",
        json!([{ "id": "email", "matcher": { "pii": ["EMAIL"] },
                 "action": "redact", "directions": "output" }]),
    )
    .await;
    let (status, _, body) = claude_code(
        &h,
        "/v1/messages",
        &[],
        &streaming(recorded(CLAUDE_CODE, "p/m")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let text = delta_text(&body);
    assert!(
        !text.contains("ada@example.com") && !text.contains("example.com"),
        "{text}"
    );
    assert!(
        text.contains("mail ") && text.contains("now thanks"),
        "{text}"
    );
    // Every event that is not a text delta arrives byte for byte.
    for block in upstream
        .split_inclusive("\n\n")
        .filter(|b| !b.contains("text_delta"))
    {
        assert!(body.contains(block), "missing {block}");
    }
    assert!(body.ends_with(
        upstream
            .rsplit_once("event: message_delta")
            .map(|(_, t)| format!("event: message_delta{t}"))
            .unwrap()
            .as_str()
    ));
}

#[tokio::test]
async fn a_caller_gone_mid_stream_is_still_recorded() {
    use axum::body::Body;
    use axum::http::Request;
    use futures::StreamExt;
    use tower::ServiceExt;
    let h = harness("anthropic").await;
    // The answer is not finished: the caller leaves before the end.
    let unfinished = ANTHROPIC_STREAM
        .split("event: message_delta")
        .next()
        .unwrap();
    mock_anthropic(&h, sse_answer(unfinished)).await;
    let resp = h
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/messages")
                .header("authorization", format!("Bearer {}", h.key))
                .header("content-type", "application/json")
                .body(Body::from(
                    streaming(recorded(CLAUDE_CODE, "p/m")).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let mut stream = resp.into_body().into_data_stream();
    stream.next().await.unwrap().unwrap();
    drop(stream);
    let records = h.sink.wait_for(1).await;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(h.sink.records().len(), 1);
    assert!(records[0].estimated);
    assert_eq!(records[0].status, 499);
}

// -------------------------------------------------- input guardrails

const MAIL: &str = "ada@example.com";

async fn input_guardrail(h: &Harness, action: &str) {
    guardrail(
        h,
        "pii-in",
        json!([{ "id": "email", "matcher": { "pii": ["EMAIL"] },
                 "action": action, "directions": "input" }]),
    )
    .await;
}

/// The recorded Claude Code request with a user block, a tool result with
/// text blocks, and a system block that carry the address.
fn with_mail(model: &str) -> Value {
    let mut v = recorded(CLAUDE_CODE, model);
    v["messages"] = json!([
        { "role": "user", "content": [
            { "type": "text", "text": format!("write to {MAIL} now") }
        ] },
        { "role": "assistant", "content": [
            { "type": "tool_use", "id": "tu_1", "name": "Read", "input": {} }
        ] },
        { "role": "user", "content": [
            { "type": "tool_result", "tool_use_id": "tu_1", "content": [
                { "type": "text", "text": format!("file says {MAIL}") },
                { "type": "text", "text": "plain" }
            ] }
        ] }
    ]);
    v
}

#[tokio::test]
async fn input_block_refuses_a_native_call() {
    let h = harness("anthropic").await;
    mock_anthropic(&h, json_answer(ANTHROPIC_ANSWER)).await;
    input_guardrail(&h, "block").await;
    let (status, _, body) = claude_code(&h, "/v1/messages", &[], &with_mail("p/m")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(!body.contains(MAIL), "{body}");
    assert!(received(&h.upstream).await.is_empty());
}

#[tokio::test]
async fn input_redaction_edits_only_text() {
    let h = harness("anthropic").await;
    mock_anthropic(&h, json_answer(ANTHROPIC_ANSWER)).await;
    input_guardrail(&h, "redact").await;
    let sent = with_mail("p/m");
    let (status, _, body) = claude_code(&h, "/v1/messages", &[], &sent).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let got: Value = serde_json::from_slice(&received(&h.upstream).await[0].body).unwrap();
    let raw = got.to_string();
    assert!(!raw.contains(MAIL), "{raw}");
    // Put the original text back: nothing else differs.
    let mut restored = got.clone();
    restored["messages"][0]["content"][0]["text"] =
        sent["messages"][0]["content"][0]["text"].clone();
    restored["messages"][2]["content"][0]["content"][0]["text"] =
        sent["messages"][2]["content"][0]["content"][0]["text"].clone();
    let mut want = sent.clone();
    want["model"] = json!("m");
    assert_eq!(restored, want);
    assert_ne!(got["messages"][0], sent["messages"][0]);
}

#[tokio::test]
async fn redaction_reaches_tool_result_text_blocks() {
    let h = harness("anthropic").await;
    mock_anthropic(&h, json_answer(ANTHROPIC_ANSWER)).await;
    input_guardrail(&h, "redact").await;
    claude_code(&h, "/v1/messages", &[], &with_mail("p/m")).await;
    let got: Value = serde_json::from_slice(&received(&h.upstream).await[0].body).unwrap();
    let block = got["messages"][2]["content"][0]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(
        block.starts_with("file says ") && !block.contains(MAIL),
        "{block}"
    );
    assert_eq!(
        got["messages"][2]["content"][0]["content"][1]["text"],
        "plain"
    );
}

#[tokio::test]
async fn redaction_applies_on_translation_too() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "c1", "model": "m",
            "choices": [{ "message": { "role": "assistant", "content": "ok" }, "finish_reason": "stop" }],
            "usage": { "prompt_tokens": 4, "completion_tokens": 2 }
        })))
        .mount(&h.upstream)
        .await;
    input_guardrail(&h, "redact").await;
    let (status, _, body) = claude_code(&h, "/v1/messages", &[], &with_mail("p/m")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let sent = String::from_utf8(received(&h.upstream).await[0].body.clone()).unwrap();
    assert!(!sent.contains(MAIL), "{sent}");
    assert!(sent.contains("file says"), "{sent}");
}

#[tokio::test]
async fn redaction_reaches_a_codex_input_and_tool_output() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(json_answer(RESPONSES_ANSWER))
        .mount(&h.upstream)
        .await;
    input_guardrail(&h, "redact").await;
    let mut sent = recorded(CODEX, "p/m");
    sent["instructions"] = json!(format!("contact {MAIL}"));
    sent["input"] = json!([
        { "type": "message", "role": "user", "content": [
            { "type": "input_text", "text": format!("hi {MAIL}") } ] },
        { "type": "function_call", "call_id": "c1", "name": "shell", "arguments": "{}" },
        { "type": "function_call_output", "call_id": "c1", "output": format!("out {MAIL}") }
    ]);
    let (status, _, body) = codex(&h, &[], &sent).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let got = String::from_utf8(received(&h.upstream).await[0].body.clone()).unwrap();
    assert!(!got.contains(MAIL), "{got}");
    let v: Value = serde_json::from_str(&got).unwrap();
    assert_eq!(v["input"][1], sent["input"][1]);
    assert!(v["input"][2]["output"]
        .as_str()
        .unwrap()
        .starts_with("out "));
}

// ------------------------------------------------- count_tokens, hello, models

const COUNT: &str = r#"{"input_tokens": 42}"#;

async fn count_tokens(h: &Harness, body: &Value) -> (StatusCode, axum::http::HeaderMap, String) {
    let bearer = format!("Bearer {}", h.key);
    post_to(
        &h.app,
        "/v1/messages/count_tokens?beta=true",
        &[
            ("authorization", &bearer),
            ("anthropic-version", "2023-06-01"),
            ("anthropic-beta", "token-counting-2024-11-01"),
            ("x-claude-code-session-id", "s-1"),
        ],
        &body.to_string(),
    )
    .await
}

#[tokio::test]
async fn count_tokens_passes_through() {
    let h = harness("anthropic").await;
    Mock::given(method("POST"))
        .and(path("/v1/messages/count_tokens"))
        .respond_with(json_answer(COUNT))
        .mount(&h.upstream)
        .await;
    let (status, _, body) = count_tokens(
        &h,
        &json!({"model":"p/m","messages":[{"role":"user","content":"hi"}]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, COUNT);
    let seen = received(&h.upstream).await;
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].url.query(), Some("beta=true"));
    let got: Value = serde_json::from_slice(&seen[0].body).unwrap();
    assert_eq!(got["model"], "m");
    assert_eq!(got["messages"][0]["content"], "hi");
    assert_eq!(
        seen[0].headers.get("anthropic-beta").unwrap(),
        "token-counting-2024-11-01"
    );
    assert_eq!(seen[0].headers.get("x-api-key").unwrap(), "provider-secret");
    assert!(seen[0].headers.get("x-claude-code-session-id").is_none());
    // Counting is not a call: no log row.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(h.sink.records().is_empty());
}

#[tokio::test]
async fn count_tokens_without_an_anthropic_target_is_404() {
    let h = harness("openai").await;
    let (status, _, body) = count_tokens(&h, &json!({"model":"p/m","messages":[]})).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["type"], "error");
    assert_eq!(v["error"]["type"], "not_found_error");
    assert!(received(&h.upstream).await.is_empty());
}

#[tokio::test]
async fn hello_answers() {
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;
    let h = harness("openai").await;
    let resp = h
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("HEAD")
                .uri("/api/hello")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(bytes.is_empty());
}

#[tokio::test]
async fn models_list_has_display_names_and_takes_a_limit() {
    let h = harness("openai").await;
    let bearer = format!("Bearer {}", h.key);
    let get = |uri: &'static str| {
        let app = h.app.clone();
        let bearer = bearer.clone();
        async move {
            use axum::body::Body;
            use axum::http::Request;
            use tower::ServiceExt;
            let resp = app
                .oneshot(
                    Request::builder()
                        .uri(uri)
                        .header("authorization", bearer)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = resp.status();
            let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap();
            (status, serde_json::from_slice::<Value>(&bytes).unwrap())
        }
    };
    let (status, plain) = get("/v1/models").await;
    assert_eq!(status, StatusCode::OK);
    let (status, limited) = get("/v1/models?limit=1000").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(plain, limited);
    let data = plain["data"].as_array().unwrap();
    assert!(!data.is_empty());
    for m in data {
        assert_eq!(m["display_name"], m["id"], "{m}");
    }
}
