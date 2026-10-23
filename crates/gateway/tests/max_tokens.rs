//! The name of the output limit a chat call sends: Azure takes
//! `max_completion_tokens`, other OpenAI-compatible servers keep `max_tokens`.

mod common;

use axum::http::StatusCode;
use common::{harness, harness_translating, post_to, Harness};
use serde_json::{json, Value};
use wiremock::matchers::method;
use wiremock::{Mock, ResponseTemplate};

fn answer() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "c1", "model": "m",
        "choices": [{ "message": { "role": "assistant", "content": "hello" }, "finish_reason": "stop" }],
        "usage": { "prompt_tokens": 1, "completion_tokens": 2 }
    }))
}

async fn call(h: &Harness, path: &str, body: Value) -> StatusCode {
    let bearer = format!("Bearer {}", h.key);
    post_to(
        &h.app,
        path,
        &[("authorization", &bearer)],
        &body.to_string(),
    )
    .await
    .0
}

async fn sent(h: &Harness) -> Value {
    let requests = h.upstream.received_requests().await.unwrap();
    serde_json::from_slice(&requests[0].body).unwrap()
}

#[tokio::test]
async fn an_azure_target_gets_max_completion_tokens_from_chat_and_responses() {
    for (path, body) in [
        (
            "/v1/chat/completions",
            json!({"model":"p/m","max_tokens":7,"messages":[{"role":"user","content":"hi"}]}),
        ),
        (
            "/v1/responses",
            json!({"model":"p/m","max_output_tokens":7,"input":"hi"}),
        ),
    ] {
        let h = harness_translating("azure").await;
        Mock::given(method("POST"))
            .respond_with(answer())
            .mount(&h.upstream)
            .await;
        assert_eq!(call(&h, path, body).await, StatusCode::OK, "{path}");
        let v = sent(&h).await;
        assert_eq!(v["max_completion_tokens"], 7, "{path}: {v}");
        assert!(v.get("max_tokens").is_none(), "{path}: {v}");
    }
}

#[tokio::test]
async fn another_openai_compatible_host_keeps_max_tokens() {
    // The mock server is on 127.0.0.1, not api.openai.com.
    for (path, body) in [
        (
            "/v1/chat/completions",
            json!({"model":"p/m","max_tokens":7,"messages":[{"role":"user","content":"hi"}]}),
        ),
        (
            "/v1/responses",
            json!({"model":"p/m","max_output_tokens":7,"input":"hi"}),
        ),
    ] {
        let h = harness_translating("openai").await;
        Mock::given(method("POST"))
            .respond_with(answer())
            .mount(&h.upstream)
            .await;
        assert_eq!(call(&h, path, body).await, StatusCode::OK, "{path}");
        let v = sent(&h).await;
        assert_eq!(v["max_tokens"], 7, "{path}: {v}");
        assert!(v.get("max_completion_tokens").is_none(), "{path}: {v}");
    }
}

#[tokio::test]
async fn a_passed_through_responses_call_keeps_the_limit_the_agent_named() {
    // The body is forwarded as the agent sent it: nothing renames the limit.
    for kind in ["azure", "openai"] {
        let h = harness(kind).await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"id":"r","object":"response","status":"completed","model":"m","output":[],"usage":{"input_tokens":1,"output_tokens":2}}"#,
                "application/json",
            ))
            .mount(&h.upstream)
            .await;
        let body = json!({"model":"p/m","max_output_tokens":7,"store":false,"input":"hi"});
        assert_eq!(
            call(&h, "/v1/responses", body).await,
            StatusCode::OK,
            "{kind}"
        );
        let v = sent(&h).await;
        assert_eq!(v["max_output_tokens"], 7, "{kind}: {v}");
        assert!(v.get("max_tokens").is_none(), "{kind}: {v}");
        assert!(v.get("max_completion_tokens").is_none(), "{kind}: {v}");
    }
}

#[tokio::test]
async fn a_passed_through_messages_call_keeps_its_max_tokens() {
    let h = harness("anthropic").await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"{"id":"m","type":"message","role":"assistant","model":"m","content":[],"stop_reason":"max_tokens","usage":{"input_tokens":1,"output_tokens":7}}"#,
            "application/json",
        ))
        .mount(&h.upstream)
        .await;
    let body = json!({"model":"p/m","max_tokens":7,"messages":[{"role":"user","content":"hi"}]});
    assert_eq!(call(&h, "/v1/messages", body).await, StatusCode::OK);
    assert_eq!(sent(&h).await["max_tokens"], 7);
}
