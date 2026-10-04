mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{allow_model, harness, harness_with_limit, harness_with_response_limit, post_chat};
use serde_json::{json, Value};
use tower::ServiceExt;
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, ResponseTemplate};

const BODY: &str = r#"{"model":"p/gpt-4o","messages":[{"role":"user","content":"hi"}]}"#;

fn openai_ok() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "c1", "model": "gpt-4o",
        "choices": [{ "message": { "role": "assistant", "content": "hello" }, "finish_reason": "stop" }],
        "usage": { "prompt_tokens": 1, "completion_tokens": 2 }
    }))
}

fn error_message(body: &str) -> String {
    let v: Value = serde_json::from_str(body).expect("error body must be JSON");
    v["error"]["message"]
        .as_str()
        .expect("error.message must be a string")
        .to_string()
}

#[tokio::test]
async fn health_needs_no_key() {
    let h = harness("openai").await;
    let resp = h
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn proxies_to_openai_provider_with_decrypted_credential() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("authorization", "Bearer provider-secret"))
        .and(body_partial_json(json!({ "model": "gpt-4o" })))
        .respond_with(openai_ok())
        .expect(1)
        .mount(&h.upstream)
        .await;
    let (status, body) = post_chat(&h.app, Some(&h.key), BODY).await;
    assert_eq!(status, StatusCode::OK);
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["choices"][0]["message"]["content"], "hello");
    assert_eq!(v["usage"]["total_tokens"], 3);
    assert!(!body.contains("provider-secret"));
}

#[tokio::test]
async fn proxies_to_anthropic_provider_and_returns_openai_shape() {
    let h = harness("anthropic").await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", "provider-secret"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "m1", "model": "claude-sonnet-5",
            "content": [{ "type": "text", "text": "bonjour" }],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 4, "output_tokens": 5 }
        })))
        .expect(1)
        .mount(&h.upstream)
        .await;
    let body = r#"{"model":"p/claude-sonnet-5","messages":[{"role":"user","content":"hi"}]}"#;
    let (status, out) = post_chat(&h.app, Some(&h.key), body).await;
    assert_eq!(status, StatusCode::OK);
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["object"], "chat.completion");
    assert_eq!(v["choices"][0]["message"]["content"], "bonjour");
    assert_eq!(v["choices"][0]["finish_reason"], "stop");
}

#[tokio::test]
async fn bad_keys_get_401_and_never_reach_the_provider() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(openai_ok())
        .expect(0)
        .mount(&h.upstream)
        .await;

    let revoked = ultrafast_gateway::secrets::generate_key();
    let id = h
        .store
        .insert_key("r", &revoked.hash, &revoked.display, None)
        .await
        .unwrap();
    h.store.revoke_key(id).await.unwrap();
    let expired = ultrafast_gateway::secrets::generate_key();
    h.store
        .insert_key(
            "e",
            &expired.hash,
            &expired.display,
            Some("2000-01-01 00:00:00"),
        )
        .await
        .unwrap();
    // `/v1` reads the snapshot, so the rows written above must be loaded.
    h.state.refresh().await.unwrap();

    let cases: Vec<Option<String>> = vec![
        None,
        Some(String::new()),
        Some("sk-not-ours".into()),
        Some("uf-sk-unknown".into()),
        Some(revoked.full),
        Some(expired.full),
    ];
    for key in cases {
        let (status, body) = post_chat(&h.app, key.as_deref(), BODY).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "key {key:?}");
        assert!(!error_message(&body).is_empty());
    }
}

