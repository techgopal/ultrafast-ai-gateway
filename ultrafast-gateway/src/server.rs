//! # HTTP Server Module
//!
//! This module provides the HTTP server setup and configuration for the Ultrafast Gateway.
//! It handles server initialization, middleware setup, routing, and application state management.
//!
//! ## Overview
//!
//! The server module is responsible for:
//! - **Server Initialization**: Setting up the HTTP server with Axum
//! - **Middleware Configuration**: Authentication, CORS, logging, metrics, and plugins
//! - **Route Registration**: API endpoints for chat, embeddings, and admin functions
//! - **Application State**: Shared state across all handlers
//! - **Plugin Integration**: Dynamic plugin loading and management
//!
//! ## Architecture
//!
//! The server uses Axum as the web framework with the following layers:
//!
//! 1. **Timeout Layer**: Request timeout handling
//! 2. **CORS Middleware**: Cross-origin resource sharing
//! 3. **Logging Middleware**: Request/response logging
//! 4. **Metrics Middleware**: Performance monitoring
//! 5. **Authentication Middleware**: API key and JWT validation
//! 6. **Input Validation Middleware**: Request validation and sanitization
//! 7. **Plugin Middleware**: Dynamic request/response modification
//!
//! ## API Endpoints
//!
//! ### Core API Endpoints
//!
//! - `POST /v1/chat/completions` - Chat completion API
//! - `POST /v1/embeddings` - Text embedding API
//! - `POST /v1/images/generations` - Image generation API
//!
//! ### Admin Endpoints
//!
//! - `GET /health` - Health check endpoint
//! - `GET /metrics` - Performance metrics
//! - `GET /admin/providers` - Provider status
//! - `GET /admin/config` - Configuration status
//!
//! ### WebSocket Endpoints
//!
//! - `GET /ws/dashboard` - Real-time dashboard WebSocket
//!
//! ## Usage
//!
//! ```rust
//! use ultrafast_gateway::server::create_server;
//! use ultrafast_gateway::config::Config;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let config = Config::load("config.toml")?;
//!     let app = create_server(config).await?;
//!     
//!     // The server is ready to handle requests
//!     Ok(())
//! }
//! ```
//!
//! ## Middleware Stack
//!
//! The server applies middleware in the following order:
//!
//! 1. **Timeout**: Ensures requests don't hang indefinitely
//! 2. **CORS**: Handles cross-origin requests
//! 3. **Logging**: Records request/response details
//! 4. **Metrics**: Tracks performance metrics
//! 5. **Authentication**: Validates API keys and JWT tokens
//! 6. **Input Validation**: Validates and sanitizes requests
//! 7. **Plugin Processing**: Applies dynamic plugins
//!
//! ## Application State
//!
//! The `AppState` struct contains shared state accessible to all handlers:
//!
//! - **Configuration**: Server and provider configuration
//! - **Client**: Ultrafast SDK client for provider communication
//! - **Plugin Manager**: Dynamic plugin management
//! - **Cache Manager**: Redis and in-memory caching
//! - **WebSocket Manager**: Real-time dashboard connections
//!
//! ## Error Handling
//!
//! The server includes comprehensive error handling:
//!
//! - **Timeout Errors**: Automatic request cancellation
//! - **Authentication Errors**: Proper HTTP status codes
//! - **Validation Errors**: Detailed error messages
//! - **Provider Errors**: Graceful fallback handling
//! - **Plugin Errors**: Non-blocking plugin failures

use crate::config::Config;
use crate::dashboard::websocket::WebSocketManager;
use crate::gateway_caching::CacheManager;
use crate::handlers;
use crate::middleware::{
    auth_middleware, cors_middleware, logging_middleware, metrics_middleware, plugin_middleware,
};
use crate::plugins::{create_plugin, PluginManager};
use axum::{
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use tower::ServiceBuilder;
use tower_http::timeout::TimeoutLayer;
use ultrafast_models_sdk::UltrafastClient;

/// Application state shared across all HTTP handlers.
///
/// Contains all the necessary components for handling requests:
/// configuration, client, plugins, caching, and WebSocket management.
///
/// # Thread Safety
///
/// All fields are wrapped in `Arc` for thread-safe sharing across
/// multiple request handlers.
///
/// # Example
///
/// ```rust
/// let app_state = AppState {
///     config: Arc::new(config),
///     client: Arc::new(client),
///     plugin_manager: Arc::new(plugin_manager),
///     cache_manager: Arc::new(cache_manager),
///     websocket_manager: Some(Arc::new(websocket_manager)),
/// };
/// ```
pub struct AppState {
    /// Server and provider configuration
    pub config: Arc<Config>,
    /// Ultrafast SDK client for provider communication
    pub client: Arc<UltrafastClient>,
    /// Dynamic plugin management system
    pub plugin_manager: Arc<PluginManager>,
    /// Redis and in-memory caching layer
    pub cache_manager: Arc<CacheManager>,
    /// Real-time dashboard WebSocket connections
    pub websocket_manager: Option<Arc<WebSocketManager>>,
}

impl Clone for AppState {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            client: self.client.clone(),
            plugin_manager: self.plugin_manager.clone(),
            cache_manager: self.cache_manager.clone(),
            websocket_manager: self.websocket_manager.clone(),
        }
    }
}

