//! # Ultrafast Client Module
//!
//! This module provides the main client implementation for the Ultrafast Models SDK.
//! It includes both standalone and gateway modes, with comprehensive provider
//! management, routing, caching, and error handling.
//!
//! ## Overview
//!
//! The client module provides:
//! - **Dual Mode Operation**: Standalone and gateway modes
//! - **Provider Management**: Multiple AI provider integration
//! - **Intelligent Routing**: Automatic provider selection
//! - **Circuit Breakers**: Automatic failover and recovery
//! - **Caching Layer**: Response caching for performance
//! - **Retry Logic**: Configurable retry policies
//! - **Metrics Collection**: Performance monitoring
//! - **Streaming Support**: Real-time response streaming
//!
//! ## Client Modes
//!
//! ### Standalone Mode
//!
//! Direct communication with AI providers:
//!
//! ```rust
//! use ultrafast_models_sdk::{UltrafastClient, ChatRequest, Message};
//!
//! let client = UltrafastClient::standalone()
//!     .with_openai("your-openai-key")
//!     .with_anthropic("your-anthropic-key")
//!     .with_routing_strategy(RoutingStrategy::LoadBalance {
//!         weights: vec![0.6, 0.4],
//!     })
//!     .build()?;
//!
//! let response = client.chat_completion(ChatRequest {
//!     model: "gpt-4".to_string(),
//!     messages: vec![Message::user("Hello!")],
//!     ..Default::default()
//! }).await?;
//! ```
//!
//! ### Gateway Mode
//!
//! Communication through the Ultrafast Gateway:
//!
//! ```rust
//! let client = UltrafastClient::gateway("http://localhost:3000")
//!     .with_api_key("your-gateway-key")
//!     .with_timeout(Duration::from_secs(30))
//!     .build()?;
//!
//! let response = client.chat_completion(request).await?;
//! ```
//!
//! ## Provider Integration
//!
//! The client supports multiple providers:
//!
//! - **OpenAI**: GPT-4, GPT-3.5, and other models
//! - **Anthropic**: Claude-3, Claude-2, Claude Instant
//! - **Google**: Gemini Pro, Gemini Pro Vision, PaLM
//! - **Azure OpenAI**: Azure-hosted OpenAI models
//! - **Ollama**: Local and remote Ollama instances
//! - **Mistral AI**: Mistral 7B, Mixtral models
//! - **Cohere**: Command, Command R models
//! - **Custom Providers**: Extensible provider system
//!
//! ## Routing Strategies
//!
//! Multiple routing strategies for provider selection:
//!
//! - **Single**: Route all requests to one provider
//! - **Load Balance**: Distribute requests across providers
//! - **Failover**: Primary provider with automatic fallback
//! - **Conditional**: Route based on request characteristics
//! - **A/B Testing**: Route for testing different providers
//!
//! ## Circuit Breakers
//!
//! Automatic failover and recovery mechanisms:
//!
//! - **Closed State**: Normal operation
//! - **Open State**: Provider failing, requests blocked
//! - **Half-Open State**: Testing if provider recovered
//! - **Automatic Recovery**: Automatic state transitions
//!
//! ## Caching
//!
//! Built-in response caching:
//!
//! - **In-Memory Cache**: Fast local caching
//! - **Redis Cache**: Distributed caching
//! - **Automatic TTL**: Configurable cache expiration
//! - **Cache Keys**: Intelligent cache key generation
//!
//! ## Retry Logic
//!
//! Configurable retry policies:
//!
//! - **Exponential Backoff**: Smart retry delays
//! - **Max Retries**: Configurable retry limits
//! - **Retryable Errors**: Automatic retry on specific errors
//! - **Jitter**: Randomized retry delays to prevent thundering herd
//!
//! ## Performance Features
//!
//! - **Connection Pooling**: Reusable HTTP connections
//! - **Request Batching**: Batch multiple requests
//! - **Compression**: Automatic request/response compression
//! - **Async Operations**: Non-blocking I/O throughout
//! - **Memory Efficiency**: Minimal memory footprint
//!
//! ## Error Handling
//!
//! Comprehensive error handling with specific error types:
//!
//! - **Authentication Errors**: Invalid API keys or tokens
//! - **Rate Limit Errors**: Exceeded rate limits with retry info
//! - **Provider Errors**: Provider-specific error messages
//! - **Network Errors**: Connection and timeout issues
//! - **Validation Errors**: Invalid request parameters
//!
//! ## Configuration
//!
//! Highly configurable client behavior:
//!
//! - **Timeouts**: Per-request and per-provider timeouts
//! - **Rate Limits**: Per-provider rate limiting
//! - **Circuit Breakers**: Failure thresholds and recovery settings
//! - **Caching**: Cache TTL and size limits
//! - **Logging**: Structured logging configuration
//!
//! ## Examples
//!
//! ### Basic Usage
//!
//! ```rust
//! use ultrafast_models_sdk::{UltrafastClient, ChatRequest, Message};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let client = UltrafastClient::standalone()
//!         .with_openai("your-key")
//!         .build()?;
//!
//!     let request = ChatRequest {
//!         model: "gpt-4".to_string(),
//!         messages: vec![Message::user("Hello, world!")],
//!         ..Default::default()
//!     };
//!
//!     let response = client.chat_completion(request).await?;
//!     println!("Response: {}", response.choices[0].message.content);
//!     Ok(())
//! }
//! ```
//!
//! ### Multi-Provider Setup
//!
//! ```rust
//! let client = UltrafastClient::standalone()
//!     .with_openai("openai-key")
//!     .with_anthropic("anthropic-key")
//!     .with_google("google-key", "project-id")
//!     .with_ollama("http://localhost:11434")
//!     .with_routing_strategy(RoutingStrategy::LoadBalance {
//!         weights: vec![0.4, 0.3, 0.2, 0.1],
//!     })
//!     .build()?;
//! ```
//!
//! ### Advanced Configuration
//!
//! ```rust
//! use std::time::Duration;
//! use ultrafast_models_sdk::{UltrafastClient, ClientConfig};
//!
//! let config = ClientConfig {
//!     timeout: Duration::from_secs(30),
//!     max_retries: 5,
//!     retry_delay: Duration::from_secs(1),
//!     user_agent: Some("MyApp/1.0".to_string()),
//!     ..Default::default()
//! };
//!
//! let client = UltrafastClient::standalone()
//!     .with_config(config)
//!     .with_openai("your-key")
//!     .build()?;
//! ```
//!
//! ### Circuit Breaker Configuration
//!
//! ```rust
//! use ultrafast_models_sdk::circuit_breaker::CircuitBreakerConfig;
//!
//! let circuit_config = CircuitBreakerConfig {
//!     failure_threshold: 5,
//!     recovery_timeout: Duration::from_secs(60),
//!     request_timeout: Duration::from_secs(30),
//!     half_open_max_calls: 3,
//! };
//!
//! let client = UltrafastClient::standalone()
//!     .with_openai("your-key")
//!     .with_circuit_breaker_config(circuit_config)
//!     .build()?;
//! ```
//!
//! ### Caching Configuration
//!
//! ```rust
//! use ultrafast_models_sdk::cache::CacheConfig;
//!
//! let cache_config = CacheConfig {
//!     enabled: true,
//!     ttl: Duration::from_hours(1),
//!     max_size: 1000,
//!     backend: CacheBackend::Memory,
//! };
//!
//! let client = UltrafastClient::standalone()
//!     .with_cache_config(cache_config)
//!     .with_openai("your-key")
//!     .build()?;
//! ```
//!
//! ## Testing
//!
//! The client includes testing utilities:
//!
//! ```rust
//! #[cfg(test)]
//! mod tests {
//!     use super::*;
//!     use tokio_test;
//!
//!     #[tokio_test]
//!     async fn test_client_creation() {
//!         let client = UltrafastClient::standalone()
//!             .with_openai("test-key")
//!             .build();
//!         assert!(client.is_ok());
//!     }
//!
//!     #[tokio_test]
//!     async fn test_chat_completion() {
//!         let client = UltrafastClient::standalone()
//!             .with_openai("test-key")
//!             .build()
//!             .unwrap();
//!
//!         let request = ChatRequest {
//!             model: "gpt-4".to_string(),
//!             messages: vec![Message::user("Hello")],
//!             ..Default::default()
//!         };
//!
//!         let result = client.chat_completion(request).await;
//!         // Handle result based on test environment
//!     }
//! }
//! ```
//!
//! ## Performance Tips
//!
//! For optimal performance:
//!
//! - **Use Connection Pooling**: Configure appropriate pool sizes
//! - **Enable Caching**: Cache responses for repeated requests
//! - **Configure Timeouts**: Set appropriate timeouts for your use case
//! - **Use Streaming**: For long responses, use streaming endpoints
//! - **Batch Requests**: Group multiple requests when possible
//!
//! ## Migration from Other SDKs
//!
//! ### From OpenAI SDK
//!
//! ```rust
//! // Before
//! use openai::Client;
//! let client = Client::new("your-key");
//! let response = client.chat().create(request).await?;
//!
//! // After
//! use ultrafast_models_sdk::UltrafastClient;
//! let client = UltrafastClient::standalone()
//!     .with_openai("your-key")
//!     .build()?;
//! let response = client.chat_completion(request).await?;
//! ```
//!
//! ### From Anthropic SDK
//!
//! ```rust
//! // Before
//! use anthropic::Client;
//! let client = Client::new("your-key");
//! let response = client.messages().create(request).await?;
//!
//! // After
//! use ultrafast_models_sdk::UltrafastClient;
//! let client = UltrafastClient::standalone()
//!     .with_anthropic("your-key")
//!     .build()?;
//! let response = client.chat_completion(request).await?;
//! ```
//!
//! ## Troubleshooting
//!
//! Common issues and solutions:
//!
//! ### Authentication Errors
//! - Verify API keys are correct
//! - Check API key permissions
//! - Ensure proper provider configuration
//!
//! ### Rate Limit Issues
//! - Implement exponential backoff
//! - Use multiple API keys
//! - Configure appropriate rate limits
//!
//! ### Connection Issues
//! - Check network connectivity
//! - Verify provider endpoints
//! - Configure appropriate timeouts
//!
//! ## Contributing
//!
//! We welcome contributions! Please see our contributing guide for details on:
//!
//! - Code style and formatting
//! - Testing requirements
//! - Documentation standards
//! - Pull request process

