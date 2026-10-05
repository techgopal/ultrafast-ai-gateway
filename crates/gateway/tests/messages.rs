mod common;

use axum::http::StatusCode;
use common::{allow_model, harness, post_to, Harness};
use serde_json::{json, Value};
use ultrafast_gateway::store::{Grants, RouteSettings, TargetsInput};
use ultrafast_gateway::telemetry::AttemptOutcome;
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, ResponseTemplate};

const BODY: &str = r#"{"model":"p/m","max_tokens":20,"system":"be brief","messages":[{"role":"user","content":[{"type":"text","text":"hi"}]}]}"#;

async fn messages(h: &Harness, body: &str) -> (StatusCode, axum::http::HeaderMap, String) {
    let bearer = format!("Bearer {}", h.key);
    post_to(
        &h.app,
        "/v1/messages",
        &[
            ("authorization", &bearer),
            ("anthropic-version", "2023-06-01"),
        ],
        body,
    )
    .await
}

fn json_of(body: &str) -> Value {
    serde_json::from_str(body).unwrap_or_else(|e| panic!("{e}: {body}"))
}

fn openai_ok() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "c1", "model": "m",
        "choices": [{ "message": { "role": "assistant", "content": "hello" }, "finish_reason": "length" }],
        "usage": { "prompt_tokens": 4, "completion_tokens": 2 }
    }))
}

fn anthropic_ok() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "msg_9", "type": "message", "role": "assistant", "model": "m",
        "content": [{ "type": "text", "text": "salut" }],
        "stop_reason": "end_turn",
        "usage": { "input_tokens": 5, "output_tokens": 3 }
    }))
}

fn sse(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(body.to_string(), "text/event-stream")
}

/// `(event name, data)` of every event of an Anthropic stream.
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

#[tokio::test]
async fn an_openai_provider_serves_a_message() {
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
        .respond_with(openai_ok())
        .expect(1)
        .mount(&h.upstream)
        .await;
    let (status, _, body) = messages(&h, BODY).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let v = json_of(&body);
    assert_eq!(v["type"], "message");
    assert_eq!(v["role"], "assistant");
    assert_eq!(v["content"], json!([{ "type": "text", "text": "hello" }]));
    assert_eq!(v["stop_reason"], "max_tokens");
    assert_eq!(v["usage"], json!({ "input_tokens": 4, "output_tokens": 2 }));
    let r = &h.sink.records()[0];
    assert_eq!(r.endpoint, "messages");
    assert_eq!(r.status, 200);
}

