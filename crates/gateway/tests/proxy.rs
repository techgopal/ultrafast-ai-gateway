mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{harness, harness_with_limit, post_chat};
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
    assert_eq!(s, StatusCode::BAD_GATEWAY);
    assert!(error_message(&b).contains("Bad Gateway"));

    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(429)
                .set_body_json(json!({ "error": { "message": "slow down" } })),
        )
        .mount(&h.upstream)
        .await;
    let (s, b) = post_chat(&h.app, Some(&h.key), BODY).await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(error_message(&b), "slow down");
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
async fn unreachable_provider_gets_502() {
    let h = harness("openai").await;
    h.store
        .insert_provider("dead", "openai", "http://127.0.0.1:1", None)
        .await
        .unwrap();
    let body = r#"{"model":"dead/m","messages":[{"role":"user","content":"x"}]}"#;
    let (s, b) = post_chat(&h.app, Some(&h.key), body).await;
    assert_eq!(s, StatusCode::BAD_GATEWAY);
    assert!(!error_message(&b).is_empty());
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
