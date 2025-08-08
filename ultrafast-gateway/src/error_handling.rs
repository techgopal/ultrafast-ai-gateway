//! # Error Handling and Validation Module
//!
//! This module provides comprehensive error handling, validation, and debugging
//! utilities for the Ultrafast Gateway. It includes standardized error patterns,
//! validation helpers, and context-aware error handling.
//!
//! ## Overview
//!
//! The error handling system provides:
//! - **Standardized Error Types**: Consistent error patterns across the gateway
//! - **Context-Aware Handling**: Rich error context with debugging information
//! - **Validation Utilities**: Input validation and sanitization helpers
//! - **Retry Logic**: Automatic retry with exponential backoff
//! - **Error Logging**: Structured error logging with severity levels
//! - **Result Extensions**: Convenient error conversion and handling
//!
//! ## Error Types
//!
//! The system defines several error categories:
//!
//! - **Configuration Errors**: Invalid or missing configuration
//! - **Authentication Errors**: API key and JWT validation failures
//! - **Rate Limiting Errors**: Request and token limit violations
//! - **Content Filtering Errors**: Content moderation failures
//! - **Plugin Errors**: Dynamic plugin execution failures
//! - **Cache Errors**: Cache operation failures
//! - **Internal Errors**: System-level failures
//! - **Service Unavailable**: Provider or service outages
//! - **Invalid Request**: Malformed or invalid requests
//!
//! ## Error Context
//!
//! Each error includes rich context for debugging:
//!
//! - **Module**: Which module generated the error
//! - **Operation**: What operation was being performed
//! - **Details**: Specific error details and stack traces
//! - **Timestamp**: When the error occurred
//! - **Request ID**: Associated request identifier
//! - **User ID**: Associated user identifier
//! - **Severity**: Error severity level
//!
//! ## Usage
//!
//! ```rust
//! use ultrafast_gateway::error_handling::{ErrorHandler, ErrorContext, ErrorSeverity};
//!
//! // Basic error handling
//! let result = ErrorHandler::handle_sync_operation(
//!     || -> anyhow::Result<String> { Ok("success".to_string()) },
//!     "database operation",
//!     ErrorType::Internal,
//! );
//!
//! // Context-aware error handling
//! let context = ErrorContext::new("auth", "validate_token", "JWT validation failed")
//!     .with_request_id("req-123".to_string())
//!     .with_user_id("user-456".to_string())
//!     .with_severity(ErrorSeverity::High);
//!
//! let result = ErrorHandler::handle_with_context(
//!     || -> anyhow::Result<String> { Ok("success".to_string()) },
//!     context,
//! );
//! ```
//!
//! ## Validation Utilities
//!
//! The module provides comprehensive validation helpers:
//!
//! ```rust
//! use ultrafast_gateway::error_handling::ErrorHandler;
//!
//! // String validation
//! ErrorHandler::validate_string("test", "API key", 1)?;
//!
//! // Range validation
//! ErrorHandler::validate_range(50, 0, 100, "temperature")?;
//!
//! // Option validation
//! let value = ErrorHandler::require_some(Some("value"), "required field")?;
//! ```
//!
//! ## Retry Logic
//!
//! Automatic retry with exponential backoff:
//!
//! ```rust
//! use ultrafast_gateway::error_handling::ErrorHandler;
//!
//! let result = ErrorHandler::retry_with_backoff(
//!     || async { /* operation */ },
//!     3, // max retries
//!     Duration::from_secs(1), // initial delay
//!     context,
//! ).await;
//! ```
//!
//! ## Result Extensions
//!
//! Convenient error conversion and handling:
//!
//! ```rust
//! use ultrafast_gateway::error_handling::ResultExt;
//!
//! let result: Result<String, std::io::Error> = Ok("success".to_string());
//! let gateway_result = result.with_gateway_context("file operation");
//! ```
//!
//! ## Error Severity Levels
//!
//! Errors are categorized by severity:
//!
//! - **Low**: Non-critical issues (warnings)
//! - **Medium**: Issues that may affect functionality
//! - **High**: Issues that significantly impact operation
//! - **Critical**: Issues that require immediate attention
//!
//! ## Logging Integration
//!
//! All errors are automatically logged with appropriate levels:
//!
//! - **Error**: Critical and high severity errors
//! - **Warn**: Medium severity issues
//! - **Info**: Low severity issues and rate limits
//! - **Debug**: Detailed error context