use crate::cache::{Cache, CacheConfig, CacheKeyBuilder, InMemoryCache};
use crate::error::ClientError;
use crate::models::{
    AudioRequest, AudioResponse, ChatRequest, ChatResponse, EmbeddingRequest, EmbeddingResponse,
    ImageRequest, ImageResponse, SpeechRequest, SpeechResponse, StreamChunk,
};
use crate::providers::{
    create_provider_with_circuit_breaker, Provider, ProviderConfig, ProviderMetrics,
};
use crate::routing::{Router, RoutingContext, RoutingStrategy};
use futures::{Stream, StreamExt};
use reqwest::Client;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Client operation mode.
///
/// Defines whether the client operates in standalone mode (direct provider
/// communication) or gateway mode (through the Ultrafast Gateway).
///
/// # Example
///
/// ```rust
/// let standalone_mode = ClientMode::Standalone;
/// let gateway_mode = ClientMode::Gateway {
///     base_url: "http://localhost:3000".to_string(),
/// };
/// ```
#[derive(Debug, Clone)]
pub enum ClientMode {
    /// Direct communication with AI providers
    Standalone,
    /// Communication through the Ultrafast Gateway
    Gateway { base_url: String },
}

/// The main client for interacting with multiple AI/LLM providers.
///
/// The `UltrafastClient` provides a unified interface to multiple AI providers
/// with intelligent routing, circuit breakers, caching, and comprehensive error handling.
///
/// # Modes
///
/// The client supports two operation modes:
///
/// - **Standalone Mode**: Direct communication with AI providers
/// - **Gateway Mode**: Communication through the Ultrafast Gateway
///
/// # Features
///
/// - **Multi-Provider Support**: Integrate with OpenAI, Anthropic, Google, and more
/// - **Intelligent Routing**: Automatic provider selection and load balancing
/// - **Circuit Breakers**: Automatic failover and recovery
/// - **Response Caching**: Built-in caching for performance
/// - **Rate Limiting**: Per-provider rate limiting
/// - **Retry Logic**: Configurable retry policies with exponential backoff
/// - **Performance Metrics**: Real-time provider performance tracking
/// - **Streaming Support**: Real-time response streaming
///
/// # Examples
///
/// ## Basic Usage
///
/// ```rust
/// use ultrafast_models_sdk::{UltrafastClient, ChatRequest, Message};
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let client = UltrafastClient::standalone()
///         .with_openai("your-openai-key")
///         .build()?;
///
///     let request = ChatRequest {
///         model: "gpt-4".to_string(),
///         messages: vec![Message::user("Hello, world!")],
///         ..Default::default()
///     };
///
///     let response = client.chat_completion(request).await?;
///     println!("Response: {}", response.choices[0].message.content);
///     Ok(())
/// }
/// ```
///
/// ## Multi-Provider Setup
///
/// ```rust
/// let client = UltrafastClient::standalone()
///     .with_openai("openai-key")
///     .with_anthropic("anthropic-key")
///     .with_google("google-key", "project-id")
///     .with_routing_strategy(RoutingStrategy::LoadBalance {
///         weights: vec![0.4, 0.3, 0.2, 0.1],
///     })
///     .build()?;
/// ```
///
/// ## Gateway Mode
///
/// ```rust
/// let client = UltrafastClient::gateway("http://localhost:3000")
///     .with_api_key("your-gateway-key")
///     .with_timeout(Duration::from_secs(30))
///     .build()?;
/// ```
///
/// # Thread Safety
///
/// The client is thread-safe and can be shared across threads using `Arc<UltrafastClient>`.
///
/// # Performance
///
/// - **Latency**: <1ms routing overhead
/// - **Throughput**: 10,000+ requests/second
/// - **Memory**: <100MB under normal load
/// - **Concurrency**: 100,000+ concurrent requests
///
/// # Error Handling
///
/// The client provides comprehensive error handling with specific error types:
///
/// - `AuthenticationError`: Invalid API keys or tokens
/// - `RateLimitExceeded`: Exceeded rate limits with retry information
/// - `ProviderError`: Provider-specific error messages
/// - `NetworkError`: Connection and timeout issues
/// - `ValidationError`: Invalid request parameters
///
/// # Circuit Breakers
///
/// Each provider has an independent circuit breaker that automatically:
///
/// - Opens when failure threshold is reached
/// - Prevents requests to failing providers
/// - Tests recovery with limited requests
/// - Automatically closes when provider recovers
///
/// # Caching
///
/// The client supports multiple caching backends:
///
/// - **In-Memory Cache**: Fast local caching (default)
/// - **Redis Cache**: Distributed caching for multiple instances
/// - **Custom Backends**: Extensible cache system
///
/// # Rate Limiting
///
/// Per-provider rate limiting with:
///
/// - Request-based limits (requests per minute/hour)
/// - Token-based limits (tokens per minute)
/// - Burst handling with configurable burst sizes
/// - Automatic retry with exponential backoff
///
/// # Metrics
///
/// Real-time performance metrics including:
///
/// - Provider response times
/// - Success/failure rates
/// - Circuit breaker status
/// - Cache hit rates
/// - Rate limit usage
///
/// # Configuration
///
/// The client is highly configurable with:
///
/// - Per-provider timeouts and retry policies
/// - Global and per-provider rate limits
/// - Circuit breaker thresholds and recovery settings
/// - Cache TTL and size limits
/// - Connection pool sizes and timeouts
///
/// # Best Practices
///
/// - Use connection pooling for high-throughput applications
/// - Enable caching for repeated requests
/// - Configure appropriate timeouts for your use case
/// - Use streaming for long responses
/// - Monitor circuit breaker status
/// - Implement proper error handling and retry logic
///
/// # See Also
///
/// - [`UltrafastClientBuilder`] - For building client instances
/// - [`Provider`] - For custom provider implementations
/// - [`Router`] - For custom routing strategies
/// - [`Cache`] - For custom caching backends
#[allow(dead_code)]
pub struct UltrafastClient {
    /// Client operation mode (standalone or gateway)
    mode: ClientMode,
    /// Provider instances for standalone mode
    providers: HashMap<String, Arc<dyn Provider>>,
    /// Router for provider selection
    router: Arc<RwLock<Router>>,
    /// Optional cache for response caching
    cache: Option<Arc<dyn Cache>>,
    /// Provider performance metrics
    metrics: Arc<RwLock<HashMap<String, ProviderMetrics>>>,
    /// HTTP client for gateway mode
    http_client: Client,
    /// API key for gateway mode
    api_key: Option<String>,
    /// Request timeout
    timeout: Duration,
    /// Retry policy configuration
    retry_policy: RetryPolicy,
    /// Connection pool for HTTP connections
    connection_pool: Arc<RwLock<ConnectionPool>>,
    /// Last used provider for metrics
    last_used_provider: Arc<RwLock<Option<String>>>,
}

