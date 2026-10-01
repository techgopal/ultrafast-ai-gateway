//! The `/v1/chat/completions` handler.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures::StreamExt;
use http_body_util::LengthLimitError;
use ultrafast_translate::error::TranslateError;
use ultrafast_translate::ingress::openai::{
    parse_request, render_response, render_stream_error, render_stream_event,
};
use ultrafast_translate::provider::{
    build_request, parse_response, HttpRequest, ProviderKind, StreamDecoder, Target,
};
use ultrafast_translate::types::StreamEvent;

use crate::access::{self, Denied};
use crate::app::AppState;
use crate::auth::authenticate;
use crate::errors::{caller_message, error_response, translate_error_response};

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
        &format!("Unknown model '{model}'."),
    )
}

fn forbidden(model: &str) -> Response {
    error_response(
        StatusCode::FORBIDDEN,
        "permission_error",
        &format!("You do not have access to model '{model}'."),
    )
}

/// `GET /v1/models`: what the key can call, models and routes.
pub async fn list_models(State(state): State<Arc<AppState>>, request: Request) -> Response {
    let snapshot = state.snapshot.load_full();
    let key = match authenticate(&snapshot, request.headers()) {
        Ok(key) => key,
        Err(resp) => return resp,
    };
    let data: Vec<_> = access::callable_names(&snapshot, &key)
        .into_iter()
        .map(|(id, owned_by)| {
            serde_json::json!({
                "id": id, "object": "model", "created": 0, "owned_by": owned_by
            })
        })
        .collect();
    Json(serde_json::json!({ "object": "list", "data": data })).into_response()
}

pub async fn chat_completions(State(state): State<Arc<AppState>>, request: Request) -> Response {
    // 1. Authenticate on the headers alone. The body has not been read yet.
    let (parts, body) = request.into_parts();
    // One snapshot serves the whole request.
    let snapshot = state.snapshot.load_full();
    let key = match authenticate(&snapshot, &parts.headers) {
        Ok(key) => key,
        Err(resp) => return resp,
    };

    // 2. Read and parse the body.
    let body = match axum::body::to_bytes(body, state.max_body_bytes).await {
        Ok(b) => b,
        Err(e) => {
            let too_large = e.into_inner().is::<LengthLimitError>();
            return if too_large {
                error_response(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "invalid_request_error",
                    "Request body is too large.",
                )
            } else {
                error_response(
                    StatusCode::BAD_REQUEST,
                    "invalid_request_error",
                    "Request body could not be read.",
                )
            };
        }
    };
    let req = match parse_request(&body) {
        Ok(r) => r,
        Err(e) => return translate_error_response(&e),
    };

    // 3. Resolve the name to something this key may call.
    let resolved = match access::resolve(&snapshot, &key, &req.model) {
        Ok(r) => r,
        Err(Denied::Unknown) => return not_found(&req.model),
        Err(Denied::Forbidden) => return forbidden(&req.model),
    };
    // Until the routing engine, the first target is called, with no retry.
    let targets = access::callable_targets(&snapshot, &key, &resolved);
    let Some(first) = targets.first() else {
        return error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "upstream_error",
            "No model is available for this request.",
        );
    };
    let provider = first.provider;
    let kind = provider.kind;
    let target = Target {
        kind,
        base_url: provider.base_url.clone(),
        api_key: provider.api_key.clone(),
        model: first.model.to_string(),
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
        return stream_to_caller(upstream, kind, target.model, provider.name.clone());
    }
    let bytes = match read_capped(upstream, state.max_provider_response_bytes).await {
        Ok(b) => b,
        Err(ReadError::TooLarge) => {
            tracing::warn!(provider = %provider.name, "provider response was too large");
            return error_response(
                StatusCode::BAD_GATEWAY,
                "upstream_error",
                "The provider response was too large.",
            );
        }
        Err(ReadError::Failed) => {
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

enum ReadError {
    TooLarge,
    Failed,
}

/// Reads a provider response, giving up once it is larger than `max` bytes.
async fn read_capped(upstream: reqwest::Response, max: usize) -> Result<Vec<u8>, ReadError> {
    let mut out = Vec::new();
    let mut chunks = upstream.bytes_stream();
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk.map_err(|_| ReadError::Failed)?;
        if chunk.len() > max - out.len() {
            return Err(ReadError::TooLarge);
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
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
    crate::secrets::fill_random(&mut bytes);
    format!("chatcmpl-{}", hex::encode(bytes))
}

/// Logs a stream failure and renders the error event the caller may see.
fn stream_failure(provider: &str, e: &TranslateError) -> String {
    let (_, _, message) = caller_message(e);
    // The provider's text about a rejected credential may quote the
    // credential, so only the masked message is logged for it.
    let credential = matches!(
        e,
        TranslateError::Provider {
            status: 401 | 403,
            ..
        }
    );
    if credential {
        tracing::warn!(provider = %provider, error = %message, "provider stream failed");
    } else {
        tracing::warn!(provider = %provider, error = %e, "provider stream failed");
    }
    render_stream_error(&message)
}

/// Forwards the provider's stream to the caller as OpenAI server-sent events.
///
/// The body owns the upstream response, so when the caller disconnects and the
/// body is dropped, the provider request is dropped with it.
fn stream_to_caller(
    upstream: reqwest::Response,
    kind: ProviderKind,
    model: String,
    provider: String,
) -> Response {
    let created = now_secs();
    let id = stream_id();
    let body = async_stream::stream! {
        let mut decoder = StreamDecoder::new(kind);
        let mut chunks = upstream.bytes_stream();
        while let Some(chunk) = chunks.next().await {
            let bytes = match chunk {
                Ok(b) => b,
                Err(e) => {
                    let e = e.without_url();
                    tracing::warn!(provider = %provider, error = %e, "provider stream was lost");
                    yield Ok::<String, Infallible>(render_stream_error(
                        "The connection to the provider was lost.",
                    ));
                    return;
                }
            };
            let events = match decoder.feed(&bytes) {
                Ok(events) => events,
                Err(e) => {
                    yield Ok(stream_failure(&provider, &e));
                    return;
                }
            };
            for ev in events {
                let done = matches!(ev, StreamEvent::Done { .. });
                yield Ok(render_stream_event(&ev, &id, &model, created));
                if done {
                    return;
                }
            }
            // An error that followed those events in the same chunk.
            if let Some(e) = decoder.take_error() {
                yield Ok(stream_failure(&provider, &e));
                return;
            }
        }
        tracing::warn!(provider = %provider, "provider stream ended before completion");
        yield Ok(render_stream_error("The provider stream ended before completion."));
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "text/event-stream")
        .header(CACHE_CONTROL, "no-cache")
        .body(Body::from_stream(body))
        .expect("static headers are valid")
}
