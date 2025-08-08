//! # Error Handling Module
//!
//! This module provides comprehensive error handling for the Ultrafast Models SDK.
//! It defines standardized error types for client operations, provider interactions,
//! and system-level failures with detailed context and debugging information.
//!
//! ## Overview
//!
//! The error handling system provides:
//! - **ClientError**: High-level client operation errors
//! - **ProviderError**: Provider-specific API and communication errors
//! - **Standardized Error Types**: Consistent error patterns across the SDK
//! - **Rich Error Context**: Detailed error information for debugging
//! - **Error Conversion**: Automatic error type conversion and propagation
//!
//! ## Error Categories
//!
//! ### Client Errors
//!
//! High-level errors that occur during client operations:
//! - **Provider Errors**: Wrapped provider-specific errors
//! - **HTTP Errors**: Network and HTTP communication failures
//! - **Serialization Errors**: JSON serialization/deserialization failures
//! - **Configuration Errors**: Invalid client configuration
//! - **Routing Errors**: Provider selection and routing failures
//! - **Cache Errors**: Caching operation failures
//! - **Timeout Errors**: Request timeout failures
//! - **Rate Limit Errors**: Rate limiting violations
//! - **Authentication Errors**: API key and authentication failures
//! - **Invalid Request Errors**: Malformed or invalid requests
//! - **Network Errors**: Network connectivity issues
//!
//! ### Provider Errors
//!
//! Provider-specific errors that occur during API interactions:
//! - **HTTP Errors**: HTTP status code errors
//! - **API Errors**: Provider API-specific errors
//! - **Authentication Errors**: Invalid API keys and credentials
//! - **Model Errors**: Unsupported or invalid model requests
//! - **Rate Limiting**: Provider rate limit violations
//! - **Quota Errors**: Provider quota exceeded
//! - **Service Errors**: Provider service unavailability
//! - **Timeout Errors**: Provider request timeouts
//! - **Serialization Errors**: Response parsing failures
//! - **Validation Errors**: Request validation failures
//!
//! ## Usage Examples
//!
//! ### Basic Error Handling
//!
//! ```rust
//! use ultrafast_models_sdk::{UltrafastClient, ChatRequest, Message, ClientError, ProviderError};
//!
//! let client = UltrafastClient::standalone()
//!     .with_openai("invalid-key")
//!     .build()?;
//!
//! let request = ChatRequest {
//!     model: "gpt-4".to_string(),
//!     messages: vec![Message::user("Hello")],
//!     ..Default::default()
//! };
//!
//! match client.chat_completion(request).await {
//!     Ok(response) => println!("Success: {}", response.choices[0].message.content),
//!     Err(ClientError::Provider(ProviderError::InvalidApiKey)) => {
//!         println!("Invalid API key provided");
//!     }
//!     Err(ClientError::Provider(ProviderError::RateLimit)) => {
//!         println!("Rate limit exceeded, retrying...");
//!     }
//!     Err(ClientError::Timeout) => {
//!         println!("Request timed out");
//!     }
//!     Err(e) => println!("Unexpected error: {}", e),
//! }
//! ```
//!
//! ### Error Conversion
//!
//! The error types support automatic conversion:
//!
//! ```rust
//! use ultrafast_models_sdk::error::{ClientError, ProviderError};
//!
//! // ProviderError automatically converts to ClientError
//! let provider_error = ProviderError::InvalidApiKey;
//! let client_error: ClientError = provider_error.into();
//!
//! // HTTP errors convert to ProviderError
//! let http_error = reqwest::Error::from(std::io::Error::new(
//!     std::io::ErrorKind::ConnectionRefused,
//!     "Connection refused"
//! ));
//! let provider_error: ProviderError = http_error.into();
//! ```
//!
//! ### Custom Error Handling
//!
//! ```rust
//! use ultrafast_models_sdk::{ClientError, ProviderError};
//!
//! fn handle_client_error(error: &ClientError) {
//!     match error {
//!         ClientError::Provider(ProviderError::RateLimit) => {
//!             // Implement exponential backoff
//!             std::thread::sleep(std::time::Duration::from_secs(1));
//!         }
//!         ClientError::Provider(ProviderError::ServiceUnavailable) => {
//!             // Switch to backup provider
//!             println!("Switching to backup provider");
//!         }
//!         ClientError::Timeout => {
//!             // Increase timeout for next request
//!             println!("Increasing timeout");
//!         }
//!         _ => {
//!             // Log and handle other errors
//!             println!("Handling error: {}", error);
//!         }
//!     }
//! }
//! ```
//!
//! ## Error Recovery Strategies
//!
//! The SDK provides several error recovery mechanisms:
//!
//! - **Automatic Retry**: Configurable retry logic with exponential backoff
//! - **Circuit Breakers**: Automatic failover for failing providers
//! - **Load Balancing**: Route requests to healthy providers
//! - **Fallback Providers**: Automatic fallback to backup providers
//! - **Rate Limit Handling**: Automatic rate limit backoff
//!
//! ## Best Practices
//!
//! - Always handle specific error types rather than using catch-all patterns
//! - Implement appropriate retry logic for transient errors
//! - Log errors with sufficient context for debugging
//! - Use circuit breakers to prevent cascading failures
//! - Monitor error rates and implement alerting
//! - Provide user-friendly error messages for end users