/// Create and configure the HTTP server with all middleware and routes.
///
/// This function initializes all components of the gateway:
/// - Ultrafast SDK client with configured providers
/// - Cache manager (Redis or in-memory)
/// - Authentication service and rate limiter
/// - Metrics collection system
/// - Plugin manager with configured plugins
/// - WebSocket manager for real-time dashboard
/// - HTTP server with middleware stack and routes
///
/// # Arguments
///
/// * `config` - The gateway configuration containing all settings
///
/// # Returns
///
/// Returns a configured Axum router ready to handle HTTP requests.
///
/// # Errors
///
/// Returns an error if:
/// - SDK client cannot be created
/// - Cache manager cannot be initialized
/// - Authentication service cannot be set up
/// - Metrics system cannot be initialized
/// - Plugin manager cannot be created
///
/// # Example
///
/// ```rust
/// let config = Config::load("config.toml")?;
/// let app = create_server(config).await?;
/// ```
pub async fn create_server(config: Config) -> anyhow::Result<Router> {
    // Create the SDK client in standalone mode with configured providers
    let mut client_builder = UltrafastClient::standalone();

    // Add configured providers to the client
    for (name, provider_config) in &config.providers {
        client_builder = client_builder.with_provider(name.clone(), provider_config.clone());
    }

    // If no providers configured, add Ollama as default for development
    if config.providers.is_empty() {
        client_builder = client_builder.with_ollama("http://localhost:11434");
    }

    // Build the client with the configured routing strategy
    let client = client_builder
        .with_routing_strategy(config.routing.strategy.clone())
        .build()?;

    // Initialize cache manager with the configured backend
    let cache_manager = Arc::new(CacheManager::new(config.cache.clone()).await?);

    // Initialize authentication service and rate limiter with cache manager
    crate::auth::initialize_auth_service(config.auth.clone(), cache_manager.clone()).await;

    // Perform security sanity check for JWT secrets
    if let Err(e) = {
        // Create a temporary auth service instance for sanity checking
        let tmp = crate::auth::AuthService::new(config.auth.clone());
        tmp.sanity_check()
    } {
        return Err(anyhow::anyhow!(e.to_string()));
    }

    // Initialize rate limiter with cache manager for distributed rate limiting
    crate::auth::initialize_rate_limiter(cache_manager.clone()).await?;

    // Initialize metrics collector with configuration
    let metrics_config = crate::metrics::MetricsConfig {
        enabled: config.metrics.enabled,
        max_requests: config.metrics.max_requests,
        retention_duration: config.metrics.retention_duration,
        cleanup_interval: config.metrics.cleanup_interval,
    };
    crate::metrics::initialize_metrics(metrics_config).await;

    // Initialize plugin manager for dynamic functionality
    let mut plugin_manager = PluginManager::new();

    // Register plugins from configuration
    for plugin_config in &config.plugins {
        if plugin_config.enabled {
            match create_plugin(plugin_config) {
                Ok(plugin) => {
                    if let Err(e) = plugin_manager.register_plugin(plugin).await {
                        tracing::error!("Failed to register plugin {}: {}", plugin_config.name, e);
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to create plugin {}: {}", plugin_config.name, e);
                }
            }
        }
    }

    let plugin_manager = Arc::new(plugin_manager);

    // Initialize WebSocket manager for dashboard real-time updates
    let websocket_manager = {
        let ws_manager = Arc::new(WebSocketManager::new());
        ws_manager.start_background_tasks().await;
        Some(ws_manager)
    };

    let state = AppState {
        config: Arc::new(config.clone()),
        client: Arc::new(client),
        plugin_manager,
        cache_manager,
        websocket_manager,
    };

    // Warn if permissive CORS is used in production-like settings
    if config.server.cors.enabled && config.server.cors.allowed_origins.iter().any(|o| o == "*") {
        tracing::warn!(
            "CORS is enabled with wildcard origins. This is unsafe for production. Configure explicit allowed_origins."
        );
    }

    // Create the main router
    let app = Router::new()
        // OpenAI-compatible endpoints
        .route("/v1/chat/completions", post(handlers::chat_completions))
        .route("/v1/completions", post(handlers::completions))
        .route("/v1/embeddings", post(handlers::embeddings))
        .route("/v1/images/generations", post(handlers::image_generations))
        .route(
            "/v1/audio/transcriptions",
            post(handlers::audio_transcriptions),
        )
        .route("/v1/audio/speech", post(handlers::text_to_speech))
        .route("/v1/models", get(handlers::list_models))
        // Health and admin endpoints
        .route("/health", get(handlers::health_check))
        .route("/metrics", get(handlers::metrics))
        .route("/metrics/prometheus", get(handlers::prometheus_metrics))
        .route("/admin/providers", get(handlers::list_providers))
        .route("/admin/config", get(handlers::get_config))
        .route(
            "/admin/circuit-breakers",
            get(handlers::get_circuit_breaker_metrics),
        )
        // Dashboard routes
        .route("/dashboard", get(handlers::dashboard))
        .route("/dashboard.js", get(handlers::dashboard_js))
        .route("/dashboard.css", get(handlers::dashboard_css))
        .route("/ws/dashboard", get(handlers::dashboard_websocket))
        // Middleware stack (plugins now handle input validation)
        .layer(
            ServiceBuilder::new()
                .layer(cors_middleware(&config.server.cors)) // 1. CORS (first)
                .layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    auth_middleware,
                )) // 2. Authentication (includes rate limiting)
                .layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    plugin_middleware::plugin_middleware,
                )) // 3. Plugins (after auth)
                .layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    logging_middleware,
                )) // 4. Logging (only authenticated requests)
                .layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    metrics_middleware,
                )) // 5. Metrics (only authenticated requests)
                .layer(TimeoutLayer::new(config.server.timeout)), // 6. Timeout (last)
        )
        .with_state(state);

    Ok(app)
}
