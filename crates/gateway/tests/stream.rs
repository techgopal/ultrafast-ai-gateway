mod common;

use axum::http::StatusCode;
use common::{allow_model, hanging_upstream, harness, post_chat};
use futures::StreamExt;
use serde_json::Value;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, ResponseTemplate};

const BODY: &str = r#"{"model":"p/m","stream":true,"messages":[{"role":"user","content":"hi"}]}"#;

/// Returns the JSON payload of every `data:` line except `[DONE]`.
fn payloads(body: &str) -> Vec<Value> {
    body.lines()
        .filter_map(|l| l.strip_prefix("data: "))
        .filter(|d| *d != "[DONE]")
        .map(|d| serde_json::from_str(d).expect("every data line must be JSON"))
        .collect()
}

fn text(body: &str) -> String {
    payloads(body)
        .iter()
        .filter_map(|v| {
            v["choices"][0]["delta"]["content"]
                .as_str()
                .map(str::to_string)
        })
        .collect()
}

fn sse(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(body.to_string(), "text/event-stream")
}

#[tokio::test]
async fn streams_openai_provider() {
    let h = harness("openai").await;
    let upstream = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"h\u{e9}l\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"lo \u{1f600}\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":3}}\n\n",
        "data: [DONE]\n\n",
    );
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_partial_json(serde_json::json!({ "stream": true })))
        .respond_with(sse(upstream))
        .mount(&h.upstream)
        .await;
    let (status, body) = post_chat(&h.app, Some(&h.key), BODY).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(text(&body), "h\u{e9}llo \u{1f600}");
    let all = payloads(&body);
    let last = all.last().unwrap();
    assert_eq!(last["choices"][0]["finish_reason"], "stop");
    assert_eq!(last["usage"]["total_tokens"], 5);
    assert!(body.ends_with("data: [DONE]\n\n"));
    assert!(all.iter().all(|v| v["model"] == "m"));
    let first_id = all[0]["id"].as_str().unwrap();
    assert!(first_id.starts_with("chatcmpl-"));
    assert!(all.iter().all(|v| v["id"] == first_id));
}

#[tokio::test]
async fn streams_anthropic_provider_in_openai_shape() {
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
    let (status, body) = post_chat(&h.app, Some(&h.key), BODY).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(text(&body), "salut");
    let all = payloads(&body);
    assert_eq!(all.last().unwrap()["choices"][0]["finish_reason"], "length");
    assert_eq!(all.last().unwrap()["usage"]["total_tokens"], 7);
    assert!(body.ends_with("data: [DONE]\n\n"));
}

#[tokio::test]
async fn response_has_event_stream_content_type() {
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(sse("data: [DONE]\n\n"))
        .mount(&h.upstream)
        .await;
    let resp = h
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("authorization", format!("Bearer {}", h.key))
                .body(Body::from(BODY))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.headers()["content-type"], "text/event-stream");
    assert_eq!(resp.headers()["cache-control"], "no-cache");
}

#[tokio::test]
async fn stream_that_ends_early_sends_an_error_event_and_no_done() {
    let h = harness("openai").await;
    let upstream =
        "data: {\"choices\":[{\"delta\":{\"content\":\"par\"},\"finish_reason\":null}]}\n\n";
    Mock::given(method("POST"))
        .respond_with(sse(upstream))
        .mount(&h.upstream)
        .await;
    let (status, body) = post_chat(&h.app, Some(&h.key), BODY).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(text(&body), "par");
    let last = payloads(&body).pop().unwrap();
    assert!(last["error"]["message"]
        .as_str()
        .unwrap()
        .contains("ended before"));
    assert!(!body.contains("[DONE]"));
}

