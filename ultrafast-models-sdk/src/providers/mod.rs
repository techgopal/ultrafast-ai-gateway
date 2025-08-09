//! # Provider System Module
//!
//! This module provides the provider abstraction layer for the Ultrafast Models SDK.
//! It defines the interface for different AI/LLM providers and provides factory
//! functions for creating provider instances with circuit breaker protection.
//!
//! ## Overview
//!
//! The provider system provides:
//! - **Unified Provider Interface**: Common trait for all AI providers
//! - **Provider Factory**: Easy creation of provider instances
//! - **Circuit Breaker Integration**: Automatic failure protection
//! - **Health Monitoring**: Provider health and performance tracking
//! - **Configuration Management**: Flexible provider configuration
//! - **Metrics Collection**: Performance and cost tracking
//!
//! ## Supported Providers
//!
//! The SDK supports multiple AI providers:
//!
//! - **OpenAI**: GPT-4, GPT-3.5, and other OpenAI models
//! - **Anthropic**: Claude-3, Claude-2, Claude Instant
//! - **Google**: Gemini Pro, Gemini Pro Vision, PaLM
//! - **Azure OpenAI**: Azure-hosted OpenAI models
//! - **Ollama**: Local and remote Ollama instances
//! - **Mistral AI**: Mistral 7B, Mixtral, and other models
//! - **Cohere**: Command, Command R, and other Cohere models
//! - **Custom Providers**: Extensible provider system
//!
//! ## Provider Interface
//!
//! All providers implement the `Provider` trait, which provides:
//!
//! - **Chat Completions**: Conversational AI model interactions
//! - **Streaming Support**: Real-time response streaming
//! - **Embeddings**: Text embedding generation
//! - **Image Generation**: AI-powered image creation
//! - **Audio Processing**: Speech-to-text and text-to-speech
//! - **Health Checks**: Provider availability monitoring
//!
//! ## Usage Examples
//!
//! ### Creating Providers
//!
//! ```rust
//! use ultrafast_models_sdk::providers::{create_provider, ProviderConfig};
//! use std::time::Duration;
//!
//! // Create OpenAI provider
//! let config = ProviderConfig::new("openai", "your-openai-key")
//!     .with_timeout(Duration::from_secs(30))
//!     .with_max_retries(3)
//!     .with_base_url("https://api.openai.com/v1".to_string());
//!
//! let provider = create_provider(config)?;
//!
//! // Use the provider
//! let response = provider.chat_completion(request).await?;
//! ```
//!
//! ### Provider with Circuit Breaker
//!
//! ```rust
//! use ultrafast_models_sdk::providers::{create_provider_with_circuit_breaker, ProviderConfig};
//! use ultrafast_models_sdk::circuit_breaker::CircuitBreakerConfig;
//! use std::time::Duration;
//!
//! let provider_config = ProviderConfig::new("anthropic", "your-anthropic-key");
//! let circuit_config = CircuitBreakerConfig {
//!     failure_threshold: 5,
//!     recovery_timeout: Duration::from_secs(60),
//!     request_timeout: Duration::from_secs(30),
//!     half_open_max_calls: 3,
//! };
//!
//! let provider = create_provider_with_circuit_breaker(provider_config, Some(circuit_config))?;
//! ```
//!
//! ### Health Monitoring
//!
//! ```rust
//! use ultrafast_models_sdk::providers::{Provider, ProviderMetrics};
//!
//! // Check provider health
//! let health = provider.health_check().await?;
//! println!("Provider status: {:?}", health.status);
//! println!("Latency: {:?}ms", health.latency_ms);
//! println!("Error rate: {:.2}%", health.error_rate * 100.0);
//!
//! // Get provider metrics
//! let metrics = provider.get_metrics();
//! println!("Total requests: {}", metrics.total_requests);
//! println!("Success rate: {:.2}%", metrics.success_rate() * 100.0);
//! println!("Average latency: {:.2}ms", metrics.average_latency_ms);
//! ```
//!
//! ### Custom Provider Implementation
//!
//! ```rust
//! use ultrafast_models_sdk::providers::{Provider, ProviderConfig};
//! use ultrafast_models_sdk::models::{ChatRequest, ChatResponse, ProviderError};
//! use async_trait::async_trait;
//!
//! struct CustomProvider {
//!     config: ProviderConfig,
//!     metrics: ProviderMetrics,
//! }
//!
//! #[async_trait]
//! impl Provider for CustomProvider {
//!     fn name(&self) -> &str {
//!         "custom"
//!     }
//!
//!     fn supports_streaming(&self) -> bool {
//!         true
//!     }
//!
//!     fn supports_function_calling(&self) -> bool {
//!         false
//!     }
//!
//!     fn supported_models(&self) -> Vec<String> {
//!         vec!["custom-model".to_string()]
//!     }
//!
//!     async fn chat_completion(&self, request: ChatRequest) -> Result<ChatResponse, ProviderError> {
//!         // Implement your custom provider logic here
//!         todo!("Implement custom provider")
//!     }
//!
//!     async fn health_check(&self) -> Result<ProviderHealth, ProviderError> {
//!         // Implement health check logic
//!         Ok(ProviderHealth {
//!             status: HealthStatus::Healthy,
//!             latency_ms: Some(100),
//!             error_rate: 0.0,
//!             last_check: chrono::Utc::now(),
//!             details: std::collections::HashMap::new(),
//!         })
//!     }
//! }
//! ```
//!
//! ## Provider Configuration
//!
//! Each provider can be configured with:
//!
//! - **API Key**: Authentication credentials
//! - **Base URL**: Custom endpoint URL
//! - **Timeout**: Request timeout duration
//! - **Retry Policy**: Retry configuration
//! - **Rate Limiting**: Request rate limits
//! - **Model Mapping**: Custom model name mappings
//! - **Headers**: Custom HTTP headers
//!
//! ## Health Monitoring
//!
//! The provider system includes comprehensive health monitoring:
//!
//! - **Status Tracking**: Healthy, Degraded, Unhealthy, Unknown
//! - **Latency Monitoring**: Response time tracking
//! - **Error Rate Calculation**: Success/failure ratio
//! - **Last Check Time**: Timestamp of last health check
//! - **Detailed Metrics**: Provider-specific health information
//!
//! ## Performance Metrics
//!
//! Each provider tracks detailed performance metrics:
//!
//! - **Request Counts**: Total, successful, and failed requests
//! - **Latency Analysis**: Average and percentile response times
//! - **Token Usage**: Input and output token tracking
//! - **Cost Tracking**: Provider cost calculation
//! - **Rate Limit Hits**: Rate limit violation tracking
//!
//! ## Best Practices
//!
//! - **Circuit Breakers**: Always use circuit breakers for production deployments
//! - **Health Monitoring**: Regularly check provider health
//! - **Error Handling**: Implement proper error handling and retry logic
//! - **Cost Monitoring**: Track provider costs and usage
//! - **Load Balancing**: Use multiple providers for redundancy
//! - **Configuration**: Store provider configs securely