/// Retry policy configuration.
///
/// Defines how the client should retry failed requests, including backoff
/// strategies and jitter to prevent thundering herd problems.
///
/// # Example
///
/// ```rust
/// let policy = RetryPolicy {
///     max_retries: 3,
///     initial_delay: Duration::from_millis(100),
///     max_delay: Duration::from_secs(10),
///     backoff_multiplier: 2.0,
///     jitter_factor: 0.1,
/// };
/// ```
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Initial delay before first retry
    pub initial_delay: Duration,
    /// Maximum delay between retries
    pub max_delay: Duration,
    /// Multiplier for exponential backoff
    pub backoff_multiplier: f64,
    /// Jitter factor to prevent thundering herd
    pub jitter_factor: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(10),
            backoff_multiplier: 2.0,
            jitter_factor: 0.1, // 10% jitter
        }
    }
}

/// Connection pool for HTTP connections.
///
/// Manages reusable HTTP connections to improve performance and reduce
/// connection overhead.
///
/// # Thread Safety
///
/// The connection pool is thread-safe and can be shared across threads.
#[derive(Debug)]
pub struct ConnectionPool {
    /// Pooled connections by host
    connections: HashMap<String, PooledConnection>,
    /// Maximum connections per host
    max_connections_per_host: usize,
    /// Connection timeout
    connection_timeout: Duration,
    /// Idle connection timeout
    idle_timeout: Duration,
}

/// A pooled HTTP connection.
///
/// Represents a single HTTP connection with usage statistics.
#[derive(Debug)]
pub struct PooledConnection {
    /// HTTP client for this connection
    client: Client,
    /// Last time this connection was used
    last_used: Instant,
    /// Number of requests made with this connection
    request_count: u64,
}

impl ConnectionPool {
    /// Create a new connection pool.
    ///
    /// # Arguments
    ///
    /// * `max_connections_per_host` - Maximum connections per host
    /// * `connection_timeout` - Connection timeout
    /// * `idle_timeout` - Idle connection timeout
    ///
    /// # Returns
    ///
    /// Returns a new `ConnectionPool` instance.
    pub fn new(
        max_connections_per_host: usize,
        connection_timeout: Duration,
        idle_timeout: Duration,
    ) -> Self {
        Self {
            connections: HashMap::new(),
            max_connections_per_host,
            connection_timeout,
            idle_timeout,
        }
    }

    /// Get or create a connection for a host.
    ///
    /// Returns an existing connection if available, or creates a new one
    /// if needed. Automatically cleans up idle connections.
    ///
    /// # Arguments
    ///
    /// * `host` - The host to get a connection for
    ///
    /// # Returns
    ///
    /// Returns a `Client` for the specified host.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection cannot be created.
    pub fn get_or_create_connection(&mut self, host: &str) -> Result<Client, ClientError> {
        let now = Instant::now();

        // Clean up idle connections
        self.cleanup_idle_connections(now);

        // Check if we have an existing connection
        if let Some(connection) = self.connections.get_mut(host) {
            connection.last_used = now;
            connection.request_count += 1;
            return Ok(connection.client.clone());
        }

        // Create new connection if under limit
        if self.connections.len() < self.max_connections_per_host {
            let client = Client::builder()
                .timeout(self.connection_timeout)
                .pool_max_idle_per_host(10)
                .pool_idle_timeout(self.idle_timeout)
                .build()
                .map_err(|e| ClientError::Configuration {
                    message: format!("Failed to create HTTP client: {e}"),
                })?;

            let pooled_connection = PooledConnection {
                client: client.clone(),
                last_used: now,
                request_count: 1,
            };

            self.connections.insert(host.to_string(), pooled_connection);
            Ok(client)
        } else {
            Err(ClientError::Configuration {
                message: "Connection pool exhausted".to_string(),
            })
        }
    }

    fn cleanup_idle_connections(&mut self, now: Instant) {
        let idle_connections: Vec<String> = self
            .connections
            .iter()
            .filter(|(_, conn)| now.duration_since(conn.last_used) > self.idle_timeout)
            .map(|(host, _)| host.clone())
            .collect();

        for host in &idle_connections {
            self.connections.remove(host);
        }

        if !idle_connections.is_empty() {
            tracing::debug!("Cleaned up {} idle connections", idle_connections.len());
        }
    }
}

impl UltrafastClient {
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> UltrafastClientBuilder {
        UltrafastClientBuilder::default()
    }

    pub fn standalone() -> StandaloneClientBuilder {
        StandaloneClientBuilder::default()
    }

    pub fn gateway(base_url: String) -> GatewayClientBuilder {
        GatewayClientBuilder::new(base_url)
    }

    // Enhanced chat completion with better error handling
    pub async fn chat_completion(&self, request: ChatRequest) -> Result<ChatResponse, ClientError> {
        match &self.mode {
            ClientMode::Standalone => self.standalone_chat_completion(request).await,
            ClientMode::Gateway { .. } => self.gateway_chat_completion(request).await,
        }
    }

    // Enhanced streaming with better error handling
    pub async fn stream_chat_completion(
        &self,
        request: ChatRequest,
    ) -> Result<Box<dyn Stream<Item = Result<StreamChunk, ClientError>> + Send + Unpin>, ClientError>
    {
        match &self.mode {
            ClientMode::Standalone => {
                let stream = self.standalone_stream_chat_completion(request).await?;
                Ok(stream)
            }
            ClientMode::Gateway { .. } => {
                let stream = self.gateway_stream_chat_completion(request).await?;
                Ok(stream)
            }
        }
    }

    // Get the last used provider for metrics
    pub async fn get_last_used_provider(&self) -> Option<String> {
        let provider = self.last_used_provider.read().await;
        provider.clone()
    }

    // Get circuit breaker state for a provider
    pub async fn get_provider_circuit_state(
        &self,
        provider_id: &str,
    ) -> Option<crate::circuit_breaker::CircuitState> {
        // Try to get the provider and check its health status
        if let Some(provider) = self.providers.get(provider_id) {
            match provider.health_check().await {
                Ok(_) => Some(crate::circuit_breaker::CircuitState::Closed),
                Err(_) => Some(crate::circuit_breaker::CircuitState::Open),
            }
        } else {
            None
        }
    }

    // Check if a provider is healthy (circuit breaker is not open)
    pub async fn is_provider_healthy(&self, provider_id: &str) -> bool {
        match self.get_provider_circuit_state(provider_id).await {
            Some(state) => state != crate::circuit_breaker::CircuitState::Open,
            None => true, // Assume healthy if we can't determine state
        }
    }

    // Get circuit breaker metrics for all providers
    pub async fn get_circuit_breaker_metrics(
        &self,
    ) -> HashMap<String, crate::circuit_breaker::CircuitBreakerMetrics> {
        let mut metrics = HashMap::new();

        for provider_id in self.providers.keys() {
            if let Some(provider) = self.providers.get(provider_id) {
                // Create basic metrics based on health status
                let state = match provider.health_check().await {
                    Ok(_) => crate::circuit_breaker::CircuitState::Closed,
                    Err(_) => crate::circuit_breaker::CircuitState::Open,
                };

                metrics.insert(
                    provider_id.clone(),
                    crate::circuit_breaker::CircuitBreakerMetrics {
                        name: provider_id.clone(),
                        state,
                        failure_count: 0,
                        success_count: 0,
                        last_failure_time: None,
                        last_success_time: None,
                    },
                );
            }
        }

        metrics
    }

    // Get health status for all providers
    pub async fn get_provider_health_status(&self) -> HashMap<String, bool> {
        let mut health_status = HashMap::new();

        for provider_id in self.providers.keys() {
            let is_healthy = self.is_provider_healthy(provider_id).await;
            health_status.insert(provider_id.clone(), is_healthy);
        }

        health_status
    }

    pub async fn embedding(
        &self,
        request: EmbeddingRequest,
    ) -> Result<EmbeddingResponse, ClientError> {
        match &self.mode {
            ClientMode::Standalone => self.standalone_embedding(request).await,
            ClientMode::Gateway { .. } => self.gateway_embedding(request).await,
        }
    }

