//! `POST /v1/images/generations`: image generation over OpenAI, Azure and
//! OpenAI-compatible providers, through access, guardrails, logs and metrics.

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{allow_model, harness, harness_with_metrics_token, post_to, Harness};
use serde_json::{json, Value};
use tower::ServiceExt;
use ultrafast_gateway::cache::{CacheScope, RouteCache};
use ultrafast_gateway::logs::{cost, Price};
use ultrafast_gateway::store::{Grants, NewGuardrail, RouteSettings, TargetsInput};
use ultrafast_gateway::telemetry::AttemptOutcome;
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SETTINGS: RouteSettings = RouteSettings {
    retries: 0,
    first_token_timeout_ms: 30_000,
    total_timeout_ms: 300_000,
    breaker_failures: 5,
    breaker_window_s: 60,
    breaker_open_s: 30,
};

const BODY: &str = r#"{"model":"p/m","prompt":"a red fox","n":2,"size":"1024x1024","quality":"low","output_format":"png"}"#;

async fn generate(h: &Harness, body: &str) -> (StatusCode, Value) {
    let bearer = format!("Bearer {}", h.key);
    let (s, _, text) = post_to(
        &h.app,
        "/v1/images/generations",
        &[("authorization", &bearer)],
        body,
    )
    .await;
    (
        s,
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}: {text}")),
    )
}

/// What gpt-image-1 answers: base64 images and token usage.
fn with_usage() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "created": 1700000000, "size": "1024x1024", "quality": "low", "output_format": "png",
        "data": [{ "b64_json": "AAAA" }, { "b64_json": "BBBB" }],
        "usage": { "input_tokens": 12, "output_tokens": 800, "total_tokens": 812,
                   "input_tokens_details": { "text_tokens": 12, "image_tokens": 0 } }
    }))
}

/// What dall-e answers: no usage.
fn without_usage() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "created": 1700000000,
        "data": [{ "b64_json": "AAAA", "revised_prompt": "a fox, red" }]
    }))
}

#[tokio::test]
async fn an_openai_provider_generates() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .and(path("/images/generations"))
        .and(header("authorization", "Bearer provider-secret"))
        .and(body_partial_json(json!({
            "model": "m", "prompt": "a red fox", "n": 2, "size": "1024x1024",
            "quality": "low", "output_format": "png"
        })))
        .respond_with(with_usage())
        .expect(1)
        .mount(&h.upstream)
        .await;
    let (s, v) = generate(&h, BODY).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["created"], 1_700_000_000u64);
    assert_eq!(v["data"][0]["b64_json"], "AAAA");
    assert_eq!(v["data"][1]["b64_json"], "BBBB");
    assert_eq!(v["size"], "1024x1024");
    assert_eq!(v["usage"]["input_tokens"], 12);
    assert_eq!(v["usage"]["output_tokens"], 800);
    assert_eq!(v["usage"]["total_tokens"], 812);
    let r = &h.sink.records()[0];
    assert_eq!(r.endpoint, "images");
    assert!(!r.stream);
    let u = r.usage.expect("usage");
    assert_eq!((u.input_tokens, u.output_tokens), (12, 800));
}

#[tokio::test]
async fn an_openai_compatible_provider_with_a_path_prefix_generates() {
    let h = harness("openai").await;
    let other = MockServer::start().await;
    h.store
        .insert_provider("o", "openai", &format!("{}/v1", other.uri()), None)
        .await
        .unwrap();
    allow_model(&h.store, "o", "img").await;
    h.state.refresh().await.unwrap();
    Mock::given(method("POST"))
        .and(path("/v1/images/generations"))
        .and(body_partial_json(json!({ "model": "img" })))
        .respond_with(with_usage())
        .expect(1)
        .mount(&other)
        .await;
    let (s, v) = generate(&h, r#"{"model":"o/img","prompt":"x"}"#).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert!(h.upstream.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn an_azure_provider_generates_on_the_deployment() {
    let h = harness("azure").await;
    Mock::given(method("POST"))
        .and(path("/openai/deployments/m/images/generations"))
        .and(header("api-key", "provider-secret"))
        .respond_with(with_usage())
        .expect(1)
        .mount(&h.upstream)
        .await;
    let (s, v) = generate(&h, BODY).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let seen = &h.upstream.received_requests().await.unwrap()[0];
    assert!(seen.url.query().unwrap().starts_with("api-version="));
    let sent: Value = serde_json::from_slice(&seen.body).unwrap();
    assert!(sent.get("model").is_none(), "{sent}");
    assert_eq!(sent["prompt"], "a red fox");
}

#[tokio::test]
async fn anthropic_and_gemini_are_refused_before_any_upstream_call() {
    for kind in ["anthropic", "gemini"] {
        let h = harness(kind).await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&h.upstream)
            .await;
        let (s, v) = generate(&h, BODY).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{kind}: {v}");
        assert_eq!(v["error"]["type"], "invalid_request_error");
        assert_eq!(
            v["error"]["message"],
            "This model does not support image generation."
        );
        assert!(h.upstream.received_requests().await.unwrap().is_empty());
        let r = &h.sink.records()[0];
        assert_eq!(r.endpoint, "images");
        assert_eq!(r.attempts[0].outcome, AttemptOutcome::Skipped);
    }
}