use crate::error::ProviderError;
use crate::models::{
    AudioRequest, AudioResponse, ChatRequest, ChatResponse, EmbeddingRequest, EmbeddingResponse,
    ImageRequest, ImageResponse, SpeechRequest, SpeechResponse, StreamChunk,
};
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

// Provider implementations
pub mod anthropic;
pub mod azure;
pub mod circuit_breaker_provider;
pub mod cohere;
pub mod custom;
pub mod gemini;
pub mod google;
pub mod groq;
pub mod http_client;
pub mod mistral;
pub mod ollama;
pub mod openai;
pub mod openrouter;
pub mod perplexity;

// Use the canonical duration serde helpers from the common module (keep import if used in this module)
#[allow(unused_imports)]
use crate::common::duration_serde;

/// Type alias for streaming response results.
///
/// Represents a pinned boxed stream of streaming chunks or errors.
pub type StreamResult = Pin<Box<dyn Stream<Item = Result<StreamChunk, ProviderError>> + Send>>;

/// Trait for AI/LLM provider implementations.
///
/// This trait defines the interface that all AI providers must implement,
/// providing a unified API for different AI services.
///
/// # Examples
///
/// ```rust
/// use ultrafast_models_sdk::providers::{Provider, ProviderConfig};
/// use ultrafast_models_sdk::models::{ChatRequest, ChatResponse};
/// use async_trait::async_trait;
///
/// struct MyProvider {
///     config: ProviderConfig,
/// }
///
/// #[async_trait]
/// impl Provider for MyProvider {
///     fn name(&self) -> &str { "my-provider" }
///     fn supports_streaming(&self) -> bool { true }
///     fn supports_function_calling(&self) -> bool { false }
///     fn supported_models(&self) -> Vec<String> { vec!["my-model".to_string()] }
///
///     async fn chat_completion(&self, request: ChatRequest) -> Result<ChatResponse, ProviderError> {
///         // Implementation here
///         todo!()
///     }
/// }
/// ```
#[async_trait::async_trait]
pub trait Provider: Send + Sync + Any {
    /// Get the provider name/identifier.
    ///
    /// Returns a unique identifier for this provider.
    fn name(&self) -> &str;