use crate::gateway_error::GatewayError;
use anyhow::Result;
use std::fmt;
use tracing::{error, info, warn};

/// Standardized error handling patterns for the gateway.
///
/// Provides consistent error handling across all modules with proper
/// logging, context, and error type conversion.
///
/// # Example
///
/// ```rust
/// use ultrafast_gateway::error_handling::{ErrorHandler, ErrorType};
///
/// let result = ErrorHandler::handle_sync_operation(
///     || -> anyhow::Result<String> { Ok("success".to_string()) },
///     "database operation",
///     ErrorType::Internal,
/// );
/// ```
pub struct ErrorHandler;

impl ErrorHandler {
    /// Convert anyhow errors to GatewayError with proper context.
    ///
    /// Converts anyhow errors to gateway-specific errors with logging
    /// and proper error categorization.
    ///
    /// # Arguments
    ///
    /// * `result` - The anyhow result to convert
    /// * `context` - Context string for the operation
    ///
    /// # Returns
    ///
    /// Returns the original value on success, or a GatewayError on failure.
    pub fn anyhow_to_gateway<T>(result: Result<T>, context: &str) -> Result<T, GatewayError> {
        result.map_err(|e| {
            let context_msg = format!("{context}: {e}");
            error!("{}", context_msg);
            GatewayError::Internal {
                message: context_msg,
            }
        })
    }

    /// Handle configuration errors with proper validation context.
    ///
    /// Creates a configuration error with appropriate logging and context.
    ///
    /// # Arguments
    ///
    /// * `message` - Error message describing the configuration issue
    ///
    /// # Returns
    ///
    /// Returns a GatewayError::Config with the provided message.
    pub fn config_error(message: &str) -> GatewayError {
        warn!("Configuration error: {}", message);
        GatewayError::Config {
            message: message.to_string(),
        }
    }

    /// Handle authentication errors with proper security context.
    ///
    /// Creates an authentication error with appropriate logging and context.
    ///
    /// # Arguments
    ///
    /// * `message` - Error message describing the authentication issue
    ///
    /// # Returns
    ///
    /// Returns a GatewayError::Auth with the provided message.
    pub fn auth_error(message: &str) -> GatewayError {
        warn!("Authentication error: {}", message);
        GatewayError::Auth {
            message: message.to_string(),
        }
    }

    /// Handle rate limiting errors with proper throttling context.
    ///
    /// Creates a rate limit error with appropriate logging and context.
    ///
    /// # Arguments
    ///
    /// * `message` - Error message describing the rate limit issue
    ///
    /// # Returns
    ///
    /// Returns a GatewayError::RateLimit with the provided message.
    pub fn rate_limit_error(message: &str) -> GatewayError {
        info!("Rate limit exceeded: {}", message);
        GatewayError::RateLimit {
            message: message.to_string(),
        }
    }

    /// Handle content filtering errors with proper moderation context.
    ///
    /// Creates a content filter error with appropriate logging and context.
    ///
    /// # Arguments
    ///
    /// * `message` - Error message describing the content filter issue
    ///
    /// # Returns
    ///
    /// Returns a GatewayError::ContentFiltered with the provided message.
    pub fn content_filter_error(message: &str) -> GatewayError {
        warn!("Content filtered: {}", message);
        GatewayError::ContentFiltered {
            message: message.to_string(),
        }
    }

