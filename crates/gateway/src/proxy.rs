//! The `/v1/chat/completions` handler.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::extract::rejection::BytesRejection;
use axum::extract::State;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use bytes::Bytes;
use futures::StreamExt;
use rand::rngs::OsRng;
use rand::RngCore;
use ultrafast_translate::ingress::openai::{
    parse_request, render_response, render_stream_error, render_stream_event,
};
use ultrafast_translate::provider::{
    build_request, parse_response, HttpRequest, ProviderKind, StreamDecoder, Target,
};
use ultrafast_translate::types::StreamEvent;

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

fn stream_id() -> String {
    let mut bytes = [0u8; 12];
    OsRng.fill_bytes(&mut bytes);
    format!("chatcmpl-{}", hex::encode(bytes))
}

/// Forwards the provider's stream to the caller as OpenAI server-sent events.
///
/// The body owns the upstream response, so when the caller disconnects and the
/// body is dropped, the provider request is dropped with it.
pub fn stream_response(upstream: reqwest::Response, kind: ProviderKind, model: String) -> Response {
    let created = now_secs();
    let id = stream_id();
    let body = async_stream::stream! {
        let mut decoder = StreamDecoder::new(kind);
        let mut chunks = upstream.bytes_stream();
        while let Some(chunk) = chunks.next().await {
            let bytes = match chunk {
                Ok(b) => b,
                Err(_) => {
                    yield Ok::<String, Infallible>(render_stream_error(
                        "The connection to the provider was lost.",
                    ));
                    return;
                }
            };
            // The decoder returns either events or an error for a whole feed.
            // Feeding one line at a time completes at most one provider event
            // per call, so events that precede an error are still forwarded.
            for line in bytes.split_inclusive(|b| *b == b'\n') {
                match decoder.feed(line) {
                    Ok(events) => {
                        for ev in events {
                            let done = matches!(ev, StreamEvent::Done { .. });
                            yield Ok(render_stream_event(&ev, &id, &model, created));
                            if done {
                                return;
                            }
                        }
                    }
                    Err(e) => {
                        yield Ok(render_stream_error(&e.to_string()));
                        return;
                    }
                }
            }
        }
        yield Ok(render_stream_error("The provider stream ended before completion."));
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "text/event-stream")
        .header(CACHE_CONTROL, "no-cache")
        .body(Body::from_stream(body))
        .expect("static headers are valid")
}