    /// Check if this provider supports streaming responses.
    ///
    /// Returns `true` if the provider supports streaming chat completions.
    fn supports_streaming(&self) -> bool;

    /// Check if this provider supports function calling.
    ///
    /// Returns `true` if the provider supports function calling and tool usage.
    fn supports_function_calling(&self) -> bool;

    /// Get the list of models supported by this provider.
    ///
    /// Returns a vector of model names that this provider can handle.
    fn supported_models(&self) -> Vec<String>;

    /// Perform a chat completion request.
    ///
    /// # Arguments
    ///
    /// * `request` - The chat completion request
    ///
    /// # Returns
    ///
    /// Returns a chat completion response or an error.
    async fn chat_completion(&self, request: ChatRequest) -> Result<ChatResponse, ProviderError>;

    /// Perform a streaming chat completion request.
    ///
    /// # Arguments
    ///
    /// * `request` - The chat completion request
    ///
    /// # Returns
    ///
    /// Returns a stream of chat completion chunks or an error.
    async fn stream_chat_completion(
        &self,
        request: ChatRequest,
    ) -> Result<StreamResult, ProviderError>;

    /// Generate embeddings for text input.
    ///
    /// # Arguments
    ///
    /// * `request` - The embedding request
    ///
    /// # Returns
    ///
    /// Returns an embedding response or an error.
    ///
    /// # Default Implementation
    ///
    /// Returns a configuration error by default. Providers that support
    /// embeddings should override this method.
    async fn embedding(
        &self,
        _request: EmbeddingRequest,
    ) -> Result<EmbeddingResponse, ProviderError> {
        Err(ProviderError::Configuration {
            message: "Embeddings not supported by this provider".to_string(),
        })
    }

    /// Generate images from text prompts.
    ///
    /// # Arguments
    ///
    /// * `request` - The image generation request
    ///
    /// # Returns
    ///
    /// Returns an image generation response or an error.
    ///
    /// # Default Implementation
    ///
    /// Returns a configuration error by default. Providers that support
    /// image generation should override this method.
    async fn image_generation(
        &self,
        _request: ImageRequest,
    ) -> Result<ImageResponse, ProviderError> {
        Err(ProviderError::Configuration {
            message: "Image generation not supported by this provider".to_string(),
        })
    }

    /// Transcribe audio to text.
    ///
    /// # Arguments
    ///
    /// * `request` - The audio transcription request
    ///
    /// # Returns
    ///
    /// Returns an audio transcription response or an error.
    ///
    /// # Default Implementation
    ///
    /// Returns a configuration error by default. Providers that support
    /// audio transcription should override this method.
    async fn audio_transcription(
        &self,
        _request: AudioRequest,
    ) -> Result<AudioResponse, ProviderError> {
        Err(ProviderError::Configuration {
            message: "Audio transcription not supported by this provider".to_string(),
        })
    }

    /// Convert text to speech.
    ///
    /// # Arguments
    ///
    /// * `request` - The text-to-speech request
    ///
    /// # Returns
    ///
    /// Returns a speech response or an error.
    ///
    /// # Default Implementation
    ///
    /// Returns a configuration error by default. Providers that support
    /// text-to-speech should override this method.
    async fn text_to_speech(
        &self,
        _request: SpeechRequest,
    ) -> Result<SpeechResponse, ProviderError> {
        Err(ProviderError::Configuration {
            message: "Text-to-speech not supported by this provider".to_string(),
        })
    }

    /// Perform a health check on this provider.
    ///
    /// # Returns
    ///
    /// Returns provider health information or an error.
    async fn health_check(&self) -> Result<ProviderHealth, ProviderError>;
}