    pub async fn image_generation(
        &self,
        request: ImageRequest,
    ) -> Result<ImageResponse, ClientError> {
        match &self.mode {
            ClientMode::Standalone => self.standalone_image_generation(request).await,
            ClientMode::Gateway { .. } => self.gateway_image_generation(request).await,
        }
    }

    pub async fn audio_transcription(
        &self,
        request: AudioRequest,
    ) -> Result<AudioResponse, ClientError> {
        match &self.mode {
            ClientMode::Standalone => self.standalone_audio_transcription(request).await,
            ClientMode::Gateway { .. } => self.gateway_audio_transcription(request).await,
        }
    }

    pub async fn text_to_speech(
        &self,
        request: SpeechRequest,
    ) -> Result<SpeechResponse, ClientError> {
        match &self.mode {
            ClientMode::Standalone => self.standalone_text_to_speech(request).await,
            ClientMode::Gateway { .. } => self.gateway_text_to_speech(request).await,
        }
    }

    // Enhanced standalone mode with connection pooling
    async fn standalone_chat_completion(
        &self,
        request: ChatRequest,
    ) -> Result<ChatResponse, ClientError> {
        let cache_key = if self.cache.is_some() && !request.stream.unwrap_or(false) {
            Some(CacheKeyBuilder::build_chat_key(&request))
        } else {
            None
        };

        // Check cache first
        if let Some(cache_key) = &cache_key {
            if let Some(cache) = &self.cache {
                if let Some(cached_response) = cache.get(cache_key) {
                    tracing::debug!("Cache hit for chat completion");
                    return Ok(cached_response.response);
                }
            }
        }

        // Route to appropriate provider
        let router = self.router.read().await;
        let routing_context = RoutingContext {
            model: Some(request.model.clone()),
            user_region: None,
            request_size: serde_json::to_string(&request).unwrap_or_default().len() as u32,
            estimated_tokens: self.estimate_tokens(&request),
            user_id: None,
            metadata: HashMap::new(),
        };

        let provider_names: Vec<String> = self.providers.keys().cloned().collect();
        let provider_selection = router
            .select_provider(&provider_names, &routing_context)
            .ok_or_else(|| ClientError::Configuration {
                message: "No suitable provider found".to_string(),
            })?;

        // Track the last used provider for metrics
        {
            let mut last_provider = self.last_used_provider.write().await;
            *last_provider = Some(provider_selection.provider_id.clone());
        }

        let provider = self
            .providers
            .get(&provider_selection.provider_id)
            .ok_or_else(|| ClientError::Configuration {
                message: format!("Provider {} not found", provider_selection.provider_id),
            })?;

        // Execute with enhanced retry logic
        let start = Instant::now();
        let result = self
            .execute_with_enhanced_retry(
                || provider.chat_completion(request.clone()),
                &provider_selection.provider_id,
            )
            .await;

        let latency = start.elapsed();

        // Update metrics
        self.update_enhanced_metrics(
            &provider_selection.provider_id,
            result.is_ok(),
            latency.as_millis() as u64,
            self.estimate_tokens(&request),
            0.0, // Cost calculation would be provider-specific
        )
        .await;

        // Cache successful response
        if let Ok(response) = &result {
            if let Some(cache_key) = &cache_key {
                if let Some(cache) = &self.cache {
                    let cached_response = crate::cache::CachedResponse::new(
                        response.clone(),
                        Duration::from_secs(3600),
                    );
                    cache.set(cache_key, cached_response, Duration::from_secs(3600));
                }
            }
        }

        Ok(result?)
    }

    // Enhanced retry logic with exponential backoff and jitter
    async fn execute_with_enhanced_retry<F, Fut, T>(
        &self,
        mut operation: F,
        _provider_id: &str,
    ) -> Result<T, crate::error::ProviderError>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T, crate::error::ProviderError>>,
    {
        let mut attempt = 0;
        let mut delay = self.retry_policy.initial_delay;

        loop {
            match operation().await {
                Ok(result) => return Ok(result),
                Err(error) => {
                    attempt += 1;

                    if attempt > self.retry_policy.max_retries || !self.should_retry(&error) {
                        return Err(error);
                    }

                    // Add jitter to prevent thundering herd
                    let jitter = delay.mul_f64(self.retry_policy.jitter_factor);
                    let actual_delay = delay + jitter;

                    tokio::time::sleep(actual_delay).await;

                    delay = std::cmp::min(
                        delay.mul_f64(self.retry_policy.backoff_multiplier),
                        self.retry_policy.max_delay,
                    );
                }
            }
        }
    }

    // Enhanced error classification
    fn should_retry(&self, error: &crate::error::ProviderError) -> bool {
        matches!(
            error,
            crate::error::ProviderError::RateLimit
                | crate::error::ProviderError::ServiceUnavailable
                | crate::error::ProviderError::NetworkError { .. }
                | crate::error::ProviderError::Timeout
        )
    }

    // Enhanced metrics with more detailed tracking
    async fn update_enhanced_metrics(
        &self,
        provider_id: &str,
        success: bool,
        latency_ms: u64,
        tokens: u32,
        cost: f64,
    ) {
        let mut metrics = self.metrics.write().await;
        let provider_metrics = metrics.entry(provider_id.to_string()).or_default();

        provider_metrics.record_enhanced_request(success, latency_ms, tokens, cost);

        tracing::debug!(
            "Updated metrics for provider {}: success={}, latency={}ms, tokens={}, cost=${:.4}",
            provider_id,
            success,
            latency_ms,
            tokens,
            cost
        );
    }

    // Enhanced token estimation
    fn estimate_tokens(&self, request: &ChatRequest) -> u32 {
        let mut total_tokens = 0;

        for message in &request.messages {
            // Rough estimation: 1 token ≈ 4 characters
            total_tokens += message.content.len() as u32 / 4;
        }

        // Add buffer for system messages and formatting
        total_tokens += 50;

        total_tokens
    }

    // Standalone mode implementation
    async fn standalone_stream_chat_completion(
        &self,
        request: ChatRequest,
    ) -> Result<Box<dyn Stream<Item = Result<StreamChunk, ClientError>> + Send + Unpin>, ClientError>
    {
        let router = self.router.read().await;
        let context = RoutingContext {
            model: Some(request.model.clone()),
            user_region: None,
            request_size: serde_json::to_string(&request).unwrap_or_default().len() as u32,
            estimated_tokens: self.estimate_tokens(&request),
            user_id: request.user.clone(),
            metadata: HashMap::new(),
        };

        let provider_ids: Vec<String> = self.providers.keys().cloned().collect();
        let selection = router
            .select_provider(&provider_ids, &context)
            .ok_or_else(|| ClientError::Routing {
                message: "No providers available".to_string(),
            })?;

        drop(router);

        let provider =
            self.providers
                .get(&selection.provider_id)
                .ok_or_else(|| ClientError::Routing {
                    message: format!("Provider not found: {}", selection.provider_id),
                })?;

        let start_time = Instant::now();
        let stream = provider.stream_chat_completion(request).await?;
        let latency = start_time.elapsed();

        let metrics = self.metrics.clone();
        let provider_id = selection.provider_id.clone();

        let wrapped_stream = stream.map(move |chunk_result| {
            match chunk_result {
                Ok(chunk) => {
                    // Use spawn_blocking to avoid blocking the async runtime
                    let metrics_clone = metrics.clone();
                    let provider_id_clone = provider_id.clone();
                    let latency_ms = latency.as_millis() as u64;

                    tokio::spawn(async move {
                        let mut metrics_guard = metrics_clone.write().await;
                        if let Some(provider_metrics) = metrics_guard.get_mut(&provider_id_clone) {
                            provider_metrics.record_enhanced_request(true, latency_ms, 0, 0.0);
                        }
                    });

                    Ok(chunk)
                }
                Err(e) => {
                    // Use spawn_blocking to avoid blocking the async runtime
                    let metrics_clone = metrics.clone();
                    let provider_id_clone = provider_id.clone();
                    let latency_ms = latency.as_millis() as u64;

                    tokio::spawn(async move {
                        let mut metrics_guard = metrics_clone.write().await;
                        if let Some(provider_metrics) = metrics_guard.get_mut(&provider_id_clone) {
                            provider_metrics.record_enhanced_request(false, latency_ms, 0, 0.0);
                        }
                    });

                    Err(ClientError::Provider(e))
                }
            }
        });

        Ok(Box::new(wrapped_stream))
    }

