//! Shared state and the route table.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;

use crate::proxy;
use crate::secrets::Cipher;
use crate::store::Store;

pub const DEFAULT_MAX_BODY_BYTES: usize = 10 * 1024 * 1024;

pub struct AppState {
    pub store: Store,
    pub cipher: Cipher,
    pub http: reqwest::Client,
    pub max_body_bytes: usize,
}

pub fn router(state: Arc<AppState>) -> Router {
    let limit = state.max_body_bytes;
    Router::new()
        .route("/health", get(|| async { Json(json!({ "status": "ok" })) }))
        .route("/v1/chat/completions", post(proxy::chat_completions))
        .layer(DefaultBodyLimit::max(limit))
        .with_state(state)
}

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const TOTAL_TIMEOUT: Duration = Duration::from_secs(300);

/// The client for provider calls. It has timeouts so a hung provider cannot
/// hold a request forever, and never follows redirects, so a provider cannot
/// send a request carrying a credential to another host.
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(TOTAL_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("the HTTP client configuration is valid")
}