#[tokio::test]
async fn provider_error_inside_stream_is_forwarded_as_error_event() {
    let h = harness("openai").await;
    let upstream = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"a\"},\"finish_reason\":null}]}\n\n",
        "data: {\"error\":{\"message\":\"over \\\"loaded\\\"\"}}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"never\"},\"finish_reason\":null}]}\n\n",
    );
    Mock::given(method("POST"))
        .respond_with(sse(upstream))
        .mount(&h.upstream)
        .await;
    let (_, body) = post_chat(&h.app, Some(&h.key), BODY).await;
    assert_eq!(text(&body), "a");
    let last = payloads(&body).pop().unwrap();
    assert!(last["error"]["message"]
        .as_str()
        .unwrap()
        .contains("over \"loaded\""));
    assert!(!body.contains("[DONE]"));
}

/// The same stream as the test above, with every line ending replaced.
async fn error_inside_stream_with_line_ending(ending: &str) {
    let h = harness("openai").await;
    let upstream = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"a\"},\"finish_reason\":null}]}\n\n",
        "data: {\"error\":{\"message\":\"over \\\"loaded\\\"\"}}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"never\"},\"finish_reason\":null}]}\n\n",
    )
    .replace('\n', ending);
    Mock::given(method("POST"))
        .respond_with(sse(&upstream))
        .mount(&h.upstream)
        .await;
    let (_, body) = post_chat(&h.app, Some(&h.key), BODY).await;
    assert_eq!(text(&body), "a");
    let last = payloads(&body).pop().unwrap();
    assert!(last["error"]["message"]
        .as_str()
        .unwrap()
        .contains("over \"loaded\""));
    assert!(!body.contains("[DONE]"));
}

#[tokio::test]
async fn provider_error_inside_crlf_stream_is_forwarded_as_error_event() {
    error_inside_stream_with_line_ending("\r\n").await;
}

#[tokio::test]
async fn provider_error_inside_cr_stream_is_forwarded_as_error_event() {
    error_inside_stream_with_line_ending("\r").await;
}

#[tokio::test]
async fn provider_error_before_stream_starts_is_a_normal_json_error() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503).set_body_string("unavailable"))
        .mount(&h.upstream)
        .await;
    let (status, body) = post_chat(&h.app, Some(&h.key), BODY).await;
    // Retried, and then no provider is left.
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        v["error"]["message"],
        "No provider could serve this request."
    );
}

#[tokio::test]
async fn caller_disconnect_drops_the_upstream_request() {
    use axum::body::Body;
    use axum::http::Request;
    use std::time::Duration;
    use tower::ServiceExt;
    let h = harness("openai").await;
    let (uri, closed) = hanging_upstream().await;
    h.store
        .insert_provider("hang", "openai", &uri, None)
        .await
        .unwrap();
    allow_model(&h.store, "hang", "m").await;
    // `/v1` reads the snapshot, so the row written above must be loaded.
    h.state.refresh().await.unwrap();
    let body = r#"{"model":"hang/m","stream":true,"messages":[{"role":"user","content":"hi"}]}"#;
    let resp = h
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("authorization", format!("Bearer {}", h.key))
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let mut stream = resp.into_body().into_data_stream();
    let first = tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .expect("the first chunk must arrive while upstream is still open")
        .unwrap()
        .unwrap();
    assert_eq!(text(std::str::from_utf8(&first).unwrap()), "one");
    // The caller goes away.
    drop(stream);
    tokio::time::timeout(Duration::from_secs(5), closed)
        .await
        .expect("the upstream connection must be closed when the caller disconnects")
        .unwrap();
}

#[tokio::test]
async fn oversized_stream_event_sends_an_error_event_and_no_done() {
    let h = harness("openai").await;
    let upstream = format!(
        concat!(
            "data: {{\"choices\":[{{\"delta\":{{\"content\":\"a\"}},\"finish_reason\":null}}]}}\n\n",
            "data: {{\"choices\":[{{\"delta\":{{\"content\":\"{}\"}},\"finish_reason\":null}}]}}\n\n",
            "data: [DONE]\n\n",
        ),
        "x".repeat(1024 * 1024 + 1)
    );
    Mock::given(method("POST"))
        .respond_with(sse(&upstream))
        .mount(&h.upstream)
        .await;
    let (status, body) = post_chat(&h.app, Some(&h.key), BODY).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(text(&body), "a");
    let last = payloads(&body).pop().unwrap();
    assert!(last["error"]["message"]
        .as_str()
        .unwrap()
        .contains("stream event exceeds the size limit"));
    assert!(!body.contains("[DONE]"));
}

