//! The `/v1/chat/completions` handler.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::rejection::BytesRejection;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use bytes::Bytes;
use ultrafast_translate::ingress::openai::{parse_request, render_response};
use ultrafast_translate::provider::{
    build_request, parse_response, HttpRequest, ProviderKind, Target,
};

use crate::app::AppState;
use crate::auth::authenticate;
use crate::errors::{error_response, translate_error_response};

pub(crate) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn not_found(model: &str) -> Response {
    error_response(
        StatusCode::NOT_FOUND,
        "not_found_error",
        &format!("Unknown model '{model}'. Use the form provider/model."),
    )
}

fn server_error(message: &str) -> Response {
    error_response(StatusCode::INTERNAL_SERVER_ERROR, "server_error", message)
}

pub async fn chat_completions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    // 1. Authenticate before looking at anything else.
    if let Err(resp) = authenticate(&state.store, &headers).await {
        return resp;
    }

    // 2. Read and parse the body.
    let body = match body {
        Ok(b) => b,
        Err(_) => {
            return error_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                "invalid_request_error",
                "Request body is too large or could not be read.",
            )
        }
    };
    let req = match parse_request(&body) {
        Ok(r) => r,
        Err(e) => return translate_error_response(&e),
    };

    // 3. Resolve provider/model.
    let Some((provider_name, model)) = req.model.split_once('/') else {
        return not_found(&req.model);
    };
    if provider_name.is_empty() || model.is_empty() {
        return not_found(&req.model);
    }
    let provider = match state.store.provider_by_name(provider_name).await {
        Ok(Some(p)) => p,
        Ok(None) => return not_found(&req.model),
        Err(e) => {
            tracing::error!(error = %e, "provider lookup failed");
            return server_error("Could not load the provider.");
        }
    };
    let Some(kind) = ProviderKind::parse(&provider.kind) else {
        tracing::error!(provider = %provider.name, kind = %provider.kind, "unknown provider kind");
        return server_error("The provider is misconfigured.");
    };
    let api_key = match provider
        .credential
        .as_deref()
        .map(|c| state.cipher.decrypt(c))
    {
        None => None,
        Some(Ok(bytes)) => match String::from_utf8(bytes) {
            Ok(s) => Some(s),
            Err(_) => return server_error("The provider credential is unreadable."),
        },
        Some(Err(e)) => {
            tracing::error!(provider = %provider.name, error = %e, "credential decrypt failed");
            return server_error("The provider credential is unreadable.");
        }
    };
    let target = Target {
        kind,
        base_url: provider.base_url,
        api_key,
        model: model.to_string(),
    };

    // 4. Call the provider.
    let out = match build_request(&target, &req) {
        Ok(o) => o,
        Err(e) => return translate_error_response(&e),
    };
    let upstream = match send(&state.http, out).await {
        Ok(r) => r,
        Err(e) => {
            // `without_url` keeps credentials in query strings out of logs and replies.
            let e = e.without_url();
            tracing::warn!(provider = %provider.name, error = %e, "provider unreachable");
            return error_response(
                StatusCode::BAD_GATEWAY,
                "upstream_error",
                &format!("Could not reach provider '{}'.", provider.name),
            );
        }
    };

    let status = upstream.status().as_u16();
    // Redirects are not followed, and a redirect is never a usable answer.
    if (300..400).contains(&status) {
        tracing::warn!(provider = %provider.name, status, "provider answered with a redirect");
        return error_response(
            StatusCode::BAD_GATEWAY,
            "upstream_error",
            &format!("Provider '{}' answered with a redirect.", provider.name),
        );
    }
    if req.stream && status < 400 {
        return stream_response(upstream, kind, target.model);
    }
    let bytes = match upstream.bytes().await {
        Ok(b) => b,
        Err(_) => {
            return error_response(
                StatusCode::BAD_GATEWAY,
                "upstream_error",
                "The provider response could not be read.",
            )
        }
    };
    match parse_response(kind, status, &bytes) {
        Ok(r) => Json(render_response(&r, now_secs())).into_response(),
        Err(e) => translate_error_response(&e),
    }
}

async fn send(
    http: &reqwest::Client,
    out: HttpRequest,
) -> Result<reqwest::Response, reqwest::Error> {
    let mut rb = http.post(&out.url);
    for (k, v) in &out.headers {
        rb = rb.header(k, v);
    }
    rb.body(out.body).send().await
}

pub fn stream_response(
    _upstream: reqwest::Response,
    _kind: ProviderKind,
    _model: String,
) -> Response {
    error_response(
        StatusCode::NOT_IMPLEMENTED,
        "invalid_request_error",
        "Streaming is not available in this build.",
    )
}
