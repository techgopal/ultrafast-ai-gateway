//! External guardrail webhooks in the `/v1` pipeline: the request they get,
//! the signature, what each answer does, fail modes, the timeout, and
//! streams that are held until the hook has answered.

mod common;

use std::time::{Duration, Instant};

use axum::http::StatusCode;
use common::{harness, harness_with_response_limit, post_chat, post_to, Harness};
use serde_json::{json, Value};
use ultrafast_gateway::alerts::sign::{signature, SIGNATURE_HEADER};
use ultrafast_gateway::guardrails::log::LoggedAction;
use ultrafast_gateway::store::NewGuardrail;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const EMAIL: &str = "ada@example.com";
const SECRET: &str = "whsec_test-secret-for-the-hook";
const HOOK_PATH: &str = "/hooks/private-path-9f3a";

struct Spec<'a> {
    name: &'a str,
    timeout_ms: i64,
    fail_mode: &'a str,
    directions: &'a str,
    enabled: bool,
    is_default: bool,
}

impl<'a> Spec<'a> {
    fn new(name: &'a str) -> Self {
        Spec {
            name,
            timeout_ms: 3000,
            fail_mode: "open",
            directions: "both",
            enabled: true,
            is_default: true,
        }
    }
    fn closed(mut self) -> Self {
        self.fail_mode = "closed";
        self
    }
    fn directions(mut self, d: &'a str) -> Self {
        self.directions = d;
        self
    }
    fn timeout(mut self, ms: i64) -> Self {
        self.timeout_ms = ms;
        self
    }
    fn off(mut self) -> Self {
        self.enabled = false;
        self
    }
}

/// Adds an external guardrail that calls `hook`.
async fn external(h: &Harness, hook: &MockServer, spec: Spec<'_>) -> i64 {
    let url = format!("{}{HOOK_PATH}", hook.uri());
    let enc = h.state.cipher.encrypt(url.as_bytes());
    let secret = h.state.cipher.encrypt(SECRET.as_bytes());
    let mut tx = h.store.begin().await.unwrap();
    let id = tx
        .insert_guardrail(NewGuardrail {
            name: spec.name,
            description: "",
            kind: "external",
            rules: "[]",
            url: Some((&enc, &hook.uri())),
            secret_enc: Some(&secret),
            timeout_ms: spec.timeout_ms,
            fail_mode: spec.fail_mode,
            directions: spec.directions,
            enabled: spec.enabled,
            is_default: spec.is_default,
        })
        .await
        .unwrap();
    tx.commit().await.unwrap();
    h.state.refresh().await.unwrap();
    id
}

async fn rules_guardrail(h: &Harness, name: &str, rules: Value) {
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

async fn hook_says(hook: &MockServer, answer: Value) {
    Mock::given(method("POST"))
        .and(path(HOOK_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(answer))
        .mount(hook)
        .await;
}

async fn hook_answers(hook: &MockServer, template: ResponseTemplate) {
    Mock::given(method("POST"))
        .and(path(HOOK_PATH))
        .respond_with(template)
        .mount(hook)
        .await;
}

async fn hook_calls(hook: &MockServer) -> Vec<Request> {
    hook.received_requests().await.unwrap()
}

fn body_of(r: &Request) -> Value {
    serde_json::from_slice(&r.body).unwrap()
}

fn completion(content: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "c1", "model": "m",
        "choices": [{ "message": { "role": "assistant", "content": content }, "finish_reason": "stop" }],
        "usage": { "prompt_tokens": 9, "completion_tokens": 4 }
    }))
}

async fn mount_chat(h: &Harness, template: ResponseTemplate) {
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(template)
        .mount(&h.upstream)
        .await;
}

async fn sent_to_provider(h: &Harness) -> Vec<Value> {
    h.upstream
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .collect()
}

fn chat_body(user_text: &str) -> String {
    json!({ "model": "p/m", "max_tokens": 50,
            "messages": [{ "role": "user", "content": user_text }] })
    .to_string()
}

fn json_of(body: &str) -> Value {
    serde_json::from_str(body).unwrap_or_else(|e| panic!("{e}: {body}"))
}

fn stream_body() -> String {
    json!({ "model": "p/m", "stream": true,
            "messages": [{ "role": "user", "content": "hi" }] })
    .to_string()
}

fn sse(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(body.to_string(), "text/event-stream")
}

fn delta(text: &str) -> String {
    format!(
        "data: {}\n\n",
        json!({ "choices": [{ "delta": { "content": text }, "finish_reason": null }] })
    )
}

fn stream_end() -> String {
    concat!(
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":3}}\n\n",
        "data: [DONE]\n\n",
    )
    .to_string()
}