const MASKED: &str = "Provider rejected the gateway's credential.";

async fn assert_stream_error(kind: &str, upstream: &str, expected: &str) -> String {
    let h = harness(kind).await;
    Mock::given(method("POST"))
        .respond_with(sse(upstream))
        .mount(&h.upstream)
        .await;
    let (status, body) = post_chat(&h.app, Some(&h.key), BODY).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(text(&body), "a");
    let last = payloads(&body).pop().unwrap();
    assert_eq!(last["error"]["message"], expected);
    assert_eq!(last["error"]["type"], "upstream_error");
    assert!(!body.contains("[DONE]"));
    body
}

#[tokio::test]
async fn credential_error_inside_openai_stream_is_masked() {
    let upstream = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"a\"},\"finish_reason\":null}]}\n\n",
        "data: {\"error\":{\"message\":\"Incorrect API key provided: sk-abc\",\"type\":\"invalid_request_error\",\"code\":\"invalid_api_key\"}}\n\n",
    );
    let body = assert_stream_error("openai", upstream, MASKED).await;
    assert!(!body.contains("sk-abc"));
    assert!(!body.contains("Incorrect"));
}

#[tokio::test]
async fn credential_error_inside_anthropic_stream_is_masked() {
    for kind in ["authentication_error", "permission_error"] {
        let upstream = format!(
            concat!(
                "event: content_block_delta\ndata: {{\"type\":\"content_block_delta\",\"delta\":{{\"type\":\"text_delta\",\"text\":\"a\"}}}}\n\n",
                "event: error\ndata: {{\"type\":\"error\",\"error\":{{\"type\":\"{}\",\"message\":\"invalid x-api-key sk-ant-abc\"}}}}\n\n",
            ),
            kind
        );
        let body = assert_stream_error("anthropic", &upstream, MASKED).await;
        assert!(!body.contains("sk-ant-abc"), "{kind}");
        assert!(!body.contains("x-api-key"), "{kind}");
    }
}

#[tokio::test]
async fn other_error_inside_stream_shows_only_the_provider_message() {
    let openai = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"a\"},\"finish_reason\":null}]}\n\n",
        "data: {\"error\":{\"message\":\"The server is overloaded\"}}\n\n",
    );
    assert_stream_error("openai", openai, "The server is overloaded").await;
    let anthropic = concat!(
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"a\"}}\n\n",
        "event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n",
    );
    assert_stream_error("anthropic", anthropic, "Overloaded").await;
}

mod records {
    use super::*;
    use ultrafast_gateway::telemetry::AttemptOutcome;

