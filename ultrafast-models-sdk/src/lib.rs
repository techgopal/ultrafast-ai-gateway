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
//! // Load balancing with custom weights
//! let client = UltrafastClient::standalone()
//!     .with_openai("openai-key")
//!     .with_anthropic("anthropic-key")
//!     .with_routing_strategy(RoutingStrategy::LoadBalance {
//!         weights: vec![0.6, 0.4], // 60% OpenAI, 40% Anthropic
//!     })
//!     .build()?;
//!
//! // Failover strategy
//! let client = UltrafastClient::standalone()
//!     .with_openai("primary-key")
//!     .with_anthropic("fallback-key")
//!     .with_routing_strategy(RoutingStrategy::Failover)
//!     .build()?;
//! ```
//!
//! ## Advanced Features
//!
//! ### Circuit Breakers
//!
//! Automatic failover and recovery:
//!
//! ```rust
//! use ultrafast_models_sdk::circuit_breaker::CircuitBreakerConfig;
//!
//! let client = UltrafastClient::standalone()
//!     .with_openai("your-key")
//!     .with_circuit_breaker_config(CircuitBreakerConfig {
//!         failure_threshold: 5,
//!         recovery_timeout: Duration::from_secs(60),
//!         request_timeout: Duration::from_secs(30),
//!         half_open_max_calls: 3,
//!     })
//!     .build()?;
//! ```
//!
//! ### Caching
//!
//! Built-in response caching:
//!
//! ```rust
//! use ultrafast_models_sdk::cache::CacheConfig;
//!
//! let client = UltrafastClient::standalone()
//!     .with_openai("your-key")
//!     .with_cache_config(CacheConfig {
//!         enabled: true,
//!         ttl: Duration::from_hours(1),
//!         max_size: 1000,
//!     })
//!     .build()?;
//! ```
//!
//! ### Rate Limiting
//!
//! Per-provider rate limiting:
//!
//! ```rust
//! use ultrafast_models_sdk::rate_limiting::RateLimitConfig;
//!
//! let client = UltrafastClient::standalone()
//!     .with_openai("your-key")
//!     .with_rate_limit_config(RateLimitConfig {
//!         requests_per_minute: 100,
//!         tokens_per_minute: 10000,
//!         burst_size: 10,
//!     })
//!     .build()?;
//! ```
//!
//! ## API Examples
//!
//! ### Chat Completions
//!
//! ```rust
//! use ultrafast_models_sdk::{ChatRequest, Message, Role};
//!
//! let request = ChatRequest {
//!     model: "gpt-4".to_string(),
//!     messages: vec![
//!         Message {
//!             role: Role::System,
//!             content: "You are a helpful assistant.".to_string(),
//!         },
//!         Message {
//!             role: Role::User,
//!             content: "What is the capital of France?".to_string(),
//!         },
//!     ],
//!     temperature: Some(0.7),
//!     max_tokens: Some(150),
//!     stream: Some(false),
//!     ..Default::default()
//! };
//!
//! let response = client.chat_completion(request).await?;
//! println!("Response: {}", response.choices[0].message.content);
//! ```
//!
//! ### Streaming Responses
//!
//! ```rust
//! use futures::StreamExt;
//!
//! let mut stream = client
//!     .stream_chat_completion(ChatRequest {
//!         model: "gpt-4".to_string(),
//!         messages: vec![Message::user("Tell me a story")],
//!         stream: Some(true),
//!         ..Default::default()
//!     })
//!     .await?;
//!
//! while let Some(chunk) = stream.next().await {
//!     match chunk {
//!         Ok(chunk) => {
//!             if let Some(content) = &chunk.choices[0].delta.content {
//!                 print!("{}", content);
//!             }
//!         }
//!         Err(e) => eprintln!("Error: {:?}", e),
//!     }
//! }
//! ```
//!
//! ### Embeddings
//!
//! ```rust
//! use ultrafast_models_sdk::{EmbeddingRequest, EmbeddingInput};
//!
//! let request = EmbeddingRequest {
//!     model: "text-embedding-ada-002".to_string(),
//!     input: EmbeddingInput::String("This is a test sentence.".to_string()),
//!     ..Default::default()
//! };
//!
//! let response = client.embedding(request).await?;
//! println!("Embedding dimensions: {}", response.data[0].embedding.len());
//! ```
//!
//! ### Image Generation
//!
//! ```rust
//! use ultrafast_models_sdk::ImageGenerationRequest;
//!
//! let request = ImageGenerationRequest {
//!     model: "dall-e-3".to_string(),
//!     prompt: "A beautiful sunset over the ocean".to_string(),
//!     n: Some(1),
//!     size: Some("1024x1024".to_string()),
//!     ..Default::default()
//! };
//!
//! let response = client.generate_image(request).await?;
//! println!("Image URL: {}", response.data[0].url);
//! ```
//!
//! ## Error Handling
//!
//! Comprehensive error handling with specific error types:
//!
//! ```rust
//! use ultrafast_models_sdk::error::UltrafastError;
//!
//! match client.chat_completion(request).await {
//!     Ok(response) => println!("Success: {:?}", response),
//!     Err(UltrafastError::AuthenticationError { .. }) => {
//!         eprintln!("Authentication failed");
//!     }
//!     Err(UltrafastError::RateLimitExceeded { retry_after, .. }) => {
//!         eprintln!("Rate limit exceeded, retry after: {:?}", retry_after);
//!     }
//!     Err(UltrafastError::ProviderError { provider, message, .. }) => {
//!         eprintln!("Provider {} error: {}", provider, message);
//!     }
//!     Err(e) => eprintln!("Other error: {:?}", e),
//! }
//! ```
//!
//! ## Configuration
//!
//! Advanced client configuration:
//!
//! ```rust
//! use ultrafast_models_sdk::{UltrafastClient, ClientConfig};
//! use std::time::Duration;
//!
//! let config = ClientConfig {
//!     timeout: Duration::from_secs(30),
//!     max_retries: 3,
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
//! ## Testing
//!
//! The SDK includes testing utilities:
//!
//! ```rust
//! #[cfg(test)]
//! mod tests {
//!     use super::*;
//!     use tokio_test;
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
//!         assert!(result.is_ok());
//!     }
//! }
//! ```
//!
//! ## Performance Optimization
//!
//! Tips for optimal performance:
//!
//! ```rust
//! // Use connection pooling
//! let client = UltrafastClient::standalone()
//!     .with_connection_pool_size(10)
//!     .with_openai("your-key")
//!     .build()?;
//!
//! // Enable compression
//! let client = UltrafastClient::standalone()
//!     .with_compression(true)
//!     .with_openai("your-key")
//!     .build()?;
//!
//! // Configure timeouts
//! let client = UltrafastClient::standalone()
//!     .with_timeout(Duration::from_secs(15))
//!     .with_openai("your-key")
//!     .build()?;
//! ```
//!
//! ## Migration Guide
//!
//! ### From OpenAI SDK
//!
//! ```rust
//! // Before (OpenAI SDK)
//! use openai::Client;
//! let client = Client::new("your-key");
//! let response = client.chat().create(request).await?;
//!
//! // After (Ultrafast SDK)
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
//! // Before (Anthropic SDK)
//! use anthropic::Client;
//! let client = Client::new("your-key");
//! let response = client.messages().create(request).await?;
//!
//! // After (Ultrafast SDK)
//! use ultrafast_models_sdk::UltrafastClient;
//! let client = UltrafastClient::standalone()
//!     .with_anthropic("your-key")
//!     .build()?;
//! let response = client.chat_completion(request).await?;
//! ```
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