    /// Handle plugin errors with proper lifecycle context.
    ///
    /// Creates a plugin error with appropriate logging and context.
    ///
    /// # Arguments
    ///
    /// * `message` - Error message describing the plugin issue
    ///
    /// # Returns
    ///
    /// Returns a GatewayError::Plugin with the provided message.
    pub fn plugin_error(message: &str) -> GatewayError {
        error!("Plugin error: {}", message);
        GatewayError::Plugin {
            message: message.to_string(),
        }
    }

    /// Handle cache errors with proper storage context.
    ///
    /// Creates a cache error with appropriate logging and context.
    ///
    /// # Arguments
    ///
    /// * `message` - Error message describing the cache issue
    ///
    /// # Returns
    ///
    /// Returns a GatewayError::Cache with the provided message.
    pub fn cache_error(message: &str) -> GatewayError {
        warn!("Cache error: {}", message);
        GatewayError::Cache {
            message: message.to_string(),
        }
    }

    /// Handle serialization errors with proper data context.
    ///
    /// Creates a serialization error with appropriate logging and context.
    ///
    /// # Arguments
    ///
    /// * `e` - The serialization error
    /// * `context` - Context string for the serialization operation
    ///
    /// # Returns
    ///
    /// Returns a GatewayError::Serialization with the error and context.
    pub fn serialization_error(e: serde_json::Error, context: &str) -> GatewayError {
        let message = format!("{context}: {e}");
        error!("Serialization error: {}", message);
        GatewayError::Serialization(e)
    }

    /// Handle internal errors with proper system context.
    ///
    /// Creates an internal error with appropriate logging and context.
    ///
    /// # Arguments
    ///
    /// * `message` - Error message describing the internal issue
    ///
    /// # Returns
    ///
    /// Returns a GatewayError::Internal with the provided message.
    pub fn internal_error(message: &str) -> GatewayError {
        error!("Internal error: {}", message);
        GatewayError::Internal {
            message: message.to_string(),
        }
    }

    /// Handle service unavailable errors with proper availability context.
    ///
    /// Creates a service unavailable error with appropriate logging.
    ///
    /// # Returns
    ///
    /// Returns a GatewayError::ServiceUnavailable.
    pub fn service_unavailable() -> GatewayError {
        warn!("Service unavailable");
        GatewayError::ServiceUnavailable
    }

    /// Handle invalid request errors with proper validation context.
    ///
    /// Creates an invalid request error with appropriate logging and context.
    ///
    /// # Arguments
    ///
    /// * `message` - Error message describing the invalid request
    ///
    /// # Returns
    ///
    /// Returns a GatewayError::InvalidRequest with the provided message.
    pub fn invalid_request(message: &str) -> GatewayError {
        warn!("Invalid request: {}", message);
        GatewayError::InvalidRequest {
            message: message.to_string(),
        }
    }

    /// Log error with proper context and return appropriate GatewayError.
    ///
    /// Logs the error with appropriate level based on error type and
    /// converts it to a GatewayError with proper context.
    ///
    /// # Arguments
    ///
    /// * `error` - The error to log and convert
    /// * `context` - Context string for the operation
    /// * `error_type` - Type of error for proper categorization
    ///
    /// # Returns
    ///
    /// Returns a GatewayError with the error message and context.
    pub fn log_and_convert<E: fmt::Display + fmt::Debug>(
        error: E,
        context: &str,
        error_type: ErrorType,
    ) -> GatewayError {
        let message = format!("{context}: {error}");

        match error_type {
            ErrorType::Config => {
                warn!("Configuration error: {}", message);
                GatewayError::Config { message }
            }
            ErrorType::Auth => {
                warn!("Authentication error: {}", message);
                GatewayError::Auth { message }
            }
            ErrorType::RateLimit => {
                info!("Rate limit error: {}", message);
                GatewayError::RateLimit { message }
            }
            ErrorType::ContentFilter => {
                warn!("Content filter error: {}", message);
                GatewayError::ContentFiltered { message }
            }
            ErrorType::Plugin => {
                error!("Plugin error: {}", message);
                GatewayError::Plugin { message }
            }
            ErrorType::Cache => {
                warn!("Cache error: {}", message);
                GatewayError::Cache { message }
            }
            ErrorType::Internal => {
                error!("Internal error: {}", message);
                GatewayError::Internal { message }
            }
            ErrorType::ServiceUnavailable => {
                warn!("Service unavailable: {}", message);
                GatewayError::ServiceUnavailable
            }
            ErrorType::InvalidRequest => {
                warn!("Invalid request: {}", message);
                GatewayError::InvalidRequest { message }
            }
        }
    }

