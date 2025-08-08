//! # Ultrafast Models SDK
//!
//! A high-performance Rust SDK for interacting with multiple AI/LLM providers
//! through a unified interface. The SDK provides seamless integration with
//! various AI services including OpenAI, Anthropic, Google, and more.
//!
//! ## Overview
//!
//! The Ultrafast Models SDK provides:
//! - **Unified Interface**: Single API for multiple AI providers
//! - **Intelligent Routing**: Automatic provider selection and load balancing
//! - **Circuit Breakers**: Automatic failover and recovery mechanisms
//! - **Caching Layer**: Built-in response caching for performance
//! - **Rate Limiting**: Per-provider rate limiting and throttling
//! - **Error Handling**: Comprehensive error handling and retry logic
//! - **Metrics Collection**: Performance monitoring and analytics
//!
//! ## Supported Providers
//!
//! The SDK supports a wide range of AI providers:
//!
//! - **OpenAI**: GPT-4, GPT-3.5, and other OpenAI models
//! - **Anthropic**: Claude-3, Claude-2, and Claude Instant
//! - **Google**: Gemini Pro, Gemini Pro Vision, and PaLM
//! - **Azure OpenAI**: Azure-hosted OpenAI models
//! - **Ollama**: Local and remote Ollama instances
//! - **Mistral AI**: Mistral 7B, Mixtral, and other models
//! - **Cohere**: Command, Command R, and other Cohere models
//! - **Custom Providers**: Extensible provider system
//!
//! ## Quick Start
//!
//! ```rust
//! use ultrafast_models_sdk::{UltrafastClient, ChatRequest, Message};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create a client with multiple providers
//!     let client = UltrafastClient::standalone()
//!         .with_openai("your-openai-key")
//!         .with_anthropic("your-anthropic-key")
//!         .with_ollama("http://localhost:11434")
//!         .build()?;
//!
//!     // Create a chat request
//!     let request = ChatRequest {
//!         model: "gpt-4".to_string(),
//!         messages: vec![Message::user("Hello, world!")],
//!         temperature: Some(0.7),
//!         max_tokens: Some(100),
//!         stream: Some(false),
//!         ..Default::default()
//!     };
//!
//!     // Send the request
//!     let response = client.chat_completion(request).await?;
//!     println!("Response: {}", response.choices[0].message.content);
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Client Modes
//!
//! The SDK supports two client modes:
//!
//! ### Standalone Mode
//!
//! Direct provider communication without gateway:
//!
//! ```rust
//! let client = UltrafastClient::standalone()
//!     .with_openai("your-key")
//!     .with_anthropic("your-key")
//!     .build()?;
//! ```
//!
//! ### Gateway Mode
//!
//! Communication through the Ultrafast Gateway:
//!
//! ```rust
//! let client = UltrafastClient::gateway("http://localhost:3000")
//!     .with_api_key("your-gateway-key")
//!     .build()?;
//! ```
//!
//! ## Routing Strategies
//!
//! The SDK provides multiple routing strategies:
//!
//! - **Single Provider**: Route all requests to one provider
//! - **Load Balancing**: Distribute requests across providers
//! - **Failover**: Primary provider with automatic fallback
//! - **Conditional Routing**: Route based on request characteristics
//! - **A/B Testing**: Route requests for testing different providers
//!
//! ```rust
//! use ultrafast_models_sdk::routing::RoutingStrategy;
//!
//! let client = UltrafastClient::standalone()
//!     .with_openai("key1")
//!     .with_anthropic("key2")
//!     .with_routing_strategy(RoutingStrategy::LoadBalance {
//!         weights: vec![0.6, 0.4],
//!     })
//!     .build()?;
//! ```
//!
//! ## Circuit Breakers
//!
//! Automatic failover and recovery:
//!
//! ```rust
//! use ultrafast_models_sdk::circuit_breaker::CircuitBreakerConfig;
//!
//! let config = CircuitBreakerConfig {
//!     failure_threshold: 5,
//!     recovery_timeout: Duration::from_secs(30),
//!     half_open_max_calls: 3,
//! };
//!
//! let client = UltrafastClient::standalone()
//!     .with_openai("key")
//!     .with_circuit_breaker_config(config)
//!     .build()?;
//! ```
//!
//! ## Caching
//!
//! Built-in response caching:
//!
//! ```rust
//! let client = UltrafastClient::standalone()
//!     .with_openai("key")
//!     .with_caching(true)
//!     .with_cache_ttl(Duration::from_secs(3600))
//!     .build()?;
//! ```
//!
//! ## Error Handling
//!
//! Comprehensive error handling with retry logic:
//!
//! ```rust
//! use ultrafast_models_sdk::{ClientError, ProviderError};
//!
//! match client.chat_completion(request).await {
//!     Ok(response) => println!("Success: {}", response.choices[0].message.content),
//!     Err(ClientError::Provider(ProviderError::RateLimit { .. })) => {
//!         println!("Rate limited, retrying...");
//!     }
//!     Err(ClientError::Provider(ProviderError::Timeout { .. })) => {
//!         println!("Request timed out");
//!     }
//!     Err(e) => println!("Error: {}", e),
//! }
//! ```
//!
//! ## Models and Requests
//!
//! The SDK supports various AI model types:
//!
//! - **Chat Completions**: Conversational AI models
//! - **Text Completions**: Text generation models
//! - **Embeddings**: Text embedding models
//! - **Image Generation**: Image creation models
//! - **Audio Transcription**: Speech-to-text models
//! - **Text-to-Speech**: Text-to-speech models
//!
//! ## Performance Features
//!
//! - **Connection Pooling**: Reusable HTTP connections
//! - **Request Batching**: Batch multiple requests
//! - **Streaming Support**: Real-time response streaming
//! - **Async/Await**: Non-blocking operations
//! - **Memory Efficient**: Minimal memory footprint
//!
//! ## Configuration
//!
//! The SDK is highly configurable:
//!
//! - **Timeouts**: Per-request and per-provider timeouts
//! - **Retry Logic**: Configurable retry strategies
//! - **Rate Limiting**: Per-provider rate limits
//! - **Logging**: Structured logging support
//! - **Metrics**: Performance monitoring
//!
//! ## License
//!
//! This project is licensed under either of
//!
//! * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or https://www.apache.org/licenses/LICENSE-2.0)
//! * MIT license ([LICENSE-MIT](LICENSE-MIT) or https://opensource.org/licenses/MIT)
//!
//! at your option.

