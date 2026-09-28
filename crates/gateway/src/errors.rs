//! Error responses for `/v1`, in the OpenAI error shape.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use ultrafast_translate::error::TranslateError;
use ultrafast_translate::ingress::openai::render_error;

pub fn error_response(status: StatusCode, kind: &str, message: &str) -> Response {
    (status, Json(render_error(kind, message))).into_response()
}

pub fn translate_error_response(e: &TranslateError) -> Response {
    match e {
        TranslateError::InvalidRequest(m) | TranslateError::Unsupported(m) => {
            error_response(StatusCode::BAD_REQUEST, "invalid_request_error", m)
        }
        TranslateError::Provider {
            status, message, ..
        } => {
            // A caller would read 401/403 as its own gateway key being bad, and
            // the provider's message may describe the gateway's credential.
            if *status == 401 || *status == 403 {
                return error_response(
                    StatusCode::BAD_GATEWAY,
                    "upstream_error",
                    "Provider rejected the gateway's credential.",
                );
            }
            let code = if *status >= 500 {
                StatusCode::BAD_GATEWAY
            } else {
                StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_GATEWAY)
            };
            error_response(code, "upstream_error", message)
        }
        TranslateError::Malformed(m) => {
            error_response(StatusCode::BAD_GATEWAY, "upstream_error", m)
        }
    }
}
