//! The one error type every call returns.

use std::fmt;
use std::time::Duration;

use ultrafast_translate::error::TranslateError;

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
    fn retryable(self) -> bool {
        matches!(
            self,
            ErrorKind::RateLimited | ErrorKind::Upstream | ErrorKind::Network | ErrorKind::Timeout
        )
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Holds no credential: messages come from the server's answer (scrubbed of
/// the key before they get here) or from this crate.
#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    pub kind: ErrorKind,
    pub message: String,
    pub status: Option<u16>,
    pub retryable: bool,
    /// What the server's `Retry-After` asked for, in whole seconds.
    pub retry_after: Option<Duration>,
}

impl Error {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Error {
            kind,
            message: message.into(),
            status: None,
            retryable: kind.retryable(),
            retry_after: None,
        }
    }

    /// An answer with an HTTP status of 300 or more.
    pub(crate) fn from_status(
        status: u16,
        message: impl Into<String>,
        retry_after: Option<Duration>,
    ) -> Self {
        let kind = match status {
            401 => ErrorKind::Auth,
            403 => ErrorKind::Permission,
            404 => ErrorKind::NotFound,
            408 => ErrorKind::Timeout,
            429 => ErrorKind::RateLimited,
            500.. => ErrorKind::Upstream,
            // Redirects are not followed (they would carry the key onward).
            300..=399 => ErrorKind::Upstream,
            _ => ErrorKind::InvalidRequest,
        };
        Error {
            status: Some(status),
            retry_after: if kind == ErrorKind::RateLimited {
                retry_after
            } else {
                None
            },
            ..Error::new(kind, message)
        }
    }

    pub(crate) fn from_translate(e: TranslateError, retry_after: Option<Duration>) -> Self {
        match e {
            TranslateError::InvalidRequest(m) | TranslateError::Unsupported(m) => {
                Error::new(ErrorKind::InvalidRequest, m)
            }
            TranslateError::Provider {
                status, message, ..
            } => Error::from_status(status, message, retry_after),
            TranslateError::Malformed(m) => Error::new(ErrorKind::Malformed, m),
        }
    }

    pub(crate) fn from_reqwest(e: reqwest::Error) -> Self {
        // The URL is dropped: it names hosts and query strings, not needed here.
        let e = e.without_url();
        if e.is_timeout() {
            Error::new(ErrorKind::Timeout, "the request timed out")
        } else if e.is_connect() {
            Error::new(ErrorKind::Network, "could not connect to the server")
        } else if e.is_builder() {
            Error::new(ErrorKind::InvalidRequest, "the request could not be built")
        } else {
            // A body that breaks off mid-way (reqwest calls that a decode error).
            Error::new(ErrorKind::Network, format!("network error: {e}"))
        }
    }

    /// Replaces every occurrence of the key in the message.
    pub(crate) fn scrubbed(mut self, key: Option<&str>) -> Self {
        if let Some(k) = key.filter(|k| k.len() >= 8) {
            self.message = self.message.replace(k, "[redacted]");
        }
        self
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind, self.message)
    }
}

impl std::error::Error for Error {}