#[tokio::test]
async fn an_anthropic_provider_serves_a_message_with_x_api_key() {
    let h = harness("anthropic").await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", "provider-secret"))
        .respond_with(anthropic_ok())
        .expect(1)
        .mount(&h.upstream)
        .await;
    let (status, _, body) = post_to(
        &h.app,
        "/v1/messages",
        &[("x-api-key", &h.key), ("anthropic-version", "2023-06-01")],
        BODY,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let v = json_of(&body);
    assert_eq!(v["content"][0]["text"], "salut");
    assert_eq!(v["stop_reason"], "end_turn");
    assert_eq!(v["usage"], json!({ "input_tokens": 5, "output_tokens": 3 }));
    // The caller's key is not what the provider is given.
    assert!(!body.contains(&h.key));
}

#[tokio::test]
async fn x_api_key_also_works_on_chat_and_models() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(openai_ok())
        .mount(&h.upstream)
        .await;
    let chat = r#"{"model":"p/m","messages":[{"role":"user","content":"hi"}]}"#;
    let (s, _, _) = post_to(
        &h.app,
        "/v1/chat/completions",
        &[("x-api-key", &h.key)],
        chat,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let resp = tower::ServiceExt::oneshot(
        h.app.clone(),
        axum::http::Request::builder()
            .uri("/v1/models")
            .header("x-api-key", &h.key)
            .body(axum::body::Body::empty())
            .unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let (s, _, _) = post_to(
        &h.app,
        "/v1/chat/completions",
        &[("x-api-key", "uf-sk-nope")],
        chat,
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_stream_from_an_openai_provider_becomes_anthropic_events() {
    let h = harness("openai").await;
    let upstream = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"sal\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"ut\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":3}}\n\n",
        "data: [DONE]\n\n",
    );
    Mock::given(method("POST"))
        .respond_with(sse(upstream))
        .mount(&h.upstream)
        .await;
    let body = BODY.replace("\"max_tokens\"", "\"stream\":true,\"max_tokens\"");
    let (status, headers, text) = messages(&h, &body).await;
    assert_eq!(status, StatusCode::OK, "{text}");
    assert_eq!(headers["content-type"], "text/event-stream");
    assert_anthropic_stream(&text, "salut", "end_turn", 3);
    let r = h.sink.wait_for(1).await;
    assert_eq!(r[0].endpoint, "messages");
}

#[tokio::test]
async fn a_stream_from_an_anthropic_provider_becomes_anthropic_events() {
    let h = harness("anthropic").await;
    let upstream = concat!(
        "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":3,\"output_tokens\":0}}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"sal\"}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"ut\"}}\n\n",
        "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"max_tokens\"},\"usage\":{\"output_tokens\":4}}\n\n",
        "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
    );
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(sse(upstream))
        .mount(&h.upstream)
        .await;
    let body = BODY.replace("\"max_tokens\"", "\"stream\":true,\"max_tokens\"");
    let (status, _, text) = messages(&h, &body).await;
    assert_eq!(status, StatusCode::OK, "{text}");
    assert_anthropic_stream(&text, "salut", "max_tokens", 4);
}

fn assert_anthropic_stream(text: &str, want: &str, stop: &str, output_tokens: u64) {
    let evs = events(text);
    let names: Vec<&str> = evs.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names.first(), Some(&"message_start"), "{names:?}");
    assert_eq!(names[1], "content_block_start");
    assert_eq!(
        names[names.len() - 3..],
        ["content_block_stop", "message_delta", "message_stop"]
    );
    assert!(names[2..names.len() - 3]
        .iter()
        .all(|n| *n == "content_block_delta"));
    let got: String = evs
        .iter()
        .filter(|(n, _)| n == "content_block_delta")
        .map(|(_, d)| d["delta"]["text"].as_str().unwrap())
        .collect();
    assert_eq!(got, want);
    let delta = &evs[evs.len() - 2].1;
    assert_eq!(delta["delta"]["stop_reason"], stop);
    assert_eq!(delta["usage"]["output_tokens"], output_tokens);
    assert_eq!(evs[0].1["message"]["role"], "assistant");
    assert!(!text.contains("[DONE]"));
}

#[tokio::test]
async fn an_error_inside_a_stream_is_an_error_event() {
    let h = harness("openai").await;
    let upstream = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"a\"},\"finish_reason\":null}]}\n\n",
        "data: {\"error\":{\"message\":\"overloaded\"}}\n\n",
    );
    Mock::given(method("POST"))
        .respond_with(sse(upstream))
        .mount(&h.upstream)
        .await;
    let body = BODY.replace("\"max_tokens\"", "\"stream\":true,\"max_tokens\"");
    let (_, _, text) = messages(&h, &body).await;
    let evs = events(&text);
    let (name, data) = evs.last().unwrap();
    assert_eq!(name, "error");
    assert_eq!(data["type"], "error");
    assert_eq!(data["error"]["type"], "api_error");
    assert_eq!(data["error"]["message"], "overloaded");
}

#[tokio::test]
async fn errors_are_in_the_anthropic_shape() {
    let h = harness("openai").await;
    let check = |status: StatusCode, body: &str, kind: &str| {
        assert_eq!(json_of(body)["type"], "error", "{status}: {body}");
        assert_eq!(json_of(body)["error"]["type"], kind, "{status}: {body}");
        assert!(json_of(body)["error"]["message"].is_string());
    };

    // 401: no key.
    let (s, _, body) = post_to(&h.app, "/v1/messages", &[], BODY).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    check(s, &body, "authentication_error");

    // 404: unknown model.
    let (s, _, body) = messages(&h, &BODY.replace("p/m", "p/nope")).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    check(s, &body, "not_found_error");

    // 403: a model the key may not call.
    let id = allow_model(&h.store, "p", "hidden").await;
    let mut tx = h.store.begin().await.unwrap();
    tx.replace_grants(id, &Grants::default()).await.unwrap();
    tx.commit().await.unwrap();
    h.state.refresh().await.unwrap();
    let (s, _, body) = messages(&h, &BODY.replace("p/m", "p/hidden")).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    check(s, &body, "permission_error");

    // 400: a block that is not text, no max_tokens, an unknown field.
    for bad in [
        r#"{"model":"p/m","max_tokens":1,"messages":[{"role":"user","content":[{"type":"image","source":{}}]}]}"#,
        r#"{"model":"p/m","messages":[{"role":"user","content":"x"}]}"#,
        r#"{"model":"p/m","max_tokens":1,"top_k":1,"messages":[{"role":"user","content":"x"}]}"#,
    ] {
        let (s, _, body) = messages(&h, bad).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{body}");
        check(s, &body, "invalid_request_error");
    }
    let (_, _, body) = messages(
        &h,
        r#"{"model":"p/m","max_tokens":1,"messages":[{"role":"user","content":[{"type":"thinking"}]}]}"#,
    )
    .await;
    assert_eq!(
        json_of(&body)["error"]["message"],
        "Only text content is supported."
    );

    // 503: the provider is down.
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&h.upstream)
        .await;
    let (s, _, body) = messages(&h, BODY).await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
    check(s, &body, "overloaded_error");
}

#[tokio::test]
async fn a_rate_limited_provider_is_a_429_with_retry_after() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "3"))
        .mount(&h.upstream)
        .await;
    let (s, headers, body) = messages(&h, BODY).await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(headers["retry-after"], "3");
    assert_eq!(json_of(&body)["error"]["type"], "rate_limit_error");
}

