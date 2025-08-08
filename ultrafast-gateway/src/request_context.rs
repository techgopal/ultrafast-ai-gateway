//! # Request Context Module
//!
//! This module provides request context tracking for the Ultrafast Gateway,
//! enabling comprehensive request tracing, monitoring, and debugging
//! throughout the request lifecycle.
//!
//! ## Overview
//!
//! The request context system provides:
//! - **Request Tracing**: Unique request identifiers for end-to-end tracing
//! - **Authentication Tracking**: User and API key information
//! - **Performance Monitoring**: Request duration and latency tracking
//! - **Metadata Management**: Flexible request metadata storage
//! - **Debugging Support**: Rich context for error investigation
//!
//! ## Request Lifecycle
//!
//! Each request follows a defined lifecycle with context tracking:
//!
//! 1. **Request Initiation**: Context created with unique ID
//! 2. **Authentication**: User and API key information added
//! 3. **Processing**: Metadata and timing information tracked
//! 4. **Completion**: Final duration and results recorded
//!
//! ## Context Information
//!
//! The request context includes comprehensive information:
//!
//! - **Request ID**: Unique identifier for tracing
//! - **User ID**: Authenticated user identifier
//! - **API Key**: API key used for authentication
//! - **Start Time**: Request initiation timestamp
//! - **Duration**: Request processing time
//! - **Metadata**: Custom request metadata
//!
//! ## Usage
//!
//! ```rust
//! use ultrafast_gateway::request_context::RequestContext;
//!
//! // Create basic request context
//! let context = RequestContext::new();
//!
//! // Create authenticated request context
//! let auth_context = RequestContext::with_auth(
//!     Some("user-123".to_string()),
//!     Some("sk-...".to_string())
//! );
//!
//! // Add custom metadata
//! let context = context
//!     .with_metadata("provider".to_string(), "openai".to_string())
//!     .with_metadata("model".to_string(), "gpt-4".to_string());
//!
//! // Track request duration
//! let duration = context.duration();
//! println!("Request took: {:?}", duration);
//! ```
//!
//! ## Tracing and Monitoring
//!
//! The request context enables comprehensive tracing:
//!
//! - **Request Correlation**: Link related requests and operations
//! - **Performance Tracking**: Monitor request durations and latencies
//! - **Error Investigation**: Rich context for debugging issues
//! - **User Analytics**: Track user behavior and patterns
//! - **Provider Monitoring**: Monitor provider performance per request
//!
//! ## Integration
//!
//! The request context integrates with other gateway systems:
//!
//! - **Logging**: Request context included in log entries
//! - **Metrics**: Performance metrics tied to request context
//! - **Error Handling**: Error context includes request information
//! - **Caching**: Cache keys can include request context
//! - **Rate Limiting**: Rate limits applied per request context

use std::time::Instant;
use uuid::Uuid;

/// Request context for tracking individual requests throughout their lifecycle.
///
/// This struct provides comprehensive request tracking including unique
/// identifiers, authentication information, timing data, and custom metadata.
/// It enables end-to-end request tracing and performance monitoring.
#[derive(Debug, Clone)]
pub struct RequestContext {
    /// Unique request ID for tracing
    pub request_id: String,
    /// User ID if authenticated
    pub user_id: Option<String>,
    /// API key used for authentication
    pub api_key: Option<String>,
    /// Request start time for latency tracking
    pub start_time: Instant,
    /// Request metadata
    pub metadata: std::collections::HashMap<String, String>,
}

impl RequestContext {
    /// Create a new request context with a unique identifier.
    ///
    /// This method creates a fresh request context with a new UUID,
    /// current timestamp, and empty metadata. The context is ready
    /// for tracking a new request throughout its lifecycle.
    ///
    /// # Returns
    ///
    /// Returns a new `RequestContext` instance with default values.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use ultrafast_gateway::request_context::RequestContext;
    ///
    /// let context = RequestContext::new();
    /// println!("Request ID: {}", context.request_id);
    /// ```
    pub fn new() -> Self {
        Self {
            request_id: Uuid::new_v4().to_string(),
            user_id: None,
            api_key: None,
            start_time: Instant::now(),
            metadata: std::collections::HashMap::new(),
        }
    }

    /// Create a new request context with authentication information.
    ///
    /// This method creates a request context with pre-populated
    /// authentication data, including user ID and API key information.
    /// This is useful for authenticated requests where the user
    /// information is known at context creation time.
    ///
    /// # Arguments
    ///
    /// * `user_id` - Optional user identifier for authenticated requests
    /// * `api_key` - Optional API key used for authentication
    ///
    /// # Returns
    ///
    /// Returns a new `RequestContext` instance with authentication data.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use ultrafast_gateway::request_context::RequestContext;
    ///
    /// let context = RequestContext::with_auth(
    ///     Some("user-123".to_string()),
    ///     Some("sk-abc123".to_string())
    /// );
    /// ```
    pub fn with_auth(user_id: Option<String>, api_key: Option<String>) -> Self {
        Self {
            request_id: Uuid::new_v4().to_string(),
            user_id,
            api_key,
            start_time: Instant::now(),
            metadata: std::collections::HashMap::new(),
        }
    }

    /// Get the elapsed duration since the request started.
    ///
    /// This method calculates the time elapsed since the request
    /// context was created, providing accurate timing information
    /// for performance monitoring and debugging.
    ///
    /// # Returns
    ///
    /// Returns the elapsed duration since the request started.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use ultrafast_gateway::request_context::RequestContext;
    /// use std::thread;
    /// use std::time::Duration;
    ///
    /// let context = RequestContext::new();
    /// thread::sleep(Duration::from_millis(100));
    /// let duration = context.duration();
    /// println!("Request duration: {:?}", duration);
    /// ```
    pub fn duration(&self) -> std::time::Duration {
        self.start_time.elapsed()
    }

    /// Add metadata to the request context.
    ///
    /// This method adds a key-value pair to the request metadata,
    /// enabling custom data storage for debugging, monitoring, and
    /// request tracking. The method returns `self` for method chaining.
    ///
    /// # Arguments
    ///
    /// * `key` - Metadata key
    /// * `value` - Metadata value
    ///
    /// # Returns
    ///
    /// Returns `self` for method chaining.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use ultrafast_gateway::request_context::RequestContext;
    ///
    /// let context = RequestContext::new()
    ///     .with_metadata("provider".to_string(), "openai".to_string())
    ///     .with_metadata("model".to_string(), "gpt-4".to_string());
    /// ```
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }

    /// Get a metadata value by key.
    ///
    /// This method retrieves a metadata value from the request context
    /// using the provided key. Returns `None` if the key doesn't exist.
    ///
    /// # Arguments
    ///
    /// * `key` - The metadata key to retrieve
    ///
    /// # Returns
    ///
    /// Returns `Some(value)` if the key exists, `None` otherwise.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use ultrafast_gateway::request_context::RequestContext;
    ///
    /// let context = RequestContext::new()
    ///     .with_metadata("provider".to_string(), "openai".to_string());
    ///
    /// if let Some(provider) = context.get_metadata("provider") {
    ///     println!("Provider: {}", provider);
    /// }
    /// ```
    pub fn get_metadata(&self, key: &str) -> Option<&String> {
        self.metadata.get(key)
    }
}

impl Default for RequestContext {
    fn default() -> Self {
        Self::new()
    }
}