fn openai_payloads(body: &str) -> Vec<Value> {
    body.lines()
        .filter_map(|l| l.strip_prefix("data: "))
        .filter(|d| *d != "[DONE]")
        .map(json_of)
        .collect()
}

fn openai_text(body: &str) -> String {
    openai_payloads(body)
        .iter()
        .filter_map(|v| v["choices"][0]["delta"]["content"].as_str())
        .collect()
}

fn finish_reason(body: &str) -> Option<String> {
    openai_payloads(body).iter().rev().find_map(|v| {
        v["choices"][0]["finish_reason"]
            .as_str()
            .map(str::to_string)
    })
}

// ---- input ------------------------------------------------------------------

#[tokio::test]
async fn an_allowed_input_is_one_signed_call_with_all_the_texts() {
    let h = harness("openai").await;
    let hook = MockServer::start().await;
    hook_says(&hook, json!({ "action": "allow" })).await;
    mount_chat(&h, completion("hello")).await;
    external(&h, &hook, Spec::new("ext").directions("input")).await;
    let body = json!({ "model": "p/m", "max_tokens": 50, "messages": [
        { "role": "system", "content": "be brief" },
        { "role": "user", "content": "first question" },
        { "role": "assistant", "content": "an answer" },
        { "role": "user", "content": "second question" } ] })
    .to_string();
    let (s, text) = post_chat(&h.app, Some(&h.key), &body).await;
    assert_eq!(s, StatusCode::OK, "{text}");
    assert_eq!(sent_to_provider(&h).await.len(), 1);

    let calls = hook_calls(&hook).await;
    assert_eq!(calls.len(), 1, "one call per request");
    let v = body_of(&calls[0]);
    assert_eq!(v["version"], 1);
    assert_eq!(v["direction"], "input");
    assert_eq!(v["endpoint"], "chat");
    assert_eq!(v["model"], "p/m");
    assert_eq!(
        v["texts"],
        json!(["be brief", "first question", "an answer", "second question"])
    );
    for field in ["route", "key_id", "team_id", "user_id"] {
        assert!(v.get(field).is_some(), "{field} is always present: {v}");
    }
    assert!(v["route"].is_null());
    assert!(v["key_id"].is_i64());
    // Metadata and texts only: nothing of the credentials.
    let raw = String::from_utf8_lossy(&calls[0].body).to_string();
    assert!(
        !raw.contains(&h.key) && !raw.contains("provider-secret"),
        "{raw}"
    );
    assert!(calls[0].headers.get("authorization").is_none());
}

#[tokio::test]
async fn the_signature_is_the_alert_scheme_over_the_exact_body() {
    let h = harness("openai").await;
    let hook = MockServer::start().await;
    hook_says(&hook, json!({ "action": "allow" })).await;
    mount_chat(&h, completion("hello")).await;
    external(&h, &hook, Spec::new("ext")).await;
    let (s, _) = post_chat(&h.app, Some(&h.key), &chat_body("hi")).await;
    assert_eq!(s, StatusCode::OK);
    let calls = hook_calls(&hook).await;
    let header = calls[0]
        .headers
        .get(SIGNATURE_HEADER)
        .expect("a signature")
        .to_str()
        .unwrap()
        .to_string();
    let (t, v1) = header
        .strip_prefix("t=")
        .and_then(|r| r.split_once(",v1="))
        .expect("t=<secs>,v1=<hex>");
    let t: i64 = t.parse().unwrap();
    assert_eq!(v1, signature(SECRET, t, &calls[0].body));
    assert_eq!(
        calls[0].headers.get("content-type").unwrap(),
        "application/json"
    );
}

#[tokio::test]
async fn a_blocking_hook_refuses_the_call_before_any_provider_call() {
    let h = harness("openai").await;
    let hook = MockServer::start().await;
    hook_says(&hook, json!({ "action": "block", "reason": "not today" })).await;
    mount_chat(&h, completion("hello")).await;
    external(&h, &hook, Spec::new("policy-hook")).await;
    let (s, body) = post_chat(&h.app, Some(&h.key), &chat_body("hi")).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{body}");
    let v = json_of(&body);
    assert_eq!(v["error"]["code"], "guardrail_blocked");
    assert_eq!(v["error"]["message"], "Blocked by guardrail 'policy-hook'.");
    // The hook's reason is not passed on.
    assert!(!body.contains("not today"), "{body}");
    assert!(sent_to_provider(&h).await.is_empty());
    let records = h.sink.wait_for(1).await;
    let g = records[0].guardrails.clone().unwrap();
    assert_eq!(g.action, LoggedAction::Blocked);
}