#[tokio::test]
async fn a_rejected_provider_credential_is_an_api_error_not_the_callers() {
    let h = harness("anthropic").await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(401).set_body_json(
            json!({ "type": "error", "error": { "type": "authentication_error", "message": "bad key sk-abc" } }),
        ))
        .mount(&h.upstream)
        .await;
    let (s, _, body) = messages(&h, BODY).await;
    assert_eq!(s, StatusCode::BAD_GATEWAY);
    assert_eq!(json_of(&body)["error"]["type"], "api_error");
    assert!(!body.contains("sk-abc"));
}

#[tokio::test]
async fn a_route_skips_a_target_the_key_may_not_call() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(openai_ok())
        .mount(&h.upstream)
        .await;
    let hidden = allow_model(&h.store, "p", "hidden").await;
    let mut tx = h.store.begin().await.unwrap();
    tx.replace_grants(hidden, &Grants::default()).await.unwrap();
    let visible = tx.insert_model(1, "visible").await.unwrap();
    assert!(tx.set_model_enabled(visible, true).await.unwrap());
    tx.replace_grants(
        visible,
        &Grants {
            everyone: true,
            ..Grants::default()
        },
    )
    .await
    .unwrap();
    let route = tx
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
        route,
        &TargetsInput {
            primaries: vec![(hidden, 1)],
            fallbacks: vec![visible],
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    h.state.refresh().await.unwrap();
    let (s, _, body) = messages(&h, &BODY.replace("p/m", "r")).await;
    assert_eq!(s, StatusCode::OK, "{body}");
    let seen: Vec<_> = h.sink.records()[0]
        .attempts
        .iter()
        .map(|a| (a.model.clone(), a.outcome))
        .collect();
    assert_eq!(
        seen,
        [
            ("hidden".to_string(), AttemptOutcome::Skipped),
            ("visible".to_string(), AttemptOutcome::Ok)
        ]
    );
    assert_eq!(h.upstream.received_requests().await.unwrap().len(), 1);
}

mod tools {
    use super::*;

    fn tool_message(stream: bool) -> String {
        json!({
            "model": "p/m", "max_tokens": 50, "stream": stream,
            "messages": [{ "role": "user", "content": "weather in Paris?" }],
            "tools": [{ "name": "get_weather", "description": "Weather by city",
                "input_schema": { "type": "object", "properties": { "city": { "type": "string" } } } }],
            "tool_choice": { "type": "auto" }
        })
        .to_string()
    }

    #[tokio::test]
    async fn anthropic_ingress_tool_call_to_openai_provider() {
        let h = harness("openai").await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_partial_json(json!({
                "tools": [{ "type": "function", "function": { "name": "get_weather" } }],
                "tool_choice": "auto"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "c1", "model": "m",
                "choices": [{ "message": { "role": "assistant", "content": null,
                    "tool_calls": [{ "id": "call_1", "type": "function",
                        "function": { "name": "get_weather", "arguments": "{\"city\":\"Paris\"}" } }] },
                    "finish_reason": "tool_calls" }],
                "usage": { "prompt_tokens": 4, "completion_tokens": 6 }
            })))
            .expect(1)
            .mount(&h.upstream)
            .await;
        let (status, _, body) = messages(&h, &tool_message(false)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let v = json_of(&body);
        assert_eq!(v["stop_reason"], "tool_use");
        assert_eq!(
            v["content"],
            json!([{ "type": "tool_use", "id": "call_1", "name": "get_weather",
                "input": { "city": "Paris" } }])
        );
        let records = h.sink.records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].status, 200);
        assert_eq!(records[0].endpoint, "messages");
        assert_eq!(records[0].attempts[0].outcome, AttemptOutcome::Ok);
    }

    #[tokio::test]
    async fn anthropic_ingress_streamed_tool_call_from_openai_provider() {
        let h = harness("openai").await;
        let chunks = concat!(
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"get_weather\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"city\\\":\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"Paris\\\"}\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":4,\"completion_tokens\":6}}\n\n",
            "data: [DONE]\n\n",
        );
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(sse(chunks))
            .mount(&h.upstream)
            .await;
        let (status, _, body) = messages(&h, &tool_message(true)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let evs = events(&body);
        let start = evs
            .iter()
            .find(|(n, d)| n == "content_block_start" && d["content_block"]["type"] == "tool_use")
            .expect("a tool_use block");
        assert_eq!(start.1["content_block"]["id"], "call_1");
        assert_eq!(start.1["content_block"]["name"], "get_weather");
        let args: String = evs
            .iter()
            .filter_map(|(_, d)| d["delta"]["partial_json"].as_str())
            .collect();
        assert_eq!(args, r#"{"city":"Paris"}"#);
        let delta = evs.iter().find(|(n, _)| n == "message_delta").unwrap();
        assert_eq!(delta.1["delta"]["stop_reason"], "tool_use");
        let records = h.sink.wait_for(1).await;
        assert_eq!(records[0].status, 200);
        assert_eq!(records[0].attempts[0].outcome, AttemptOutcome::Ok);
    }
}