    /// Handle async operations with proper error context
    pub async fn handle_async_operation<F, Fut, T>(
        operation: F,
        context: &str,
        error_type: ErrorType,
    ) -> Result<T, GatewayError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T, anyhow::Error>>,
    {
        match operation().await {
            Ok(result) => Ok(result),
            Err(e) => {
                let gateway_error = Self::log_and_convert(e, context, error_type);
                Err(gateway_error)
            }
        }
    }

    /// Handle sync operations with proper error context
    pub fn handle_sync_operation<F, T>(
        operation: F,
        context: &str,
        error_type: ErrorType,
    ) -> Result<T, GatewayError>
    where
        F: FnOnce() -> Result<T, anyhow::Error>,
    {
        match operation() {
            Ok(result) => Ok(result),
            Err(e) => {
                let gateway_error = Self::log_and_convert(e, context, error_type);
                Err(gateway_error)
            }
        }
    }

    /// Validate configuration with proper error context
    pub fn validate_config<T, F>(value: T, validator: F, context: &str) -> Result<T, GatewayError>
    where
        F: FnOnce(&T) -> Result<(), String>,
    {
        match validator(&value) {
            Ok(()) => Ok(value),
            Err(message) => {
                let full_message = format!("{context}: {message}");
                warn!("Configuration validation failed: {}", full_message);
                Err(GatewayError::Config {
                    message: full_message,
                })
            }
        }
    }

    /// Handle optional values with proper error context
    pub fn require_some<T>(value: Option<T>, context: &str) -> Result<T, GatewayError> {
        value.ok_or_else(|| {
            let message = format!("{context}: value is required but was None");
            warn!("Required value missing: {}", message);
            GatewayError::Config { message }
        })
    }

    /// Handle string validation with proper error context
    pub fn validate_string(
        value: &str,
        context: &str,
        min_length: usize,
    ) -> Result<(), GatewayError> {
        if value.len() < min_length {
            let message = format!("{context}: string too short (minimum {min_length} characters)");
            warn!("String validation failed: {}", message);
            return Err(GatewayError::Config { message });
        }
        Ok(())
    }

    /// Handle numeric validation with proper error context
    pub fn validate_range<T: PartialOrd + fmt::Display>(
        value: T,
        min: T,
        max: T,
        context: &str,
    ) -> Result<(), GatewayError> {
        if value < min || value > max {
            let message = format!("{context}: value {value} is outside valid range [{min}, {max}]");
            warn!("Range validation failed: {}", message);
            return Err(GatewayError::Config { message });
        }
        Ok(())
    }
}

/// Error types for consistent categorization
#[derive(Debug, Clone, Copy)]
pub enum ErrorType {
    Config,
    Auth,
    RateLimit,
    ContentFilter,
    Plugin,
    Cache,
    Internal,
    ServiceUnavailable,
    InvalidRequest,
}

/// Extension trait for Result to add error handling utilities
pub trait ResultExt<T, E> {
    /// Convert anyhow error to GatewayError with context
    fn with_gateway_context(self, context: &str) -> Result<T, GatewayError>
    where
        E: Into<anyhow::Error>,
        T: fmt::Debug;

    /// Log error and convert to GatewayError
    fn log_and_convert(self, context: &str, error_type: ErrorType) -> Result<T, GatewayError>
    where
        E: fmt::Display + fmt::Debug;
}

