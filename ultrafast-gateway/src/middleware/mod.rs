//! # HTTP Middleware Module
//!
//! This module provides HTTP middleware components for the Ultrafast Gateway.
//! It includes authentication, logging, metrics collection, CORS handling,
//! and input validation middleware.
//!
//! ## Overview
//!
//! The middleware system provides:
//! - **Authentication Middleware**: API key and JWT token validation
//! - **Logging Middleware**: Request/response logging with context
//! - **Metrics Middleware**: Performance metrics collection
//! - **CORS Middleware**: Cross-origin resource sharing
//! - **Input Validation**: Request validation and sanitization
//! - **Plugin Middleware**: Dynamic request/response modification
//!
//! ## Middleware Stack Order
//!
//! The middleware is applied in the following order:
//!
//! 1. **Timeout Middleware**: Request timeout handling
//! 2. **CORS Middleware**: Cross-origin request handling
//! 3. **Logging Middleware**: Request/response logging
//! 4. **Metrics Middleware**: Performance tracking
//! 5. **Authentication Middleware**: API key validation
//! 6. **Input Validation Middleware**: Request validation
//! 7. **Plugin Middleware**: Dynamic modifications
//!
//! ## Authentication Middleware
//!
//! Handles API key and JWT token validation:
//!
//! - **API Key Extraction**: Extracts keys from headers
//! - **JWT Validation**: Validates JWT tokens
//! - **Rate Limiting**: Applies rate limits per user
//! - **Permission Checking**: Validates user permissions
//! - **Session Management**: Handles user sessions
//!
//! ## Logging Middleware
//!
//! Provides comprehensive request/response logging:
//!
//! - **Request Context**: Logs request method, URI, and headers
//! - **Response Status**: Tracks response status codes
//! - **Latency Tracking**: Measures request processing time
//! - **Request ID**: Unique request identifiers for tracing
//! - **Error Logging**: Detailed error information
//!
//! ## Metrics Middleware
//!
//! Collects performance metrics for each request:
//!
//! - **Request Metrics**: Method, path, status, latency
//! - **User Tracking**: User ID and session information
//! - **Provider Metrics**: Provider selection and performance
//! - **Cost Tracking**: Token usage and cost calculation
//! - **Error Metrics**: Error rates and types
//!
//! ## CORS Middleware
//!
//! Handles cross-origin resource sharing:
//!
//! - **Origin Validation**: Validates request origins
//! - **Method Allowance**: Controls allowed HTTP methods
//! - **Header Management**: Manages allowed headers
//! - **Preflight Handling**: Handles OPTIONS requests
//! - **Cache Control**: Manages CORS response caching
//!
//! ## Input Validation Middleware
//!
//! Validates and sanitizes request data:
//!
//! - **Request Validation**: Validates request structure
//! - **Content Sanitization**: Removes malicious content
//! - **Size Limits**: Enforces request size limits
//! - **Format Validation**: Validates data formats
//! - **Security Checks**: Performs security validations
//!
//! ## Plugin Middleware
//!
//! Provides dynamic request/response modification:
//!
//! - **Request Modification**: Modifies incoming requests
//! - **Response Modification**: Modifies outgoing responses
//! - **Content Filtering**: Filters request/response content
//! - **Custom Logic**: Executes custom plugin logic
//! - **Error Handling**: Handles plugin errors gracefully
//!
//! ## Usage
//!
//! ```rust
//! use ultrafast_gateway::middleware::{
//!     auth_middleware, cors_middleware, logging_middleware,
//!     metrics_middleware, input_validation_middleware
//! };
//!
//! // Apply middleware to router
//! let app = Router::new()
//!     .layer(cors_middleware(&cors_config))
//!     .layer(axum::middleware::from_fn(logging_middleware))
//!     .layer(axum::middleware::from_fn(metrics_middleware))
//!     .layer(axum::middleware::from_fn(auth_middleware))
//!     .layer(axum::middleware::from_fn(input_validation_middleware));
//! ```
//!
//! ## Configuration
//!
//! Middleware can be configured via the gateway configuration:
//!
//! ```toml
//! [server.cors]
//! enabled = true
//! allowed_origins = ["*"]
//! allowed_methods = ["GET", "POST", "PUT", "DELETE"]
//!
//! [auth]
//! enabled = true
//! jwt_secret = "your-secret"
//!
//! [metrics]
//! enabled = true
//! max_requests = 1000
//! ```
//!
//! ## Error Handling
//!
//! Each middleware includes comprehensive error handling:
//!
//! - **Authentication Errors**: Proper HTTP status codes
//! - **Validation Errors**: Detailed error messages
//! - **Rate Limit Errors**: Rate limit headers and responses
//! - **CORS Errors**: Proper CORS error responses
//! - **Plugin Errors**: Non-blocking plugin failures
//!
//! ## Performance Impact
//!
//! The middleware is designed for minimal performance impact:
//!
//! - **Efficient Logging**: Structured logging with minimal overhead
//! - **Async Operations**: Non-blocking async middleware
//! - **Caching**: Cached authentication and validation results
//! - **Selective Metrics**: Metrics collection only for relevant requests
//! - **Optimized Validation**: Fast validation algorithms

