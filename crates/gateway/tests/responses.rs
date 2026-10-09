//! `POST /v1/responses`: the OpenAI Responses API (stateless) over every
//! provider kind, streamed and not, with tools, guardrails and the cache.

mod common;

use axum::http::StatusCode;
use common::{harness, post_to, seed_team, seed_user, Harness};
use serde_json::{json, Value};
use ultrafast_gateway::cache::{CacheScope, RouteCache};
use ultrafast_gateway::store::{NewGuardrail, RouteSettings, TargetsInput};
use wiremock::matchers::{body_partial_json, method, path, path_regex};
use wiremock::{Mock, ResponseTemplate};

async fn responses(h: &Harness, body: &Value) -> (StatusCode, axum::http::HeaderMap, String) {
    let bearer = format!("Bearer {}", h.key);
    post_to(
        &h.app,
        "/v1/responses",
        &[("authorization", &bearer)],
        &body.to_string(),
    )
    .await
}

fn json_of(body: &str) -> Value {
    serde_json::from_str(body).unwrap_or_else(|e| panic!("{e}: {body}"))
}

fn sse(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(body.to_string(), "text/event-stream")
}

fn events(body: &str) -> Vec<(String, Value)> {
    body.split("\n\n")
        .filter(|e| !e.trim().is_empty())
        .map(|e| {
            let name = e.lines().find_map(|l| l.strip_prefix("event: ")).unwrap();
            let data = e.lines().find_map(|l| l.strip_prefix("data: ")).unwrap();
            (name.to_string(), json_of(data))
        })
        .collect()
}

fn names(evs: &[(String, Value)]) -> Vec<&str> {
    evs.iter().map(|(n, _)| n.as_str()).collect()
}

fn openai_ok(content: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "c1", "model": "m",
        "choices": [{ "message": { "role": "assistant", "content": content }, "finish_reason": "stop" }],
        "usage": { "prompt_tokens": 4, "completion_tokens": 2 }
    }))
}

const OPENAI_TEXT_AND_CALLS: &str = concat!(
    "data: {\"choices\":[{\"delta\":{\"content\":\"Let me \"},\"finish_reason\":null}]}\n\n",
    "data: {\"choices\":[{\"delta\":{\"content\":\"check\"},\"finish_reason\":null}]}\n\n",
    "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_a\",\"type\":\"function\",\"function\":{\"name\":\"weather\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n",
    "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"city\\\":\"}}]},\"finish_reason\":null}]}\n\n",
    "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"Oslo\\\"}\"}}]},\"finish_reason\":null}]}\n\n",
    "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"id\":\"call_b\",\"type\":\"function\",\"function\":{\"name\":\"time\",\"arguments\":\"{}\"}}]},\"finish_reason\":null}]}\n\n",
    "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
    "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":9}}\n\n",
    "data: [DONE]\n\n",
);

/// What a stream of text then two calls must look like, whatever the provider.
fn assert_text_and_two_calls(text: &str) {
    let evs = events(text);
    let n = names(&evs);
    assert_eq!(
        n[..2],
        ["response.created", "response.in_progress"],
        "{n:?}"
    );
    assert_eq!(*n.last().unwrap(), "response.completed", "{n:?}");
    for (i, (_, v)) in evs.iter().enumerate() {
        assert_eq!(v["sequence_number"], i, "{v}");
    }
    let delta: String = evs
        .iter()
        .filter(|(n, _)| n == "response.output_text.delta")
        .map(|(_, v)| v["delta"].as_str().unwrap())
        .collect();
    assert_eq!(delta, "Let me check");
    assert_eq!(
        evs.iter()
            .filter(|(n, _)| n == "response.output_item.added")
            .count(),
        3
    );
    let fin = &evs.last().unwrap().1["response"];
    let out = fin["output"].as_array().unwrap();
    assert_eq!(out.len(), 3, "{fin}");
    assert_eq!(out[0]["content"][0]["text"], "Let me check");
    assert_eq!(out[1]["name"], "weather");
    assert_eq!(out[1]["call_id"], "call_a");
    assert_eq!(out[1]["arguments"], "{\"city\":\"Oslo\"}");
    assert_eq!(out[2]["name"], "time");
    assert!(!text.contains("[DONE]"));
}

