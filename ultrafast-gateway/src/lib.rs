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
//!     max_tokens: Some(100),
//!     temperature: Some(0.7),
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
//! The gateway implements enterprise-grade security features:
//!
//! - **API Key Management**: Virtual API keys with rate limiting
//! - **JWT Authentication**: Stateless token-based authentication
//! - **Request Validation**: Comprehensive input sanitization
//! - **Content Filtering**: Plugin-based content moderation
//! - **Rate Limiting**: Per-user and per-provider limits
//!
//! ## Monitoring & Observability
//!
//! Built-in monitoring capabilities include:
//!
//! - **Real-time Dashboard**: WebSocket-based live metrics
//! - **Performance Metrics**: Latency, throughput, and error rates
//! - **Provider Health**: Circuit breaker status and health checks
//! - **Cost Tracking**: Real-time cost monitoring per provider
//! - **Logging**: Structured logging with multiple output formats
//!
//! ## Examples
//!
//! ### Basic Server Setup
//!
//! ```rust
//! use ultrafast_gateway::{create_server, config::Config};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Load configuration from file
//!     let config = Config::from_file("config.toml")?;
//!     
//!     // Create and start the server
//!     let app = create_server(config).await?;
//!     
//!     println!("Gateway server started successfully!");
//!     Ok(())
//! }
//! ```
//!
//! ### Custom Configuration
//!
//! ```rust
//! use ultrafast_gateway::config::{Config, ServerConfig, ProviderConfig};
//!
//! let config = Config {
//!     server: ServerConfig {
//!         host: "0.0.0.0".to_string(),
//!         port: 8080,
//!         timeout: Duration::from_secs(30),
//!         ..Default::default()
//!     },
//!     providers: HashMap::new(),
//!     auth: Default::default(),
//!     cache: Default::default(),
//!     routing: Default::default(),
//!     metrics: Default::default(),
//!     logging: Default::default(),
//!     plugins: Vec::new(),
//! };
//! ```
//!
//! ### Plugin System
//!
//! ```rust
//! use ultrafast_gateway::plugins::{Plugin, PluginConfig};
//!
//! #[derive(Debug)]
//! struct CustomPlugin;
//!
//! impl Plugin for CustomPlugin {
//!     fn name(&self) -> &'static str { "custom_plugin" }
//!     fn process_request(&self, request: &mut Request) -> Result<(), Error> {
//!         // Custom request processing logic
//!         Ok(())
//!     }
//! }
//! ```
//!
//! ## Error Handling
//!
//! The gateway provides comprehensive error handling:
//!
//! ```rust
//! use ultrafast_gateway::error_handling::{ErrorHandler, ErrorType};
//!
//! match result {
//!     Ok(response) => println!("Success: {:?}", response),
//!     Err(ErrorType::AuthenticationError) => println!("Authentication failed"),
//!     Err(ErrorType::RateLimitExceeded) => println!("Rate limit exceeded"),
//!     Err(ErrorType::ProviderUnavailable) => println!("Provider unavailable"),
//!     Err(e) => println!("Other error: {:?}", e),
//! }
//! ```
//!
//! ## Testing
//!
//! The library includes comprehensive testing utilities:
//!
//! ```rust
//! #[cfg(test)]
//! mod tests {
//!     use super::*;
//!     use tokio_test;
//!
//!     #[tokio_test]
//!     async fn test_server_creation() {
//!         let config = Config::default();
//!         let result = create_server(config).await;
//!         assert!(result.is_ok());
//!     }
//! }
//! ```
//!
//! ## Performance Tuning
//!
//! For optimal performance, consider these settings:
//!
//! ```toml
//! [server]
//! # Use multiple worker threads
//! worker_threads = 8
//!
//! [cache]
//! # Enable Redis for distributed caching
//! backend = "Redis"
//! ttl = "6h"
//!
//! [routing]
//! # Aggressive health checking for fast failover
//! health_check_interval = "10s"
//! failover_threshold = 0.7
//! ```
//!
//! ## Deployment
//!
//! The gateway is designed for production deployment:
//!
//! - **Docker Support**: Multi-stage Dockerfile with Alpine Linux
//! - **Health Checks**: Built-in health check endpoints
//! - **Graceful Shutdown**: Proper signal handling and cleanup
//! - **Resource Limits**: Configurable memory and CPU limits
//! - **Monitoring**: Prometheus metrics and Grafana dashboards
//!
//! ## Contributing
//!
//! We welcome contributions! Please see our contributing guide for details on:
//!
//! - Code style and formatting
//! - Testing requirements
//! - Documentation standards
//! - Pull request process
//!
//! ## License
//!
//! This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
//!
//! ## Support
//!
//! For support and questions:
//!
//! - **Issues**: [GitHub Issues](https://github.com/techgopal/ultrafast-ai-gateway/issues)
//! - **Discussions**: [GitHub Discussions](https://github.com/techgopal/ultrafast-ai-gateway/discussions)
//! - **Documentation**: [Project Wiki](https://github.com/techgopal/ultrafast-ai-gateway/wiki)

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