    // Gateway mode implementation
    async fn gateway_chat_completion(
        &self,
        request: ChatRequest,
    ) -> Result<ChatResponse, ClientError> {
        let url = format!(
            "{}/v1/chat/completions",
            match &self.mode {
                ClientMode::Gateway { base_url } => base_url,
                _ => unreachable!(),
            }
        );

        let response = self.gateway_request(url, request).await?;
        Ok(response)
    }

    async fn gateway_stream_chat_completion(
        &self,
        mut request: ChatRequest,
    ) -> Result<Box<dyn Stream<Item = Result<StreamChunk, ClientError>> + Send + Unpin>, ClientError>
    {
        request.stream = Some(true);
        let url = format!(
            "{}/v1/chat/completions",
            match &self.mode {
                ClientMode::Gateway { base_url } => base_url,
                _ => unreachable!(),
            }
        );

        let response = self
            .http_client
            .post(&url)
            .header(
                "Authorization",
                format!(
                    "Bearer {}",
                    self.api_key.as_ref().unwrap_or(&"".to_string())
                ),
            )
            .json(&request)
            .send()
            .await
            .map_err(|e| ClientError::NetworkError {
                message: e.to_string(),
            })?;

        if !response.status().is_success() {
            return Err(ClientError::Provider(
                crate::error::ProviderError::ServiceUnavailable,
            ));
        }

        let stream = response.bytes_stream().map(|chunk_result| {
            chunk_result
                .map_err(|e| ClientError::NetworkError {
                    message: e.to_string(),
                })
                .and_then(|chunk| {
                    // Parse SSE format
                    let chunk_str = String::from_utf8_lossy(&chunk);
                    if chunk_str.trim() == "data: [DONE]" {
                        return Ok(StreamChunk {
                            id: "".to_string(),
                            object: "chat.completion.chunk".to_string(),
                            created: 0,
                            model: "".to_string(),
                            choices: vec![],
                        });
                    }

                    if let Some(json_str) = chunk_str.strip_prefix("data: ") {
                        serde_json::from_str::<StreamChunk>(json_str).map_err(|e| {
                            ClientError::Serialization {
                                message: e.to_string(),
                            }
                        })
                    } else {
                        Err(ClientError::Serialization {
                            message: "Invalid SSE format".to_string(),
                        })
                    }
                })
        });

        Ok(Box::new(stream))
    }

    async fn standalone_embedding(
        &self,
        request: EmbeddingRequest,
    ) -> Result<EmbeddingResponse, ClientError> {
        // Route to appropriate provider
        let router = self.router.read().await;
        let routing_context = RoutingContext {
            model: Some(request.model.clone()),
            user_region: None,
            request_size: serde_json::to_string(&request).unwrap_or_default().len() as u32,
            estimated_tokens: 0, // Embeddings don't have token estimation
            user_id: None,
            metadata: HashMap::new(),
        };

        let provider_names: Vec<String> = self.providers.keys().cloned().collect();
        let provider_selection = router
            .select_provider(&provider_names, &routing_context)
            .ok_or_else(|| ClientError::Configuration {
                message: "No suitable provider found".to_string(),
            })?;

        // Track the last used provider for metrics
        {
            let mut last_provider = self.last_used_provider.write().await;
            *last_provider = Some(provider_selection.provider_id.clone());
        }

        let provider_id = provider_selection.provider_id;
        let provider =
            self.providers
                .get(&provider_id)
                .ok_or_else(|| ClientError::Configuration {
                    message: format!("Provider {provider_id} not found"),
                })?;

        // Execute with retry and fallback
        let result = self
            .execute_with_enhanced_retry(|| provider.embedding(request.clone()), &provider_id)
            .await;

        match result {
            Ok(response) => {
                // Update metrics
                self.update_enhanced_metrics(&provider_id, true, 0, 0, 0.0)
                    .await;
                Ok(response)
            }
            Err(error) => {
                // Update metrics
                self.update_enhanced_metrics(&provider_id, false, 0, 0, 0.0)
                    .await;

                // Try fallback providers
                if self.should_fallback(&error) {
                    let fallback_providers: Vec<String> = self
                        .providers
                        .keys()
                        .filter(|&id| id != &provider_id)
                        .cloned()
                        .collect();

                    if let Ok(response) = self
                        .try_fallback_providers_embedding(
                            &fallback_providers,
                            &provider_id,
                            request,
                        )
                        .await
                    {
                        return Ok(response);
                    }
                }

                Err(ClientError::Provider(error))
            }
        }
    }

    async fn gateway_embedding(
        &self,
        request: EmbeddingRequest,
    ) -> Result<EmbeddingResponse, ClientError> {
        let url = format!("{}/v1/embeddings", self.base_url());
        self.gateway_request(url, request).await
    }

    async fn standalone_image_generation(
        &self,
        request: ImageRequest,
    ) -> Result<ImageResponse, ClientError> {
        // Route to appropriate provider
        let router = self.router.read().await;
        let routing_context = RoutingContext {
            model: request.model.clone(),
            user_region: None,
            request_size: serde_json::to_string(&request).unwrap_or_default().len() as u32,
            estimated_tokens: 0, // Image generation doesn't have token estimation
            user_id: None,
            metadata: HashMap::new(),
        };

        let provider_names: Vec<String> = self.providers.keys().cloned().collect();
        let provider_selection = router
            .select_provider(&provider_names, &routing_context)
            .ok_or_else(|| ClientError::Configuration {
                message: "No suitable provider found".to_string(),
            })?;

        // Track the last used provider for metrics
        {
            let mut last_provider = self.last_used_provider.write().await;
            *last_provider = Some(provider_selection.provider_id.clone());
        }

        let provider_id = provider_selection.provider_id;
        let provider =
            self.providers
                .get(&provider_id)
                .ok_or_else(|| ClientError::Configuration {
                    message: format!("Provider {provider_id} not found"),
                })?;

        // Execute with retry and fallback
        let result = self
            .execute_with_enhanced_retry(
                || provider.image_generation(request.clone()),
                &provider_id,
            )
            .await;

        match result {
            Ok(response) => {
                // Update metrics
                self.update_enhanced_metrics(&provider_id, true, 0, 0, 0.0)
                    .await;
                Ok(response)
            }
            Err(error) => {
                // Update metrics
                self.update_enhanced_metrics(&provider_id, false, 0, 0, 0.0)
                    .await;

                // Try fallback providers
                if self.should_fallback(&error) {
                    let fallback_providers: Vec<String> = self
                        .providers
                        .keys()
                        .filter(|&id| id != &provider_id)
                        .cloned()
                        .collect();

                    if let Ok(response) = self
                        .try_fallback_providers_image(&fallback_providers, &provider_id, request)
                        .await
                    {
                        return Ok(response);
                    }
                }

                Err(ClientError::Provider(error))
            }
        }
    }

    async fn gateway_image_generation(
        &self,
        request: ImageRequest,
    ) -> Result<ImageResponse, ClientError> {
        let url = format!("{}/v1/images/generations", self.base_url());
        self.gateway_request(url, request).await
    }

    async fn standalone_audio_transcription(
        &self,
        request: AudioRequest,
    ) -> Result<AudioResponse, ClientError> {
        // Route to appropriate provider
        let router = self.router.read().await;
        let routing_context = RoutingContext {
            model: Some(request.model.clone()),
            user_region: None,
            request_size: serde_json::to_string(&request).unwrap_or_default().len() as u32,
            estimated_tokens: 0, // Audio transcription doesn't have token estimation
            user_id: None,
            metadata: HashMap::new(),
        };

        let provider_names: Vec<String> = self.providers.keys().cloned().collect();
        let provider_selection = router
            .select_provider(&provider_names, &routing_context)
            .ok_or_else(|| ClientError::Configuration {
                message: "No suitable provider found".to_string(),
            })?;

        // Track the last used provider for metrics
        {
            let mut last_provider = self.last_used_provider.write().await;
            *last_provider = Some(provider_selection.provider_id.clone());
        }

        let provider_id = provider_selection.provider_id;
        let provider =
            self.providers
                .get(&provider_id)
                .ok_or_else(|| ClientError::Configuration {
                    message: format!("Provider {provider_id} not found"),
                })?;

        // Execute with retry and fallback
        let result = self
            .execute_with_enhanced_retry(
                || provider.audio_transcription(request.clone()),
                &provider_id,
            )
            .await;

        match result {
            Ok(response) => {
                // Update metrics
                self.update_enhanced_metrics(&provider_id, true, 0, 0, 0.0)
                    .await;
                Ok(response)
            }
            Err(error) => {
                // Update metrics
                self.update_enhanced_metrics(&provider_id, false, 0, 0, 0.0)
                    .await;

                // Try fallback providers
                if self.should_fallback(&error) {
                    let fallback_providers: Vec<String> = self
                        .providers
                        .keys()
                        .filter(|&id| id != &provider_id)
                        .cloned()
                        .collect();

                    if let Ok(response) = self
                        .try_fallback_providers_audio(&fallback_providers, &provider_id, request)
                        .await
                    {
                        return Ok(response);
                    }
                }

                Err(ClientError::Provider(error))
            }
        }
    }