const BODY: &str =
    r#"{"model":"p/m","instructions":"be brief","input":"hi","max_output_tokens":20}"#;

#[tokio::test]
async fn an_openai_provider_serves_a_response() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_partial_json(json!({
            "model": "m", "max_tokens": 20,
            "messages": [
                { "role": "system", "content": "be brief" },
                { "role": "user", "content": "hi" }
            ]
        })))
        .respond_with(openai_ok("hello"))
        .expect(1)
        .mount(&h.upstream)
        .await;
    let (status, headers, body) = responses(&h, &json_of(BODY)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(headers["content-type"].to_str().unwrap().contains("json"));
    let v = json_of(&body);
    assert_eq!(v["object"], "response");
    let id = v["id"].as_str().unwrap();
    assert!(id.starts_with("resp_") && id.len() == 5 + 24, "{id}");
    assert_eq!(v["status"], "completed");
    assert_eq!(v["max_output_tokens"], 20);
    assert_eq!(v["output"][0]["type"], "message");
    assert_eq!(v["output"][0]["content"][0]["text"], "hello");
    assert_eq!(v["usage"]["total_tokens"], 6);
    let r = &h.sink.records()[0];
    assert_eq!(r.endpoint, "responses");
    assert_eq!(r.status, 200);
}

#[tokio::test]
async fn an_anthropic_provider_serves_a_response() {
    let h = harness("anthropic").await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(body_partial_json(json!({
            "system": "be brief",
            "messages": [{ "role": "user", "content": "hi" }]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "msg_9", "type": "message", "role": "assistant", "model": "m",
            "content": [{ "type": "text", "text": "salut" }],
            "stop_reason": "max_tokens",
            "usage": { "input_tokens": 5, "output_tokens": 3 }
        })))
        .expect(1)
        .mount(&h.upstream)
        .await;
    let (status, _, body) = responses(&h, &json_of(BODY)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let v = json_of(&body);
    assert_eq!(v["output"][0]["content"][0]["text"], "salut");
    assert_eq!(v["status"], "incomplete");
    assert_eq!(v["incomplete_details"]["reason"], "max_output_tokens");
    assert_eq!(v["usage"]["input_tokens"], 5);
}

fn gemini_chunk(text: &str, finish: Option<&str>) -> Value {
    let mut c = json!({ "content": { "role": "model", "parts": [{ "text": text }] } });
    if let Some(f) = finish {
        c["finishReason"] = json!(f);
    }
    json!({ "candidates": [c],
            "usageMetadata": { "promptTokenCount": 3, "candidatesTokenCount": 2 } })
}

#[tokio::test]
async fn a_gemini_provider_serves_a_response() {
    let h = harness("gemini").await;
    Mock::given(method("POST"))
        .and(path_regex(r":generateContent$"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(gemini_chunk("bonjour", Some("STOP"))),
        )
        .expect(1)
        .mount(&h.upstream)
        .await;
    let (status, _, body) = responses(&h, &json_of(BODY)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let v = json_of(&body);
    assert_eq!(v["output"][0]["content"][0]["text"], "bonjour");
    assert_eq!(v["status"], "completed");
}

#[tokio::test]
async fn a_stream_from_an_openai_provider_has_ordered_events() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(sse(OPENAI_TEXT_AND_CALLS))
        .mount(&h.upstream)
        .await;
    let body = json!({"model":"p/m","input":"x","stream":true});
    let (status, headers, text) = responses(&h, &body).await;
    assert_eq!(status, StatusCode::OK, "{text}");
    assert_eq!(headers["content-type"], "text/event-stream");
    assert_text_and_two_calls(&text);
    let r = h.sink.wait_for(1).await;
    assert_eq!(r[0].endpoint, "responses");
    let usage = r[0].usage.unwrap();
    assert_eq!((usage.input_tokens, usage.output_tokens), (7, 9));
}

#[tokio::test]
async fn a_stream_from_an_anthropic_provider_has_ordered_events() {
    let h = harness("anthropic").await;
    let upstream = concat!(
        "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":3,\"output_tokens\":0}}}\n\n",
        "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Let me \"}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"check\"}}\n\n",
        "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"call_a\",\"name\":\"weather\",\"input\":{}}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"city\\\":\"}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"Oslo\\\"}\"}}\n\n",
        "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
        "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":2,\"content_block\":{\"type\":\"tool_use\",\"id\":\"call_b\",\"name\":\"time\",\"input\":{}}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":2,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{}\"}}\n\n",
        "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":2}\n\n",
        "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":9}}\n\n",
        "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
    );
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(sse(upstream))
        .mount(&h.upstream)
        .await;
    let (status, _, text) = responses(&h, &json!({"model":"p/m","input":"x","stream":true})).await;
    assert_eq!(status, StatusCode::OK, "{text}");
    assert_text_and_two_calls(&text);
}

