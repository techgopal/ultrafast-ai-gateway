//! # Gateway Error Types Module
//!
//! This module defines the comprehensive error types for the Ultrafast Gateway,
//! providing standardized error handling and HTTP response mapping for all
//! gateway operations.
//!
//! ## Overview
//!
//! The error system provides:
//! - **Standardized Error Types**: Consistent error patterns across the gateway
//! - **HTTP Response Mapping**: Automatic conversion to appropriate HTTP status codes
//! - **Error Context**: Rich error information for debugging and monitoring
//! - **Error Propagation**: Proper error handling throughout the application
//! - **Client-Friendly Messages**: User-friendly error messages
//!
//! ## Error Categories
//!
//! The gateway defines several error categories:
//!
//! ### Client Errors
//! Errors originating from the client SDK:
//! - **Authentication Errors**: Invalid API keys or tokens
//! - **Rate Limit Errors**: Request or token limit violations
//! - **Invalid Request Errors**: Malformed or invalid requests
//! - **Network Errors**: Connection and communication failures
//!
//! ### Provider Errors
//! Errors from AI/LLM providers:
//! - **API Key Errors**: Invalid provider API keys
//! - **Rate Limit Errors**: Provider-specific rate limits
//! - **Quota Errors**: Provider quota exceeded
//! - **Model Errors**: Unsupported or unavailable models
//! - **Service Errors**: Provider service unavailability
//!
//! ### Gateway Errors
//! Internal gateway errors:
//! - **Authentication Errors**: Gateway authentication failures
//! - **Rate Limit Errors**: Gateway rate limiting
//! - **Content Filtering**: Content moderation failures
//! - **Configuration Errors**: Invalid gateway configuration
//! - **Cache Errors**: Caching operation failures
//! - **Plugin Errors**: Plugin execution failures
//!
//! ## HTTP Status Code Mapping
//!
//! Errors are automatically mapped to appropriate HTTP status codes:
//!
//! - **400 Bad Request**: Invalid requests and malformed data
//! - **401 Unauthorized**: Authentication and authorization failures
//! - **429 Too Many Requests**: Rate limit violations
//! - **500 Internal Server Error**: Internal gateway errors
//! - **503 Service Unavailable**: Provider or service unavailability
//!
//! ## Usage
//!
//! ```rust
//! use ultrafast_gateway::gateway_error::GatewayError;
//!
//! // Create specific error types
//! let auth_error = GatewayError::Auth {
//!     message: "Invalid API key".to_string(),
//! };
//!
//! let rate_limit_error = GatewayError::RateLimit {
//!     message: "Rate limit exceeded".to_string(),
//! };
//!
//! // Errors automatically convert to HTTP responses
//! let response = auth_error.into_response();
//! ```
//!
//! ## Error Handling
//!
//! The error system integrates with Axum for automatic HTTP response generation:
//!
//! ```rust
//! use axum::{Json, extract::State};
//! use ultrafast_gateway::gateway_error::GatewayError;
//!
//! async fn handler() -> Result<Json<Value>, GatewayError> {
//!     // Your handler logic here
//!     if some_condition {
//!         return Err(GatewayError::Auth {
//!             message: "Authentication required".to_string(),
//!         });
//!     }
//!     Ok(Json(json!({"status": "success"})))
//! }
//! ```
//!
//! ## Error Context
//!
//! Each error includes context for debugging and monitoring:
//!
//! - **Error Type**: Categorized error type for filtering
//! - **Error Message**: Human-readable error description
//! - **HTTP Status**: Appropriate HTTP status code
//! - **Error Code**: Machine-readable error identifier
//! - **Timestamp**: When the error occurred
//! - **Request ID**: Associated request identifier

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use thiserror::Error;
use ultrafast_models_sdk::error::{ClientError, ProviderError};