    #[tokio::test]
    async fn a_finished_stream_is_recorded_with_its_usage() {
        let h = harness("openai").await;
        Mock::given(method("POST"))
            .respond_with(sse(concat!(
                "data: {\"choices\":[{\"delta\":{\"content\":\"one\"},\"finish_reason\":null}]}\n\n",
                "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":4}}\n\n",
                "data: [DONE]\n\n"
            )))
            .mount(&h.upstream)
            .await;
        let body =
            r#"{"model":"p/gpt-4o","stream":true,"messages":[{"role":"user","content":"hi"}]}"#;
        let (status, _) = post_chat(&h.app, Some(&h.key), body).await;
        assert_eq!(status, StatusCode::OK);
        let records = h.sink.wait_for(1).await;
        assert_eq!(records.len(), 1);
        let r = &records[0];
        assert!(r.stream);
        assert_eq!(r.status, 200);
        let usage = r.usage.expect("usage from the final event");
        assert_eq!((usage.input_tokens, usage.output_tokens), (3, 4));
        assert_eq!(r.attempts.len(), 1);
        assert_eq!(r.attempts[0].outcome, AttemptOutcome::Ok);
    }

    #[tokio::test]
    async fn a_stream_that_fails_midway_is_recorded_once_as_failed() {
        let h = harness("openai").await;
        Mock::given(method("POST"))
            .respond_with(sse(
                "data: {\"choices\":[{\"delta\":{\"content\":\"one\"},\"finish_reason\":null}]}\n\n",
            ))
            .mount(&h.upstream)
            .await;
        let body =
            r#"{"model":"p/gpt-4o","stream":true,"messages":[{"role":"user","content":"hi"}]}"#;
        post_chat(&h.app, Some(&h.key), body).await;
        let records = h.sink.wait_for(1).await;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(h.sink.records().len(), 1);
        assert!(records[0].usage.is_none());
        assert_eq!(records[0].attempts[0].outcome, AttemptOutcome::Retryable);
    }

    #[tokio::test]
    async fn a_caller_that_drops_the_stream_is_recorded() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;
        let h = harness("openai").await;
        let (uri, closed) = hanging_upstream().await;
        h.store
            .insert_provider("hang", "openai", &uri, None)
            .await
            .unwrap();
        allow_model(&h.store, "hang", "m").await;
        h.state.refresh().await.unwrap();
        let body =
            r#"{"model":"hang/m","stream":true,"messages":[{"role":"user","content":"hi"}]}"#;
        let resp = h
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("authorization", format!("Bearer {}", h.key))
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        let mut stream = resp.into_body().into_data_stream();
        stream.next().await.unwrap().unwrap();
        assert!(h.sink.records().is_empty(), "not recorded while running");
        drop(stream);
        let _ = closed.await;
        let records = h.sink.wait_for(1).await;
        // Exactly one record, however long afterwards.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert_eq!(h.sink.records().len(), 1);
        assert_eq!(records[0].requested, "hang/m");
        assert_eq!(records[0].status, 499);
        assert!(records[0].stream);
        // The target that was streaming is the one attempt, and it did not
        // finish: retryable, with the status the provider had answered.
        let attempts: Vec<_> = records[0]
            .attempts
            .iter()
            .map(|a| (a.provider.as_str(), a.model.as_str(), a.outcome, a.status))
            .collect();
        assert_eq!(
            attempts,
            [("hang", "m", AttemptOutcome::Retryable, Some(200))]
        );
        assert!(records[0].usage.is_none());
    }
}

#[tokio::test]
async fn streams_gemini_provider_in_openai_shape() {
    let h = harness("gemini").await;
    let upstream = concat!(
        "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"sal\"}]}}],\"usageMetadata\":{\"promptTokenCount\":3}}\n\n",
        "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"ut\"}]},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":3,\"candidatesTokenCount\":4}}\n\n",
    );
    Mock::given(method("POST"))
        .and(path("/v1beta/models/m:streamGenerateContent"))
        .and(wiremock::matchers::query_param("alt", "sse"))
        .respond_with(sse(upstream))
        .expect(1)
        .mount(&h.upstream)
        .await;
    let (status, body) = post_chat(&h.app, Some(&h.key), BODY).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(text(&body), "salut");
    let all = payloads(&body);
    assert_eq!(all.last().unwrap()["choices"][0]["finish_reason"], "stop");
    assert_eq!(all.last().unwrap()["usage"]["total_tokens"], 7);
    assert!(body.ends_with("data: [DONE]\n\n"));
}

#[tokio::test]
async fn streams_azure_provider_in_openai_shape() {
    let h = harness("azure").await;
    let upstream = concat!(
        "data: {\"choices\":[],\"prompt_filter_results\":[{\"prompt_index\":0}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"az\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"ure\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":3}}\n\n",
        "data: [DONE]\n\n",
    );
    Mock::given(method("POST"))
        .and(path("/openai/deployments/m/chat/completions"))
        .and(body_partial_json(serde_json::json!({ "stream": true })))
        .respond_with(sse(upstream))
        .expect(1)
        .mount(&h.upstream)
        .await;
    let (status, body) = post_chat(&h.app, Some(&h.key), BODY).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(text(&body), "azure");
    let all = payloads(&body);
    assert_eq!(all.last().unwrap()["usage"]["total_tokens"], 5);
    assert!(body.ends_with("data: [DONE]\n\n"));
}
