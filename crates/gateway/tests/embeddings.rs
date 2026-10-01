mod common;

use axum::http::StatusCode;
use common::{allow_model, harness, post_to, Harness};
use serde_json::{json, Value};
use ultrafast_gateway::store::{RouteSettings, TargetsInput};
use ultrafast_gateway::telemetry::AttemptOutcome;
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const BODY: &str = r#"{"model":"p/m","input":["a","b"],"dimensions":2}"#;

async fn embed(h: &Harness, body: &str) -> (StatusCode, Value) {
    let bearer = format!("Bearer {}", h.key);
    let (s, _, text) = post_to(
        &h.app,
        "/v1/embeddings",
        &[("authorization", &bearer)],
        body,
    )
    .await;
    (
        s,
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}: {text}")),
    )
}

fn openai_vectors() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "object": "list", "model": "emb-v1",
        "data": [
            { "object": "embedding", "index": 0, "embedding": [0.5, 0.25] },
            { "object": "embedding", "index": 1, "embedding": [1.0, 2.0] }
        ],
        "usage": { "prompt_tokens": 6, "total_tokens": 6 }
    }))
}

fn assert_vectors(v: &Value, tokens: u64) {
    assert_eq!(v["object"], "list");
    assert_eq!(v["data"].as_array().unwrap().len(), 2);
    assert_eq!(v["data"][0]["object"], "embedding");
    assert_eq!(v["data"][0]["index"], 0);
    assert_eq!(v["data"][1]["embedding"], json!([1.0, 2.0]));
    assert_eq!(v["usage"]["prompt_tokens"], tokens);
}

#[tokio::test]
async fn an_openai_provider_embeds() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .and(path("/embeddings"))
        .and(header("authorization", "Bearer provider-secret"))
        .and(body_partial_json(
            json!({ "model": "m", "input": ["a", "b"], "dimensions": 2 }),
        ))
        .respond_with(openai_vectors())
        .expect(1)
        .mount(&h.upstream)
        .await;
    let (s, v) = embed(&h, BODY).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_vectors(&v, 6);
    let r = &h.sink.records()[0];
    assert_eq!(r.endpoint, "embeddings");
    assert_eq!(r.usage.map(|u| u.input_tokens), Some(6));
}

#[tokio::test]
async fn a_single_string_input_works() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .and(body_partial_json(json!({ "input": ["a"] })))
        .respond_with(openai_vectors())
        .expect(1)
        .mount(&h.upstream)
        .await;
    let (s, _) = embed(&h, r#"{"model":"p/m","input":"a"}"#).await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn an_azure_provider_embeds_on_the_deployment() {
    let h = harness("azure").await;
    Mock::given(method("POST"))
        .and(path("/openai/deployments/m/embeddings"))
        .and(header("api-key", "provider-secret"))
        .respond_with(openai_vectors())
        .expect(1)
        .mount(&h.upstream)
        .await;
    let (s, v) = embed(&h, BODY).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_vectors(&v, 6);
}

#[tokio::test]
async fn a_gemini_provider_embeds_in_a_batch() {
    let h = harness("gemini").await;
    Mock::given(method("POST"))
        .and(path("/v1beta/models/m:batchEmbedContents"))
        .and(header("x-goog-api-key", "provider-secret"))
        .and(body_partial_json(json!({ "requests": [
            { "model": "models/m", "content": { "parts": [{ "text": "a" }] }, "outputDimensionality": 2 },
            { "content": { "parts": [{ "text": "b" }] } }
        ]})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "embeddings": [{ "values": [0.5, 0.25] }, { "values": [1.0, 2.0] }]
        })))
        .expect(1)
        .mount(&h.upstream)
        .await;
    let (s, v) = embed(&h, BODY).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_vectors(&v, 0);
}

#[tokio::test]
async fn an_anthropic_model_cannot_embed() {
    let h = harness("anthropic").await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&h.upstream)
        .await;
    let (s, v) = embed(&h, BODY).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(v["error"]["type"], "invalid_request_error");
    assert_eq!(
        v["error"]["message"],
        "This model does not support embeddings."
    );
    assert!(h.upstream.received_requests().await.unwrap().is_empty());
    let r = &h.sink.records()[0];
    assert_eq!(r.endpoint, "embeddings");
    assert_eq!(r.attempts[0].outcome, AttemptOutcome::Skipped);
}

#[tokio::test]
async fn a_route_skips_the_target_that_cannot_embed() {
    let h = harness("anthropic").await;
    let ant_model = allow_model(&h.store, "p", "claude-x").await;
    let other = MockServer::start().await;
    h.store
        .insert_provider("o", "openai", &other.uri(), None)
        .await
        .unwrap();
    let emb_model = allow_model(&h.store, "o", "emb").await;
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
            primaries: vec![(ant_model, 1)],
            fallbacks: vec![emb_model],
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    h.state.refresh().await.unwrap();
    Mock::given(method("POST"))
        .and(path("/embeddings"))
        .respond_with(openai_vectors())
        .expect(1)
        .mount(&other)
        .await;
    let (s, v) = embed(&h, r#"{"model":"r","input":["a","b"]}"#).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_vectors(&v, 6);
    let r = &h.sink.records()[0];
    let seen: Vec<_> = r
        .attempts
        .iter()
        .map(|a| (a.provider.as_str(), a.outcome))
        .collect();
    assert_eq!(
        seen,
        [("p", AttemptOutcome::Skipped), ("o", AttemptOutcome::Ok)]
    );
}

#[tokio::test]
async fn refuses_what_it_cannot_do_and_needs_a_key() {
    let h = harness("openai").await;
    for bad in [
        r#"{"model":"p/m","input":"x","encoding_format":"base64"}"#,
        r#"{"model":"p/m","input":[[1,2]]}"#,
        r#"{"model":"p/m","input":[]}"#,
    ] {
        let (s, v) = embed(&h, bad).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{v}");
        assert_eq!(v["error"]["type"], "invalid_request_error");
    }
    let (s, v) = embed(&h, r#"{"model":"p/nope","input":"x"}"#).await;
    assert_eq!(s, StatusCode::NOT_FOUND, "{v}");
    let (s, _, _) = post_to(&h.app, "/v1/embeddings", &[], BODY).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn provider_errors_follow_the_common_rules() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(400).set_body_json(json!({ "error": { "message": "too long" } })),
        )
        .mount(&h.upstream)
        .await;
    let (s, v) = embed(&h, BODY).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(v["error"]["message"], "too long");
}