#[tokio::test]
async fn a_stream_from_a_gemini_provider_has_ordered_events() {
    let h = harness("gemini").await;
    let part = |p: Value| {
        format!(
            "data: {}\n\n",
            json!({"candidates":[{"content":{"role":"model","parts":[p]}}]})
        )
    };
    let upstream = format!(
        "{}{}{}{}data: {}\n\n",
        part(json!({"text":"Let me "})),
        part(json!({"text":"check"})),
        part(json!({"functionCall":{"name":"weather","args":{"city":"Oslo"}}})),
        part(json!({"functionCall":{"name":"time","args":{}}})),
        json!({"candidates":[{"content":{"role":"model","parts":[{"text":""}]},"finishReason":"STOP"}],
               "usageMetadata":{"promptTokenCount":3,"candidatesTokenCount":9}})
    );
    Mock::given(method("POST"))
        .and(path_regex(r":streamGenerateContent$"))
        .respond_with(sse(&upstream))
        .mount(&h.upstream)
        .await;
    let (status, _, text) = responses(&h, &json!({"model":"p/m","input":"x","stream":true})).await;
    assert_eq!(status, StatusCode::OK, "{text}");
    let evs = events(&text);
    // Gemini gives its calls no id: the gateway makes them up. Names and
    // arguments are what must arrive.
    let fin = &evs.last().unwrap().1["response"];
    assert_eq!(fin["output"][0]["content"][0]["text"], "Let me check");
    assert_eq!(fin["output"][1]["name"], "weather");
    assert_eq!(fin["output"][1]["arguments"], "{\"city\":\"Oslo\"}");
    assert_eq!(fin["output"][2]["name"], "time");
    for (i, (_, v)) in evs.iter().enumerate() {
        assert_eq!(v["sequence_number"], i);
    }
}

#[tokio::test]
async fn a_tool_round_trip_reaches_the_provider_as_chat_messages() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_partial_json(json!({
            "messages": [
                { "role": "user", "content": "weather in Oslo?" },
                { "role": "assistant", "tool_calls": [{
                    "id": "call_1", "type": "function",
                    "function": { "name": "weather", "arguments": "{\"city\":\"Oslo\"}" }
                }]},
                { "role": "tool", "tool_call_id": "call_1", "content": "rain" }
            ],
            "tools": [{ "type": "function", "function": { "name": "weather" } }],
            "tool_choice": "auto"
        })))
        .respond_with(openai_ok("It rains."))
        .expect(1)
        .mount(&h.upstream)
        .await;
    let body = json!({
        "model": "p/m",
        "tools": [{ "type": "function", "name": "weather", "description": "d",
                    "parameters": { "type": "object", "properties": { "city": { "type": "string" } } } }],
        "tool_choice": "auto",
        "input": [
            { "role": "user", "content": "weather in Oslo?" },
            { "type": "function_call", "call_id": "call_1", "name": "weather",
              "arguments": "{\"city\":\"Oslo\"}" },
            { "type": "function_call_output", "call_id": "call_1", "output": "rain" }
        ]
    });
    let (status, _, text) = responses(&h, &body).await;
    assert_eq!(status, StatusCode::OK, "{text}");
    let v = json_of(&text);
    assert_eq!(v["output"][0]["content"][0]["text"], "It rains.");
    assert_eq!(v["tools"][0]["name"], "weather");
}