#[tokio::test]
async fn a_redacting_hook_changes_what_the_provider_sees() {
    let h = harness("openai").await;
    let hook = MockServer::start().await;
    hook_answers(
        &hook,
        ResponseTemplate::new(200).set_body_json(json!({
            "action": "redact", "texts": ["be brief", "mail [HIDDEN] please"] })),
    )
    .await;
    mount_chat(&h, completion("hello")).await;
    external(&h, &hook, Spec::new("ext")).await;
    let body = json!({ "model": "p/m", "max_tokens": 50, "messages": [
        { "role": "system", "content": "be brief" },
        { "role": "user", "content": format!("mail {EMAIL} please") } ] })
    .to_string();
    let (s, text) = post_chat(&h.app, Some(&h.key), &body).await;
    assert_eq!(s, StatusCode::OK, "{text}");
    let sent = sent_to_provider(&h).await;
    assert_eq!(sent[0]["messages"][1]["content"], "mail [HIDDEN] please");
    assert!(!sent[0].to_string().contains("example.com"));
    let records = h.sink.wait_for(1).await;
    let g = records[0].guardrails.clone().unwrap();
    assert_eq!(g.action, LoggedAction::Redacted);
    assert!(!format!("{g:?}").contains("HIDDEN"));
}

#[tokio::test]
async fn a_disabled_guardrail_is_never_called() {
    let h = harness("openai").await;
    let hook = MockServer::start().await;
    hook_says(&hook, json!({ "action": "block" })).await;
    mount_chat(&h, completion("hello")).await;
    external(&h, &hook, Spec::new("ext").off()).await;
    let (s, _) = post_chat(&h.app, Some(&h.key), &chat_body("hi")).await;
    assert_eq!(s, StatusCode::OK);
    assert!(hook_calls(&hook).await.is_empty());
}

#[tokio::test]
async fn a_guardrail_for_the_other_direction_is_not_called() {
    let h = harness("openai").await;
    let hook = MockServer::start().await;
    hook_says(&hook, json!({ "action": "block" })).await;
    mount_chat(&h, completion("hello")).await;
    external(&h, &hook, Spec::new("ext").directions("output")).await;
    // The input is not asked about; the answer is, and the hook blocks it.
    let (s, body) = post_chat(&h.app, Some(&h.key), &chat_body("hi")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        json_of(&body)["choices"][0]["finish_reason"],
        "content_filter"
    );
    let calls = hook_calls(&hook).await;
    assert_eq!(calls.len(), 1);
    assert_eq!(body_of(&calls[0])["direction"], "output");
}

#[tokio::test]
async fn rules_run_first_and_the_hook_sees_their_redactions() {
    let h = harness("openai").await;
    let hook = MockServer::start().await;
    hook_says(&hook, json!({ "action": "allow" })).await;
    mount_chat(&h, completion("hello")).await;
    // The hook is attached first, yet it is asked after the rules.
    external(&h, &hook, Spec::new("a-ext").directions("input")).await;
    rules_guardrail(
        &h,
        "z-pii",
        json!([{ "id": "email", "matcher": { "pii": ["EMAIL"] },
                 "action": "redact", "directions": "input" }]),
    )
    .await;
    let (s, _) = post_chat(&h.app, Some(&h.key), &chat_body(&format!("mail {EMAIL}"))).await;
    assert_eq!(s, StatusCode::OK);
    let calls = hook_calls(&hook).await;
    assert_eq!(
        body_of(&calls[0])["texts"],
        json!(["mail [REDACTED:EMAIL]"])
    );
}

#[tokio::test]
async fn a_rule_that_blocks_means_the_hook_is_not_called() {
    let h = harness("openai").await;
    let hook = MockServer::start().await;
    hook_says(&hook, json!({ "action": "allow" })).await;
    mount_chat(&h, completion("hello")).await;
    external(&h, &hook, Spec::new("ext").directions("input")).await;
    rules_guardrail(
        &h,
        "words",
        json!([{ "id": "w", "matcher": { "keywords": { "words": ["swordfish"] } },
                 "action": "block", "directions": "input" }]),
    )
    .await;
    let (s, _) = post_chat(&h.app, Some(&h.key), &chat_body("swordfish")).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(hook_calls(&hook).await.is_empty());
}

// ---- failures ---------------------------------------------------------------

fn failures() -> Vec<(&'static str, ResponseTemplate, &'static str)> {
    vec![
        (
            "not json",
            ResponseTemplate::new(200).set_body_string("nope"),
            "invalid",
        ),
        (
            "wrong shape",
            ResponseTemplate::new(200).set_body_json(json!(["allow"])),
            "invalid",
        ),
        (
            "unknown action",
            ResponseTemplate::new(200).set_body_json(json!({ "action": "maybe" })),
            "invalid",
        ),
        (
            "redact without texts",
            ResponseTemplate::new(200).set_body_json(json!({ "action": "redact" })),
            "invalid",
        ),
        (
            "redact with another number of texts",
            ResponseTemplate::new(200)
                .set_body_json(json!({ "action": "redact", "texts": ["a", "b"] })),
            "invalid",
        ),
        (
            "an error status",
            ResponseTemplate::new(500).set_body_json(json!({ "action": "allow" })),
            "status",
        ),
        (
            "a redirect",
            ResponseTemplate::new(302).insert_header("location", "http://127.0.0.1:1/x"),
            "status",
        ),
        (
            "a body over 1 MiB",
            ResponseTemplate::new(200).set_body_json(json!({
                "action": "allow", "reason": "x".repeat(1024 * 1024 + 10) })),
            "too_large",
        ),
    ]
}

