//! # Configuration Management Module
//!
//! This module provides comprehensive configuration management for the Ultrafast Gateway.
//! It handles loading, validation, and environment variable overrides for all gateway settings.
//!
//! ## Overview
//!
//! The configuration system supports:
//! - TOML-based configuration files
//! - Environment variable overrides
//! - Runtime validation and schema checking
//! - Default configurations for development and production
//!
//! ## Configuration Structure
//!
//! The main `Config` struct contains all gateway settings:
//!
//! - **Server**: HTTP server settings (host, port, timeouts, CORS)
//! - **Providers**: LLM provider configurations (API keys, endpoints, rate limits)
//! - **Routing**: Request routing strategies and load balancing
//! - **Authentication**: API key management and rate limiting
//! - **Caching**: Redis and in-memory caching settings
//! - **Logging**: Log levels, formats, and output destinations
//! - **Metrics**: Performance monitoring and analytics
//! - **Plugins**: Extensible plugin system configuration
//!
//! ## Usage
//!
//! ```rust
//! use ultrafast_gateway::config::Config;
//!
//! // Load configuration from file
//! let config = Config::load("config.toml")?;
//!
//! // Validate configuration
//! config.validate()?;
//!
//! // Apply environment overrides
//! let mut config = Config::load("config.toml")?;
//! config.apply_env_overrides()?;
//! ```
//!
//! ## Environment Variables
//!
//! The configuration system supports environment variable overrides:
//!
//! - `GATEWAY_SERVER_HOST`: Override server host
//! - `GATEWAY_SERVER_PORT`: Override server port
//! - `GATEWAY_AUTH_ENABLED`: Enable/disable authentication
//! - `GATEWAY_CACHE_BACKEND`: Set cache backend (memory/redis)
//! - `GATEWAY_LOG_LEVEL`: Set logging level
//!
//! ## Configuration File Example
//!
//! ```toml
//! [server]
//! host = "0.0.0.0"
//! port = 3000
//! timeout = "30s"
//! max_body_size = 10485760
//!
//! [server.cors]
//! enabled = true
//! allowed_origins = ["*"]
//! allowed_methods = ["GET", "POST", "PUT", "DELETE"]
//!
//! [providers.openai]
//! enabled = true
//! api_key = "your-openai-key"
//! base_url = "https://api.openai.com/v1"
//! timeout = "30s"
//! max_retries = 3
//!
//! [auth]
//! enabled = true
//! jwt_secret = "your-jwt-secret"
//!
//! [auth.rate_limiting]
//! requests_per_minute = 100
//! requests_per_hour = 1000
//! tokens_per_minute = 10000
//!
//! [cache]
//! enabled = true
//! backend = "memory"
//! ttl = "1h"
//! max_size = 1000
//!
//! [metrics]
//! enabled = true
//! max_requests = 1000
//! retention_duration = "24h"
//! cleanup_interval = "1h"
//! ```

use crate::error_handling::{ErrorHandler, ErrorType};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::time::Duration;
use ultrafast_models_sdk::providers::ProviderConfig;
use ultrafast_models_sdk::routing::RoutingStrategy;

/// Main configuration struct for the Ultrafast Gateway.
///
/// This struct contains all configuration settings for the gateway,
/// including server settings, provider configurations, authentication,
/// caching, logging, metrics, and plugins.
///
/// # Example
///
/// ```rust
/// use ultrafast_gateway::config::Config;
///
/// let config = Config::load("config.toml")?;
/// config.validate()?;
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// HTTP server configuration settings
    pub server: ServerConfig,
    /// LLM provider configurations mapped by provider name
    pub providers: HashMap<String, ProviderConfig>,
    /// Request routing strategy and load balancing settings
    pub routing: RoutingConfig,
    /// Authentication and authorization settings
    pub auth: AuthConfig,
    /// Caching configuration (Redis or in-memory)
    pub cache: CacheConfig,
    /// Logging configuration (level, format, output)
    pub logging: LoggingConfig,
    /// Metrics and monitoring configuration
    pub metrics: MetricsConfig,
    /// Plugin system configuration
    pub plugins: Vec<PluginConfig>,
}