impl<T, E> ResultExt<T, E> for Result<T, E> {
    fn with_gateway_context(self, context: &str) -> Result<T, GatewayError>
    where
        E: Into<anyhow::Error>,
    {
        self.map_err(|e| {
            let anyhow_error: anyhow::Error = e.into();
            let context_msg = format!("{context}: {anyhow_error}");
            error!("{}", context_msg);
            GatewayError::Internal {
                message: context_msg,
            }
        })
    }

    fn log_and_convert(self, context: &str, error_type: ErrorType) -> Result<T, GatewayError>
    where
        E: fmt::Display + fmt::Debug,
    {
        self.map_err(|e| ErrorHandler::log_and_convert(e, context, error_type))
    }
}

/// Extension trait for Option to add error handling utilities
pub trait OptionExt<T> {
    /// Convert None to GatewayError with context
    fn ok_or_gateway_error(self, context: &str) -> Result<T, GatewayError>;
}

impl<T> OptionExt<T> for Option<T> {
    fn ok_or_gateway_error(self, context: &str) -> Result<T, GatewayError> {
        self.ok_or_else(|| ErrorHandler::config_error(&format!("{context}: value is required")))
    }
}

/// Enhanced error context with more detailed information
#[derive(Debug, Clone)]
pub struct ErrorContext {
    pub module: String,
    pub operation: String,
    pub details: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub request_id: Option<String>,
    pub user_id: Option<String>,
    pub severity: ErrorSeverity,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ErrorSeverity {
    Low,
    Medium,
    High,
    Critical,
}

impl ErrorContext {
    pub fn new(module: &str, operation: &str, details: &str) -> Self {
        Self {
            module: module.to_string(),
            operation: operation.to_string(),
            details: details.to_string(),
            timestamp: chrono::Utc::now(),
            request_id: None,
            user_id: None,
            severity: ErrorSeverity::Medium,
        }
    }

    pub fn with_request_id(mut self, request_id: String) -> Self {
        self.request_id = Some(request_id);
        self
    }

    pub fn with_user_id(mut self, user_id: String) -> Self {
        self.user_id = Some(user_id);
        self
    }

    pub fn with_severity(mut self, severity: ErrorSeverity) -> Self {
        self.severity = severity;
        self
    }

    pub fn format(&self) -> String {
        let mut parts = vec![
            format!("[{}] {}", self.module, self.operation),
            format!("Details: {}", self.details),
            format!("Timestamp: {}", self.timestamp),
        ];

        if let Some(request_id) = &self.request_id {
            parts.push(format!("Request ID: {request_id}"));
        }

        if let Some(user_id) = &self.user_id {
            parts.push(format!("User ID: {user_id}"));
        }

        parts.push(format!("Severity: {:?}", self.severity));

        parts.join(" | ")
    }

    pub fn is_critical(&self) -> bool {
        matches!(self.severity, ErrorSeverity::Critical)
    }

    pub fn should_retry(&self) -> bool {
        matches!(self.severity, ErrorSeverity::Low | ErrorSeverity::Medium)
    }
}

impl ErrorHandler {
    /// Enhanced error handling with context and severity
    pub fn handle_with_context<T, F>(operation: F, context: ErrorContext) -> Result<T, GatewayError>
    where
        F: FnOnce() -> Result<T, anyhow::Error>,
    {
        match operation() {
            Ok(result) => Ok(result),
            Err(e) => {
                let error_msg = format!("{}: {}", context.format(), e);

                match context.severity {
                    ErrorSeverity::Low => {
                        tracing::debug!("{}", error_msg);
                    }
                    ErrorSeverity::Medium => {
                        tracing::warn!("{}", error_msg);
                    }
                    ErrorSeverity::High => {
                        tracing::error!("{}", error_msg);
                    }
                    ErrorSeverity::Critical => {
                        tracing::error!("CRITICAL ERROR: {}", error_msg);
                        // Could trigger alerts here
                    }
                }

                match context.module.as_str() {
                    "config" => Err(GatewayError::Config { message: error_msg }),
                    "auth" => Err(GatewayError::Auth { message: error_msg }),
                    "rate_limit" => Err(GatewayError::RateLimit { message: error_msg }),
                    "cache" => Err(GatewayError::Cache { message: error_msg }),
                    "plugin" => Err(GatewayError::Plugin { message: error_msg }),
                    _ => Err(GatewayError::Internal { message: error_msg }),
                }
            }
        }
    }