#[tokio::test]
async fn an_unusable_answer_follows_the_fail_mode() {
    for (what, template, reason) in failures() {
        // Fail open: the call goes on, unchanged, and is flagged.
        let h = harness("openai").await;
        let hook = MockServer::start().await;
        hook_answers(&hook, template.clone()).await;
        mount_chat(&h, completion("hello")).await;
        external(&h, &hook, Spec::new("ext").directions("input")).await;
        let (s, body) = post_chat(&h.app, Some(&h.key), &chat_body("original text")).await;
        assert_eq!(s, StatusCode::OK, "{what}: {body}");
        let sent = sent_to_provider(&h).await;
        assert_eq!(sent[0]["messages"][0]["content"], "original text", "{what}");
        let records = h.sink.wait_for(1).await;
        let g = records[0]
            .guardrails
            .clone()
            .unwrap_or_else(|| panic!("{what}: no record"));
        assert_eq!(g.action, LoggedAction::Flagged, "{what}");
        let flags = &g.input.unwrap().flags;
        assert_eq!(flags.len(), 1, "{what}");
        assert_eq!(
            flags[0].rule_id,
            format!("external_error:{reason}"),
            "{what}"
        );

        // Fail closed: the call is refused, and says why in the record.
        let h = harness("openai").await;
        let hook = MockServer::start().await;
        hook_answers(&hook, template).await;
        mount_chat(&h, completion("hello")).await;
        external(&h, &hook, Spec::new("strict").closed().directions("input")).await;
        let (s, body) = post_chat(&h.app, Some(&h.key), &chat_body("original text")).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{what}: {body}");
        assert_eq!(
            json_of(&body)["error"]["message"],
            "Blocked by guardrail 'strict'.",
            "{what}"
        );
        assert!(sent_to_provider(&h).await.is_empty(), "{what}");
        let records = h.sink.wait_for(1).await;
        let g = records[0].guardrails.clone().unwrap();
        assert_eq!(g.action, LoggedAction::Blocked, "{what}");
        let input = g.input.unwrap();
        assert_eq!(input.blocked_by.unwrap().name, "strict");
        assert_eq!(input.flags[0].rule_id, format!("external_error:{reason}"));
    }
}

#[tokio::test]
async fn a_hook_that_is_down_follows_the_fail_mode() {
    let h = harness("openai").await;
    // Nothing listens on this port any more.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let url = format!("http://127.0.0.1:{port}{HOOK_PATH}");
    let enc = h.state.cipher.encrypt(url.as_bytes());
    let secret = h.state.cipher.encrypt(SECRET.as_bytes());
    let mut tx = h.store.begin().await.unwrap();
    tx.insert_guardrail(NewGuardrail {
        name: "ext",
        description: "",
        kind: "external",
        rules: "[]",
        url: Some((&enc, "http://127.0.0.1")),
        secret_enc: Some(&secret),
        timeout_ms: 3000,
        fail_mode: "open",
        directions: "input",
        enabled: true,
        is_default: true,
    })
    .await
    .unwrap();
    tx.commit().await.unwrap();
    h.state.refresh().await.unwrap();
    mount_chat(&h, completion("hello")).await;
    let (s, _) = post_chat(&h.app, Some(&h.key), &chat_body("hi")).await;
    assert_eq!(s, StatusCode::OK);
    let records = h.sink.wait_for(1).await;
    let flags = records[0].guardrails.clone().unwrap().input.unwrap().flags;
    assert_eq!(flags[0].rule_id, "external_error:connect");
}