use thiserror::Error;

/// High-level client operation errors.
///
/// This enum represents errors that can occur during client operations,
/// including provider errors, network issues, configuration problems,
/// and system-level failures.
///
/// # Examples
///
/// ```rust
/// use ultrafast_models_sdk::ClientError;
///
/// // Handle specific error types
/// match client.chat_completion(request).await {
///     Ok(response) => println!("Success"),
///     Err(ClientError::Provider(provider_error)) => {
///         println!("Provider error: {}", provider_error);
///     }
///     Err(ClientError::Timeout) => {
///         println!("Request timed out");
///     }
///     Err(e) => println!("Other error: {}", e),
/// }
/// ```
#[derive(Error, Debug)]
pub enum ClientError {
    /// Wrapped provider-specific errors
    #[error("Provider error: {0}")]
    Provider(#[from] ProviderError),

    /// HTTP client and network communication errors
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// JSON serialization and deserialization errors
    #[error("Serialization error: {message}")]
    Serialization { message: String },

    /// Invalid or missing configuration errors
    #[error("Configuration error: {message}")]
    Configuration { message: String },

    /// Provider routing and selection errors
    #[error("Routing error: {message}")]
    Routing { message: String },

    /// Cache operation failures
    #[error("Cache error: {message}")]
    Cache { message: String },

    /// Request timeout errors
    #[error("Timeout error")]
    Timeout,

    /// Rate limit exceeded errors
    #[error("Rate limit exceeded")]
    RateLimit,

    /// Authentication and authorization failures
    #[error("Authentication failed: {message}")]
    Authentication { message: String },

    /// Invalid or malformed request errors
    #[error("Invalid request: {message}")]
    InvalidRequest { message: String },

    /// Network connectivity and communication errors
    #[error("Network error: {message}")]
    NetworkError { message: String },
}

/// Provider-specific API and communication errors.
///
/// This enum represents errors that can occur during interactions with
/// individual AI providers, including API errors, authentication failures,
/// rate limiting, and service unavailability.
///
/// # Examples
///
/// ```rust
/// use ultrafast_models_sdk::ProviderError;
///
/// // Handle provider-specific errors
/// match provider.chat_completion(request).await {
///     Ok(response) => println!("Success"),
///     Err(ProviderError::InvalidApiKey) => {
///         println!("Invalid API key");
///     }
///     Err(ProviderError::RateLimit) => {
///         println!("Rate limit exceeded");
///     }
///     Err(ProviderError::ServiceUnavailable) => {
///         println!("Service unavailable");
///     }
///     Err(e) => println!("Other error: {}", e),
/// }
/// ```
#[derive(Error, Debug)]
pub enum ProviderError {
    /// HTTP client and network communication errors
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    /// Provider API-specific errors with status code and message
    #[error("API error: {code} - {message}")]
    Api { code: u16, message: String },

    /// Invalid or missing API key errors
    #[error("Invalid API key")]
    InvalidApiKey,

    /// Requested model not found or unsupported
    #[error("Model not found: {model}")]
    ModelNotFound { model: String },

    /// Rate limit exceeded for this provider
    #[error("Rate limit exceeded")]
    RateLimit,

    /// Provider quota exceeded
    #[error("Quota exceeded")]
    QuotaExceeded,

    /// Provider service temporarily unavailable
    #[error("Service unavailable")]
    ServiceUnavailable,

    /// Request timeout errors
    #[error("Timeout")]
    Timeout,

    /// JSON serialization and deserialization errors
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Invalid or malformed response format
    #[error("Invalid response format")]
    InvalidResponse,

    /// Invalid or missing provider configuration
    #[error("Configuration error: {message}")]
    Configuration { message: String },

    /// Unsupported provider type
    #[error("Provider not supported: {provider}")]
    ProviderNotSupported { provider: String },

    /// Unsupported feature for this provider
    #[error("Feature not supported: {feature}")]
    FeatureNotSupported { feature: String },

    /// Authentication and authorization failures
    #[error("Authentication failed: {reason}")]
    AuthenticationFailed { reason: String },

    /// Request validation failures
    #[error("Request validation failed: {field} - {message}")]
    ValidationError { field: String, message: String },

    /// Network connectivity and communication errors
    #[error("Network error: {message}")]
    NetworkError { message: String },

    /// Retryable errors that can be attempted again
    #[error("Retryable error: {message}")]
    RetryableError { message: String },
}