#[tokio::test]
async fn a_call_the_model_makes_comes_back_as_a_function_call_item() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "c1", "model": "m",
            "choices": [{ "message": { "role": "assistant", "content": null, "tool_calls": [{
                "id": "call_7", "type": "function",
                "function": { "name": "weather", "arguments": "{\"city\":\"Rome\"}" } }] },
                "finish_reason": "tool_calls" }],
            "usage": { "prompt_tokens": 4, "completion_tokens": 2 }
        })))
        .mount(&h.upstream)
        .await;
    let (_, _, text) = responses(&h, &json!({"model":"p/m","input":"x"})).await;
    let v = json_of(&text);
    assert_eq!(v["output"].as_array().unwrap().len(), 1);
    assert_eq!(v["output"][0]["type"], "function_call");
    assert_eq!(v["output"][0]["call_id"], "call_7");
    assert_eq!(v["output"][0]["arguments"], "{\"city\":\"Rome\"}");
}

#[tokio::test]
async fn text_format_reaches_the_provider_as_a_response_format() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .and(body_partial_json(json!({ "response_format": {
            "type": "json_schema",
            "json_schema": { "name": "out", "schema": { "type": "object" }, "strict": true }
        }})))
        .respond_with(openai_ok("{}"))
        .expect(1)
        .mount(&h.upstream)
        .await;
    let body = json!({"model":"p/m","input":"x","text":{"format":{
        "type":"json_schema","name":"out","schema":{"type":"object"},"strict":true}}});
    let (status, _, text) = responses(&h, &body).await;
    assert_eq!(status, StatusCode::OK, "{text}");
}