    /// Enhanced async error handling with context
    pub async fn handle_async_with_context<F, Fut, T>(
        operation: F,
        context: ErrorContext,
    ) -> Result<T, GatewayError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<T, anyhow::Error>>,
    {
        match operation().await {
            Ok(result) => Ok(result),
            Err(e) => {
                let error_msg = format!("{}: {}", context.format(), e);

                match context.severity {
                    ErrorSeverity::Low => {
                        tracing::debug!("{}", error_msg);
                    }
                    ErrorSeverity::Medium => {
                        tracing::warn!("{}", error_msg);
                    }
                    ErrorSeverity::High => {
                        tracing::error!("{}", error_msg);
                    }
                    ErrorSeverity::Critical => {
                        tracing::error!("CRITICAL ERROR: {}", error_msg);
                    }
                }

                match context.module.as_str() {
                    "config" => Err(GatewayError::Config { message: error_msg }),
                    "auth" => Err(GatewayError::Auth { message: error_msg }),
                    "rate_limit" => Err(GatewayError::RateLimit { message: error_msg }),
                    "cache" => Err(GatewayError::Cache { message: error_msg }),
                    "plugin" => Err(GatewayError::Plugin { message: error_msg }),
                    _ => Err(GatewayError::Internal { message: error_msg }),
                }
            }
        }
    }

    /// Enhanced validation with better error messages
    pub fn validate_with_context<T, F>(
        value: T,
        validator: F,
        context: ErrorContext,
    ) -> Result<T, GatewayError>
    where
        F: FnOnce(&T) -> Result<(), String>,
    {
        match validator(&value) {
            Ok(()) => Ok(value),
            Err(error_msg) => {
                let full_error = format!("{}: {}", context.format(), error_msg);

                match context.severity {
                    ErrorSeverity::Low => {
                        tracing::debug!("{}", full_error);
                    }
                    ErrorSeverity::Medium => {
                        tracing::warn!("{}", full_error);
                    }
                    ErrorSeverity::High => {
                        tracing::error!("{}", full_error);
                    }
                    ErrorSeverity::Critical => {
                        tracing::error!("CRITICAL VALIDATION ERROR: {}", full_error);
                    }
                }

                Err(GatewayError::InvalidRequest {
                    message: full_error,
                })
            }
        }
    }

    /// Enhanced string validation with length constraints
    pub fn validate_string_with_constraints(
        value: &str,
        context: ErrorContext,
        min_length: usize,
        max_length: Option<usize>,
    ) -> Result<(), GatewayError> {
        if value.len() < min_length {
            let error_msg = format!(
                "String too short: {} chars (minimum: {})",
                value.len(),
                min_length
            );
            return Err(GatewayError::InvalidRequest {
                message: format!("{}: {}", context.format(), error_msg),
            });
        }

        if let Some(max_len) = max_length {
            if value.len() > max_len {
                let error_msg = format!(
                    "String too long: {} chars (maximum: {})",
                    value.len(),
                    max_len
                );
                return Err(GatewayError::InvalidRequest {
                    message: format!("{}: {}", context.format(), error_msg),
                });
            }
        }

        Ok(())
    }

    /// Enhanced range validation with better error messages
    pub fn validate_range_with_context<T: PartialOrd + fmt::Display>(
        value: T,
        min: T,
        max: T,
        context: ErrorContext,
    ) -> Result<(), GatewayError> {
        if value < min || value > max {
            let error_msg = format!("Value {value} out of range [{min}, {max}]");
            return Err(GatewayError::InvalidRequest {
                message: format!("{}: {}", context.format(), error_msg),
            });
        }
        Ok(())
    }