/// Configuration for provider instances.
///
/// This struct contains all the configuration parameters needed to
/// create and configure a provider instance.
///
/// # Examples
///
/// ```rust
/// use ultrafast_models_sdk::providers::ProviderConfig;
/// use std::time::Duration;
///
/// let config = ProviderConfig::new("openai", "your-api-key")
///     .with_timeout(Duration::from_secs(30))
///     .with_max_retries(3)
///     .with_base_url("https://api.openai.com/v1".to_string());
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// Provider name/identifier
    pub name: String,
    /// API key for authentication
    pub api_key: String,
    /// Optional base URL for the provider API
    pub base_url: Option<String>,
    /// Request timeout duration
    #[serde(with = "crate::common::duration_serde")]
    pub timeout: Duration,
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Delay between retry attempts
    #[serde(with = "crate::common::duration_serde")]
    pub retry_delay: Duration,
    /// Optional rate limiting configuration
    pub rate_limit: Option<RateLimit>,
    /// Model name mappings (from client model names to provider model names)
    pub model_mapping: HashMap<String, String>,
    /// Custom HTTP headers to include in requests
    pub headers: HashMap<String, String>,
    /// Whether this provider is enabled
    pub enabled: bool,
    /// Optional circuit breaker configuration
    pub circuit_breaker: Option<crate::circuit_breaker::CircuitBreakerConfig>,
}

impl ProviderConfig {
    /// Create a new provider configuration.
    ///
    /// # Arguments
    ///
    /// * `name` - Provider name/identifier
    /// * `api_key` - API key for authentication
    ///
    /// # Examples
    ///
    /// ```rust
    /// use ultrafast_models_sdk::providers::ProviderConfig;
    ///
    /// let config = ProviderConfig::new("openai", "your-api-key");
    /// ```
    pub fn new(name: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            api_key: api_key.into(),
            base_url: None,
            timeout: Duration::from_secs(30),
            max_retries: 3,
            retry_delay: Duration::from_secs(1),
            rate_limit: None,
            model_mapping: HashMap::new(),
            headers: HashMap::new(),
            enabled: true,
            circuit_breaker: None,
        }
    }

    /// Set the base URL for the provider API.
    ///
    /// # Arguments
    ///
    /// * `base_url` - The base URL for API requests
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Set the request timeout duration.
    ///
    /// # Arguments
    ///
    /// * `timeout` - The timeout duration
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Set the rate limiting configuration.
    ///
    /// # Arguments
    ///
    /// * `rate_limit` - The rate limiting configuration
    pub fn with_rate_limit(mut self, rate_limit: RateLimit) -> Self {
        self.rate_limit = Some(rate_limit);
        self
    }

    /// Add a model name mapping.
    ///
    /// # Arguments
    ///
    /// * `from` - The client model name
    /// * `to` - The provider model name
    pub fn with_model_mapping(mut self, from: impl Into<String>, to: impl Into<String>) -> Self {
        self.model_mapping.insert(from.into(), to.into());
        self
    }

    /// Add a custom HTTP header.
    ///
    /// # Arguments
    ///
    /// * `key` - The header name
    /// * `value` - The header value
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }
}

/// Rate limiting configuration for providers.
///
/// Defines rate limits for requests and tokens per minute.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimit {
    /// Maximum requests allowed per minute
    pub requests_per_minute: u32,
    /// Maximum tokens allowed per minute
    pub tokens_per_minute: u32,
}

/// Provider health information.
///
/// Contains detailed health status and metrics for a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderHealth {
    /// Current health status
    pub status: HealthStatus,
    /// Response latency in milliseconds (if available)
    pub latency_ms: Option<u64>,
    /// Error rate as a percentage (0.0 to 1.0)
    pub error_rate: f64,
    /// Timestamp of the last health check
    pub last_check: chrono::DateTime<chrono::Utc>,
    /// Additional health details
    pub details: HashMap<String, String>,
}

/// Provider health status enumeration.
///
/// Represents the different health states a provider can be in.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HealthStatus {
    /// Provider is healthy and responding normally
    Healthy,
    /// Provider is degraded but still functional
    Degraded,
    /// Provider is unhealthy and not responding
    Unhealthy,
    /// Provider health status is unknown
    Unknown,
}

/// Performance metrics for a provider.
///
/// Tracks comprehensive performance and usage metrics for provider monitoring.
#[derive(Debug, Clone)]
pub struct ProviderMetrics {
    /// Total number of requests made
    pub total_requests: u64,
    /// Number of successful requests
    pub successful_requests: u64,
    /// Number of failed requests
    pub failed_requests: u64,
    /// Average response latency in milliseconds
    pub average_latency_ms: f64,
    /// Total tokens processed (input + output)
    pub tokens_processed: u64,
    /// Total cost in USD
    pub cost_usd: f64,
    /// Number of rate limit hits
    pub rate_limit_hits: u64,
    /// Timestamp of the last request
    pub last_request: Option<chrono::DateTime<chrono::Utc>>,
}