pub mod cache;
pub mod circuit_breaker;
pub mod client;
pub mod common;
pub mod error;
pub mod models;
pub mod providers;
pub mod routing;

pub use circuit_breaker::{CircuitBreaker, CircuitBreakerConfig, CircuitState};
pub use client::{ClientMode, UltrafastClient, UltrafastClientBuilder};
pub use error::{ClientError, ProviderError};
pub use models::{
    AudioRequest, AudioResponse, ChatRequest, ChatResponse, Choice, EmbeddingRequest,
    EmbeddingResponse, ImageRequest, ImageResponse, Message, Role, SpeechRequest, SpeechResponse,
    Usage,
};
pub use providers::{
    create_provider_with_circuit_breaker, Provider, ProviderConfig, ProviderMetrics,
};
pub use routing::{Condition, RoutingRule, RoutingStrategy};

/// Result type for SDK operations.
///
/// This is a convenience type alias for SDK operations that can fail.
/// It uses `ClientError` as the error type.
pub type Result<T> = std::result::Result<T, ClientError>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ChatRequest, Message, Role};

    #[test]
    fn test_message_creation() {
        let user_msg = Message::user("Hello, world!");
        assert_eq!(user_msg.role, Role::User);
        assert_eq!(user_msg.content, "Hello, world!");

        let assistant_msg = Message::assistant("Hi there!");
        assert_eq!(assistant_msg.role, Role::Assistant);
        assert_eq!(assistant_msg.content, "Hi there!");

        let system_msg = Message::system("You are a helpful assistant.");
        assert_eq!(system_msg.role, Role::System);
        assert_eq!(system_msg.content, "You are a helpful assistant.");
    }

    #[test]
    fn test_chat_request_default() {
        let request = ChatRequest::default();
        assert_eq!(request.model, "");
        assert_eq!(request.messages.len(), 0);
        assert_eq!(request.temperature, None);
        assert_eq!(request.max_tokens, None);
        assert_eq!(request.stream, None);
    }

    #[test]
    fn test_provider_config_creation() {
        let config = ProviderConfig::new("test-provider", "test-key");
        assert_eq!(config.name, "test-provider");
        assert_eq!(config.api_key, "test-key");
        assert_eq!(config.timeout.as_secs(), 30);
        assert_eq!(config.max_retries, 3);
        assert!(config.enabled);
    }

    #[test]
    fn test_routing_strategy_creation() {
        let single = RoutingStrategy::Single;
        let fallback = RoutingStrategy::Fallback;
        let load_balance = RoutingStrategy::LoadBalance {
            weights: vec![0.5, 0.5],
        };
        let conditional = RoutingStrategy::Conditional { rules: vec![] };
        let ab_testing = RoutingStrategy::ABTesting { split: 0.5 };

        assert!(matches!(single, RoutingStrategy::Single));
        assert!(matches!(fallback, RoutingStrategy::Fallback));
        assert!(matches!(load_balance, RoutingStrategy::LoadBalance { .. }));
        assert!(matches!(conditional, RoutingStrategy::Conditional { .. }));
        assert!(matches!(ab_testing, RoutingStrategy::ABTesting { .. }));
    }

    #[test]
    fn test_condition_matching() {
        let context = routing::RoutingContext {
            model: Some("gpt-4".to_string()),
            user_region: Some("us-east-1".to_string()),
            request_size: 1000,
            estimated_tokens: 500,
            user_id: Some("user123".to_string()),
            metadata: std::collections::HashMap::new(),
        };

        let model_condition = Condition::ModelName("gpt-4".to_string());
        assert!(model_condition.matches(&context));

        let region_condition = Condition::UserRegion("us-east-1".to_string());
        assert!(region_condition.matches(&context));

        let size_condition = Condition::RequestSize(500);
        assert!(size_condition.matches(&context));

        let token_condition = Condition::TokenCount(300);
        assert!(token_condition.matches(&context));
    }
}