#[tokio::test]
async fn refusals_are_openai_errors_and_no_provider_is_called() {
    let h = harness("openai").await;
    let cases = [
        (
            json!({"store":true}),
            "store is not supported; the gateway keeps no responses",
        ),
        (
            json!({"previous_response_id":"resp_1"}),
            "previous_response_id",
        ),
        (json!({"conversation":"c"}), "conversation"),
        (json!({"background":true}), "background"),
        (json!({"tools":[{"type":"web_search"}]}), "web_search"),
        (
            json!({"prompt":{"id":"greet"}}),
            "prompt templates are not available yet",
        ),
        (
            json!({"input":[{"type":"reasoning","summary":[]}]}),
            "reasoning",
        ),
    ];
    for (extra, want) in cases {
        let mut body = json!({"model":"p/m","input":"x"});
        for (k, v) in extra.as_object().unwrap() {
            body[k] = v.clone();
        }
        let (status, _, text) = responses(&h, &body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}: {text}");
        let v = json_of(&text);
        assert_eq!(v["error"]["type"], "invalid_request_error", "{text}");
        assert!(
            v["error"]["message"].as_str().unwrap().contains(want),
            "{text}"
        );
    }
    let (s, _, text) = post_to(&h.app, "/v1/responses", &[], BODY).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert!(json_of(&text)["error"]["message"].is_string());
    let (s, _, _) = responses(&h, &json!({"model":"p/nope","input":"x"})).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert!(h.upstream.received_requests().await.unwrap().is_empty());
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
async fn a_guardrail_blocks_the_input_of_a_response_call() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(openai_ok("hello"))
        .mount(&h.upstream)
        .await;
    guardrail(
        &h,
        "no-secrets",
        json!([{ "id": "w", "matcher": { "keywords": { "words": ["swordfish"] } },
                 "action": "block", "directions": "input" }]),
    )
    .await;
    // In a plain string, in an item's text, and in a tool result.
    for input in [
        json!("the word is swordfish"),
        json!([{"role":"user","content":[{"type":"input_text","text":"the word is swordfish"}]}]),
        json!([{"role":"user","content":"x"},
               {"type":"function_call","call_id":"c","name":"w","arguments":"{}"},
               {"type":"function_call_output","call_id":"c","output":"swordfish"}]),
    ] {
        let (status, _, body) = responses(&h, &json!({"model":"p/m","input":input})).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
        let v = json_of(&body);
        assert_eq!(v["error"]["code"], "guardrail_blocked");
        assert!(!body.contains("swordfish"));
    }
    assert!(h.upstream.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_guardrail_redacts_the_output_of_a_response_call() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(openai_ok("write to ada@example.com now"))
        .mount(&h.upstream)
        .await;
    guardrail(
        &h,
        "pii",
        json!([{ "id": "email", "matcher": { "pii": ["EMAIL"] },
                 "action": "redact", "directions": "output" }]),
    )
    .await;
    let (status, _, body) = responses(&h, &json!({"model":"p/m","input":"x"})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!body.contains("ada@example.com"), "{body}");
}

const SETTINGS: RouteSettings = RouteSettings {
    retries: 0,
    first_token_timeout_ms: 30_000,
    total_timeout_ms: 300_000,
    breaker_failures: 5,
    breaker_window_s: 60,
    breaker_open_s: 30,
};

#[tokio::test]
async fn a_cached_answer_is_served_again_as_a_new_response() {
    let h = harness("openai").await;
    let models = h.store.list_models().await.unwrap();
    let model = models.iter().find(|m| m.name == "gpt-4o").unwrap().id;
    let mut tx = h.store.begin().await.unwrap();
    let id = tx.insert_route("r", &SETTINGS, true).await.unwrap();
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
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(openai_ok("hello"))
        .mount(&h.upstream)
        .await;
    let body = json!({"model":"r","input":"hi","max_output_tokens":10,"temperature":0});
    let (s1, _, first) = responses(&h, &body).await;
    let (s2, _, second) = responses(&h, &body).await;
    assert_eq!(
        (s1, s2),
        (StatusCode::OK, StatusCode::OK),
        "{first} {second}"
    );
    let (a, b) = (json_of(&first), json_of(&second));
    assert_eq!(a["output"][0]["content"][0]["text"], "hello");
    assert_eq!(b["output"][0]["content"][0]["text"], "hello");
    assert_ne!(a["id"], b["id"]);
    assert_eq!(b["max_output_tokens"], 10);
    assert_eq!(h.upstream.received_requests().await.unwrap().len(), 1);
    // The same call on chat completions shares the entry.
    let _ = (seed_team, seed_user);
}

/// The official `openai` SDK parses what the gateway sends. Runs when a
/// Python with the SDK is found (`UF_OPENAI_PYTHON`, or the venv under
/// `~/.cache/uf-p15-oai`); the fixture-based parser tests in the translate
/// crate run everywhere.
#[tokio::test]
async fn the_official_openai_sdk_parses_the_stream() {
    let python = std::env::var("UF_OPENAI_PYTHON").unwrap_or_else(|_| {
        format!(
            "{}/.cache/uf-p15-oai/bin/python",
            std::env::var("HOME").unwrap_or_default()
        )
    });
    if !std::path::Path::new(&python).exists() {
        eprintln!("skipped: no Python with the openai SDK at {python}");
        return;
    }
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(sse(OPENAI_TEXT_AND_CALLS))
        .up_to_n_times(1)
        .mount(&h.upstream)
        .await;
    let (status, _, stream) =
        responses(&h, &json!({"model":"p/m","input":"x","stream":true})).await;
    assert_eq!(status, StatusCode::OK, "{stream}");
    h.upstream.reset().await;
    Mock::given(method("POST"))
        .respond_with(openai_ok("hello"))
        .mount(&h.upstream)
        .await;
    let (_, _, whole) = responses(&h, &json!({"model":"p/m","input":"x"})).await;
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"));
    let (sse_path, json_path) = (dir.join("responses.sse"), dir.join("responses.json"));
    std::fs::write(&sse_path, &stream).unwrap();
    std::fs::write(&json_path, &whole).unwrap();
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/responses_sdk_check.py");
    let out = std::process::Command::new(&python)
        .arg(&script)
        .arg(&sse_path)
        .arg(&json_path)
        .arg("Let me check")
        .arg(r#"[["weather", "{\"city\":\"Oslo\"}"], ["time", "{}"]]"#)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}