#[tokio::test]
async fn a_slow_hook_never_holds_a_call_past_its_timeout() {
    for fail_mode in ["open", "closed"] {
        let h = harness("openai").await;
        let hook = MockServer::start().await;
        hook_answers(
            &hook,
            ResponseTemplate::new(200)
                .set_body_json(json!({ "action": "allow" }))
                .set_delay(Duration::from_secs(5)),
        )
        .await;
        mount_chat(&h, completion("hello")).await;
        let mut spec = Spec::new("ext").directions("input").timeout(500);
        spec.fail_mode = fail_mode;
        external(&h, &hook, spec).await;
        let started = Instant::now();
        let (s, _) = post_chat(&h.app, Some(&h.key), &chat_body("hi")).await;
        let took = started.elapsed();
        assert!(
            took < Duration::from_millis(500 + 200),
            "{fail_mode}: took {took:?}"
        );
        assert!(took >= Duration::from_millis(450), "{fail_mode}: {took:?}");
        match fail_mode {
            "open" => assert_eq!(s, StatusCode::OK),
            _ => assert_eq!(s, StatusCode::BAD_REQUEST),
        }
        let records = h.sink.wait_for(1).await;
        let input = records[0].guardrails.clone().unwrap().input.unwrap();
        assert_eq!(input.flags[0].rule_id, "external_error:timeout");
    }
}

#[tokio::test]
async fn failures_are_counted_by_reason_and_never_reveal_the_url_or_the_text() {
    let h = harness("openai").await;
    let hook = MockServer::start().await;
    hook_answers(&hook, ResponseTemplate::new(500)).await;
    mount_chat(&h, completion("hello")).await;
    external(&h, &hook, Spec::new("ext").directions("input")).await;
    let (s, _) = post_chat(&h.app, Some(&h.key), &chat_body("very private words")).await;
    assert_eq!(s, StatusCode::OK);
    let records = h.sink.wait_for(1).await;
    let dump = format!("{:?}", records[0].guardrails);
    assert!(!dump.contains("private"), "{dump}");
    assert!(!dump.contains("hooks"), "{dump}");
    assert!(!dump.contains("127.0.0.1"), "{dump}");
    // Counted when the record is emitted.
    let metrics = h.state.metrics.render(&[]);
    assert!(
        metrics.contains("uf_guardrail_external_errors_total{reason=\"status\"} 1"),
        "{metrics}"
    );
    assert!(!metrics.contains("hooks"), "{metrics}");
}

// ---- output, whole answers ----------------------------------------------------

#[tokio::test]
async fn the_hook_gets_the_answer_and_the_tool_arguments_in_one_call() {
    let h = harness("openai").await;
    let hook = MockServer::start().await;
    hook_says(&hook, json!({ "action": "allow" })).await;
    mount_chat(
        &h,
        ResponseTemplate::new(200).set_body_json(json!({
            "id": "c1", "model": "m",
            "choices": [{ "message": { "role": "assistant", "content": "the answer",
                "tool_calls": [{ "id": "t", "type": "function",
                    "function": { "name": "f", "arguments": "{\"q\":1}" } }] },
                "finish_reason": "tool_calls" }],
            "usage": { "prompt_tokens": 9, "completion_tokens": 4 }
        })),
    )
    .await;
    external(&h, &hook, Spec::new("ext").directions("output")).await;
    let (s, _) = post_chat(&h.app, Some(&h.key), &chat_body("hi")).await;
    assert_eq!(s, StatusCode::OK);
    let calls = hook_calls(&hook).await;
    assert_eq!(calls.len(), 1);
    let v = body_of(&calls[0]);
    assert_eq!(v["direction"], "output");
    assert_eq!(v["texts"], json!(["the answer", "{\"q\":1}"]));
}

#[tokio::test]
async fn a_hook_can_redact_or_block_a_whole_answer() {
    let h = harness("openai").await;
    let hook = MockServer::start().await;
    hook_says(
        &hook,
        json!({ "action": "redact", "texts": ["write to [GONE] now"] }),
    )
    .await;
    mount_chat(&h, completion(&format!("write to {EMAIL} now"))).await;
    external(&h, &hook, Spec::new("ext").directions("output")).await;
    let (s, body) = post_chat(&h.app, Some(&h.key), &chat_body("hi")).await;
    assert_eq!(s, StatusCode::OK, "{body}");
    let v = json_of(&body);
    assert_eq!(v["choices"][0]["message"]["content"], "write to [GONE] now");

    let h = harness("openai").await;
    let hook = MockServer::start().await;
    hook_says(&hook, json!({ "action": "block" })).await;
    mount_chat(&h, completion("secret stuff")).await;
    external(&h, &hook, Spec::new("ext").directions("output")).await;
    let (s, body) = post_chat(&h.app, Some(&h.key), &chat_body("hi")).await;
    assert_eq!(s, StatusCode::OK);
    let v = json_of(&body);
    assert_eq!(v["choices"][0]["message"]["content"], "");
    assert_eq!(v["choices"][0]["finish_reason"], "content_filter");
}

