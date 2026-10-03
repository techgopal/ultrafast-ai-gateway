//! How a failed call is classified. The Rust client and the WebAssembly build
//! behind the TypeScript client share this, so the two never disagree on a
//! kind, on whether to retry, or on what a `Retry-After` means.

use std::fmt;

use crate::error::TranslateError;
use crate::provider::provider_error;

/// A `Retry-After` longer than this is capped (seconds).
pub const RETRY_AFTER_CAP_SECS: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorKind {
    Auth,
    Permission,
    NotFound,
    InvalidRequest,
    RateLimited,
    Upstream,
    Network,
    Timeout,
    Malformed,
}

impl ErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorKind::Auth => "auth",
            ErrorKind::Permission => "permission",
            ErrorKind::NotFound => "not_found",
            ErrorKind::InvalidRequest => "invalid_request",
            ErrorKind::RateLimited => "rate_limited",
            ErrorKind::Upstream => "upstream",
            ErrorKind::Network => "network",
            ErrorKind::Timeout => "timeout",
            ErrorKind::Malformed => "malformed",
        }
    }

    /// Whether the same call may succeed when made again.
    pub fn retryable(self) -> bool {
        matches!(
            self,
            ErrorKind::RateLimited | ErrorKind::Upstream | ErrorKind::Network | ErrorKind::Timeout
        )
    }

    /// The kind of an answer with an HTTP status of 300 or more.
    pub fn of_status(status: u16) -> ErrorKind {
        match status {
            401 => ErrorKind::Auth,
            403 => ErrorKind::Permission,
            404 => ErrorKind::NotFound,
            408 => ErrorKind::Timeout,
            429 => ErrorKind::RateLimited,
            500.. => ErrorKind::Upstream,
            // Redirects are not followed (they would carry the key onward):
            // the target is wrong, and asking again will not change it.
            300..=399 => ErrorKind::InvalidRequest,
            _ => ErrorKind::InvalidRequest,
        }
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An error, classified. Holds no credential unless the caller's message does:
/// the caller scrubs its own key.
#[derive(Debug, Clone, PartialEq)]
pub struct Classified {
    pub kind: ErrorKind,
    pub retryable: bool,
    pub status: Option<u16>,
    pub message: String,
    /// Whole seconds; set for 429 and 503 answers only.
    pub retry_after_secs: Option<u64>,
}

impl Classified {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Classified {
            kind,
            retryable: kind.retryable(),
            status: None,
            message: message.into(),
            retry_after_secs: None,
        }
    }

    pub fn from_status(status: u16, message: impl Into<String>, retry_after: Option<u64>) -> Self {
        let kind = ErrorKind::of_status(status);
        Classified {
            status: Some(status),
            retry_after_secs: if kind == ErrorKind::RateLimited || status == 503 {
                retry_after
            } else {
                None
            },
            ..Classified::new(kind, message)
        }
    }

    pub fn from_translate(e: TranslateError, retry_after: Option<u64>) -> Self {
        match e {
            TranslateError::InvalidRequest(m) | TranslateError::Unsupported(m) => {
                Classified::new(ErrorKind::InvalidRequest, m)
            }
            TranslateError::Provider {
                status, message, ..
            } => Classified::from_status(status, message, retry_after),
            TranslateError::Malformed(m) => Classified::new(ErrorKind::Malformed, m),
        }
    }
}

/// `Retry-After` as seconds, capped; an HTTP date is not read.
pub fn parse_retry_after(value: &str) -> Option<u64> {
    let value = value.trim();
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // Digits too many for a number are a very long wait: the cap.
    Some(
        value
            .parse::<u64>()
            .unwrap_or(RETRY_AFTER_CAP_SECS)
            .min(RETRY_AFTER_CAP_SECS),
    )
}

/// `message` with every occurrence of `key` replaced; any non-empty key.
/// Every client runs its error messages through this, so a server that
/// echoes the credential back does not leak it.
pub fn scrub(message: &str, key: &str) -> String {
    if key.is_empty() {
        return message.to_string();
    }
    message.replace(key, "[redacted]")
}

/// An HTTP error answer (any provider's, or a gateway's) from its status,
/// body and `Retry-After` header value.
pub fn classify_answer(status: u16, body: &[u8], retry_after: Option<&str>) -> Classified {
    Classified::from_translate(
        provider_error(status, body),
        retry_after.and_then(parse_retry_after),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_map_to_kinds_and_retryability() {
        let cases = [
            (400, ErrorKind::InvalidRequest, false),
            (401, ErrorKind::Auth, false),
            (403, ErrorKind::Permission, false),
            (404, ErrorKind::NotFound, false),
            (408, ErrorKind::Timeout, true),
            (422, ErrorKind::InvalidRequest, false),
            (429, ErrorKind::RateLimited, true),
            (500, ErrorKind::Upstream, true),
            (503, ErrorKind::Upstream, true),
            (302, ErrorKind::InvalidRequest, false),
        ];
        for (status, kind, retry) in cases {
            let c = classify_answer(status, b"{}", None);
            assert_eq!((c.kind, c.retryable, c.status), (kind, retry, Some(status)));
        }
    }

    #[test]
    fn the_message_comes_from_the_body_in_any_shape() {
        let c = classify_answer(401, br#"{"error":{"message":"bad key"}}"#, None);
        assert_eq!(c.message, "bad key");
        let c = classify_answer(502, b"upstream down", None);
        assert_eq!(c.message, "upstream down");
    }

    #[test]
    fn retry_after_is_read_for_rate_limits_only_and_capped() {
        assert_eq!(
            classify_answer(429, b"{}", Some(" 7 ")).retry_after_secs,
            Some(7)
        );
        assert_eq!(
            classify_answer(503, b"{}", Some("5")).retry_after_secs,
            Some(5)
        );
        for status in [400, 401, 500, 502] {
            assert_eq!(
                classify_answer(status, b"{}", Some("7")).retry_after_secs,
                None,
                "{status}"
            );
        }
        assert_eq!(parse_retry_after("999999999"), Some(RETRY_AFTER_CAP_SECS));
        assert_eq!(
            parse_retry_after("99999999999999999999999"),
            Some(RETRY_AFTER_CAP_SECS)
        );
        for bad in ["", "-1", "1.5", "Wed, 21 Oct 2015 07:28:00 GMT", "+3"] {
            assert_eq!(parse_retry_after(bad), None, "{bad}");
        }
    }

    #[test]
    fn scrub_replaces_every_occurrence_of_any_non_empty_key() {
        assert_eq!(
            scrub("bad k-1 and k-1", "k-1"),
            "bad [redacted] and [redacted]"
        );
        assert_eq!(scrub("a=b", "b"), "a=[redacted]");
        assert_eq!(scrub("nothing here", "zz"), "nothing here");
        assert_eq!(scrub("keep", ""), "keep");
    }

    #[test]
    fn translate_errors_classify() {
        let c = Classified::from_translate(TranslateError::Malformed("x".into()), None);
        assert_eq!(
            (c.kind, c.retryable, c.status),
            (ErrorKind::Malformed, false, None)
        );
        let c = Classified::from_translate(TranslateError::Unsupported("x".into()), None);
        assert_eq!(c.kind, ErrorKind::InvalidRequest);
    }
}
