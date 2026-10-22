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