#[tokio::test]
async fn non_bearer_authorization_header_gets_401() {
    let h = harness("openai").await;
    let resp = h
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("authorization", format!("Basic {}", h.key))
                .body(Body::from(BODY))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn invalid_requests_get_400_and_unknown_targets_get_404() {
    let h = harness("openai").await;
    let (s, b) = post_chat(&h.app, Some(&h.key), "{not json").await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(!error_message(&b).is_empty());

    let tools = r#"{"model":"p/m","messages":[{"role":"user","content":"x"}],"tools":[{}]}"#;
    let (s, b) = post_chat(&h.app, Some(&h.key), tools).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(error_message(&b).contains("tools"));

    let no_slash = r#"{"model":"gpt-4o","messages":[{"role":"user","content":"x"}]}"#;
    assert_eq!(
        post_chat(&h.app, Some(&h.key), no_slash).await.0,
        StatusCode::NOT_FOUND
    );

    let unknown = r#"{"model":"nope/gpt-4o","messages":[{"role":"user","content":"x"}]}"#;
    assert_eq!(
        post_chat(&h.app, Some(&h.key), unknown).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn oversized_body_gets_413_in_json() {
    let h = harness_with_limit("openai", 64).await;
    let big = format!(
        r#"{{"model":"p/m","messages":[{{"role":"user","content":"{}"}}]}}"#,
        "x".repeat(200)
    );
    let (s, b) = post_chat(&h.app, Some(&h.key), &big).await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
    assert!(!error_message(&b).is_empty());
}

#[tokio::test]
async fn provider_errors_are_mapped() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(502).set_body_string("<html>Bad Gateway</html>"))
        .mount(&h.upstream)
        .await;
    let (s, b) = post_chat(&h.app, Some(&h.key), BODY).await;
    // Retried, and then no provider is left: the provider's own text is not shown.
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(error_message(&b), "No provider could serve this request.");

    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(429)
                .set_body_json(json!({ "error": { "message": "slow down" } })),
        )
        .mount(&h.upstream)
        .await;
    let (s, b) = post_chat(&h.app, Some(&h.key), BODY).await;
    // Every try was a 429: the caller is told to wait.
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(
        error_message(&b),
        "The provider is rate limiting this request. Try again later."
    );

    // An answer no retry can change keeps its status and its message.
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(422)
                .set_body_json(json!({ "error": { "message": "bad temperature" } })),
        )
        .mount(&h.upstream)
        .await;
    let (s, b) = post_chat(&h.app, Some(&h.key), BODY).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error_message(&b), "bad temperature");
}

async fn assert_credential_rejection_is_502(status: u16) {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(status)
                .set_body_json(json!({ "error": { "message": "Incorrect API key sk-abc" } })),
        )
        .mount(&h.upstream)
        .await;
    let (s, b) = post_chat(&h.app, Some(&h.key), BODY).await;
    assert_eq!(s, StatusCode::BAD_GATEWAY);
    let v: Value = serde_json::from_str(&b).unwrap();
    assert_eq!(v["error"]["type"], "upstream_error");
    assert_eq!(
        error_message(&b),
        "Provider rejected the gateway's credential."
    );
    assert!(!b.contains("sk-abc"));
}

#[tokio::test]
async fn provider_401_is_502_with_a_fixed_message() {
    assert_credential_rejection_is_502(401).await;
}

#[tokio::test]
async fn provider_403_is_502_with_a_fixed_message() {
    assert_credential_rejection_is_502(403).await;
}

#[tokio::test]
async fn unreachable_provider_gets_503() {
    let h = harness("openai").await;
    h.store
        .insert_provider("dead", "openai", "http://127.0.0.1:1", None)
        .await
        .unwrap();
    allow_model(&h.store, "dead", "m").await;
    // `/v1` reads the snapshot, so the row written above must be loaded.
    h.state.refresh().await.unwrap();
    let body = r#"{"model":"dead/m","messages":[{"role":"user","content":"x"}]}"#;
    let (s, b) = post_chat(&h.app, Some(&h.key), body).await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(error_message(&b), "No provider could serve this request.");
    assert!(!b.contains("dead"), "the answer does not name the provider");
}

#[tokio::test]
async fn provider_redirect_is_an_error_and_is_not_followed() {
    let h = harness("openai").await;
    let elsewhere = wiremock::MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(openai_ok())
        .expect(0)
        .mount(&elsewhere)
        .await;
    Mock::given(method("GET"))
        .respond_with(openai_ok())
        .expect(0)
        .mount(&elsewhere)
        .await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", format!("{}/chat/completions", elsewhere.uri())),
        )
        .expect(1)
        .mount(&h.upstream)
        .await;
    let (s, b) = post_chat(&h.app, Some(&h.key), BODY).await;
    assert_eq!(s, StatusCode::BAD_GATEWAY);
    assert!(!error_message(&b).is_empty());
    assert!(!b.contains(&elsewhere.uri()));
    assert!(elsewhere.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn bearer_scheme_is_case_insensitive() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(openai_ok())
        .expect(3)
        .mount(&h.upstream)
        .await;
    for scheme in ["bearer", "BEARER", "bEaReR"] {
        let resp = h
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/chat/completions")
                    .header("authorization", format!("{scheme} {}", h.key))
                    .body(Body::from(BODY))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "scheme {scheme}");
    }
}

