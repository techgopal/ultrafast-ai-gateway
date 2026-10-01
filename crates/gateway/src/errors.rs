//! Error responses for `/v1`, in the OpenAI error shape.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use ultrafast_translate::error::TranslateError;
use ultrafast_translate::ingress::openai::render_error;

pub fn error_response(status: StatusCode, kind: &str, message: &str) -> Response {
    (status, Json(render_error(kind, message))).into_response()
}

/// Decides what a caller may see of an error: the status, the error type
/// and the message. Every reply built from a [`TranslateError`], streamed or
/// not, goes through here.
pub fn caller_message(e: &TranslateError) -> (StatusCode, &'static str, String) {
    match e {
        TranslateError::InvalidRequest(m) | TranslateError::Unsupported(m) => {
            (StatusCode::BAD_REQUEST, "invalid_request_error", m.clone())
        }
        TranslateError::Provider {
            status, message, ..
        } => {
            // A caller would read 401/403 as its own gateway key being bad, and
            // the provider's message may describe the gateway's credential.
            if *status == 401 || *status == 403 {
                return (
                    StatusCode::BAD_GATEWAY,
                    "upstream_error",
                    "Provider rejected the gateway's credential.".to_string(),
                );
            }
            let code = if *status >= 500 {
                StatusCode::BAD_GATEWAY
            } else {
                StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_GATEWAY)
            };
            (code, "upstream_error", message.clone())
        }
        TranslateError::Malformed(m) => (StatusCode::BAD_GATEWAY, "upstream_error", m.clone()),
    }
}

pub fn translate_error_response(e: &TranslateError) -> Response {
    let (status, kind, message) = caller_message(e);
    error_response(status, kind, &message)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MASKED: &str = "Provider rejected the gateway's credential.";

    fn provider(status: u16, message: &str) -> TranslateError {
        TranslateError::Provider {
            status,
            retryable: false,
            message: message.into(),
        }
    }

    #[test]
    fn credential_rejections_are_masked() {
        for status in [401, 403] {
            let (code, kind, message) = caller_message(&provider(status, "bad key sk-abc"));
            assert_eq!(code, StatusCode::BAD_GATEWAY);
            assert_eq!(kind, "upstream_error");
            assert_eq!(message, MASKED);
        }
    }

    #[test]
    fn other_provider_errors_keep_the_provider_message() {
        let cases = [
            (500, StatusCode::BAD_GATEWAY),
            (502, StatusCode::BAD_GATEWAY),
            (529, StatusCode::BAD_GATEWAY),
            (429, StatusCode::TOO_MANY_REQUESTS),
            (404, StatusCode::NOT_FOUND),
        ];
        for (status, expected) in cases {
            let (code, kind, message) = caller_message(&provider(status, "slow down"));
            assert_eq!(code, expected, "{status}");
            assert_eq!(kind, "upstream_error");
            assert_eq!(message, "slow down");
        }
    }

    #[test]
    fn malformed_is_502_and_request_errors_are_400() {
        assert_eq!(
            caller_message(&TranslateError::Malformed("bad json".into())),
            (StatusCode::BAD_GATEWAY, "upstream_error", "bad json".into())
        );
        for e in [
            TranslateError::InvalidRequest("no messages".into()),
            TranslateError::Unsupported("no messages".into()),
        ] {
            assert_eq!(
                caller_message(&e),
                (
                    StatusCode::BAD_REQUEST,
                    "invalid_request_error",
                    "no messages".into()
                )
            );
        }
    }

    #[test]
    fn response_uses_the_same_policy() {
        let resp = translate_error_response(&provider(403, "bad key sk-abc"));
        assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
    }
}
