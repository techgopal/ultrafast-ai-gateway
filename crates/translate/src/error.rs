use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Error)]
pub enum TranslateError {
    /// The request is not valid in the caller's format.
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    /// The request is valid but uses something this target cannot express.
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// The provider answered with an error.
    #[error("provider error {status}: {message}")]
    Provider {
        status: u16,
        retryable: bool,
        message: String,
    },
    /// The provider answered with something that could not be read.
    #[error("malformed provider response: {0}")]
    Malformed(String),
}