use crate::config::CorsConfig;
use crate::server::AppState;
use axum::body::Body;
use axum::extract::State;
use axum::http::Request;
use axum::http::{self, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use std::collections::HashMap;
use std::time::Instant;
use tower_http::cors::{Any, CorsLayer};
// Unused imports removed - using dedicated modules now

pub mod input_validation;
pub mod plugin_middleware;

// Re-export the input validation middleware
pub use input_validation::input_validation_middleware;

/// Logging middleware for request/response tracking.
///
/// Logs detailed information about each request including method, URI,
/// status code, latency, and request ID for tracing.
///
/// # Arguments
///
/// * `_state` - Application state (unused in this middleware)
/// * `req` - The incoming HTTP request
/// * `next` - The next middleware in the chain
///
/// # Returns
///
/// Returns the HTTP response with logging information.
///
/// # Example
///
/// ```rust
/// let app = Router::new()
///     .layer(axum::middleware::from_fn(logging_middleware));
/// ```
pub async fn logging_middleware(
    State(_state): State<AppState>,
    req: Request<Body>,
    next: Next,
) -> Response {
    let start = Instant::now();
    let method = req.method().clone();
    let uri = req.uri().clone();

    // Extract request context if available
    let request_id = req
        .extensions()
        .get::<crate::request_context::RequestContext>()
        .map(|ctx| ctx.request_id.clone())
        .unwrap_or_else(|| "unknown".to_string());

    let response = next.run(req).await;

    let latency = start.elapsed();
    let status = response.status();

    tracing::info!(
        request_id = %request_id,
        method = %method,
        uri = %uri,
        status = %status,
        latency_ms = latency.as_millis(),
        "Request processed"
    );

    response
}

/// Metrics middleware for performance tracking.
///
/// Collects performance metrics for each request including latency,
/// status codes, and user information. Skips metrics for dashboard
/// and health check endpoints.
///
/// # Arguments
///
/// * `_state` - Application state (unused in this middleware)
/// * `req` - The incoming HTTP request
/// * `next` - The next middleware in the chain
///
/// # Returns
///
/// Returns the HTTP response with metrics recorded.
///
/// # Example
///
/// ```rust
/// let app = Router::new()
///     .layer(axum::middleware::from_fn(metrics_middleware));
/// ```
pub async fn metrics_middleware(
    State(_state): State<AppState>,
    mut req: Request<Body>,
    next: Next,
) -> Response {
    let start = std::time::Instant::now();
    let method = req.method().clone();
    let uri = req.uri().clone();
    let request_id = uuid::Uuid::new_v4().to_string();

    // Add request ID to extensions for logging
    req.extensions_mut().insert(request_id.clone());

    let response = next.run(req).await;
    let latency = start.elapsed();
    let status = response.status();

    // Skip recording metrics for dashboard-related requests
    let should_record_metrics = !uri.path().starts_with("/dashboard")
        && !uri.path().starts_with("/metrics")
        && !uri.path().starts_with("/health")
        && !uri.path().starts_with("/ws/");

    if should_record_metrics {
        // Record metrics using the dedicated metrics module
        let metrics = crate::metrics::RequestMetricsBuilder::new(
            method.to_string(),
            uri.path().to_string(),
            status.as_u16(),
            latency,
        )
        .user_id(request_id.clone())
        .build();

        crate::metrics::record_request(metrics).await;

        // Log request ID for debugging
        tracing::debug!(
            request_id = %request_id,
            method = %method,
            uri = %uri,
            status = %status,
            latency_ms = latency.as_millis(),
            "Metrics recorded"
        );
    }

    response
}

// Legacy function - now using dedicated metrics module
pub async fn get_metrics() -> HashMap<String, serde_json::Value> {
    let aggregated = crate::metrics::get_aggregated_metrics().await;
    let mut result = HashMap::new();

    result.insert(
        "requests_per_minute".to_string(),
        serde_json::json!(aggregated.requests_per_minute),
    );
    result.insert(
        "average_latency_ms".to_string(),
        serde_json::json!(aggregated.average_latency_ms),
    );
    result.insert(
        "error_rate".to_string(),
        serde_json::json!(aggregated.error_rate),
    );
    result.insert(
        "active_connections".to_string(),
        serde_json::json!(aggregated.active_connections),
    );

    result
}

// Rate limiting now handled by auth module

pub async fn auth_middleware(
    State(state): State<AppState>,
    mut req: Request<Body>,
    next: Next,
) -> Response {
    // Allow OPTIONS requests (CORS preflight) without authentication
    if req.method() == http::Method::OPTIONS {
        return next.run(req).await;
    }

    if !state.config.auth.enabled {
        return next.run(req).await;
    }

    // Phase 3 Optimization: Async processing of authentication and validation
    let auth_header = req
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok());

    let api_key = match crate::auth::AuthService::extract_api_key_from_header(auth_header) {
        Some(key) => key,
        None => {
            let error_response = Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .header("Content-Type", "application/json")
                .body(Body::from(r#"{"error": {"message": "Missing Authorization header", "type": "authentication_error"}}"#))
                .unwrap();
            return error_response;
        }
    };

    // Phase 3 Optimization: Process authentication and validation asynchronously
    let auth_future = crate::auth::validate_api_key_global(&api_key);
    let rate_limit_future = async {
        // Pre-validate API key format for rate limiting
        if api_key.starts_with("sk-") {
            let user_id = api_key
                .split('-')
                .next_back()
                .unwrap_or("unknown")
                .to_string();
            crate::auth::check_rate_limits(&user_id, crate::auth::RateLimits::new(100, 1000, 10000))
                .await
        } else {
            Err(crate::gateway_error::GatewayError::InvalidRequest {
                message: "Invalid API key format".to_string(),
            })
        }
    };

    // Execute both futures concurrently
    let (auth_result, rate_limit_result) = tokio::join!(auth_future, rate_limit_future);

    // Handle authentication result
    let auth_context = match auth_result {
        Ok(context) => context,
        Err(e) => {
            let error_response = Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .header("Content-Type", "application/json")
                .body(Body::from(format!(
                    r#"{{"error": {{"message": "{e}", "type": "authentication_error"}}}}"#
                )))
                .unwrap();
            return error_response;
        }
    };

    // Handle rate limiting result
    let _updated_limits = match rate_limit_result {
        Ok(limits) => limits,
        Err(e) => {
            let error_response = Response::builder()
                .status(StatusCode::TOO_MANY_REQUESTS)
                .header("Content-Type", "application/json")
                .body(Body::from(format!(
                    r#"{{"error": {{"message": "{e}", "type": "rate_limit_error"}}}}"#
                )))
                .unwrap();
            return error_response;
        }
    };

    // Create request context with authentication info
    let request_context = crate::request_context::RequestContext::with_auth(
        Some(auth_context.user_id.clone()),
        Some(auth_context.api_key.clone()),
    );

    // Inject request context into request extensions
    req.extensions_mut().insert(request_context);

    next.run(req).await
}

pub fn cors_middleware(cors_config: &CorsConfig) -> CorsLayer {
    if !cors_config.enabled {
        return CorsLayer::new();
    }

    let mut cors = CorsLayer::new();

    if cors_config.allowed_origins.contains(&"*".to_string()) {
        cors = cors.allow_origin(Any);
    } else {
        for origin in &cors_config.allowed_origins {
            if let Ok(origin) = origin.parse::<http::header::HeaderValue>() {
                cors = cors.allow_origin(origin);
            }
        }
    }

    cors = cors.allow_methods(Any).allow_headers(Any);

    if let Some(max_age) = cors_config.max_age {
        cors = cors.max_age(max_age);
    }

    cors
}