impl Default for ProviderMetrics {
    fn default() -> Self {
        Self {
            total_requests: 0,
            successful_requests: 0,
            failed_requests: 0,
            average_latency_ms: 0.0,
            tokens_processed: 0,
            cost_usd: 0.0,
            rate_limit_hits: 0,
            last_request: None,
        }
    }
}

impl ProviderMetrics {
    /// Record a request with enhanced metrics.
    ///
    /// Updates all metrics based on the request result.
    ///
    /// # Arguments
    ///
    /// * `success` - Whether the request was successful
    /// * `latency_ms` - Response latency in milliseconds
    /// * `tokens` - Number of tokens processed
    /// * `cost` - Cost of the request in USD
    pub fn record_enhanced_request(
        &mut self,
        success: bool,
        latency_ms: u64,
        tokens: u32,
        cost: f64,
    ) {
        self.total_requests += 1;
        self.last_request = Some(chrono::Utc::now());

        if success {
            self.successful_requests += 1;
        } else {
            self.failed_requests += 1;
        }

        // Update average latency using exponential moving average
        let alpha = 0.1; // Smoothing factor
        self.average_latency_ms =
            alpha * latency_ms as f64 + (1.0 - alpha) * self.average_latency_ms;

        self.tokens_processed += tokens as u64;
        self.cost_usd += cost;
    }

    /// Calculate the success rate.
    ///
    /// Returns the percentage of successful requests as a value between 0.0 and 1.0.
    pub fn success_rate(&self) -> f64 {
        if self.total_requests == 0 {
            1.0 // No requests means 100% success rate
        } else {
            self.successful_requests as f64 / self.total_requests as f64
        }
    }

    /// Calculate the failure rate.
    ///
    /// Returns the percentage of failed requests as a value between 0.0 and 1.0.
    pub fn failure_rate(&self) -> f64 {
        if self.total_requests == 0 {
            0.0 // No requests means 0% failure rate
        } else {
            self.failed_requests as f64 / self.total_requests as f64
        }
    }

    /// Calculate the average tokens per request.
    ///
    /// Returns the average number of tokens processed per request.
    pub fn average_tokens_per_request(&self) -> f64 {
        if self.total_requests == 0 {
            0.0
        } else {
            self.tokens_processed as f64 / self.total_requests as f64
        }
    }

    /// Calculate the average cost per request.
    ///
    /// Returns the average cost per request in USD.
    pub fn average_cost_per_request(&self) -> f64 {
        if self.total_requests == 0 {
            0.0
        } else {
            self.cost_usd / self.total_requests as f64
        }
    }

    /// Calculate the rate limit hit rate.
    ///
    /// Returns the percentage of requests that hit rate limits.
    pub fn rate_limit_hit_rate(&self) -> f64 {
        if self.total_requests == 0 {
            0.0
        } else {
            self.rate_limit_hits as f64 / self.total_requests as f64
        }
    }

    /// Check if the provider is considered healthy.
    ///
    /// Returns `true` if the provider has a good success rate and reasonable latency.
    pub fn is_healthy(&self) -> bool {
        let good_success_rate = self.success_rate() > 0.8; // 80% success rate
        let reasonable_latency = self.average_latency_ms < 10000.0; // Less than 10 seconds
        let recent_activity = self
            .last_request
            .map(|last| {
                let now = chrono::Utc::now();
                let duration = now.signed_duration_since(last);
                duration.num_minutes() < 5 // Activity within last 5 minutes
            })
            .unwrap_or(false);

        good_success_rate && reasonable_latency && recent_activity
    }

    /// Get the health status based on metrics.
    ///
    /// Returns a health status based on current performance metrics.
    pub fn health_status(&self) -> HealthStatus {
        if self.is_healthy() {
            HealthStatus::Healthy
        } else if self.success_rate() > 0.5 {
            HealthStatus::Degraded
        } else {
            HealthStatus::Unhealthy
        }
    }

    /// Reset all metrics to zero.
    ///
    /// Clears all performance metrics and resets counters.
    pub fn reset(&mut self) {
        self.total_requests = 0;
        self.successful_requests = 0;
        self.failed_requests = 0;
        self.average_latency_ms = 0.0;
        self.tokens_processed = 0;
        self.cost_usd = 0.0;
        self.rate_limit_hits = 0;
        self.last_request = None;
    }
}