/// Configuration for metrics collection and monitoring.
///
/// Controls the collection, retention, and cleanup of performance metrics
/// and analytics data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
    /// Whether metrics collection is enabled
    pub enabled: bool,
    /// Maximum number of requests to track in memory
    pub max_requests: usize,
    /// How long to retain metrics data
    #[serde(with = "ultrafast_models_sdk::common::duration_serde")]
    pub retention_duration: Duration,
    /// How often to clean up old metrics data
    #[serde(with = "ultrafast_models_sdk::common::duration_serde")]
    pub cleanup_interval: Duration,
}

/// HTTP server configuration settings.
///
/// Defines the server's network binding, timeouts, CORS settings,
/// and request size limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Host address to bind the server to
    pub host: String,
    /// Port number to listen on
    pub port: u16,
    /// Request timeout duration
    #[serde(with = "ultrafast_models_sdk::common::duration_serde")]
    pub timeout: Duration,
    /// Maximum request body size in bytes
    pub max_body_size: usize,
    /// CORS (Cross-Origin Resource Sharing) configuration
    pub cors: CorsConfig,
}

/// CORS (Cross-Origin Resource Sharing) configuration.
///
/// Controls which origins, methods, and headers are allowed
/// in cross-origin requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorsConfig {
    /// Whether CORS is enabled
    pub enabled: bool,
    /// List of allowed origin domains
    pub allowed_origins: Vec<String>,
    /// List of allowed HTTP methods
    pub allowed_methods: Vec<String>,
    /// List of allowed HTTP headers
    pub allowed_headers: Vec<String>,
    /// Maximum age for CORS preflight responses
    pub max_age: Option<Duration>,
}

/// Request routing configuration.
///
/// Defines how requests are routed to different providers,
/// including load balancing and failover strategies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingConfig {
    /// The routing strategy to use (single, load_balancing, fallback, etc.)
    pub strategy: RoutingStrategy,
    /// How often to check provider health
    #[serde(with = "ultrafast_models_sdk::common::duration_serde")]
    pub health_check_interval: Duration,
    /// Threshold for marking a provider as failed
    pub failover_threshold: f64,
}

/// Authentication and authorization configuration.
///
/// Controls API key management, rate limiting, and user permissions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    /// Whether authentication is enabled
    pub enabled: bool,
    /// List of configured API keys
    pub api_keys: Vec<ApiKeyConfig>,
    /// Global rate limiting settings
    pub rate_limiting: RateLimitConfig,
}

/// Configuration for an individual API key.
///
/// Defines permissions, rate limits, and metadata for a specific API key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyConfig {
    /// The API key value
    pub key: String,
    /// Human-readable name for the API key
    pub name: String,
    /// Whether this API key is enabled
    pub enabled: bool,
    /// Rate limiting settings specific to this key
    pub rate_limit: Option<RateLimitConfig>,
    /// List of models this key is allowed to access
    pub allowed_models: Option<Vec<String>>,
    /// Additional metadata for the API key
    pub metadata: HashMap<String, String>,
}

/// Rate limiting configuration.
///
/// Defines limits for requests and tokens per time period.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Maximum requests per minute
    pub requests_per_minute: u32,
    /// Maximum requests per hour
    pub requests_per_hour: u32,
    /// Maximum tokens per minute
    pub tokens_per_minute: u32,
}

/// Caching configuration.
///
/// Controls how responses are cached to improve performance
/// and reduce provider API calls.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// Whether caching is enabled
    pub enabled: bool,
    /// The caching backend to use
    pub backend: CacheBackend,
    /// Time-to-live for cached responses
    #[serde(with = "ultrafast_models_sdk::common::duration_serde")]
    pub ttl: Duration,
    /// Maximum number of cached items
    pub max_size: usize,
}

/// Available caching backends.
///
/// The gateway supports both in-memory and Redis-based caching.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CacheBackend {
    /// In-memory caching (faster but not shared across instances)
    Memory,
    /// Redis-based caching (shared across multiple instances)
    Redis { url: String },
}

/// Logging configuration.
///
/// Controls log levels, output formats, and destinations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Log level (trace, debug, info, warn, error)
    pub level: String,
    /// Log output format
    pub format: LogFormat,
    /// Log output destination
    pub output: LogOutput,
}

/// Available log output formats.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogFormat {
    /// JSON format for structured logging
    Json,
    /// Human-readable pretty format
    Pretty,
    /// Compact single-line format
    Compact,
}