async fn post_raw(app: &axum::Router, key: &str, body: Body) -> (StatusCode, String) {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("authorization", format!("Bearer {key}"))
                .body(body)
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

#[tokio::test]
async fn oversized_body_with_invalid_key_gets_401_not_413() {
    let h = harness_with_limit("openai", 64).await;
    let big = format!(
        r#"{{"model":"p/m","messages":[{{"role":"user","content":"{}"}}]}}"#,
        "x".repeat(200)
    );
    let (s, b) = post_chat(&h.app, Some("uf-sk-unknown"), &big).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert!(!error_message(&b).is_empty());
}

#[tokio::test]
async fn body_is_not_read_before_the_key_is_checked() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    let h = harness("openai").await;
    let polled = Arc::new(AtomicBool::new(false));
    let flag = polled.clone();
    let stream = futures::stream::once(async move {
        flag.store(true, Ordering::SeqCst);
        Ok::<_, std::io::Error>(bytes::Bytes::from_static(BODY.as_bytes()))
    });
    let (s, _) = post_raw(&h.app, "uf-sk-unknown", Body::from_stream(stream)).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert!(
        !polled.load(Ordering::SeqCst),
        "the body was read before authentication"
    );
}

#[tokio::test]
async fn unreadable_body_gets_400_not_413() {
    let h = harness("openai").await;
    let stream = futures::stream::once(async {
        Err::<bytes::Bytes, _>(std::io::Error::other("connection reset"))
    });
    let (s, b) = post_raw(&h.app, &h.key, Body::from_stream(stream)).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let v: Value = serde_json::from_str(&b).unwrap();
    assert_eq!(v["error"]["type"], "invalid_request_error");
    assert_eq!(error_message(&b), "Request body could not be read.");
}

#[tokio::test]
async fn oversized_provider_response_gets_502() {
    let h = harness_with_response_limit("openai", 256).await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "c1", "model": "gpt-4o",
            "choices": [{ "message": { "role": "assistant", "content": "x".repeat(1000) }, "finish_reason": "stop" }],
        })))
        .mount(&h.upstream)
        .await;
    let (s, b) = post_chat(&h.app, Some(&h.key), BODY).await;
    assert_eq!(s, StatusCode::BAD_GATEWAY);
    let v: Value = serde_json::from_str(&b).unwrap();
    assert_eq!(v["error"]["type"], "upstream_error");
    assert_eq!(error_message(&b), "The provider response was too large.");
}

#[tokio::test]
async fn oversized_provider_error_response_gets_502() {
    let h = harness_with_response_limit("openai", 256).await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429).set_body_string("y".repeat(1000)))
        .mount(&h.upstream)
        .await;
    let (s, b) = post_chat(&h.app, Some(&h.key), BODY).await;
    assert_eq!(s, StatusCode::BAD_GATEWAY);
    assert_eq!(error_message(&b), "The provider response was too large.");
}

#[tokio::test]
async fn provider_response_within_the_limit_is_returned() {
    let h = harness_with_response_limit("openai", 4096).await;
    Mock::given(method("POST"))
        .respond_with(openai_ok())
        .mount(&h.upstream)
        .await;
    let (s, _) = post_chat(&h.app, Some(&h.key), BODY).await;
    assert_eq!(s, StatusCode::OK);
}

mod records {
    use super::*;
    use ultrafast_gateway::telemetry::AttemptOutcome;