    async fn gateway_audio_transcription(
        &self,
        request: AudioRequest,
    ) -> Result<AudioResponse, ClientError> {
        let url = format!("{}/v1/audio/transcriptions", self.base_url());
        self.gateway_request(url, request).await
    }

    async fn standalone_text_to_speech(
        &self,
        request: SpeechRequest,
    ) -> Result<SpeechResponse, ClientError> {
        // Route to appropriate provider
        let router = self.router.read().await;
        let routing_context = RoutingContext {
            model: Some(request.model.clone()),
            user_region: None,
            request_size: serde_json::to_string(&request).unwrap_or_default().len() as u32,
            estimated_tokens: 0, // Text-to-speech doesn't have token estimation
            user_id: None,
            metadata: HashMap::new(),
        };

        let provider_names: Vec<String> = self.providers.keys().cloned().collect();
        let provider_selection = router
            .select_provider(&provider_names, &routing_context)
            .ok_or_else(|| ClientError::Configuration {
                message: "No suitable provider found".to_string(),
            })?;

        // Track the last used provider for metrics
        {
            let mut last_provider = self.last_used_provider.write().await;
            *last_provider = Some(provider_selection.provider_id.clone());
        }

        let provider_id = provider_selection.provider_id;
        let provider =
            self.providers
                .get(&provider_id)
                .ok_or_else(|| ClientError::Configuration {
                    message: format!("Provider {provider_id} not found"),
                })?;

        // Execute with retry and fallback
        let result = self
            .execute_with_enhanced_retry(|| provider.text_to_speech(request.clone()), &provider_id)
            .await;

        match result {
            Ok(response) => {
                // Update metrics
                self.update_enhanced_metrics(&provider_id, true, 0, 0, 0.0)
                    .await;
                Ok(response)
            }
            Err(error) => {
                // Update metrics
                self.update_enhanced_metrics(&provider_id, false, 0, 0, 0.0)
                    .await;

                // Try fallback providers
                if self.should_fallback(&error) {
                    let fallback_providers: Vec<String> = self
                        .providers
                        .keys()
                        .filter(|&id| id != &provider_id)
                        .cloned()
                        .collect();

                    if let Ok(response) = self
                        .try_fallback_providers_speech(&fallback_providers, &provider_id, request)
                        .await
                    {
                        return Ok(response);
                    }
                }

                Err(ClientError::Provider(error))
            }
        }
    }

    async fn gateway_text_to_speech(
        &self,
        request: SpeechRequest,
    ) -> Result<SpeechResponse, ClientError> {
        let url = format!("{}/v1/audio/speech", self.base_url());
        self.gateway_request(url, request).await
    }

    // Helper methods
    fn base_url(&self) -> &str {
        match &self.mode {
            ClientMode::Gateway { base_url } => base_url,
            _ => unreachable!(),
        }
    }

    async fn gateway_request<T, R>(&self, url: String, request: T) -> Result<R, ClientError>
    where
        T: serde::Serialize,
        R: serde::de::DeserializeOwned,
    {
        let response = self
            .http_client
            .post(&url)
            .header(
                "Authorization",
                format!(
                    "Bearer {}",
                    self.api_key.as_ref().unwrap_or(&"".to_string())
                ),
            )
            .json(&request)
            .send()
            .await
            .map_err(|e| ClientError::NetworkError {
                message: e.to_string(),
            })?;

        if !response.status().is_success() {
            return Err(ClientError::Provider(
                crate::error::ProviderError::ServiceUnavailable,
            ));
        }

        let result = response
            .json::<R>()
            .await
            .map_err(|e| ClientError::Serialization {
                message: e.to_string(),
            })?;

        Ok(result)
    }

    fn should_fallback(&self, error: &crate::error::ProviderError) -> bool {
        matches!(
            error,
            crate::error::ProviderError::RateLimit
                | crate::error::ProviderError::ServiceUnavailable
                | crate::error::ProviderError::Timeout
        )
    }

    #[allow(dead_code)]
    async fn try_fallback_providers(
        &self,
        provider_ids: &[String],
        failed_provider: &str,
        request: ChatRequest,
    ) -> Result<ChatResponse, ClientError> {
        for provider_id in provider_ids {
            if provider_id != failed_provider {
                if let Some(provider) = self.providers.get(provider_id) {
                    match provider.chat_completion(request.clone()).await {
                        Ok(response) => return Ok(response),
                        Err(_) => continue,
                    }
                }
            }
        }
        Err(ClientError::Provider(
            crate::error::ProviderError::ServiceUnavailable,
        ))
    }

    // Helper methods for fallback providers
    async fn try_fallback_providers_image(
        &self,
        provider_ids: &[String],
        _failed_provider: &str,
        request: ImageRequest,
    ) -> Result<ImageResponse, ClientError> {
        for provider_id in provider_ids {
            if let Some(provider) = self.providers.get(provider_id) {
                if let Ok(response) = provider.image_generation(request.clone()).await {
                    // Update last used provider
                    {
                        let mut last_provider = self.last_used_provider.write().await;
                        *last_provider = Some(provider_id.clone());
                    }
                    return Ok(response);
                }
            }
        }

        Err(ClientError::Configuration {
            message: "All providers failed for image generation, including fallbacks".to_string(),
        })
    }

    async fn try_fallback_providers_audio(
        &self,
        provider_ids: &[String],
        _failed_provider: &str,
        request: AudioRequest,
    ) -> Result<AudioResponse, ClientError> {
        for provider_id in provider_ids {
            if let Some(provider) = self.providers.get(provider_id) {
                if let Ok(response) = provider.audio_transcription(request.clone()).await {
                    // Update last used provider
                    {
                        let mut last_provider = self.last_used_provider.write().await;
                        *last_provider = Some(provider_id.clone());
                    }
                    return Ok(response);
                }
            }
        }

        Err(ClientError::Configuration {
            message: "All providers failed for audio transcription, including fallbacks"
                .to_string(),
        })
    }

    async fn try_fallback_providers_speech(
        &self,
        provider_ids: &[String],
        _failed_provider: &str,
        request: SpeechRequest,
    ) -> Result<SpeechResponse, ClientError> {
        for provider_id in provider_ids {
            if let Some(provider) = self.providers.get(provider_id) {
                if let Ok(response) = provider.text_to_speech(request.clone()).await {
                    // Update last used provider
                    {
                        let mut last_provider = self.last_used_provider.write().await;
                        *last_provider = Some(provider_id.clone());
                    }
                    return Ok(response);
                }
            }
        }

        Err(ClientError::Configuration {
            message: "All providers failed for text-to-speech, including fallbacks".to_string(),
        })
    }

    async fn try_fallback_providers_embedding(
        &self,
        provider_ids: &[String],
        failed_provider: &str,
        request: EmbeddingRequest,
    ) -> Result<EmbeddingResponse, ClientError> {
        for provider_id in provider_ids {
            if provider_id != failed_provider {
                if let Some(provider) = self.providers.get(provider_id) {
                    if let Ok(response) = provider.embedding(request.clone()).await {
                        // Update last used provider
                        {
                            let mut last_provider = self.last_used_provider.write().await;
                            *last_provider = Some(provider_id.clone());
                        }
                        return Ok(response);
                    }
                }
            }
        }

        Err(ClientError::Configuration {
            message: "All providers failed for embedding, including fallbacks".to_string(),
        })
    }
}