#[tokio::test]
async fn an_answer_checked_by_a_hook_that_failed_is_not_kept_in_the_cache() {
    let h = harness("openai").await;
    let hook = MockServer::start().await;
    hook_answers(&hook, ResponseTemplate::new(500)).await;
    mount_chat(&h, completion("hello")).await;
    external(&h, &hook, Spec::new("ext").directions("output")).await;
    common_route_with_cache(&h).await;
    let body =
        json!({ "model": "r", "messages": [{ "role": "user", "content": "hi" }] }).to_string();
    for _ in 0..2 {
        let (s, _) = post_chat(&h.app, Some(&h.key), &body).await;
        assert_eq!(s, StatusCode::OK);
    }
    // Neither was a hit: the unchecked answer was never kept.
    assert_eq!(sent_to_provider(&h).await.len(), 2);
}

#[tokio::test]
async fn an_answer_a_hook_redacted_is_kept_redacted() {
    let h = harness("openai").await;
    let hook = MockServer::start().await;
    hook_says(&hook, json!({ "action": "redact", "texts": ["clean"] })).await;
    mount_chat(&h, completion(&format!("dirty {EMAIL}"))).await;
    external(&h, &hook, Spec::new("ext").directions("output")).await;
    common_route_with_cache(&h).await;
    let body =
        json!({ "model": "r", "messages": [{ "role": "user", "content": "hi" }] }).to_string();
    for _ in 0..2 {
        let (_, text) = post_chat(&h.app, Some(&h.key), &body).await;
        assert_eq!(json_of(&text)["choices"][0]["message"]["content"], "clean");
    }
    assert_eq!(sent_to_provider(&h).await.len(), 1);
    assert_eq!(hook_calls(&hook).await.len(), 1);
}

async fn common_route_with_cache(h: &Harness) {
    use ultrafast_gateway::cache::{CacheScope, RouteCache};
    use ultrafast_gateway::store::{RouteSettings, TargetsInput};
    let models = h.store.list_models().await.unwrap();
    let model = models.iter().find(|m| m.name == "m").unwrap().id;
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
}

// ---- output, streams ----------------------------------------------------------

#[tokio::test]
async fn a_stream_is_held_until_the_hook_has_seen_all_of_it_then_released() {
    let h = harness("openai").await;
    let hook = MockServer::start().await;
    hook_says(
        &hook,
        json!({ "action": "redact", "texts": ["write to [GONE] now bye"] }),
    )
    .await;
    let upstream = format!(
        "{}{}{}{}{}",
        delta("write to ad"),
        delta("a@example.com"),
        delta(" now"),
        delta(" bye"),
        stream_end()
    );
    mount_chat(&h, sse(&upstream)).await;
    external(&h, &hook, Spec::new("ext").directions("output")).await;
    let (s, text) = post_chat(&h.app, Some(&h.key), &stream_body()).await;
    assert_eq!(s, StatusCode::OK);
    let calls = hook_calls(&hook).await;
    assert_eq!(calls.len(), 1, "one call at the end of the stream");
    assert_eq!(
        body_of(&calls[0])["texts"],
        json!(["write to ada@example.com now bye"])
    );
    assert_eq!(openai_text(&text), "write to [GONE] now bye");
    assert!(!text.contains("example.com"));
    assert_eq!(finish_reason(&text).as_deref(), Some("stop"));
    assert!(text.ends_with("data: [DONE]\n\n"));
    // The provider's usage is kept.
    let records = h.sink.wait_for(1).await;
    assert_eq!(records[0].usage.unwrap().output_tokens, 3);
    assert!(!records[0].estimated);
}

#[tokio::test]
async fn a_blocked_stream_releases_nothing_and_ends_with_content_filter() {
    let h = harness("openai").await;
    let hook = MockServer::start().await;
    hook_says(&hook, json!({ "action": "block" })).await;
    let upstream = format!(
        "{}{}{}",
        delta("first words "),
        delta("second words"),
        stream_end()
    );
    mount_chat(&h, sse(&upstream)).await;
    external(&h, &hook, Spec::new("ext").directions("output")).await;
    let (s, text) = post_chat(&h.app, Some(&h.key), &stream_body()).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(openai_text(&text), "", "{text}");
    assert!(!text.contains("words"), "{text}");
    assert_eq!(finish_reason(&text).as_deref(), Some("content_filter"));
    assert!(text.ends_with("data: [DONE]\n\n"));
    let records = h.sink.wait_for(1).await;
    let out = records[0].guardrails.clone().unwrap().output.unwrap();
    assert_eq!(out.action, LoggedAction::Blocked);
    assert_eq!(out.blocked_by.unwrap().name, "ext");
}