    /// Enhanced retry logic with exponential backoff
    pub async fn retry_with_backoff<F, Fut, T>(
        mut operation: F,
        max_retries: u32,
        initial_delay: std::time::Duration,
        context: ErrorContext,
    ) -> Result<T, GatewayError>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T, anyhow::Error>>,
    {
        let mut delay = initial_delay;
        let mut last_error_message = None;

        for attempt in 0..=max_retries {
            match operation().await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    last_error_message = Some(e.to_string());

                    if attempt < max_retries {
                        tracing::warn!(
                            "Attempt {} failed in {}: {}. Retrying in {:?}",
                            attempt + 1,
                            context.operation,
                            e,
                            delay
                        );

                        tokio::time::sleep(delay).await;
                        delay =
                            std::cmp::min(delay.mul_f64(2.0), std::time::Duration::from_secs(30));
                    } else {
                        tracing::error!(
                            "All {} attempts failed in {}: {}",
                            max_retries + 1,
                            context.operation,
                            e,
                        );
                        break;
                    }
                }
            }
        }

        Err(GatewayError::Internal {
            message: last_error_message.unwrap_or_else(|| "Unknown error".to_string()),
        })
    }
}

/// Macro for creating error context
#[macro_export]
macro_rules! error_context {
    ($module:expr, $operation:expr, $details:expr) => {
        ErrorContext::new($module, $operation, $details)
    };
}

/// Macro for handling errors with context
#[macro_export]
macro_rules! handle_error {
    ($result:expr, $context:expr, $error_type:expr) => {
        match $result {
            Ok(value) => Ok(value),
            Err(e) => {
                let gateway_error = ErrorHandler::log_and_convert(e, $context, $error_type);
                Err(gateway_error)
            }
        }
    };
}

/// Macro for validating configuration
#[macro_export]
macro_rules! validate_config {
    ($value:expr, $validator:expr, $context:expr) => {
        ErrorHandler::validate_config($value, $validator, $context)
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_handler_config_error() {
        let error = ErrorHandler::config_error("test error");
        assert!(matches!(error, GatewayError::Config { .. }));
    }

    #[test]
    fn test_error_handler_auth_error() {
        let error = ErrorHandler::auth_error("test auth error");
        assert!(matches!(error, GatewayError::Auth { .. }));
    }

    #[test]
    fn test_error_handler_rate_limit_error() {
        let error = ErrorHandler::rate_limit_error("test rate limit error");
        assert!(matches!(error, GatewayError::RateLimit { .. }));
    }

    #[test]
    fn test_validate_string() {
        assert!(ErrorHandler::validate_string("test", "test", 3).is_ok());
        assert!(ErrorHandler::validate_string("ab", "test", 3).is_err());
    }

    #[test]
    fn test_validate_range() {
        assert!(ErrorHandler::validate_range(5, 1, 10, "test").is_ok());
        assert!(ErrorHandler::validate_range(0, 1, 10, "test").is_err());
        assert!(ErrorHandler::validate_range(11, 1, 10, "test").is_err());
    }

    #[test]
    fn test_require_some() {
        assert!(ErrorHandler::require_some(Some(42), "test").is_ok());
        assert!(ErrorHandler::require_some(None::<i32>, "test").is_err());
    }

    #[test]
    fn test_result_ext() {
        let result: Result<i32, anyhow::Error> = Ok(42);
        assert!(result.with_gateway_context("test").is_ok());

        let result: Result<i32, anyhow::Error> = Err(anyhow::anyhow!("test error"));
        assert!(result.log_and_convert("test", ErrorType::Config).is_err());
    }

    #[test]
    fn test_option_ext() {
        let option: Option<i32> = Some(42);
        assert!(option.ok_or_gateway_error("test").is_ok());

        let option: Option<i32> = None;
        assert!(option.ok_or_gateway_error("test").is_err());
    }
}