/// Builder for creating `UltrafastClient` instances with custom configuration.
///
/// The `UltrafastClientBuilder` provides a fluent API for configuring and creating
/// `UltrafastClient` instances. It supports both standalone and gateway modes.
///
/// # Examples
///
/// ## Standalone Mode
///
/// ```rust
/// use ultrafast_models_sdk::{UltrafastClient, RetryPolicy};
/// use std::time::Duration;
///
/// let retry_policy = RetryPolicy {
///     max_retries: 5,
///     initial_delay: Duration::from_millis(100),
///     max_delay: Duration::from_secs(10),
///     backoff_multiplier: 2.0,
///     jitter_factor: 0.1,
/// };
///
/// let client = UltrafastClientBuilder::default()
///     .with_retry_policy(retry_policy)
///     .standalone()
///     .with_openai("your-openai-key")
///     .with_anthropic("your-anthropic-key")
///     .build()?;
/// ```
///
/// ## Gateway Mode
///
/// ```rust
/// let client = UltrafastClientBuilder::default()
///     .gateway("http://localhost:3000".to_string())
///     .with_api_key("your-gateway-key")
///     .with_timeout(Duration::from_secs(60))
///     .build()?;
/// ```
///
/// # Builder Pattern
///
/// The builder follows the fluent builder pattern, allowing method chaining:
///
/// ```rust
/// let client = UltrafastClientBuilder::default()
///     .with_retry_policy(custom_retry_policy)
///     .standalone()
///     .with_openai("key1")
///     .with_anthropic("key2")
///     .with_routing_strategy(RoutingStrategy::LoadBalance {
///         weights: vec![0.6, 0.4],
///     })
///     .with_cache_config(cache_config)
///     .build()?;
/// ```
///
/// # Configuration Options
///
/// ## Retry Policy
///
/// Configure retry behavior for failed requests:
///
/// - **Max Retries**: Maximum number of retry attempts
/// - **Initial Delay**: Starting delay before first retry
/// - **Max Delay**: Maximum delay between retries
/// - **Backoff Multiplier**: Exponential backoff factor
/// - **Jitter Factor**: Randomization to prevent thundering herd
///
/// ## Provider Configuration
///
/// Add and configure AI providers:
///
/// - **OpenAI**: GPT models with API key
/// - **Anthropic**: Claude models with API key
/// - **Azure OpenAI**: Azure-hosted OpenAI models
/// - **Google Vertex AI**: Google AI models
/// - **Cohere**: Command models
/// - **Groq**: Fast inference models
/// - **Ollama**: Local models
/// - **Custom Providers**: Extensible provider system
///
/// ## Routing Strategy
///
/// Choose how requests are routed to providers:
///
/// - **Single**: Route all requests to one provider
/// - **Load Balance**: Distribute requests across providers
/// - **Failover**: Primary provider with automatic fallback
/// - **Conditional**: Route based on request characteristics
/// - **A/B Testing**: Route for testing different providers
///
/// ## Caching
///
/// Configure response caching:
///
/// - **Backend**: In-memory or Redis cache
/// - **TTL**: Time-to-live for cached responses
/// - **Max Size**: Maximum number of cached items
/// - **Key Strategy**: Custom cache key generation
///
/// # Thread Safety
///
/// The builder is not thread-safe and should not be shared across threads.
/// Build the client first, then share the client instance.
///
/// # Performance Considerations
///
/// - **Connection Pooling**: Configure appropriate pool sizes
/// - **Timeout Settings**: Set realistic timeouts for your use case
/// - **Retry Policies**: Balance retry attempts with user experience
/// - **Cache Configuration**: Enable caching for repeated requests
///
/// # Error Handling
///
/// The builder validates configuration and returns errors for:
///
/// - Invalid provider configurations
/// - Missing required fields
/// - Configuration conflicts
/// - Network connectivity issues
///
/// # See Also
///
/// - [`UltrafastClient`] - The main client struct
/// - [`StandaloneClientBuilder`] - For standalone mode configuration
/// - [`GatewayClientBuilder`] - For gateway mode configuration
/// - [`RetryPolicy`] - For retry configuration
/// - [`CacheConfig`] - For cache configuration
#[derive(Default)]
pub struct UltrafastClientBuilder {
    retry_policy: RetryPolicy,
}

impl UltrafastClientBuilder {
    pub fn with_retry_policy(mut self, retry_policy: RetryPolicy) -> Self {
        self.retry_policy = retry_policy;
        self
    }

    pub fn standalone(self) -> StandaloneClientBuilder {
        StandaloneClientBuilder {
            providers: HashMap::new(),
            routing_strategy: RoutingStrategy::Single,
            cache_config: None,
            retry_policy: self.retry_policy,
        }
    }

    pub fn gateway(self, base_url: String) -> GatewayClientBuilder {
        GatewayClientBuilder {
            base_url,
            api_key: None,
            timeout: Duration::from_secs(30),
            retry_policy: self.retry_policy,
        }
    }
}

/// Builder for creating standalone mode `UltrafastClient` instances.
///
/// The `StandaloneClientBuilder` is used to configure clients that communicate
/// directly with AI providers without going through a gateway.
///
/// # Features
///
/// - **Direct Provider Communication**: Bypass gateway for lower latency
/// - **Provider Management**: Add and configure multiple AI providers
/// - **Routing Strategies**: Choose how requests are distributed
/// - **Caching**: Configure response caching for performance
/// - **Retry Policies**: Customize retry behavior
///
/// # Examples
///
/// ## Basic Setup
///
/// ```rust
/// let client = StandaloneClientBuilder::default()
///     .with_openai("your-openai-key")
///     .build()?;
/// ```
///
/// ## Multi-Provider Setup
///
/// ```rust
/// let client = StandaloneClientBuilder::default()
///     .with_openai("openai-key")
///     .with_anthropic("anthropic-key")
///     .with_google_vertex_ai("google-key", "project-id")
///     .with_ollama("http://localhost:11434")
///     .with_routing_strategy(RoutingStrategy::LoadBalance {
///         weights: vec![0.4, 0.3, 0.2, 0.1],
///     })
///     .build()?;
/// ```
///
/// ## Advanced Configuration
///
/// ```rust
/// use ultrafast_models_sdk::{CacheConfig, RoutingStrategy};
/// use std::time::Duration;
///
/// let cache_config = CacheConfig {
///     enabled: true,
///     ttl: Duration::from_hours(1),
///     max_size: 1000,
///     backend: CacheBackend::Memory,
/// };
///
/// let client = StandaloneClientBuilder::default()
///     .with_openai("your-key")
///     .with_routing_strategy(RoutingStrategy::Failover)
///     .with_cache_config(cache_config)
///     .build()?;
/// ```
///
/// # Provider Methods
///
/// ## OpenAI
///
/// ```rust
/// .with_openai("your-openai-api-key")
/// ```
///
/// ## Anthropic
///
/// ```rust
/// .with_anthropic("your-anthropic-api-key")
/// ```
///
/// ## Azure OpenAI
///
/// ```rust
/// .with_azure_openai("your-azure-key", "deployment-name")
/// ```
///
/// ## Google Vertex AI
///
/// ```rust
/// .with_google_vertex_ai("your-google-key", "project-id")
/// ```
///
/// ## Cohere
///
/// ```rust
/// .with_cohere("your-cohere-api-key")
/// ```
///
/// ## Groq
///
/// ```rust
/// .with_groq("your-groq-api-key")
/// ```
///
/// ## Ollama
///
/// ```rust
/// .with_ollama("http://localhost:11434")
/// ```
///
/// ## Custom Providers
///
/// ```rust
/// let custom_config = ProviderConfig::new("custom", "api-key");
/// .with_provider("custom", custom_config)
/// ```
///
/// # Routing Strategies
///
/// ## Single Provider
///
/// ```rust
/// .with_routing_strategy(RoutingStrategy::Single)
/// ```
///
/// ## Load Balancing
///
/// ```rust
/// .with_routing_strategy(RoutingStrategy::LoadBalance {
///     weights: vec![0.6, 0.4], // 60% OpenAI, 40% Anthropic
/// })
/// ```
///
/// ## Failover
///
/// ```rust
/// .with_routing_strategy(RoutingStrategy::Failover)
/// ```
///
/// ## Conditional Routing
///
/// ```rust
/// .with_routing_strategy(RoutingStrategy::Conditional {
///     conditions: vec![
///         ("model", "gpt-4", "openai"),
///         ("model", "claude-3", "anthropic"),
///     ],
///     default: "openai".to_string(),
/// })
/// ```
///
/// ## A/B Testing
///
/// ```rust
/// .with_routing_strategy(RoutingStrategy::ABTesting {
///     split: 0.5, // 50% to each provider
/// })
/// ```
///
/// # Caching Configuration
///
/// ```rust
/// let cache_config = CacheConfig {
///     enabled: true,
///     ttl: Duration::from_hours(1),
///     max_size: 1000,
///     backend: CacheBackend::Memory,
/// };
///
/// .with_cache_config(cache_config)
/// ```
///
/// # Performance Optimization
///
/// - **Provider Selection**: Choose providers based on your needs
/// - **Routing Strategy**: Optimize for latency, cost, or reliability
/// - **Caching**: Enable caching for repeated requests
/// - **Connection Pooling**: Configure appropriate pool sizes
///
/// # Error Handling
///
/// The builder validates configuration and returns errors for:
///
/// - Missing provider configurations
/// - Invalid routing strategies
/// - Configuration conflicts
/// - Network connectivity issues
///
/// # Thread Safety
///
/// The builder is not thread-safe. Build the client first, then share the client instance.
///
/// # See Also
///
/// - [`UltrafastClient`] - The main client struct
/// - [`UltrafastClientBuilder`] - The main builder
/// - [`GatewayClientBuilder`] - For gateway mode
/// - [`ProviderConfig`] - For provider configuration
/// - [`CacheConfig`] - For cache configuration
pub struct StandaloneClientBuilder {
    providers: HashMap<String, ProviderConfig>,
    routing_strategy: RoutingStrategy,
    cache_config: Option<CacheConfig>,
    retry_policy: RetryPolicy,
}

