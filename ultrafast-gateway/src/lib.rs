//! # Ultrafast Gateway Library
//!
//! A high-performance AI gateway built in Rust that provides a unified interface to multiple LLM providers
//! with advanced routing, caching, and monitoring capabilities.
//!
//! ## Overview
//!
//! The Ultrafast Gateway is designed to be a production-ready, enterprise-grade solution for managing
//! multiple AI/LLM providers through a single, unified API. It supports both standalone mode for direct
//! provider calls and gateway mode for centralized server operations.
//!
//! ## Key Features
//!
//! - **Multi-Provider Support**: Unified interface for 10+ LLM providers (OpenAI, Anthropic, Azure, etc.)
//! - **Advanced Routing**: Load balancing, fallback, conditional routing, and A/B testing
//! - **Enterprise Security**: Authentication, rate limiting, request validation, and content filtering
//! - **High Performance**: <1ms routing overhead, 10,000+ requests/second throughput
//! - **Real-time Monitoring**: Comprehensive metrics, cost tracking, and health monitoring
//! - **Caching & Optimization**: Redis and in-memory caching with JSON optimization
//! - **Fault Tolerance**: Circuit breakers, automatic failover, and error recovery
//!
//! ## Architecture
//!
//! The library is organized into several core modules:
//!
//! - **`auth`**: Authentication, authorization, and rate limiting
//! - **`config`**: Configuration management and validation
//! - **`server`**: HTTP server setup and request handling
//! - **`handlers`**: API endpoint handlers and business logic
//! - **`middleware`**: Request/response middleware and validation
//! - **`metrics`**: Performance monitoring and analytics
//! - **`gateway_caching`**: Caching layer with Redis support
//! - **`advanced_routing`**: Intelligent request routing strategies
//! - **`error_handling`**: Comprehensive error handling utilities
//!
//! ## Quick Start
//!
//! ```rust
//! use ultrafast_gateway::{create_server, config::Config};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Load configuration
//!     let config = Config::from_file("config.toml")?;
//!     
//!     // Create and start the server
//!     let app = create_server(config).await?;
//!     
//!     // The server is now ready to handle requests
//!     Ok(())
//! }
//! ```
//!
//! ## Provider Integration
//!
//! The gateway supports multiple providers through a unified interface:
//!
//! ```rust
//! use ultrafast_models_sdk::{UltrafastClient, ChatRequest, Message};
//!
//! let client = UltrafastClient::standalone()
//!     .with_openai("your-openai-key")
//!     .with_anthropic("your-anthropic-key")
//!     .build()?;
//!
//! let response = client.chat_completion(ChatRequest {
//!     model: "gpt-4".to_string(),
//!     messages: vec![Message::user("Hello, world!")],
//!     ..Default::default()
//! }).await?;
//! ```
//!
//! ## Configuration
//!
//! The gateway uses TOML configuration files for easy setup:
//!
//! ```toml
//! [server]
//! host = "0.0.0.0"
//! port = 3000
//!
//! [providers.openai]
//! enabled = true
//! api_key = "your-openai-key"
//! base_url = "https://api.openai.com/v1"
//!
//! [auth]
//! enabled = true
//! jwt_secret = "your-jwt-secret"
//! ```
//!
//! ## Performance
//!
//! - **Latency**: <1ms routing overhead
//! - **Throughput**: 10,000+ requests/second
//! - **Concurrency**: 100,000+ concurrent connections
//! - **Memory**: <1GB under normal load
//! - **Uptime**: 99.9% with automatic failover
//!
//! ## Security
//!
//! - Virtual API keys with user isolation
//! - JWT token-based authentication
//! - Rate limiting per user/provider
//! - Request validation and sanitization
//! - Content filtering with plugin system
//!
//! ## Monitoring
//!
//! Built-in metrics and monitoring endpoints:
//!
//! - `/health` - Service health check
//! - `/metrics` - Performance metrics
//! - `/admin/providers` - Provider status
//! - `/admin/config` - Configuration status
//!
//! ## License
//!
//! This project is licensed under either of
//!
//! * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or https://www.apache.org/licenses/LICENSE-2.0)
//! * MIT license ([LICENSE-MIT](LICENSE-MIT) or https://opensource.org/licenses/MIT)
//!
//! at your option.

pub mod advanced_routing;
pub mod auth;
pub mod config;
pub mod dashboard;
pub mod error_handling;
pub mod gateway_caching;
pub mod gateway_error;
pub mod handlers;
pub mod json_optimization;
pub mod metrics;
pub mod middleware;
pub mod plugins;
pub mod request_context;
pub mod server;
pub mod test_server;