#[tokio::test]
async fn a_blocked_anthropic_stream_ends_with_refusal() {
    let h = harness("openai").await;
    let hook = MockServer::start().await;
    hook_says(&hook, json!({ "action": "block" })).await;
    mount_chat(
        &h,
        sse(&format!("{}{}", delta("hidden words"), stream_end())),
    )
    .await;
    external(&h, &hook, Spec::new("ext").directions("output")).await;
    let bearer = format!("Bearer {}", h.key);
    let (s, _, text) = post_to(
        &h.app,
        "/v1/messages",
        &[
            ("authorization", &bearer),
            ("anthropic-version", "2023-06-01"),
        ],
        &json!({ "model": "p/m", "max_tokens": 30, "stream": true,
                 "messages": [{ "role": "user", "content": "hi" }] })
        .to_string(),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert!(!text.contains("hidden"), "{text}");
    assert!(text.contains("\"stop_reason\":\"refusal\""), "{text}");
}

#[tokio::test]
async fn a_streamed_tool_call_is_sent_to_the_hook_and_comes_back_redacted() {
    let h = harness("openai").await;
    let hook = MockServer::start().await;
    hook_says(
        &hook,
        json!({ "action": "redact", "texts": ["", "{\"to\":\"[GONE]\"}"] }),
    )
    .await;
    let start = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"f\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n";
    let args = |a: &str| {
        format!(
            "data: {}\n\n",
            json!({ "choices": [{ "delta": { "tool_calls": [
                { "index": 0, "function": { "arguments": a } }] }, "finish_reason": null }] })
        )
    };
    let upstream = format!(
        "{start}{}{}{}",
        args("{\"to\":\"ad"),
        args("a@example.com\"}"),
        stream_end()
    );
    mount_chat(&h, sse(&upstream)).await;
    external(&h, &hook, Spec::new("ext").directions("output")).await;
    let (s, text) = post_chat(&h.app, Some(&h.key), &stream_body()).await;
    assert_eq!(s, StatusCode::OK);
    let calls = hook_calls(&hook).await;
    assert_eq!(
        body_of(&calls[0])["texts"],
        json!(["", "{\"to\":\"ada@example.com\"}"])
    );
    let arguments: String = openai_payloads(&text)
        .iter()
        .filter_map(|v| v["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"].as_str())
        .collect();
    assert_eq!(arguments, "{\"to\":\"[GONE]\"}", "{text}");
    assert!(text.contains("\"name\":\"f\""), "{text}");
}

#[tokio::test]
async fn a_stream_with_only_an_input_hook_is_not_held_and_the_hook_is_asked_once() {
    let h = harness("openai").await;
    let hook = MockServer::start().await;
    hook_says(&hook, json!({ "action": "allow" })).await;
    mount_chat(
        &h,
        sse(&format!("{}{}", delta("plain words"), stream_end())),
    )
    .await;
    external(&h, &hook, Spec::new("ext").directions("input")).await;
    let (s, text) = post_chat(&h.app, Some(&h.key), &stream_body()).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(openai_text(&text), "plain words");
    let calls = hook_calls(&hook).await;
    assert_eq!(calls.len(), 1);
    assert_eq!(body_of(&calls[0])["direction"], "input");
}

#[tokio::test]
async fn a_failing_hook_on_a_stream_follows_the_fail_mode() {
    for closed in [false, true] {
        let h = harness("openai").await;
        let hook = MockServer::start().await;
        hook_answers(&hook, ResponseTemplate::new(503)).await;
        mount_chat(&h, sse(&format!("{}{}", delta("some words"), stream_end()))).await;
        let mut spec = Spec::new("ext").directions("output");
        if closed {
            spec = spec.closed();
        }
        external(&h, &hook, spec).await;
        let (s, text) = post_chat(&h.app, Some(&h.key), &stream_body()).await;
        assert_eq!(s, StatusCode::OK);
        let out = h.sink.wait_for(1).await[0]
            .guardrails
            .clone()
            .unwrap()
            .output
            .unwrap();
        assert_eq!(out.flags[0].rule_id, "external_error:status");
        if closed {
            assert_eq!(openai_text(&text), "", "{text}");
            assert_eq!(finish_reason(&text).as_deref(), Some("content_filter"));
        } else {
            assert_eq!(openai_text(&text), "some words");
            assert_eq!(finish_reason(&text).as_deref(), Some("stop"));
        }
    }
}

#[tokio::test]
async fn a_stream_over_the_buffer_cap_follows_the_fail_mode_without_calling_the_hook() {
    for closed in [false, true] {
        // The cap is the provider response cap.
        let h = harness_with_response_limit("openai", 400).await;
        let hook = MockServer::start().await;
        hook_says(&hook, json!({ "action": "block" })).await;
        let long = "word ".repeat(200);
        mount_chat(
            &h,
            sse(&format!("{}{}{}", delta(&long), delta(&long), stream_end())),
        )
        .await;
        let mut spec = Spec::new("ext").directions("output");
        if closed {
            spec = spec.closed();
        }
        external(&h, &hook, spec).await;
        let (s, text) = post_chat(&h.app, Some(&h.key), &stream_body()).await;
        assert_eq!(s, StatusCode::OK, "{text}");
        assert!(hook_calls(&hook).await.is_empty());
        let out = h.sink.wait_for(1).await[0]
            .guardrails
            .clone()
            .unwrap()
            .output
            .unwrap();
        assert_eq!(out.flags[0].rule_id, "external_error:buffer_full");
        if closed {
            assert_eq!(openai_text(&text), "");
            assert_eq!(finish_reason(&text).as_deref(), Some("content_filter"));
        } else {
            assert_eq!(openai_text(&text), format!("{long}{long}"));
        }
    }
}

#[tokio::test]
async fn on_a_stream_the_rules_scanner_goes_first_and_the_hook_sees_its_redactions() {
    let h = harness("openai").await;
    let hook = MockServer::start().await;
    hook_says(&hook, json!({ "action": "allow" })).await;
    mount_chat(
        &h,
        sse(&format!(
            "{}{}{}",
            delta("write to ada@exam"),
            delta("ple.com now"),
            stream_end()
        )),
    )
    .await;
    external(&h, &hook, Spec::new("a-ext").directions("output")).await;
    rules_guardrail(
        &h,
        "z-pii",
        json!([{ "id": "email", "matcher": { "pii": ["EMAIL"] },
                 "action": "redact", "directions": "output" }]),
    )
    .await;
    let (s, text) = post_chat(&h.app, Some(&h.key), &stream_body()).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(openai_text(&text), "write to [REDACTED:EMAIL] now");
    let calls = hook_calls(&hook).await;
    assert_eq!(calls.len(), 1);
    assert_eq!(
        body_of(&calls[0])["texts"],
        json!(["write to [REDACTED:EMAIL] now"])
    );
}

#[tokio::test]
async fn a_rule_that_cuts_a_held_stream_means_the_hook_is_not_asked() {
    let h = harness("openai").await;
    let hook = MockServer::start().await;
    hook_says(&hook, json!({ "action": "allow" })).await;
    mount_chat(
        &h,
        sse(&format!(
            "{}{}",
            delta("some swordfish words"),
            stream_end()
        )),
    )
    .await;
    external(&h, &hook, Spec::new("ext").directions("output")).await;
    rules_guardrail(
        &h,
        "words",
        json!([{ "id": "w", "matcher": { "keywords": { "words": ["swordfish"] } },
                 "action": "block", "directions": "output" }]),
    )
    .await;
    let (s, text) = post_chat(&h.app, Some(&h.key), &stream_body()).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(openai_text(&text), "");
    assert_eq!(finish_reason(&text).as_deref(), Some("content_filter"));
    assert!(hook_calls(&hook).await.is_empty());
}

#[tokio::test]
async fn embeddings_inputs_go_to_the_hook_and_a_block_refuses_the_call() {
    let h = harness("openai").await;
    let hook = MockServer::start().await;
    hook_says(&hook, json!({ "action": "block" })).await;
    external(&h, &hook, Spec::new("ext").directions("input")).await;
    let (s, _, body) = post_to(
        &h.app,
        "/v1/embeddings",
        &[("authorization", &format!("Bearer {}", h.key))],
        &json!({ "model": "p/m", "input": ["one", "two"] }).to_string(),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{body}");
    let calls = hook_calls(&hook).await;
    assert_eq!(calls.len(), 1);
    let v = body_of(&calls[0]);
    assert_eq!(v["endpoint"], "embeddings");
    assert_eq!(v["texts"], json!(["one", "two"]));
    assert!(sent_to_provider(&h).await.is_empty());
}

#[tokio::test]
async fn a_redacted_stream_is_rendered_in_the_anthropic_format_too() {
    let h = harness("openai").await;
    let hook = MockServer::start().await;
    hook_says(
        &hook,
        json!({ "action": "redact", "texts": ["write to [GONE] now"] }),
    )
    .await;
    mount_chat(
        &h,
        sse(&format!(
            "{}{}{}",
            delta("write to ada@"),
            delta("example.com now"),
            stream_end()
        )),
    )
    .await;
    external(&h, &hook, Spec::new("ext").directions("output")).await;
    let bearer = format!("Bearer {}", h.key);
    let (s, _, text) = post_to(
        &h.app,
        "/v1/messages",
        &[
            ("authorization", &bearer),
            ("anthropic-version", "2023-06-01"),
        ],
        &json!({ "model": "p/m", "max_tokens": 30, "stream": true,
                 "messages": [{ "role": "user", "content": "hi" }] })
        .to_string(),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert!(text.contains("write to [GONE] now"), "{text}");
    assert!(!text.contains("example.com"), "{text}");
    assert!(text.contains("\"stop_reason\":\"end_turn\""), "{text}");
    assert!(text.contains("event: message_stop"), "{text}");
}