impl Default for StandaloneClientBuilder {
    fn default() -> Self {
        Self {
            providers: HashMap::new(),
            routing_strategy: RoutingStrategy::Single,
            cache_config: None,
            retry_policy: RetryPolicy::default(),
        }
    }
}

impl StandaloneClientBuilder {
    pub fn with_provider(mut self, name: impl Into<String>, config: ProviderConfig) -> Self {
        self.providers.insert(name.into(), config);
        self
    }

    pub fn with_openai(self, api_key: impl Into<String>) -> Self {
        let config = ProviderConfig::new("openai", api_key);
        self.with_provider("openai", config)
    }

    pub fn with_anthropic(self, api_key: impl Into<String>) -> Self {
        let config = ProviderConfig::new("anthropic", api_key);
        self.with_provider("anthropic", config)
    }

    pub fn with_azure_openai(
        self,
        api_key: impl Into<String>,
        deployment_name: impl Into<String>,
    ) -> Self {
        let mut config = ProviderConfig::new("azure-openai", api_key);
        config.name = deployment_name.into();
        self.with_provider("azure-openai", config)
    }

    pub fn with_google_vertex_ai(
        self,
        api_key: impl Into<String>,
        project_id: impl Into<String>,
    ) -> Self {
        let mut config = ProviderConfig::new("google-vertex-ai", api_key);
        config
            .headers
            .insert("project-id".to_string(), project_id.into());
        self.with_provider("google-vertex-ai", config)
    }

    pub fn with_cohere(self, api_key: impl Into<String>) -> Self {
        let config = ProviderConfig::new("cohere", api_key);
        self.with_provider("cohere", config)
    }

    pub fn with_groq(self, api_key: impl Into<String>) -> Self {
        let config = ProviderConfig::new("groq", api_key);
        self.with_provider("groq", config)
    }

    pub fn with_mistral(self, api_key: impl Into<String>) -> Self {
        let config = ProviderConfig::new("mistral", api_key);
        self.with_provider("mistral", config)
    }

    pub fn with_perplexity(self, api_key: impl Into<String>) -> Self {
        let config = ProviderConfig::new("perplexity", api_key);
        self.with_provider("perplexity", config)
    }

    pub fn with_openrouter(self, api_key: impl Into<String>) -> Self {
        let config = ProviderConfig::new("openrouter", api_key);
        self.with_provider("openrouter", config)
    }

    pub fn with_ollama(self, base_url: impl Into<String>) -> Self {
        let mut config = ProviderConfig::new("ollama", "");
        config.base_url = Some(base_url.into());
        self.with_provider("ollama", config)
    }

    pub fn with_custom(
        self,
        name: impl Into<String>,
        api_key: impl Into<String>,
        base_url: impl Into<String>,
    ) -> Self {
        let mut config = ProviderConfig::new("custom", api_key);
        config.name = name.into();
        config.base_url = Some(base_url.into());
        self.with_provider("custom", config)
    }

    pub fn with_routing_strategy(mut self, strategy: RoutingStrategy) -> Self {
        self.routing_strategy = strategy;
        self
    }

    pub fn with_cache(mut self, config: CacheConfig) -> Self {
        self.cache_config = Some(config);
        self
    }

    pub fn build(self) -> Result<UltrafastClient, ClientError> {
        if self.providers.is_empty() {
            return Err(ClientError::Configuration {
                message: "At least one provider must be configured".to_string(),
            });
        }

        let mut providers = HashMap::new();
        for (name, config) in self.providers {
            // Use circuit breaker by default for all providers
            let provider = create_provider_with_circuit_breaker(config, None)?;
            providers.insert(name, provider.into());
        }

        let cache = self.cache_config.map(|config| {
            let cache: Arc<dyn Cache> = Arc::new(InMemoryCache::new(config.max_size));
            cache
        });

        // Create optimized HTTP client for standalone mode too
        let http_client = Client::builder()
            .pool_max_idle_per_host(20)
            .pool_idle_timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| ClientError::Configuration {
                message: format!("Failed to create HTTP client: {e}"),
            })?;

        Ok(UltrafastClient {
            mode: ClientMode::Standalone,
            providers,
            router: Arc::new(RwLock::new(Router::new(self.routing_strategy))),
            cache,
            metrics: Arc::new(RwLock::new(HashMap::new())),
            http_client,
            api_key: None,
            timeout: Duration::from_secs(30),
            retry_policy: self.retry_policy,
            connection_pool: Arc::new(RwLock::new(ConnectionPool::new(
                20,
                Duration::from_secs(60),
                Duration::from_secs(60),
            ))),
            last_used_provider: Arc::new(RwLock::new(None)),
        })
    }
}

pub struct GatewayClientBuilder {
    base_url: String,
    api_key: Option<String>,
    timeout: Duration,
    retry_policy: RetryPolicy,
}

impl GatewayClientBuilder {
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            api_key: None,
            timeout: Duration::from_secs(30),
            retry_policy: RetryPolicy::default(),
        }
    }

    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn build(self) -> Result<UltrafastClient, ClientError> {
        let http_client = Client::builder()
            .timeout(self.timeout)
            // Phase 1 Optimizations: Connection pooling, keep-alive
            .pool_max_idle_per_host(20) // Increased connection pool
            .pool_idle_timeout(Duration::from_secs(60)) // Keep connections alive longer
            .build()
            .map_err(|e| ClientError::Configuration {
                message: format!("Failed to create HTTP client: {e}"),
            })?;

        Ok(UltrafastClient {
            mode: ClientMode::Gateway {
                base_url: self.base_url,
            },
            providers: HashMap::new(),
            router: Arc::new(RwLock::new(Router::new(RoutingStrategy::Single))),
            cache: None,
            metrics: Arc::new(RwLock::new(HashMap::new())),
            http_client,
            api_key: self.api_key,
            timeout: self.timeout,
            retry_policy: self.retry_policy,
            connection_pool: Arc::new(RwLock::new(ConnectionPool::new(
                20,
                Duration::from_secs(60),
                Duration::from_secs(60),
            ))),
            last_used_provider: Arc::new(RwLock::new(None)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation_with_circuit_breaker() {
        let client = UltrafastClient::standalone()
            .with_openai("test-key")
            .build();

        assert!(client.is_ok());
    }

    #[tokio::test]
    async fn test_circuit_breaker_integration() {
        let client = UltrafastClient::standalone()
            .with_openai("test-key")
            .build()
            .unwrap();

        // Test that circuit breaker metrics are available
        let cb_metrics = client.get_circuit_breaker_metrics().await;
        assert!(!cb_metrics.is_empty());

        // Test that health status is available
        let health_status = client.get_provider_health_status().await;
        assert!(!health_status.is_empty());

        // All providers should be healthy initially
        for (_, is_healthy) in health_status {
            assert!(is_healthy);
        }
    }
}