#[tokio::test]
async fn cost_comes_from_usage_when_the_provider_gives_it() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(with_usage())
        .up_to_n_times(1)
        .mount(&h.upstream)
        .await;
    Mock::given(method("POST"))
        .respond_with(without_usage())
        .mount(&h.upstream)
        .await;
    let price = Some(Price {
        input_micros: Some(5_000_000),
        output_micros: Some(40_000_000),
    });
    let (s, _) = generate(&h, BODY).await;
    assert_eq!(s, StatusCode::OK);
    let (s, v) = generate(&h, r#"{"model":"p/m","prompt":"x"}"#).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["data"][0]["revised_prompt"], "a fox, red");
    assert!(v.get("usage").is_none());
    let records = h.sink.records();
    // 12 * $5 + 800 * $40 per million tokens = 32 060 millionths of a dollar.
    assert_eq!(cost(records[0].usage, price), (32_060, true));
    assert!(records[1].usage.is_none());
    assert_eq!(cost(records[1].usage, price), (0, false));
}

#[tokio::test]
async fn refuses_what_it_cannot_do_and_needs_a_key() {
    let h = harness("openai").await;
    for bad in [
        r#"{"model":"p/m","prompt":""}"#,
        r#"{"model":"p/m"}"#,
        r#"{"model":"p/m","prompt":"x","n":11}"#,
        r#"{"model":"p/m","prompt":"x","output_format":"gif"}"#,
        r#"{"model":"p/m","prompt":"x","stream":true}"#,
        r#"{"model":"p/m","prompt":"x","partial_images":1}"#,
        r#"{"model":"p/m","prompt":"x","image":"y"}"#,
    ] {
        let (s, v) = generate(&h, bad).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{bad}: {v}");
        assert_eq!(v["error"]["type"], "invalid_request_error");
    }
    let (s, v) = generate(&h, r#"{"model":"p/nope","prompt":"x"}"#).await;
    assert_eq!(s, StatusCode::NOT_FOUND, "{v}");
    let (s, _, text) = post_to(&h.app, "/v1/images/generations", &[], BODY).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap()["error"]["type"],
        "authentication_error"
    );
    assert!(h.upstream.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_provider_error_follows_the_common_rules() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(400).set_body_json(
            json!({ "error": { "message": "Your request was rejected by the safety system." } }),
        ))
        .mount(&h.upstream)
        .await;
    let (s, v) = generate(&h, BODY).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(
        v["error"]["message"],
        "Your request was rejected by the safety system."
    );
}

// Review focus 5: access is decided as for every other call.
#[tokio::test]
async fn a_model_the_key_may_not_call_is_refused_and_a_route_skips_it() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .and(path("/images/generations"))
        .respond_with(with_usage())
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
    let route = tx.insert_route("r", &SETTINGS, true).await.unwrap();
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
    let (s, v) = generate(&h, r#"{"model":"p/hidden","prompt":"x"}"#).await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{v}");
    assert_eq!(v["error"]["type"], "permission_error");
    assert!(h.upstream.received_requests().await.unwrap().is_empty());

    let (s, v) = generate(&h, r#"{"model":"r","prompt":"x"}"#).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let seen: Vec<_> = h
        .sink
        .records()
        .last()
        .unwrap()
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

async fn guardrail(h: &Harness, rules: Value) {
    let rules = rules.to_string();
    let mut tx = h.store.begin().await.unwrap();
    tx.insert_guardrail(NewGuardrail {
        name: "g",
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
async fn guardrails_check_the_prompt_and_never_the_images() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "created": 1,
            "data": [{ "b64_json": "swordfish", "revised_prompt": "swordfish" }]
        })))
        .mount(&h.upstream)
        .await;
    guardrail(
        &h,
        json!([
            { "id": "w", "matcher": { "keywords": { "words": ["swordfish"] } },
              "action": "block", "directions": "both" },
            { "id": "e", "matcher": { "pii": ["EMAIL"] },
              "action": "redact", "directions": "input" }
        ]),
    )
    .await;
    // Blocked on the prompt: nothing reaches the provider.
    let (s, v) = generate(&h, r#"{"model":"p/m","prompt":"the word is swordfish"}"#).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(v["error"]["code"], "guardrail_blocked");
    assert!(h.upstream.received_requests().await.unwrap().is_empty());
    // Redacted on the prompt: the provider gets the redacted text.
    let (s, v) = generate(&h, r#"{"model":"p/m","prompt":"mail ada@example.com"}"#).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let sent: Value =
        serde_json::from_slice(&h.upstream.received_requests().await.unwrap()[0].body).unwrap();
    assert!(!sent["prompt"].as_str().unwrap().contains("ada@example.com"));
    // What came back (a "forbidden" word in the data) is not inspected.
    assert_eq!(v["data"][0]["b64_json"], "swordfish");
}

#[tokio::test]
async fn an_image_call_is_never_cached() {
    let h = harness("openai").await;
    let model = allow_model(&h.store, "p", "img").await;
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
        .respond_with(with_usage())
        .mount(&h.upstream)
        .await;
    for _ in 0..2 {
        let (s, v) = generate(&h, r#"{"model":"r","prompt":"x"}"#).await;
        assert_eq!(s, StatusCode::OK, "{v}");
    }
    assert_eq!(h.upstream.received_requests().await.unwrap().len(), 2);
    assert!(h.sink.records().iter().all(|r| !r.cached));
}

#[tokio::test]
async fn the_call_is_counted_as_images() {
    let h = harness_with_metrics_token("openai", Some("scrape-token-0123456789")).await;
    Mock::given(method("POST"))
        .respond_with(with_usage())
        .mount(&h.upstream)
        .await;
    assert_eq!(generate(&h, BODY).await.0, StatusCode::OK);
    let resp = h
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .header("authorization", "Bearer scrape-token-0123456789")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(
        text.contains(r#"uf_requests_total{endpoint="images",status_class="2xx"} 1"#),
        "{text}"
    );
}