/// Available log output destinations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogOutput {
    /// Output to stdout
    Stdout,
    /// Output to a file
    File { path: String },
}

/// Plugin configuration.
///
/// Defines settings for extensible plugins that can modify
/// requests, responses, or add custom functionality.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginConfig {
    /// Plugin name/identifier
    pub name: String,
    /// Whether the plugin is enabled
    pub enabled: bool,
    /// Plugin-specific configuration
    pub config: HashMap<String, serde_json::Value>,
}

// Duration handling moved to shared module

impl Config {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let mut config: Config = toml::from_str(&content)?;

        // Apply environment variable overrides
        config.apply_env_overrides()?;

        // Validate configuration schema
        config.validate_schema()?;

        Ok(config)
    }

    /// Validate configuration schema and constraints
    pub fn validate_schema(&self) -> anyhow::Result<()> {
        // Validate server configuration
        if self.server.host.is_empty() {
            return Err(anyhow::anyhow!("Server host cannot be empty"));
        }

        if self.server.port == 0 {
            return Err(anyhow::anyhow!("Server port must be between 1 and 65535"));
        }

        // Validate provider configurations
        for (name, provider) in &self.providers {
            if provider.name.is_empty() {
                return Err(anyhow::anyhow!(
                    "Provider name cannot be empty for provider: {}",
                    name
                ));
            }

            if provider.timeout.as_secs() == 0 {
                return Err(anyhow::anyhow!(
                    "Provider timeout cannot be 0 for provider: {}",
                    name
                ));
            }

            // Validate base URL format for remote providers
            if let Some(base_url) = &provider.base_url {
                if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
                    return Err(anyhow::anyhow!(
                        "Invalid base URL format for provider {}: {}",
                        name,
                        base_url
                    ));
                }
            }
        }

        // Validate routing configuration
        if self.routing.health_check_interval.as_secs() == 0 {
            return Err(anyhow::anyhow!("Health check interval cannot be 0"));
        }

        if self.routing.failover_threshold <= 0.0 || self.routing.failover_threshold > 1.0 {
            return Err(anyhow::anyhow!(
                "Failover threshold must be between 0.0 and 1.0"
            ));
        }

        // Validate cache configuration
        if self.cache.enabled {
            if self.cache.ttl.as_secs() == 0 {
                return Err(anyhow::anyhow!(
                    "Cache TTL cannot be 0 when cache is enabled"
                ));
            }

            if self.cache.max_size == 0 {
                return Err(anyhow::anyhow!(
                    "Cache max size cannot be 0 when cache is enabled"
                ));
            }
        }

        // Validate metrics configuration
        if self.metrics.enabled {
            if self.metrics.max_requests == 0 {
                return Err(anyhow::anyhow!(
                    "Max requests cannot be 0 when metrics is enabled"
                ));
            }

            if self.metrics.retention_duration.as_secs() == 0 {
                return Err(anyhow::anyhow!(
                    "Retention duration cannot be 0 when metrics is enabled"
                ));
            }
        }

        Ok(())
    }

    pub fn apply_env_overrides(&mut self) -> anyhow::Result<()> {
        // Server overrides with better validation
        if let Ok(host) = env::var("GATEWAY_HOST") {
            if !host.is_empty() {
                self.server.host = host;
            }
        }

        if let Ok(port_str) = env::var("GATEWAY_PORT") {
            let port: u16 = port_str
                .parse()
                .map_err(|_| anyhow::anyhow!("Invalid GATEWAY_PORT: {}", port_str))?;
            if port > 0 {
                self.server.port = port;
            }
        }

        if let Ok(timeout) = env::var("GATEWAY_TIMEOUT") {
            self.server.timeout = parse_duration(&timeout)?;
        }

        // Enhanced provider API key handling with validation
        for (provider_name, provider_config) in &mut self.providers {
            let env_key = format!("{}_API_KEY", provider_name.to_uppercase().replace("-", "_"));
            if let Ok(api_key) = env::var(&env_key) {
                if !api_key.is_empty() {
                    provider_config.api_key = api_key;
                    tracing::debug!("Loaded API key for provider: {}", provider_name);
                } else {
                    tracing::warn!("Empty API key found for provider: {}", provider_name);
                }
            } else if provider_config.api_key.is_empty()
                && !provider_name.to_lowercase().contains("ollama")
            {
                tracing::warn!(
                    "No API key found for provider: {} (set {} environment variable)",
                    provider_name,
                    env_key
                );
            }

            // Load circuit breaker configuration from environment
            let cb_failure_threshold_key = format!(
                "{}_CB_FAILURE_THRESHOLD",
                provider_name.to_uppercase().replace("-", "_")
            );
            let cb_recovery_timeout_key = format!(
                "{}_CB_RECOVERY_TIMEOUT",
                provider_name.to_uppercase().replace("-", "_")
            );
            let cb_request_timeout_key = format!(
                "{}_CB_REQUEST_TIMEOUT",
                provider_name.to_uppercase().replace("-", "_")
            );

            if let (Ok(failure_threshold), Ok(recovery_timeout), Ok(request_timeout)) = (
                env::var(&cb_failure_threshold_key),
                env::var(&cb_recovery_timeout_key),
                env::var(&cb_request_timeout_key),
            ) {
                if let (Ok(failure_threshold), Ok(recovery_timeout), Ok(request_timeout)) = (
                    failure_threshold.parse::<u32>(),
                    parse_duration(&recovery_timeout),
                    parse_duration(&request_timeout),
                ) {
                    provider_config.circuit_breaker = Some(
                        ultrafast_models_sdk::circuit_breaker::CircuitBreakerConfig {
                            failure_threshold,
                            recovery_timeout,
                            request_timeout,
                            half_open_max_calls: 3, // Default value
                        },
                    );
                    tracing::debug!(
                        "Loaded circuit breaker config for provider: {}",
                        provider_name
                    );
                }
            }
        }

        // Enhanced gateway API keys handling with JSON validation
        if let Ok(gateway_api_keys_json) = env::var("GATEWAY_API_KEYS") {
            match serde_json::from_str::<Vec<ApiKeyConfig>>(&gateway_api_keys_json) {
                Ok(api_keys) => {
                    // Validate API key configurations
                    for (i, api_key) in api_keys.iter().enumerate() {
                        if api_key.key.is_empty() {
                            return Err(anyhow::anyhow!("API key at index {} is empty", i));
                        }
                        if api_key.name.is_empty() {
                            return Err(anyhow::anyhow!("API key name at index {} is empty", i));
                        }
                    }
                    self.auth.api_keys = api_keys;
                    tracing::info!(
                        "Loaded {} gateway API keys from environment",
                        self.auth.api_keys.len()
                    );
                }
                Err(e) => {
                    tracing::error!("Failed to parse GATEWAY_API_KEYS JSON: {}", e);
                    return Err(anyhow::anyhow!("Invalid GATEWAY_API_KEYS format: {}", e));
                }
            }
        } else if self.auth.enabled && self.auth.api_keys.is_empty() {
            tracing::warn!("Auth is enabled but no gateway API keys configured. Set GATEWAY_API_KEYS environment variable.");
        }

        // Enhanced auth overrides
        if let Ok(enabled) = env::var("GATEWAY_AUTH_ENABLED") {
            self.auth.enabled = enabled.parse().unwrap_or(false);
        }

        // Enhanced cache overrides with validation
        if let Ok(backend) = env::var("GATEWAY_CACHE_BACKEND") {
            match backend.as_str() {
                "memory" => self.cache.backend = CacheBackend::Memory,
                "redis" => {
                    let url = env::var("GATEWAY_REDIS_URL")
                        .unwrap_or_else(|_| "redis://localhost:6379".to_string());
                    if !url.starts_with("redis://") && !url.starts_with("rediss://") {
                        return Err(anyhow::anyhow!("Invalid Redis URL format: {}", url));
                    }
                    self.cache.backend = CacheBackend::Redis { url };
                }
                _ => {
                    return Err(anyhow::anyhow!(
                        "Invalid cache backend: {}. Use 'memory' or 'redis'",
                        backend
                    ));
                }
            }
        }

        // Enhanced logging overrides
        if let Ok(level) = env::var("GATEWAY_LOG_LEVEL") {
            let valid_levels = ["trace", "debug", "info", "warn", "error"];
            if valid_levels.contains(&level.as_str()) {
                self.logging.level = level;
            } else {
                return Err(anyhow::anyhow!(
                    "Invalid log level: {}. Use: {:?}",
                    level,
                    valid_levels
                ));
            }
        }

        Ok(())
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        ErrorHandler::handle_sync_operation(
            || {
                self.validate_server()?;
                self.validate_providers()?;
                self.validate_auth()?;
                self.validate_cache()?;
                self.validate_metrics()?;
                self.validate_logging()?;
                self.validate_plugins()?;
                Ok(())
            },
            "Configuration validation",
            ErrorType::Config,
        )
        .map_err(|e| anyhow::anyhow!("{}", e))?;
        Ok(())
    }

    fn validate_server(&self) -> anyhow::Result<()> {
        if self.server.port == 0 {
            return Err(anyhow::anyhow!(
                "Server port must be between 1 and 65535, got {}",
                self.server.port
            ));
        }

        if self.server.max_body_size == 0 {
            return Err(anyhow::anyhow!("Max body size cannot be 0"));
        }

        if self.server.max_body_size > 100 * 1024 * 1024 {
            // 100MB
            return Err(anyhow::anyhow!(
                "Max body size cannot exceed 100MB, got {} bytes",
                self.server.max_body_size
            ));
        }

        if self.server.timeout.as_secs() == 0 {
            return Err(anyhow::anyhow!("Server timeout cannot be 0"));
        }

        if self.server.timeout.as_secs() > 300 {
            // 5 minutes
            return Err(anyhow::anyhow!(
                "Server timeout cannot exceed 5 minutes, got {} seconds",
                self.server.timeout.as_secs()
            ));
        }

        // Validate host format
        if self.server.host.is_empty() {
            return Err(anyhow::anyhow!("Server host cannot be empty"));
        }

        // Validate CORS config
        if self.server.cors.enabled {
            if self.server.cors.allowed_origins.is_empty() {
                return Err(anyhow::anyhow!(
                    "CORS enabled but no allowed origins specified"
                ));
            }

            if self.server.cors.allowed_methods.is_empty() {
                return Err(anyhow::anyhow!(
                    "CORS enabled but no allowed methods specified"
                ));
            }
        }

        Ok(())
    }

    fn validate_providers(&self) -> anyhow::Result<()> {
        if self.providers.is_empty() {
            return Err(anyhow::anyhow!("At least one provider must be configured"));
        }

        // Check if at least one provider is enabled
        let enabled_providers: Vec<_> = self
            .providers
            .iter()
            .filter(|(_, provider)| provider.enabled)
            .collect();
        if enabled_providers.is_empty() {
            return Err(anyhow::anyhow!("At least one provider must be enabled"));
        }

        for (name, provider) in &self.providers {
            // Only validate enabled providers
            if !provider.enabled {
                continue;
            }

            // Validate provider name
            if name.is_empty() {
                return Err(anyhow::anyhow!("Provider name cannot be empty"));
            }

            // Allow empty API keys for local providers like Ollama
            if provider.api_key.is_empty() && !name.to_lowercase().contains("ollama") {
                return Err(anyhow::anyhow!("Provider {} has empty API key", name));
            }

            if provider.timeout.as_secs() == 0 {
                return Err(anyhow::anyhow!("Provider {} has zero timeout", name));
            }

            if provider.timeout.as_secs() > 300 {
                // 5 minutes
                return Err(anyhow::anyhow!(
                    "Provider {} timeout cannot exceed 5 minutes, got {} seconds",
                    name,
                    provider.timeout.as_secs()
                ));
            }

            if provider.max_retries > 10 {
                return Err(anyhow::anyhow!(
                    "Provider {} max_retries cannot exceed 10, got {}",
                    name,
                    provider.max_retries
                ));
            }

            // Validate base URL if provided
            if let Some(base_url) = &provider.base_url {
                if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
                    return Err(anyhow::anyhow!(
                        "Provider {} base_url must start with http:// or https://",
                        name
                    ));
                }
            }

            // Validate rate limits if provided
            if let Some(rate_limit) = &provider.rate_limit {
                if rate_limit.requests_per_minute == 0 && rate_limit.tokens_per_minute == 0 {
                    return Err(anyhow::anyhow!("Provider {} rate limit must have non-zero requests_per_minute or tokens_per_minute", name));
                }
            }
        }

        Ok(())
    }

    fn validate_auth(&self) -> anyhow::Result<()> {
        if self.auth.enabled {
            if self.auth.api_keys.is_empty() {
                return Err(anyhow::anyhow!("Auth enabled but no API keys configured"));
            }

            // Validate each API key
            for (i, api_key_config) in self.auth.api_keys.iter().enumerate() {
                if api_key_config.key.is_empty() {
                    return Err(anyhow::anyhow!("API key at index {} has empty key", i));
                }

                if api_key_config.key.len() < 16 {
                    return Err(anyhow::anyhow!(
                        "API key at index {} is too short (minimum 16 characters)",
                        i
                    ));
                }

                if api_key_config.name.is_empty() {
                    return Err(anyhow::anyhow!("API key at index {} has empty name", i));
                }

                // Validate rate limits if provided
                if let Some(rate_limit) = &api_key_config.rate_limit {
                    if rate_limit.requests_per_minute == 0 {
                        return Err(anyhow::anyhow!(
                            "API key {} rate limit requests_per_minute cannot be 0",
                            api_key_config.name
                        ));
                    }
                    if rate_limit.requests_per_hour == 0 {
                        return Err(anyhow::anyhow!(
                            "API key {} rate limit requests_per_hour cannot be 0",
                            api_key_config.name
                        ));
                    }
                    if rate_limit.tokens_per_minute == 0 {
                        return Err(anyhow::anyhow!(
                            "API key {} rate limit tokens_per_minute cannot be 0",
                            api_key_config.name
                        ));
                    }
                }
            }

            // Check for duplicate API keys
            let mut seen_keys = std::collections::HashSet::new();
            let mut seen_names = std::collections::HashSet::new();

            for api_key_config in &self.auth.api_keys {
                if !seen_keys.insert(&api_key_config.key) {
                    return Err(anyhow::anyhow!(
                        "Duplicate API key found: {}",
                        api_key_config.key
                    ));
                }
                if !seen_names.insert(&api_key_config.name) {
                    return Err(anyhow::anyhow!(
                        "Duplicate API key name found: {}",
                        api_key_config.name
                    ));
                }
            }
        }

        // Validate global rate limiting
        if self.auth.rate_limiting.requests_per_minute == 0 {
            return Err(anyhow::anyhow!(
                "Global rate limiting requests_per_minute cannot be 0"
            ));
        }
        if self.auth.rate_limiting.requests_per_hour == 0 {
            return Err(anyhow::anyhow!(
                "Global rate limiting requests_per_hour cannot be 0"
            ));
        }
        if self.auth.rate_limiting.tokens_per_minute == 0 {
            return Err(anyhow::anyhow!(
                "Global rate limiting tokens_per_minute cannot be 0"
            ));
        }

        Ok(())
    }

    fn validate_cache(&self) -> anyhow::Result<()> {
        if self.cache.enabled {
            if self.cache.max_size == 0 {
                return Err(anyhow::anyhow!("Cache enabled but max size is 0"));
            }

            if self.cache.max_size > 1_000_000 {
                // 1M entries
                return Err(anyhow::anyhow!(
                    "Cache max_size cannot exceed 1,000,000 entries, got {}",
                    self.cache.max_size
                ));
            }

            if self.cache.ttl.as_secs() == 0 {
                return Err(anyhow::anyhow!("Cache TTL cannot be 0"));
            }

            // Validate Redis URL if Redis backend is used
            if let CacheBackend::Redis { url } = &self.cache.backend {
                if !url.starts_with("redis://") && !url.starts_with("rediss://") {
                    return Err(anyhow::anyhow!(
                        "Redis URL must start with redis:// or rediss://"
                    ));
                }
            }
        }

        Ok(())
    }

    fn validate_metrics(&self) -> anyhow::Result<()> {
        if self.metrics.enabled {
            if self.metrics.max_requests == 0 {
                return Err(anyhow::anyhow!("Metrics max_requests cannot be 0"));
            }

            if self.metrics.max_requests > 1_000_000 {
                // 1M requests
                return Err(anyhow::anyhow!(
                    "Metrics max_requests cannot exceed 1,000,000, got {}",
                    self.metrics.max_requests
                ));
            }

            if self.metrics.retention_duration.as_secs() == 0 {
                return Err(anyhow::anyhow!("Metrics retention_duration cannot be 0"));
            }

            if self.metrics.cleanup_interval.as_secs() == 0 {
                return Err(anyhow::anyhow!("Metrics cleanup_interval cannot be 0"));
            }

            if self.metrics.cleanup_interval > self.metrics.retention_duration {
                return Err(anyhow::anyhow!(
                    "Metrics cleanup_interval cannot be longer than retention_duration"
                ));
            }
        }

        Ok(())
    }

    fn validate_logging(&self) -> anyhow::Result<()> {
        // Validate log level
        let valid_levels = ["trace", "debug", "info", "warn", "error"];
        if !valid_levels.contains(&self.logging.level.as_str()) {
            return Err(anyhow::anyhow!(
                "Invalid log level: {}. Must be one of: {}",
                self.logging.level,
                valid_levels.join(", ")
            ));
        }

        // Validate file output path if specified
        if let LogOutput::File { path } = &self.logging.output {
            if path.is_empty() {
                return Err(anyhow::anyhow!("Log file path cannot be empty"));
            }

            // Check if parent directory exists
            if let Some(parent) = std::path::Path::new(path).parent() {
                if !parent.exists() {
                    return Err(anyhow::anyhow!(
                        "Log file parent directory does not exist: {}",
                        parent.display()
                    ));
                }
            }
        }

        Ok(())
    }

    fn validate_plugins(&self) -> anyhow::Result<()> {
        let valid_plugins = [
            "cost_tracking",
            "content_filtering",
            "logging",
            "input_validation",
        ];
        let mut seen_plugins = std::collections::HashSet::new();

        for plugin_config in &self.plugins {
            if plugin_config.name.is_empty() {
                return Err(anyhow::anyhow!("Plugin name cannot be empty"));
            }

            if !valid_plugins.contains(&plugin_config.name.as_str()) {
                return Err(anyhow::anyhow!(
                    "Unknown plugin: {}. Valid plugins: {}",
                    plugin_config.name,
                    valid_plugins.join(", ")
                ));
            }

            if !seen_plugins.insert(&plugin_config.name) {
                return Err(anyhow::anyhow!(
                    "Duplicate plugin configuration: {}",
                    plugin_config.name
                ));
            }

            // Validate plugin-specific configurations
            if plugin_config.name.as_str() == "content_filtering" {
                if let Some(max_input_length) = plugin_config.config.get("max_input_length") {
                    if let Some(length) = max_input_length.as_u64() {
                        if length == 0 {
                            return Err(anyhow::anyhow!(
                                "content_filtering max_input_length cannot be 0"
                            ));
                        }
                        if length > 1_000_000 {
                            // 1MB
                            return Err(anyhow::anyhow!("content_filtering max_input_length cannot exceed 1,000,000 characters"));
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                host: "127.0.0.1".to_string(),
                port: 3000,
                timeout: Duration::from_secs(30),
                max_body_size: 1024 * 1024, // 1MB
                cors: CorsConfig {
                    enabled: true,
                    allowed_origins: vec!["*".to_string()],
                    allowed_methods: vec!["GET".to_string(), "POST".to_string()],
                    allowed_headers: vec!["*".to_string()],
                    max_age: Some(Duration::from_secs(3600)),
                },
            },
            providers: HashMap::new(),
            routing: RoutingConfig {
                strategy: RoutingStrategy::Single,
                health_check_interval: Duration::from_secs(30),
                failover_threshold: 0.8,
            },
            auth: AuthConfig {
                enabled: false,
                api_keys: vec![],
                rate_limiting: RateLimitConfig {
                    requests_per_minute: 60,
                    requests_per_hour: 1000,
                    tokens_per_minute: 10000,
                },
            },
            cache: CacheConfig {
                enabled: true,
                backend: CacheBackend::Memory,
                ttl: Duration::from_secs(300),
                max_size: 1000,
            },
            logging: LoggingConfig {
                level: "info".to_string(),
                format: LogFormat::Pretty,
                output: LogOutput::Stdout,
            },
            metrics: MetricsConfig {
                enabled: true,
                max_requests: 10000,
                retention_duration: Duration::from_secs(3600), // 1 hour
                cleanup_interval: Duration::from_secs(300),    // 5 minutes
            },
            plugins: vec![],
        }
    }
}

pub fn parse_duration(s: &str) -> anyhow::Result<Duration> {
    ultrafast_models_sdk::common::duration_serde::parse_duration(s)
}