    #[tokio::test]
    async fn a_success_is_recorded_with_usage_and_one_attempt() {
        let h = harness("openai").await;
        Mock::given(method("POST"))
            .respond_with(openai_ok())
            .mount(&h.upstream)
            .await;
        let (status, body) = post_chat(&h.app, Some(&h.key), BODY).await;
        assert_eq!(status, StatusCode::OK);
        let records = h.sink.records();
        assert_eq!(records.len(), 1);
        let r = &records[0];
        assert_eq!(r.requested, "p/gpt-4o");
        assert_eq!(r.endpoint, "chat");
        assert!(!r.stream);
        assert_eq!(r.status, 200);
        let usage = r.usage.expect("usage");
        assert_eq!((usage.input_tokens, usage.output_tokens), (1, 2));
        assert_eq!(r.attempts.len(), 1);
        let a = &r.attempts[0];
        assert_eq!((a.provider.as_str(), a.model.as_str()), ("p", "gpt-4o"));
        assert_eq!(a.outcome, AttemptOutcome::Ok);
        assert_eq!(a.status, Some(200));
        assert!(r.started_at.len() >= 19);
        // No prompt, answer or credential in the record.
        let dump = format!("{r:?}");
        for secret in ["hello", "provider-secret", h.key.as_str(), "\"hi\""] {
            assert!(!dump.contains(secret), "{secret} leaked into {dump}");
        }
        assert!(body.contains("hello"));
    }

    #[tokio::test]
    async fn an_upstream_error_is_recorded() {
        let h = harness("openai").await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(503).set_body_string("down"))
            .mount(&h.upstream)
            .await;
        let (status, _) = post_chat(&h.app, Some(&h.key), BODY).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        let r = &h.sink.records()[0];
        assert_eq!(r.status, 503);
        assert!(r.usage.is_none());
        // The first try and two retries.
        assert_eq!(r.attempts.len(), 3);
        for a in &r.attempts {
            assert_eq!(a.outcome, AttemptOutcome::Retryable);
            assert_eq!(a.status, Some(503));
        }
    }

    #[tokio::test]
    async fn a_rejection_by_the_provider_is_fatal() {
        let h = harness("openai").await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(400).set_body_string("bad"))
            .mount(&h.upstream)
            .await;
        post_chat(&h.app, Some(&h.key), BODY).await;
        let r = &h.sink.records()[0];
        assert_eq!(r.attempts[0].outcome, AttemptOutcome::Fatal);
    }

    #[tokio::test]
    async fn refused_calls_are_recorded_without_attempts_and_unauthenticated_ones_are_not() {
        let h = harness("openai").await;
        post_chat(&h.app, None, BODY).await;
        post_chat(&h.app, Some("uf-wrong"), BODY).await;
        assert!(h.sink.records().is_empty());
        let missing = r#"{"model":"p/nope","messages":[{"role":"user","content":"hi"}]}"#;
        let (status, _) = post_chat(&h.app, Some(&h.key), missing).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        post_chat(&h.app, Some(&h.key), "{").await;
        let records = h.sink.records();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].status, 404);
        assert_eq!(records[0].requested, "p/nope");
        assert!(records[0].attempts.is_empty());
        assert_eq!(records[1].status, 400);
    }

    #[tokio::test]
    async fn the_targets_of_a_route_not_tried_are_recorded_as_skipped() {
        use ultrafast_gateway::store::{RouteSettings, TargetsInput};
        let h = harness("openai").await;
        Mock::given(method("POST"))
            .respond_with(openai_ok())
            .mount(&h.upstream)
            .await;
        let models = h.store.list_models().await.unwrap();
        let id_of = |name: &str| models.iter().find(|m| m.name == name).unwrap().id;
        let (m1, m2) = (id_of("gpt-4o"), id_of("m"));
        let mut tx = h.store.begin().await.unwrap();
        let id = tx
            .insert_route(
                "r",
                &RouteSettings {
                    retries: 2,
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
                primaries: vec![(m1, 1)],
                fallbacks: vec![m2],
            },
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        h.state.refresh().await.unwrap();
        let body = r#"{"model":"r","messages":[{"role":"user","content":"hi"}]}"#;
        let (status, _) = post_chat(&h.app, Some(&h.key), body).await;
        assert_eq!(status, StatusCode::OK);
        let r = &h.sink.records()[0];
        assert_eq!(r.requested, "r");
        let seen: Vec<_> = r
            .attempts
            .iter()
            .map(|a| (a.model.as_str(), a.outcome))
            .collect();
        assert_eq!(
            seen,
            vec![
                ("gpt-4o", AttemptOutcome::Ok),
                ("m", AttemptOutcome::Skipped)
            ]
        );
        assert_eq!(r.attempts[1].status, None);
        assert_eq!(r.attempts[1].duration_ms, 0);
    }
}