pub use server::create_server;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use axum::http::StatusCode;
    use axum_test::TestServer;

    #[tokio::test]
    async fn test_health_check() {
        let mut config = Config::default();
        config.providers.insert(
            "openai".to_string(),
            ultrafast_models_sdk::providers::ProviderConfig {
                name: "openai".to_string(),
                api_key: "test-key".to_string(),
                base_url: Some("https://api.openai.com/v1".to_string()),
                timeout: std::time::Duration::from_secs(30),
                max_retries: 3,
                retry_delay: std::time::Duration::from_secs(1),
                enabled: true,
                model_mapping: std::collections::HashMap::new(),
                headers: std::collections::HashMap::new(),
                rate_limit: Some(ultrafast_models_sdk::providers::RateLimit {
                    requests_per_minute: 1000,
                    tokens_per_minute: 100000,
                }),
                circuit_breaker: None,
            },
        );
        let app = create_server(config).await.unwrap();
        let server = TestServer::new(app).unwrap();
        let response = server.get("/health").await;
        assert_eq!(response.status_code(), StatusCode::OK);
        let body = response.json::<serde_json::Value>();
        assert_eq!(body["status"], "healthy");
        assert!(body["timestamp"].is_string());
        assert!(body["version"].is_string());
    }

    #[tokio::test]
    async fn test_metrics_endpoint() {
        let mut config = Config::default();
        config.providers.insert(
            "openai".to_string(),
            ultrafast_models_sdk::providers::ProviderConfig {
                name: "openai".to_string(),
                api_key: "test-key".to_string(),
                base_url: Some("https://api.openai.com/v1".to_string()),
                timeout: std::time::Duration::from_secs(30),
                max_retries: 3,
                retry_delay: std::time::Duration::from_secs(1),
                enabled: true,
                model_mapping: std::collections::HashMap::new(),
                headers: std::collections::HashMap::new(),
                rate_limit: Some(ultrafast_models_sdk::providers::RateLimit {
                    requests_per_minute: 1000,
                    tokens_per_minute: 100000,
                }),
                circuit_breaker: None,
            },
        );
        let app = create_server(config).await.unwrap();
        let server = TestServer::new(app).unwrap();
        let response = server.get("/metrics").await;
        assert_eq!(response.status_code(), StatusCode::OK);
        let body = response.json::<serde_json::Value>();
        assert!(body.is_object());
    }

    #[tokio::test]
    async fn test_list_providers() {
        let mut config = Config::default();
        config.providers.insert(
            "openai".to_string(),
            ultrafast_models_sdk::providers::ProviderConfig {
                name: "openai".to_string(),
                api_key: "test-key".to_string(),
                base_url: Some("https://api.openai.com/v1".to_string()),
                timeout: std::time::Duration::from_secs(30),
                max_retries: 3,
                retry_delay: std::time::Duration::from_secs(1),
                enabled: true,
                model_mapping: std::collections::HashMap::new(),
                headers: std::collections::HashMap::new(),
                rate_limit: Some(ultrafast_models_sdk::providers::RateLimit {
                    requests_per_minute: 1000,
                    tokens_per_minute: 100000,
                }),
                circuit_breaker: None,
            },
        );
        let app = create_server(config).await.unwrap();
        let server = TestServer::new(app).unwrap();
        let response = server.get("/admin/providers").await;
        assert_eq!(response.status_code(), StatusCode::OK);
        let body = response.json::<serde_json::Value>();
        assert!(body["providers"].is_array());
        assert_eq!(body["providers"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_config_validation() {
        let mut config = Config::default();

        // Add a provider to make the config valid
        config.providers.insert(
            "test".to_string(),
            ultrafast_models_sdk::providers::ProviderConfig {
                name: "test".to_string(),
                api_key: "test-key".to_string(),
                base_url: Some("https://api.test.com/v1".to_string()),
                timeout: std::time::Duration::from_secs(30),
                max_retries: 3,
                retry_delay: std::time::Duration::from_secs(1),
                enabled: true,
                model_mapping: std::collections::HashMap::new(),
                headers: std::collections::HashMap::new(),
                rate_limit: None,
                circuit_breaker: None,
            },
        );

        // Valid config should pass
        assert!(config.validate().is_ok());

        // Invalid config - zero port
        config.server.port = 0;
        assert!(config.validate().is_err());

        // Reset and test invalid max body size
        config.server.port = 3000;
        config.server.max_body_size = 0;
        assert!(config.validate().is_err());
    }

    #[tokio::test]
    async fn test_duration_parsing() {
        use std::time::Duration;

        // Test string format
        let duration = crate::config::parse_duration("30s").unwrap();
        assert_eq!(duration, Duration::from_secs(30));

        let duration = crate::config::parse_duration("1m").unwrap();
        assert_eq!(duration, Duration::from_secs(60));

        let duration = crate::config::parse_duration("2h").unwrap();
        assert_eq!(duration, Duration::from_secs(7200));

        let duration = crate::config::parse_duration("500ms").unwrap();
        assert_eq!(duration, Duration::from_millis(500));

        // Test invalid formats
        assert!(crate::config::parse_duration("").is_err());
        assert!(crate::config::parse_duration("30x").is_err());
        assert!(crate::config::parse_duration("abc").is_err());
    }

    #[tokio::test]
    async fn test_environment_overrides() {
        let mut config = Config::default();

        // Set environment variables
        std::env::set_var("GATEWAY_HOST", "0.0.0.0");
        std::env::set_var("GATEWAY_PORT", "8080");
        std::env::set_var("GATEWAY_TIMEOUT", "60s");
        std::env::set_var("GATEWAY_LOG_LEVEL", "debug");

        // Apply overrides
        config.apply_env_overrides().unwrap();

        // Verify overrides were applied
        assert_eq!(config.server.host, "0.0.0.0");
        assert_eq!(config.server.port, 8080);
        assert_eq!(config.server.timeout.as_secs(), 60);
        assert_eq!(config.logging.level, "debug");

        // Clean up environment
        std::env::remove_var("GATEWAY_HOST");
        std::env::remove_var("GATEWAY_PORT");
        std::env::remove_var("GATEWAY_TIMEOUT");
        std::env::remove_var("GATEWAY_LOG_LEVEL");
    }
}
