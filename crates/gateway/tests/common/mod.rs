#![allow(dead_code)]

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use tower::ServiceExt;
use ultrafast_gateway::app::{
    http_client, router, AppState, DEFAULT_MAX_BODY_BYTES, DEFAULT_MAX_PROVIDER_RESPONSE_BYTES,
};
use ultrafast_gateway::secrets::{generate_key, Cipher};
use ultrafast_gateway::store::Store;
use wiremock::MockServer;

pub struct Harness {
    pub app: Router,
    pub upstream: MockServer,
    pub key: String,
    pub store: Store,
}

/// A gateway with one provider named "p" of the given kind, pointing at a mock server.
pub async fn harness(kind: &str) -> Harness {
    harness_with_limit(kind, DEFAULT_MAX_BODY_BYTES).await
}

pub async fn harness_with_limit(kind: &str, max_body_bytes: usize) -> Harness {
    harness_with_limits(kind, max_body_bytes, DEFAULT_MAX_PROVIDER_RESPONSE_BYTES).await
}

/// Like [`harness`], with a cap on the provider response size.
pub async fn harness_with_response_limit(kind: &str, max_response_bytes: usize) -> Harness {
    harness_with_limits(kind, DEFAULT_MAX_BODY_BYTES, max_response_bytes).await
}

async fn harness_with_limits(
    kind: &str,
    max_body_bytes: usize,
    max_provider_response_bytes: usize,
) -> Harness {
    let upstream = MockServer::start().await;
    let store = Store::open_in_memory().await.unwrap();
    let cipher = Cipher::from_hex(&Cipher::generate_master_hex()).unwrap();
    let credential = cipher.encrypt(b"provider-secret");
    store
        .insert_provider("p", kind, &upstream.uri(), Some(&credential))
        .await
        .unwrap();
    let key = generate_key();
    store
        .insert_key("test", &key.hash, &key.display, None)
        .await
        .unwrap();
    let state = Arc::new(AppState {
        store: store.clone(),
        cipher,
        http: http_client(),
        max_body_bytes,
        max_provider_response_bytes,
    });
    Harness {
        app: router(state),
        upstream,
        key: key.full,
        store,
    }
}

pub async fn post_chat(app: &Router, key: Option<&str>, body: &str) -> (StatusCode, String) {
    let mut req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json");
    if let Some(k) = key {
        req = req.header("authorization", format!("Bearer {k}"));
    }
    let resp = app
        .clone()
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}