/// Create a provider instance from configuration.
///
/// This function creates a provider instance based on the provider name
/// in the configuration. It automatically selects the appropriate provider
/// implementation.
///
/// # Arguments
///
/// * `config` - Provider configuration
///
/// # Returns
///
/// Returns a boxed provider instance or an error.
///
/// # Examples
///
/// ```rust
/// use ultrafast_models_sdk::providers::{create_provider, ProviderConfig};
///
/// let config = ProviderConfig::new("openai", "your-api-key");
/// let provider = create_provider(config)?;
/// ```
pub fn create_provider(config: ProviderConfig) -> Result<Box<dyn Provider>, ProviderError> {
    match config.name.as_str() {
        "openai" => {
            let provider = openai::OpenAIProvider::new(config)?;
            Ok(Box::new(provider))
        }
        "anthropic" => {
            let provider = anthropic::AnthropicProvider::new(config)?;
            Ok(Box::new(provider))
        }
        "azure" => {
            let provider = azure::AzureOpenAIProvider::new(config)?;
            Ok(Box::new(provider))
        }
        "google" => {
            let provider = google::GoogleVertexAIProvider::new(config)?;
            Ok(Box::new(provider))
        }
        "gemini" => {
            let provider = gemini::GeminiProvider::new(config)?;
            Ok(Box::new(provider))
        }
        "cohere" => {
            let provider = cohere::CohereProvider::new(config)?;
            Ok(Box::new(provider))
        }
        "groq" => {
            let provider = groq::GroqProvider::new(config)?;
            Ok(Box::new(provider))
        }
        "mistral" => {
            let provider = mistral::MistralProvider::new(config)?;
            Ok(Box::new(provider))
        }
        "perplexity" => {
            let provider = perplexity::PerplexityProvider::new(config)?;
            Ok(Box::new(provider))
        }
        "ollama" => {
            let provider = ollama::OllamaProvider::new(config)?;
            Ok(Box::new(provider))
        }
        "openrouter" => {
            let provider = openrouter::OpenRouterProvider::new(config)?;
            Ok(Box::new(provider))
        }
        "custom" => {
            // Create a default custom provider configuration
            let custom_config = custom::CustomProviderConfig {
                chat_endpoint: "/v1/chat/completions".to_string(),
                embedding_endpoint: Some("/v1/embeddings".to_string()),
                image_endpoint: None,
                audio_endpoint: None,
                speech_endpoint: None,
                request_format: custom::RequestFormat::OpenAI,
                response_format: custom::ResponseFormat::OpenAI,
                auth_type: custom::AuthType::Bearer,
            };
            let provider = custom::CustomProvider::new(config, custom_config)?;
            Ok(Box::new(provider))
        }
        _ => Err(ProviderError::ProviderNotSupported {
            provider: config.name,
        }),
    }
}

/// Create a provider instance with circuit breaker protection.
///
/// This function creates a provider instance and wraps it with circuit breaker
/// protection for automatic failure handling and recovery.
///
/// # Arguments
///
/// * `config` - Provider configuration
/// * `circuit_config` - Optional circuit breaker configuration
///
/// # Returns
///
/// Returns a boxed provider instance with circuit breaker protection.
///
/// # Examples
///
/// ```rust
/// use ultrafast_models_sdk::providers::{create_provider_with_circuit_breaker, ProviderConfig};
/// use ultrafast_models_sdk::circuit_breaker::CircuitBreakerConfig;
/// use std::time::Duration;
///
/// let provider_config = ProviderConfig::new("openai", "your-api-key");
/// let circuit_config = CircuitBreakerConfig {
///     failure_threshold: 5,
///     recovery_timeout: Duration::from_secs(60),
///     request_timeout: Duration::from_secs(30),
///     half_open_max_calls: 3,
/// };
///
/// let provider = create_provider_with_circuit_breaker(provider_config, Some(circuit_config))?;
/// ```
pub fn create_provider_with_circuit_breaker(
    config: ProviderConfig,
    circuit_config: Option<crate::circuit_breaker::CircuitBreakerConfig>,
) -> Result<Box<dyn Provider>, ProviderError> {
    let base_provider = create_provider(config.clone())?;

    if let Some(circuit_config) = circuit_config {
        let arc_provider = Arc::from(base_provider);
        let circuit_provider =
            circuit_breaker_provider::CircuitBreakerProvider::new(arc_provider, circuit_config);
        Ok(Box::new(circuit_provider))
    } else {
        Ok(base_provider)
    }
}