#[tokio::test]
async fn proxies_to_gemini_provider_and_returns_openai_shape() {
    let h = harness("gemini").await;
    Mock::given(method("POST"))
        .and(path("/v1beta/models/gpt-4o:generateContent"))
        .and(header("x-goog-api-key", "provider-secret"))
        .and(body_partial_json(
            json!({ "contents": [{ "role": "user", "parts": [{ "text": "hi" }] }] }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "candidates": [{ "content": { "role": "model", "parts": [{ "text": "ciao" }] }, "finishReason": "MAX_TOKENS" }],
            "usageMetadata": { "promptTokenCount": 4, "candidatesTokenCount": 5 },
            "modelVersion": "gemini-x", "responseId": "r1"
        })))
        .expect(1)
        .mount(&h.upstream)
        .await;
    let (status, out) = post_chat(&h.app, Some(&h.key), BODY).await;
    assert_eq!(status, StatusCode::OK, "{out}");
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["choices"][0]["message"]["content"], "ciao");
    assert_eq!(v["choices"][0]["finish_reason"], "length");
    assert_eq!(v["usage"]["total_tokens"], 9);
    assert!(!out.contains("provider-secret"));
}

#[tokio::test]
async fn gemini_errors_pass_through_the_common_rules() {
    let h = harness("gemini").await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": { "code": 400, "message": "API key not valid", "status": "INVALID_ARGUMENT" }
        })))
        .mount(&h.upstream)
        .await;
    let (status, out) = post_chat(&h.app, Some(&h.key), BODY).await;
    assert!(
        status.is_client_error() || status.is_server_error(),
        "{out}"
    );
    assert!(!out.contains("provider-secret"), "{out}");
}

async fn azure_harness(api_version: Option<&str>) -> common::Harness {
    let h = harness("azure").await;
    if let Some(v) = api_version {
        let id = h.store.provider_by_name("p").await.unwrap().unwrap().id;
        let mut tx = h.store.begin().await.unwrap();
        assert!(tx.set_provider_api_version(id, Some(v)).await.unwrap());
        tx.commit().await.unwrap();
        h.state.refresh().await.unwrap();
    }
    h
}

#[tokio::test]
async fn proxies_to_azure_provider_on_its_deployment_url() {
    let h = azure_harness(Some("2025-01-01-preview")).await;
    Mock::given(method("POST"))
        .and(path("/openai/deployments/gpt-4o/chat/completions"))
        .and(wiremock::matchers::query_param(
            "api-version",
            "2025-01-01-preview",
        ))
        .and(header("api-key", "provider-secret"))
        .respond_with(openai_ok())
        .expect(1)
        .mount(&h.upstream)
        .await;
    let (status, out) = post_chat(&h.app, Some(&h.key), BODY).await;
    assert_eq!(status, StatusCode::OK, "{out}");
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["choices"][0]["message"]["content"], "hello");
    assert_eq!(v["usage"]["total_tokens"], 3);
    let sent = &h.upstream.received_requests().await.unwrap()[0];
    let body: Value = serde_json::from_slice(&sent.body).unwrap();
    assert!(body.get("model").is_none(), "{body}");
}

#[tokio::test]
async fn azure_without_an_api_version_uses_the_default() {
    let h = azure_harness(None).await;
    Mock::given(method("POST"))
        .and(path("/openai/deployments/gpt-4o/chat/completions"))
        .and(wiremock::matchers::query_param("api-version", "2024-10-21"))
        .respond_with(openai_ok())
        .expect(1)
        .mount(&h.upstream)
        .await;
    let (status, out) = post_chat(&h.app, Some(&h.key), BODY).await;
    assert_eq!(status, StatusCode::OK, "{out}");
}
