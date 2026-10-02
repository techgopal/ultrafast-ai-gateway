//! Error responses for `/v1`, in the shape of the endpoint's API: OpenAI's,
//! or Anthropic's for `/v1/messages`.

use axum::http::header::RETRY_AFTER;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use ultrafast_translate::error::TranslateError;
use ultrafast_translate::ingress::anthropic;
use ultrafast_translate::ingress::openai::render_error;

use crate::budgets::BudgetRefusal;
use crate::limits::Refusal;

/// Which API's error body a caller expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    OpenAi,
    Anthropic,
}

impl Shape {
    pub fn error(self, status: StatusCode, kind: &str, message: &str) -> Response {
        match self {
            Shape::OpenAi => error_response(status, kind, message),
            // Anthropic's error type follows the status, not the gateway's kind.
            Shape::Anthropic => (
                status,
                Json(anthropic::render_error(status.as_u16(), message)),
            )
                .into_response(),
        }
    }

    /// A call refused by a rate limit: 429 naming the limit, and when to
    /// come back.
    pub fn rate_limited(self, refusal: &Refusal) -> Response {
        let mut response = self.error(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limit_error",
            &refusal.message(),
        );
        response
            .headers_mut()
            .insert(RETRY_AFTER, refusal.retry_after_seconds().into());
        response
    }

    /// A call refused because a budget is spent: 429 naming the budget, and
    /// when the period resets. OpenAI callers get the code `budget_exceeded`;
    /// Anthropic's error body has no code, its type is `rate_limit_error`.
    pub fn budget_exceeded(self, refusal: &BudgetRefusal) -> Response {
        let message = refusal.message();
        let mut response = match self {
            Shape::OpenAi => {
                let mut body = render_error("rate_limit_error", &message);
                body["error"]["code"] = "budget_exceeded".into();
                (StatusCode::TOO_MANY_REQUESTS, Json(body)).into_response()
            }
            Shape::Anthropic => {
                self.error(StatusCode::TOO_MANY_REQUESTS, "rate_limit_error", &message)
            }
        };
        response
            .headers_mut()
            .insert(RETRY_AFTER, refusal.retry_after_seconds().into());
        response
    }

    pub fn translate_error(self, e: &TranslateError) -> Response {
        let (status, kind, message) = caller_message(e);
        self.error(status, kind, &message)
    }
}

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
        let resp = Shape::OpenAi.translate_error(&provider(403, "bad key sk-abc"));
        assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
    }

    #[tokio::test]
    async fn the_anthropic_shape_types_errors_by_status() {
        for (status, kind) in [
            (StatusCode::UNAUTHORIZED, "authentication_error"),
            (StatusCode::FORBIDDEN, "permission_error"),
            (StatusCode::NOT_FOUND, "not_found_error"),
            (StatusCode::TOO_MANY_REQUESTS, "rate_limit_error"),
            (StatusCode::SERVICE_UNAVAILABLE, "overloaded_error"),
            (StatusCode::BAD_GATEWAY, "api_error"),
        ] {
            let resp = Shape::Anthropic.error(status, "upstream_error", "m");
            assert_eq!(resp.status(), status);
            let bytes = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
            let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(v["type"], "error");
            assert_eq!(v["error"]["type"], kind);
            assert_eq!(v["error"]["message"], "m");
        }
    }
}
