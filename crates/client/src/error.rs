//! The one error type every call returns.

use std::fmt;
use std::time::Duration;

use ultrafast_translate::classify::{scrub, Classified};
use ultrafast_translate::error::TranslateError;

pub use ultrafast_translate::classify::ErrorKind;

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

    fn of(c: Classified) -> Self {
        Error {
            kind: c.kind,
            message: c.message,
            status: c.status,
            retryable: c.retryable,
            retry_after: c.retry_after_secs.map(Duration::from_secs),
        }
    }

    /// An answer with an HTTP status of 300 or more.
    pub(crate) fn from_status(
        status: u16,
        message: impl Into<String>,
        retry_after: Option<Duration>,
    ) -> Self {
        Error::of(Classified::from_status(
            status,
            message,
            retry_after.map(|d| d.as_secs()),
        ))
    }

    pub(crate) fn from_translate(e: TranslateError, retry_after: Option<Duration>) -> Self {
        Error::of(Classified::from_translate(
            e,
            retry_after.map(|d| d.as_secs()),
        ))
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
        if let Some(k) = key {
            self.message = scrub(&self.message, k);
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