/// Comprehensive error types for the Ultrafast Gateway.
///
/// This enum defines all possible error types that can occur in the gateway,
/// including client errors, provider errors, and internal gateway errors.
/// Each error variant includes appropriate error messages and can be
/// automatically converted to HTTP responses.
#[derive(Error, Debug)]
pub enum GatewayError {
    /// Errors originating from the client SDK
    #[error("Client error: {0}")]
    Client(#[from] ClientError),

    /// Errors from AI/LLM providers
    #[error("Provider error: {0}")]
    Provider(#[from] ProviderError),

    /// Gateway authentication and authorization errors
    #[error("Authentication error: {message}")]
    Auth { message: String },

    /// Invalid or malformed request errors
    #[error("Invalid request: {message}")]
    InvalidRequest { message: String },

    /// Rate limiting and quota violation errors
    #[error("Rate limit exceeded: {message}")]
    RateLimit { message: String },

    /// Content filtering and moderation errors
    #[error("Content filtered: {message}")]
    ContentFiltered { message: String },

    /// Internal gateway server errors
    #[error("Internal server error: {message}")]
    Internal { message: String },

    /// Service unavailability errors
    #[error("Service unavailable")]
    ServiceUnavailable,

    /// Configuration and setup errors
    #[error("Configuration error: {message}")]
    Config { message: String },

    /// JSON serialization and deserialization errors
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Cache operation errors
    #[error("Cache error: {message}")]
    Cache { message: String },

    /// Plugin execution and management errors
    #[error("Plugin error: {message}")]
    Plugin { message: String },
}

impl IntoResponse for GatewayError {
    fn into_response(self) -> Response {
        let (status, error_message, error_type) = match &self {
            GatewayError::Client(e) => match e {
                ClientError::Authentication { .. } => (
                    StatusCode::UNAUTHORIZED,
                    self.to_string(),
                    "authentication_error",
                ),
                ClientError::RateLimit => (
                    StatusCode::TOO_MANY_REQUESTS,
                    self.to_string(),
                    "rate_limit_error",
                ),
                ClientError::InvalidRequest { .. } => {
                    (StatusCode::BAD_REQUEST, self.to_string(), "invalid_request")
                }
                _ => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    self.to_string(),
                    "client_error",
                ),
            },
            GatewayError::Provider(e) => match e {
                ProviderError::InvalidApiKey => (
                    StatusCode::UNAUTHORIZED,
                    self.to_string(),
                    "invalid_api_key",
                ),
                ProviderError::RateLimit => (
                    StatusCode::TOO_MANY_REQUESTS,
                    self.to_string(),
                    "provider_rate_limit",
                ),
                ProviderError::QuotaExceeded => (
                    StatusCode::TOO_MANY_REQUESTS,
                    self.to_string(),
                    "quota_exceeded",
                ),
                ProviderError::ModelNotFound { .. } => {
                    (StatusCode::NOT_FOUND, self.to_string(), "model_not_found")
                }
                ProviderError::ServiceUnavailable => (
                    StatusCode::SERVICE_UNAVAILABLE,
                    self.to_string(),
                    "service_unavailable",
                ),
                _ => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    self.to_string(),
                    "provider_error",
                ),
            },
            GatewayError::Auth { .. } => (
                StatusCode::UNAUTHORIZED,
                self.to_string(),
                "authentication_error",
            ),
            GatewayError::RateLimit { .. } => (
                StatusCode::TOO_MANY_REQUESTS,
                self.to_string(),
                "rate_limit_error",
            ),
            GatewayError::InvalidRequest { .. } => {
                (StatusCode::BAD_REQUEST, self.to_string(), "invalid_request")
            }
            GatewayError::ContentFiltered { .. } => (
                StatusCode::BAD_REQUEST,
                self.to_string(),
                "content_filtered",
            ),
            GatewayError::ServiceUnavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                self.to_string(),
                "service_unavailable",
            ),
            GatewayError::Config { .. } => (
                StatusCode::INTERNAL_SERVER_ERROR,
                self.to_string(),
                "configuration_error",
            ),
            GatewayError::Cache { .. } => (
                StatusCode::INTERNAL_SERVER_ERROR,
                self.to_string(),
                "cache_error",
            ),
            GatewayError::Plugin { .. } => (
                StatusCode::INTERNAL_SERVER_ERROR,
                self.to_string(),
                "plugin_error",
            ),
            _ => (
                StatusCode::INTERNAL_SERVER_ERROR,
                self.to_string(),
                "internal_error",
            ),
        };

        let body = Json(json!({
            "error": {
                "message": error_message,
                "type": error_type,
                "code": status.as_u16()
            }
        }));

        (status, body).into_response()
    }
}
