//! Guardrails in the `/v1` pipeline: input checks after resolve and the rate
//! limits (before budgets and the cache), output checks on whole answers before the cache keeps them,
//! stream scanning, and what a record says about it.

mod common;

use axum::http::StatusCode;
use common::{harness, post_chat, post_to, Harness};
use serde_json::{json, Value};
use ultrafast_gateway::cache::CacheScope;
use ultrafast_gateway::limits::{LimitScope, RateLimit};
use ultrafast_gateway::store::{NewGuardrail, RouteSettings, TargetsInput};
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

const EMAIL: &str = "ada@example.com";

fn email_rule(action: &str, directions: &str) -> Value {
    json!({ "id": "email", "matcher": { "pii": ["EMAIL"] }, "action": action, "directions": directions })
}

fn word_rule(id: &str, word: &str, action: &str, directions: &str) -> Value {
    json!({ "id": id, "matcher": { "keywords": { "words": [word] } },
            "action": action, "directions": directions })
}

/// Adds an enabled guardrail; `is_default` makes it apply to every call.
async fn guardrail(h: &Harness, name: &str, rules: Vec<Value>, is_default: bool) -> i64 {
    let rules = Value::Array(rules).to_string();
    let mut tx = h.store.begin().await.unwrap();
    let id = tx
        .insert_guardrail(NewGuardrail {
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
            is_default,
        })
        .await
        .unwrap();
    tx.commit().await.unwrap();
    h.state.refresh().await.unwrap();
    id
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

async fn messages(h: &Harness, body: &Value) -> (StatusCode, String) {
    let bearer = format!("Bearer {}", h.key);
    let (s, _, text) = post_to(
        &h.app,
        "/v1/messages",
        &[
            ("authorization", &bearer),
            ("anthropic-version", "2023-06-01"),
        ],
        &body.to_string(),
    )
    .await;
    (s, text)
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

// ---- input -----------------------------------------------------------------

#[tokio::test]
async fn a_blocked_input_is_a_400_in_the_openai_shape_and_calls_no_provider() {
    let h = harness("openai").await;
    mount_chat(&h, completion("hello")).await;
    guardrail(
        &h,
        "no-secrets",
        vec![word_rule("w", "swordfish", "block", "input")],
        true,
    )
    .await;
    let (status, body) = post_chat(&h.app, Some(&h.key), &chat_body("the word is swordfish")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let v = json_of(&body);
    assert_eq!(v["error"]["code"], "guardrail_blocked");
    assert_eq!(v["error"]["message"], "Blocked by guardrail 'no-secrets'.");
    assert_eq!(v["error"]["type"], "invalid_request_error");
    assert!(!body.contains("swordfish"));
    assert!(sent_to_provider(&h).await.is_empty());
}

#[tokio::test]
async fn a_blocked_input_is_a_400_in_the_anthropic_shape() {
    let h = harness("openai").await;
    mount_chat(&h, completion("hello")).await;
    guardrail(
        &h,
        "no-secrets",
        vec![word_rule("w", "swordfish", "block", "input")],
        true,
    )
    .await;
    let (status, body) = messages(
        &h,
        &json!({ "model": "p/m", "max_tokens": 20,
                 "messages": [{ "role": "user", "content": "swordfish" }] }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let v = json_of(&body);
    assert_eq!(v["type"], "error");
    assert_eq!(v["error"]["type"], "invalid_request_error");
    assert_eq!(v["error"]["message"], "Blocked by guardrail 'no-secrets'.");
    assert!(sent_to_provider(&h).await.is_empty());
}

#[tokio::test]
async fn a_blocked_input_is_not_charged_to_a_limit() {
    let h = harness("openai").await;
    mount_chat(&h, completion("hello")).await;
    guardrail(
        &h,
        "g",
        vec![word_rule("w", "swordfish", "block", "input")],
        true,
    )
    .await;
    let mut tx = h.store.begin().await.unwrap();
    tx.upsert_limit(
        LimitScope::Gateway,
        None,
        &RateLimit {
            requests_per_minute: Some(1),
            tokens_per_minute: None,
            concurrent: None,
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    h.state.refresh().await.unwrap();
    for _ in 0..3 {
        let (s, _) = post_chat(&h.app, Some(&h.key), &chat_body("swordfish")).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }
    // The one request of the minute is still there.
    let (s, body) = post_chat(&h.app, Some(&h.key), &chat_body("fine")).await;
    assert_eq!(s, StatusCode::OK, "{body}");
}

#[tokio::test]
async fn a_call_a_limit_refuses_is_not_scanned() {
    let h = harness("openai").await;
    mount_chat(&h, completion("hello")).await;
    guardrail(
        &h,
        "g",
        vec![word_rule("w", "swordfish", "block", "input")],
        true,
    )
    .await;
    let mut tx = h.store.begin().await.unwrap();
    tx.upsert_limit(
        LimitScope::Gateway,
        None,
        &RateLimit {
            requests_per_minute: Some(1),
            tokens_per_minute: None,
            concurrent: None,
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    h.state.refresh().await.unwrap();
    let (s, body) = post_chat(&h.app, Some(&h.key), &chat_body("fine")).await;
    assert_eq!(s, StatusCode::OK, "{body}");
    // The minute is spent. The limit answers before the rules look at the
    // input, so a body that would be blocked is refused as rate limited: the
    // work of scanning is only done for calls the limits let in.
    let (s, _) = post_chat(&h.app, Some(&h.key), &chat_body("swordfish")).await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn redacted_input_reaches_the_provider_without_the_match() {
    let h = harness("openai").await;
    mount_chat(&h, completion("hello")).await;
    guardrail(&h, "pii", vec![email_rule("redact", "both")], true).await;
    let body = json!({
        "model": "p/m", "max_tokens": 50,
        "messages": [
            { "role": "system", "content": "contact ops@example.com for help" },
            { "role": "user", "content": format!("mail {EMAIL} please") },
            { "role": "assistant", "content": null, "tool_calls": [
                { "id": "t1", "type": "function",
                  "function": { "name": "look", "arguments": "{\"who\":\"eve@example.com\"}" } } ] },
            { "role": "tool", "tool_call_id": "t1", "content": "found bob@example.com" }
        ]
    })
    .to_string();
    let (status, answer) = post_chat(&h.app, Some(&h.key), &body).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let sent = sent_to_provider(&h).await;
    assert_eq!(sent.len(), 1);
    let text = sent[0].to_string();
    assert!(!text.contains("example.com"), "{text}");
    assert_eq!(text.matches("[REDACTED:EMAIL]").count(), 4, "{text}");
}

#[tokio::test]
async fn a_route_guardrail_applies_only_to_that_route() {
    let h = harness("openai").await;
    mount_chat(&h, completion("hello")).await;
    let g = guardrail(
        &h,
        "route-only",
        vec![word_rule("w", "swordfish", "block", "input")],
        false,
    )
    .await;
    let route = route_to_m(&h, false).await;
    let mut tx = h.store.begin().await.unwrap();
    tx.replace_route_guardrails(route, &[g]).await.unwrap();
    tx.commit().await.unwrap();
    h.state.refresh().await.unwrap();
    let on_route =
        json!({ "model": "r", "messages": [{ "role": "user", "content": "swordfish" }] });
    let (s, _) = post_chat(&h.app, Some(&h.key), &on_route.to_string()).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let direct = chat_body("swordfish");
    let (s, body) = post_chat(&h.app, Some(&h.key), &direct).await;
    assert_eq!(s, StatusCode::OK, "{body}");
}

async fn route_to_m(h: &Harness, cache: bool) -> i64 {
    route_to_m_scoped(h, cache, CacheScope::Key).await
}

async fn route_to_m_scoped(h: &Harness, cache: bool, scope: CacheScope) -> i64 {
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
    if cache {
        tx.set_route_cache(
            id,
            &ultrafast_gateway::cache::RouteCache {
                enabled: true,
                ttl_s: 300,
                scope,
            },
        )
        .await
        .unwrap();
    }
    tx.commit().await.unwrap();
    h.state.refresh().await.unwrap();
    id
}

#[tokio::test]
async fn embeddings_input_is_redacted_before_the_provider_sees_it() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .and(path("/embeddings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list", "model": "m",
            "data": [{ "object": "embedding", "index": 0, "embedding": [0.5] },
                     { "object": "embedding", "index": 1, "embedding": [0.25] }],
            "usage": { "prompt_tokens": 6, "total_tokens": 6 }
        })))
        .mount(&h.upstream)
        .await;
    guardrail(&h, "pii", vec![email_rule("redact", "both")], true).await;
    let bearer = format!("Bearer {}", h.key);
    let (s, _, body) = post_to(
        &h.app,
        "/v1/embeddings",
        &[("authorization", &bearer)],
        &json!({ "model": "p/m", "input": [format!("write {EMAIL}"), "plain"] }).to_string(),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{body}");
    let sent = sent_to_provider(&h).await;
    assert_eq!(sent[0]["input"], json!(["write [REDACTED:EMAIL]", "plain"]));
}

#[tokio::test]
async fn a_blocked_embeddings_input_is_refused() {
    let h = harness("openai").await;
    guardrail(
        &h,
        "g",
        vec![word_rule("w", "swordfish", "block", "input")],
        true,
    )
    .await;
    let bearer = format!("Bearer {}", h.key);
    let (s, _, body) = post_to(
        &h.app,
        "/v1/embeddings",
        &[("authorization", &bearer)],
        &json!({ "model": "p/m", "input": ["fine", "swordfish"] }).to_string(),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(json_of(&body)["error"]["code"], "guardrail_blocked");
    assert!(sent_to_provider(&h).await.is_empty());
}

// ---- output, whole answers --------------------------------------------------

#[tokio::test]
async fn a_whole_answer_is_redacted() {
    let h = harness("openai").await;
    mount_chat(&h, completion(&format!("write to {EMAIL} now"))).await;
    guardrail(&h, "pii", vec![email_rule("redact", "output")], true).await;
    let (s, body) = post_chat(&h.app, Some(&h.key), &chat_body("hi")).await;
    assert_eq!(s, StatusCode::OK, "{body}");
    let v = json_of(&body);
    assert_eq!(
        v["choices"][0]["message"]["content"],
        "write to [REDACTED:EMAIL] now"
    );
    assert_eq!(v["choices"][0]["finish_reason"], "stop");
}

#[tokio::test]
async fn a_blocked_whole_answer_is_empty_with_content_filter_and_no_tool_calls() {
    let h = harness("openai").await;
    mount_chat(
        &h,
        ResponseTemplate::new(200).set_body_json(json!({
            "id": "c1", "model": "m",
            "choices": [{ "message": { "role": "assistant", "content": "all fine",
                "tool_calls": [{ "id": "t", "type": "function",
                    "function": { "name": "f", "arguments": "{\"q\":\"swordfish\"}" } }] },
                "finish_reason": "tool_calls" }],
            "usage": { "prompt_tokens": 9, "completion_tokens": 4 }
        })),
    )
    .await;
    guardrail(
        &h,
        "g",
        vec![word_rule("w", "swordfish", "block", "output")],
        true,
    )
    .await;
    let (s, body) = post_chat(&h.app, Some(&h.key), &chat_body("hi")).await;
    assert_eq!(s, StatusCode::OK, "{body}");
    let v = json_of(&body);
    let message = &v["choices"][0]["message"];
    assert_eq!(message["content"], "");
    assert!(message["tool_calls"].is_null() || message["tool_calls"] == json!([]));
    assert_eq!(v["choices"][0]["finish_reason"], "content_filter");
    assert!(!body.contains("swordfish"));
}

#[tokio::test]
async fn the_cache_holds_the_redacted_answer() {
    let h = harness("openai").await;
    mount_chat(&h, completion(&format!("write to {EMAIL} now"))).await;
    guardrail(&h, "pii", vec![email_rule("redact", "output")], true).await;
    route_to_m(&h, true).await;
    let body =
        json!({ "model": "r", "messages": [{ "role": "user", "content": "hi" }] }).to_string();
    let (s1, first) = post_chat(&h.app, Some(&h.key), &body).await;
    let (s2, second) = post_chat(&h.app, Some(&h.key), &body).await;
    assert_eq!((s1, s2), (StatusCode::OK, StatusCode::OK));
    for text in [&first, &second] {
        let v = json_of(text);
        assert_eq!(
            v["choices"][0]["message"]["content"],
            "write to [REDACTED:EMAIL] now"
        );
    }
    // The second was a hit: one provider call.
    assert_eq!(sent_to_provider(&h).await.len(), 1);
}

#[tokio::test]
async fn keys_with_different_guardrails_do_not_share_cached_answers() {
    let h = harness("openai").await;
    mount_chat(&h, completion(&format!("write to {EMAIL} now"))).await;
    let g = guardrail(&h, "pii", vec![email_rule("redact", "output")], false).await;
    route_to_m_scoped(&h, true, CacheScope::Team).await;
    // Two keys of one team share a cache; only one carries the guardrail.
    let team = common::seed_team(&h.store, "T", &[]).await;
    let plain = ultrafast_gateway::secrets::generate_key();
    let strict = ultrafast_gateway::secrets::generate_key();
    let mut tx = h.store.begin().await.unwrap();
    tx.insert_key("plain", &plain.hash, &plain.display, None, None, Some(team))
        .await
        .unwrap();
    let id = tx
        .insert_key(
            "strict",
            &strict.hash,
            &strict.display,
            None,
            None,
            Some(team),
        )
        .await
        .unwrap();
    tx.replace_key_guardrails(id, &[g]).await.unwrap();
    tx.commit().await.unwrap();
    h.state.refresh().await.unwrap();
    let body =
        json!({ "model": "r", "messages": [{ "role": "user", "content": "hi" }] }).to_string();
    let (_, plain_answer) = post_chat(&h.app, Some(&plain.full), &body).await;
    let (_, redacted) = post_chat(&h.app, Some(&strict.full), &body).await;
    assert!(plain_answer.contains(EMAIL), "{plain_answer}");
    assert!(!redacted.contains(EMAIL), "{redacted}");
    assert!(redacted.contains("[REDACTED:EMAIL]"));
}

// ---- output, streams --------------------------------------------------------

#[tokio::test]
async fn a_stream_is_redacted_across_deltas() {
    let h = harness("openai").await;
    let upstream = format!(
        "{}{}{}{}{}",
        delta("write to ad"),
        delta("a@exam"),
        delta("ple.com now"),
        delta(" bye"),
        stream_end()
    );
    mount_chat(&h, sse(&upstream)).await;
    guardrail(&h, "pii", vec![email_rule("redact", "output")], true).await;
    let body = json!({ "model": "p/m", "stream": true,
        "messages": [{ "role": "user", "content": "hi" }] })
    .to_string();
    let (s, text) = post_chat(&h.app, Some(&h.key), &body).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(openai_text(&text), "write to [REDACTED:EMAIL] now bye");
    assert_eq!(finish_reason(&text).as_deref(), Some("stop"));
    assert!(text.ends_with("data: [DONE]\n\n"));
}

fn long_clean() -> String {
    "all good here. ".repeat(60)
}

#[tokio::test]
async fn a_stream_block_ends_the_stream_with_content_filter_openai() {
    let h = harness("openai").await;
    let upstream = format!(
        "{}{}{}{}",
        delta(&long_clean()),
        delta("then forbi"),
        delta("dden words follow"),
        stream_end()
    );
    mount_chat(&h, sse(&upstream)).await;
    guardrail(
        &h,
        "g",
        vec![word_rule("w", "forbidden", "block", "output")],
        true,
    )
    .await;
    let body = json!({ "model": "p/m", "stream": true,
        "messages": [{ "role": "user", "content": "hi" }] })
    .to_string();
    let (s, text) = post_chat(&h.app, Some(&h.key), &body).await;
    assert_eq!(s, StatusCode::OK);
    let released = openai_text(&text);
    assert!(released.starts_with("all good here."), "{released}");
    assert!(!released.contains("forbi"), "{released}");
    assert!(!text.contains("dden"), "{text}");
    assert_eq!(finish_reason(&text).as_deref(), Some("content_filter"));
    assert!(text.ends_with("data: [DONE]\n\n"));
}

#[tokio::test]
async fn a_stream_block_ends_the_stream_with_refusal_anthropic() {
    let h = harness("openai").await;
    let upstream = format!(
        "{}{}{}",
        delta(&long_clean()),
        delta("then forbidden words"),
        stream_end()
    );
    mount_chat(&h, sse(&upstream)).await;
    guardrail(
        &h,
        "g",
        vec![word_rule("w", "forbidden", "block", "output")],
        true,
    )
    .await;
    let (s, text) = messages(
        &h,
        &json!({ "model": "p/m", "max_tokens": 30, "stream": true,
                 "messages": [{ "role": "user", "content": "hi" }] }),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert!(!text.contains("forbidden"), "{text}");
    assert!(text.contains("\"stop_reason\":\"refusal\""), "{text}");
    assert!(text.contains("event: message_stop"), "{text}");
}

#[tokio::test]
async fn tool_call_arguments_in_a_stream_are_scanned_per_call() {
    let h = harness("openai").await;
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
    guardrail(&h, "pii", vec![email_rule("redact", "output")], true).await;
    let body = json!({ "model": "p/m", "stream": true,
        "messages": [{ "role": "user", "content": "hi" }] })
    .to_string();
    let (s, text) = post_chat(&h.app, Some(&h.key), &body).await;
    assert_eq!(s, StatusCode::OK);
    let arguments: String = openai_payloads(&text)
        .iter()
        .filter_map(|v| v["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"].as_str())
        .collect();
    assert_eq!(arguments, "{\"to\":\"[REDACTED:EMAIL]\"}", "{text}");
}

// ---- what the record says ---------------------------------------------------

use ultrafast_gateway::guardrails::log::{GuardrailLog, LoggedAction};
use ultrafast_gateway::logs::{row_of, PriceLookup};

fn no_prices() -> PriceLookup {
    std::sync::Arc::new(|_, _| None)
}

#[tokio::test]
async fn the_record_has_ids_names_actions_and_counts_but_no_matched_text() {
    let h = harness("openai").await;
    mount_chat(&h, completion(&format!("reply to {EMAIL} and {EMAIL}"))).await;
    let id = guardrail(&h, "pii", vec![email_rule("redact", "both")], true).await;
    let (s, _) = post_chat(&h.app, Some(&h.key), &chat_body(&format!("mail {EMAIL}"))).await;
    assert_eq!(s, StatusCode::OK);
    let records = h.sink.wait_for(1).await;
    let g = records[0].guardrails.clone().expect("a record");
    assert_eq!(g.action, LoggedAction::Redacted);
    let input = g.input.as_ref().unwrap();
    assert_eq!(input.action, LoggedAction::Redacted);
    assert_eq!(input.redactions.get("EMAIL"), Some(&1));
    assert_eq!(
        (
            input.checked_with[0].id,
            input.checked_with[0].name.as_str()
        ),
        (id, "pii")
    );
    let output = g.output.as_ref().unwrap();
    assert_eq!(output.redactions.get("EMAIL"), Some(&2));

    // What is stored holds neither the address nor any part of it.
    let row = row_of(&records[0], &no_prices());
    h.store.insert_logs(&[row]).await.unwrap();
    let stored = h.store.recent_logs(10).await.unwrap();
    let dump = format!("{stored:?}");
    assert!(!dump.contains("ada"), "{dump}");
    assert!(!dump.contains("example.com"), "{dump}");
    let text = stored[0].guardrails.clone().unwrap();
    let back = GuardrailLog::from_stored(&text).unwrap();
    assert_eq!(back, g);
    assert!(text.contains("\"name\":\"pii\""), "{text}");
}

#[tokio::test]
async fn a_blocked_call_is_recorded_with_its_outcome_and_no_provider_attempt() {
    let h = harness("openai").await;
    mount_chat(&h, completion("hello")).await;
    let id = guardrail(
        &h,
        "no-secrets",
        vec![word_rule("w", "swordfish", "block", "input")],
        true,
    )
    .await;
    let (s, _) = post_chat(&h.app, Some(&h.key), &chat_body("swordfish")).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let records = h.sink.wait_for(1).await;
    let r = &records[0];
    assert_eq!(r.status, 400);
    assert!(r.usage.is_none());
    assert!(
        r.attempts.is_empty()
            || r.attempts
                .iter()
                .all(|a| a.provider.is_empty() || a.status.is_none())
    );
    let g = r.guardrails.as_ref().unwrap();
    assert_eq!(g.action, LoggedAction::Blocked);
    let by = g.input.as_ref().unwrap().blocked_by.as_ref().unwrap();
    assert_eq!((by.id, by.name.as_str()), (id, "no-secrets"));
    assert!(g.output.is_none());
}

#[tokio::test]
async fn a_flag_rule_lets_the_call_through_and_is_recorded() {
    let h = harness("openai").await;
    mount_chat(&h, completion("hello")).await;
    guardrail(
        &h,
        "watch",
        vec![word_rule("w", "swordfish", "flag", "input")],
        true,
    )
    .await;
    let (s, _) = post_chat(&h.app, Some(&h.key), &chat_body("swordfish")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        sent_to_provider(&h).await[0]["messages"][0]["content"],
        "swordfish"
    );
    let records = h.sink.wait_for(1).await;
    let g = records[0].guardrails.as_ref().unwrap();
    assert_eq!(g.action, LoggedAction::Flagged);
    assert_eq!(g.input.as_ref().unwrap().flags[0].rule_id, "w");
}

#[tokio::test]
async fn a_call_nothing_matched_has_no_guardrail_record() {
    let h = harness("openai").await;
    mount_chat(&h, completion("hello")).await;
    guardrail(&h, "pii", vec![email_rule("redact", "both")], true).await;
    let (s, _) = post_chat(&h.app, Some(&h.key), &chat_body("nothing here")).await;
    assert_eq!(s, StatusCode::OK);
    assert!(h.sink.wait_for(1).await[0].guardrails.is_none());
}

#[tokio::test]
async fn a_stream_is_recorded_with_its_redactions_and_a_cut_stream_with_an_estimate() {
    let h = harness("openai").await;
    let upstream = format!(
        "{}{}{}",
        delta("write to ada@example.com now"),
        delta(" bye"),
        stream_end()
    );
    mount_chat(&h, sse(&upstream)).await;
    guardrail(&h, "pii", vec![email_rule("redact", "output")], true).await;
    let body = json!({ "model": "p/m", "stream": true,
        "messages": [{ "role": "user", "content": "hi" }] })
    .to_string();
    let (s, _) = post_chat(&h.app, Some(&h.key), &body).await;
    assert_eq!(s, StatusCode::OK);
    let records = h.sink.wait_for(1).await;
    let g = records[0].guardrails.as_ref().unwrap();
    assert_eq!(g.output.as_ref().unwrap().redactions.get("EMAIL"), Some(&1));
    assert!(!records[0].estimated);
    assert_eq!(records[0].usage.unwrap().output_tokens, 3);

    // A cut stream: the provider's report never came, so the call is charged
    // an estimate of what was generated.
    let h = harness("openai").await;
    let upstream = format!(
        "{}{}{}{}",
        delta(&long_clean()),
        delta("then forbidden words"),
        delta(&long_clean()),
        stream_end()
    );
    mount_chat(&h, sse(&upstream)).await;
    guardrail(
        &h,
        "g",
        vec![word_rule("w", "forbidden", "block", "output")],
        true,
    )
    .await;
    let (s, _) = post_chat(&h.app, Some(&h.key), &body).await;
    assert_eq!(s, StatusCode::OK);
    let records = h.sink.wait_for(1).await;
    let r = &records[0];
    assert_eq!(r.status, 200);
    assert_eq!(r.guardrails.as_ref().unwrap().action, LoggedAction::Blocked);
    assert!(r.estimated);
    assert!(r.usage.unwrap().output_tokens > 0);
}

// ---- other shapes -----------------------------------------------------------

#[tokio::test]
async fn a_blocked_whole_answer_ends_with_refusal_for_an_anthropic_caller() {
    let h = harness("openai").await;
    mount_chat(&h, completion("the swordfish is here")).await;
    guardrail(
        &h,
        "g",
        vec![word_rule("w", "swordfish", "block", "output")],
        true,
    )
    .await;
    let (s, text) = messages(
        &h,
        &json!({ "model": "p/m", "max_tokens": 20,
                 "messages": [{ "role": "user", "content": "hi" }] }),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{text}");
    let v = json_of(&text);
    assert_eq!(v["stop_reason"], "refusal");
    assert!(!text.contains("swordfish"));
}

#[tokio::test]
async fn a_large_input_is_checked_off_the_runtime_and_redacted_all_the_same() {
    let h = harness("openai").await;
    mount_chat(&h, completion("hello")).await;
    guardrail(&h, "pii", vec![email_rule("redact", "input")], true).await;
    let big = format!("{} {EMAIL} end", "lorem ipsum ".repeat(8_000));
    assert!(big.len() > 64 * 1024);
    let (s, _) = post_chat(&h.app, Some(&h.key), &chat_body(&big)).await;
    assert_eq!(s, StatusCode::OK);
    let sent = sent_to_provider(&h).await[0].to_string();
    assert!(!sent.contains("example.com"));
    assert!(sent.contains("[REDACTED:EMAIL] end"));
}

#[tokio::test]
async fn guardrails_of_a_key_apply_to_its_calls_only() {
    let h = harness("openai").await;
    mount_chat(&h, completion("hello")).await;
    let g = guardrail(
        &h,
        "k",
        vec![word_rule("w", "swordfish", "block", "input")],
        false,
    )
    .await;
    let other = ultrafast_gateway::secrets::generate_key();
    let mut tx = h.store.begin().await.unwrap();
    let id = tx
        .insert_key("other", &other.hash, &other.display, None, None, None)
        .await
        .unwrap();
    tx.replace_key_guardrails(id, &[g]).await.unwrap();
    tx.commit().await.unwrap();
    h.state.refresh().await.unwrap();
    let (s, _) = post_chat(&h.app, Some(&other.full), &chat_body("swordfish")).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = post_chat(&h.app, Some(&h.key), &chat_body("swordfish")).await;
    assert_eq!(s, StatusCode::OK);
}

// ---- fix round 1: parts, system blocks, names --------------------------------

#[tokio::test]
async fn a_match_split_across_text_parts_is_redacted() {
    let h = harness("openai").await;
    mount_chat(&h, completion("hello")).await;
    guardrail(&h, "pii", vec![email_rule("redact", "input")], true).await;
    let body = json!({ "model": "p/m", "messages": [{ "role": "user", "content": [
        { "type": "text", "text": "mail ada@exam" },
        { "type": "text", "text": "ple.com now" } ] }] })
    .to_string();
    let (s, answer) = post_chat(&h.app, Some(&h.key), &body).await;
    assert_eq!(s, StatusCode::OK, "{answer}");
    let sent = sent_to_provider(&h).await[0].to_string();
    assert!(
        !sent.contains("example.com") && !sent.contains("ada@"),
        "{sent}"
    );
    assert!(sent.contains("[REDACTED:EMAIL]"), "{sent}");
}

#[tokio::test]
async fn a_keyword_split_across_text_parts_blocks() {
    let h = harness("openai").await;
    mount_chat(&h, completion("hello")).await;
    guardrail(
        &h,
        "g",
        vec![word_rule("w", "swordfish", "block", "input")],
        true,
    )
    .await;
    let body = json!({ "model": "p/m", "messages": [{ "role": "user", "content": [
        { "type": "text", "text": "the sword" }, { "type": "text", "text": "fish" } ] }] })
    .to_string();
    let (s, _) = post_chat(&h.app, Some(&h.key), &body).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(sent_to_provider(&h).await.is_empty());
}

#[tokio::test]
async fn images_between_text_parts_keep_their_position() {
    let h = harness("openai").await;
    mount_chat(&h, completion("hello")).await;
    guardrail(&h, "pii", vec![email_rule("redact", "input")], true).await;
    let img = "https://x.test/a.png";
    let body = json!({ "model": "p/m", "messages": [{ "role": "user", "content": [
        { "type": "text", "text": "a ada@exam" }, { "type": "text", "text": "ple.com" },
        { "type": "image_url", "image_url": { "url": img } },
        { "type": "text", "text": "tail" } ] }] })
    .to_string();
    let (s, answer) = post_chat(&h.app, Some(&h.key), &body).await;
    assert_eq!(s, StatusCode::OK, "{answer}");
    let content = sent_to_provider(&h).await[0]["messages"][0]["content"].clone();
    let parts = content.as_array().unwrap();
    assert_eq!(parts.len(), 3, "{content}");
    assert_eq!(parts[0]["text"], "a [REDACTED:EMAIL]");
    assert_eq!(parts[1]["image_url"]["url"], img);
    assert_eq!(parts[2]["text"], "tail");
}

#[tokio::test]
async fn system_blocks_are_seen_apart_and_names_are_scanned() {
    let h = harness("openai").await;
    mount_chat(&h, completion("hello")).await;
    guardrail(&h, "pii", vec![email_rule("redact", "input")], true).await;
    let (s, text) = messages(
        &h,
        &json!({ "model": "p/m", "max_tokens": 20,
            "system": [{ "type": "text", "text": "ask a@example.com" },
                       { "type": "text", "text": "sys2 b@example.com" }],
            "messages": [{ "role": "user", "content": "hi" }] }),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{text}");
    let sent = sent_to_provider(&h).await[0].to_string();
    assert!(!sent.contains("example.com"), "{sent}");
    assert_eq!(sent.matches("[REDACTED:EMAIL]").count(), 2, "{sent}");

    let body = json!({ "model": "p/m", "messages": [
        { "role": "user", "name": "zed@example.com", "content": "hi" }] })
    .to_string();
    let (s, _) = post_chat(&h.app, Some(&h.key), &body).await;
    assert_eq!(s, StatusCode::OK);
    let sent = sent_to_provider(&h).await[1].to_string();
    assert!(!sent.contains("example.com"), "{sent}");
}
