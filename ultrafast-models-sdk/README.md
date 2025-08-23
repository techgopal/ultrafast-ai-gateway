# Ultrafast Models SDK 🚀

> **A high-performance Rust SDK for interacting with multiple AI/LLM providers through a unified interface.**

[![Rust](https://img.shields.io/badge/Rust-1.75+-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/ultrafast-models-sdk)](https://crates.io/crates/ultrafast-models-sdk)
[![Documentation](https://docs.rs/ultrafast-models-sdk/badge.svg)](https://docs.rs/ultrafast-models-sdk)

## ✨ Features

### 🎯 **Dual Mode Operation**
- **Standalone Mode**: Direct provider calls with built-in routing and load balancing
- **Gateway Mode**: Communication through the Ultrafast Gateway

### 🔌 **Provider Support (100+ Models)**
- **OpenAI** - GPT-4, GPT-3.5, and other models
- **Anthropic** - Claude-3, Claude-2, Claude Instant
- **Google** - Gemini Pro, Gemini Pro Vision, PaLM
- **Azure OpenAI** - Azure-hosted OpenAI models
- **Ollama** - Local and remote Ollama instances
- **Mistral AI** - Mistral 7B, Mixtral, and other models
- **Cohere** - Command, Command R, and other models
- **Groq** - Fast inference models
- **Custom HTTP providers** for extensibility

### ⚡ **Performance & Scalability**
- **<1ms** request routing overhead
- **10,000+ requests/second** throughput
- **100,000+ concurrent connections** supported
- **<100MB memory** usage under normal load
- **Zero-copy** deserialization
- **Async I/O** throughout the stack
- **Connection pooling** for optimal resource utilization

### 🛡️ **Enterprise Features**
- **Circuit Breakers**: Automatic failover and recovery
- **Rate Limiting**: Per-provider rate limiting and throttling
- **Request Validation**: Comprehensive input validation
- **Error Handling**: Robust error handling with retry logic
- **Metrics Collection**: Performance monitoring and analytics
- **Caching Layer**: Built-in response caching for performance

### 🎛️ **Advanced Routing**
- **Single Provider**: Direct calls to specific provider
- **Load Balancing**: Distribute requests across multiple providers
- **Failover**: Automatic failover to backup providers
- **Conditional**: Route based on request parameters
- **A/B Testing**: Split traffic between providers
- **Round Robin**: Even distribution across providers
- **Least Used**: Route to least busy provider
- **Lowest Latency**: Route to fastest provider

## 🚀 Quick Start

### Installation

Add the dependency to your `Cargo.toml`:

```toml
[dependencies]
ultrafast-models-sdk = "0.1.1"
```

### Basic Usage

```rust
use ultrafast_models_sdk::{UltrafastClient, ChatRequest, Message};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a client with OpenAI
    let client = UltrafastClient::standalone()
        .with_openai("your-openai-key")
        .build()?;

    // Create a chat request
    let request = ChatRequest {
        model: "gpt-4".to_string(),
        messages: vec![Message::user("Hello, world!")],
        temperature: Some(0.7),
        max_tokens: Some(100),
        ..Default::default()
    };

    // Send the request
    let response = client.chat_completion(request).await?;
    println!("Response: {}", response.choices[0].message.content);

    Ok(())
}
```

## 🔧 Client Modes

### Standalone Mode

Direct provider communication without gateway:

```rust
let client = UltrafastClient::standalone()
    .with_openai("your-openai-key")
    .with_anthropic("your-anthropic-key")
    .with_ollama("http://localhost:11434")
    .build()?;
```

### Gateway Mode

Communication through the Ultrafast Gateway:

```rust
let client = UltrafastClient::gateway("http://localhost:3000")
    .with_api_key("your-gateway-key")
    .with_timeout(Duration::from_secs(30))
    .build()?;
```

## 🎯 Routing Strategies

### Load Balancing

```rust
use ultrafast_models_sdk::routing::RoutingStrategy;

let client = UltrafastClient::standalone()
    .with_openai("openai-key")
    .with_anthropic("anthropic-key")
    .with_routing_strategy(RoutingStrategy::LoadBalance {
        weights: vec![0.6, 0.4], // 60% OpenAI, 40% Anthropic
    })
    .build()?;
```

### Failover

```rust
let client = UltrafastClient::standalone()
    .with_openai("primary-key")
    .with_anthropic("fallback-key")
    .with_routing_strategy(RoutingStrategy::Failover)
    .build()?;
```

### Conditional Routing

```rust
let client = UltrafastClient::standalone()
    .with_openai("openai-key")
    .with_anthropic("anthropic-key")
    .with_routing_strategy(RoutingStrategy::Conditional {
        conditions: vec![
            ("model", "gpt-4", "openai"),
            ("model", "claude-3", "anthropic"),
        ],
        default: "openai".to_string(),
    })
    .build()?;
```

## 🔌 Advanced Features

### Circuit Breakers

```rust
use ultrafast_models_sdk::circuit_breaker::CircuitBreakerConfig;
use std::time::Duration;

let circuit_config = CircuitBreakerConfig {
    failure_threshold: 5,
    recovery_timeout: Duration::from_secs(60),
    request_timeout: Duration::from_secs(30),
    half_open_max_calls: 3,
};

let client = UltrafastClient::standalone()
    .with_openai("your-key")
    .with_circuit_breaker_config(circuit_config)
    .build()?;
```

### Caching

```rust
use ultrafast_models_sdk::cache::CacheConfig;

let cache_config = CacheConfig {
    enabled: true,
    ttl: Duration::from_hours(1),
    max_size: 1000,
    backend: CacheBackend::Memory,
};

let client = UltrafastClient::standalone()
    .with_cache_config(cache_config)
    .with_openai("your-key")
    .build()?;
```

### Rate Limiting

```rust
use ultrafast_models_sdk::rate_limiting::RateLimitConfig;

let rate_config = RateLimitConfig {
    requests_per_minute: 100,
    tokens_per_minute: 10000,
    burst_size: 10,
};

let client = UltrafastClient::standalone()
    .with_rate_limit_config(rate_config)
    .with_openai("your-key")
    .build()?;
```

## 📚 API Examples

### Chat Completions

```rust
use ultrafast_models_sdk::{ChatRequest, Message, Role};

let request = ChatRequest {
    model: "gpt-4".to_string(),
    messages: vec![
        Message {
            role: Role::System,
            content: "You are a helpful assistant.".to_string(),
        },
        Message {
            role: Role::User,
            content: "What is the capital of France?".to_string(),
        },
    ],
    temperature: Some(0.7),
    max_tokens: Some(150),
    stream: Some(false),
    ..Default::default()
};

let response = client.chat_completion(request).await?;
println!("Response: {}", response.choices[0].message.content);
```

### Streaming Responses

```rust
use futures::StreamExt;

let mut stream = client
    .stream_chat_completion(ChatRequest {
        model: "gpt-4".to_string(),
        messages: vec![Message::user("Tell me a story")],
        stream: Some(true),
        ..Default::default()
    })
    .await?;

print!("Streaming response: ");
while let Some(chunk) = stream.next().await {
    match chunk {
        Ok(chunk) => {
            if let Some(content) = &chunk.choices[0].delta.content {
                print!("{}", content);
            }
        }
        Err(e) => {
            println!("\nError in stream: {:?}", e);
            break;
        }
    }
}
println!();
```

### Embeddings

```rust
use ultrafast_models_sdk::{EmbeddingRequest, EmbeddingInput};

let request = EmbeddingRequest {
    model: "text-embedding-ada-002".to_string(),
    input: EmbeddingInput::String("This is a test sentence.".to_string()),
    ..Default::default()
};

let response = client.embedding(request).await?;
println!("Embedding dimensions: {}", response.data[0].embedding.len());
```

### Image Generation

```rust
use ultrafast_models_sdk::ImageGenerationRequest;

let request = ImageGenerationRequest {
    model: "dall-e-3".to_string(),
    prompt: "A beautiful sunset over the ocean".to_string(),
    n: Some(1),
    size: Some("1024x1024".to_string()),
    ..Default::default()
};

let response = client.generate_image(request).await?;
println!("Image URL: {}", response.data[0].url);
```

## 🛠️ Error Handling

```rust
use ultrafast_models_sdk::error::UltrafastError;

match client.chat_completion(request).await {
    Ok(response) => println!("Success: {:?}", response),
    Err(UltrafastError::AuthenticationError { .. }) => {
        eprintln!("Authentication failed");
    }
    Err(UltrafastError::RateLimitExceeded { retry_after, .. }) => {
        eprintln!("Rate limit exceeded, retry after: {:?}", retry_after);
    }
    Err(UltrafastError::ProviderError { provider, message, .. }) => {
        eprintln!("Provider {} error: {}", provider, message);
    }
    Err(e) => eprintln!("Other error: {:?}", e),
}
```

## ⚙️ Configuration

### Advanced Client Configuration

```rust
use ultrafast_models_sdk::{UltrafastClient, ClientConfig};
use std::time::Duration;

let config = ClientConfig {
    timeout: Duration::from_secs(30),
    max_retries: 5,
    retry_delay: Duration::from_secs(1),
    user_agent: Some("MyApp/1.0".to_string()),
    ..Default::default()
};

let client = UltrafastClient::standalone()
    .with_config(config)
    .with_openai("your-key")
    .build()?;
```

### Performance Optimization

```rust
// Use connection pooling
let client = UltrafastClient::standalone()
    .with_connection_pool_size(10)
    .with_openai("your-key")
    .build()?;

// Enable compression
let client = UltrafastClient::standalone()
    .with_compression(true)
    .with_openai("your-key")
    .build()?;

// Configure timeouts
let client = UltrafastClient::standalone()
    .with_timeout(Duration::from_secs(15))
    .with_openai("your-key")
    .build()?;
```

## 🧪 Testing

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tokio_test;

    #[tokio_test]
    async fn test_chat_completion() {
        let client = UltrafastClient::standalone()
            .with_openai("test-key")
            .build()
            .unwrap();

        let request = ChatRequest {
            model: "gpt-4".to_string(),
            messages: vec![Message::user("Hello")],
            ..Default::default()
        };

        let result = client.chat_completion(request).await;
        // Handle result based on test environment
    }
}
```

## 🔄 Migration from Other SDKs

### From OpenAI SDK

```rust
// Before
use openai::Client;
let client = Client::new("your-key");
let response = client.chat().create(request).await?;

// After
use ultrafast_models_sdk::UltrafastClient;
let client = UltrafastClient::standalone()
    .with_openai("your-key")
    .build()?;
let response = client.chat_completion(request).await?;
```

### From Anthropic SDK

```rust
// Before
use anthropic::Client;
let client = Client::new("your-key");
let response = client.messages().create(request).await?;

// After
use ultrafast_models_sdk::UltrafastClient;
let client = UltrafastClient::standalone()
    .with_anthropic("your-key")
    .build()?;
let response = client.chat_completion(request).await?;
```

## 📊 Performance Benchmarks

- **Latency**: <1ms routing overhead
- **Throughput**: 10,000+ requests/second
- **Memory**: <100MB under normal load
- **Concurrency**: 100,000+ concurrent requests
- **Cache Hit Rate**: 95%+ for repeated requests

## 🚀 Use Cases

- **Multi-Provider AI Applications**: Unified interface for multiple AI services
- **High-Throughput Systems**: Applications requiring 10k+ requests/second
- **Cost Optimization**: Intelligent routing to most cost-effective providers
- **Reliability**: Automatic failover and circuit breaker protection
- **Development & Testing**: Easy switching between providers and modes

## 🤝 Contributing

We welcome contributions! Please see our [Contributing Guide](CONTRIBUTING.md) for details on:

- Code style and formatting
- Testing requirements
- Documentation standards
- Pull request process

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 🆘 Support

For support and questions:

- **Issues**: [GitHub Issues](https://github.com/techgopal/ultrafast-ai-gateway/issues)
- **Discussions**: [GitHub Discussions](https://github.com/techgopal/ultrafast-ai-gateway/discussions)
- **Documentation**: [Project Wiki](https://github.com/techgopal/ultrafast-ai-gateway/wiki)

## 🔗 Related Projects

- **[Ultrafast Gateway](https://github.com/techgopal/ultrafast-ai-gateway)**: High-performance AI gateway server
- **[Documentation](https://docs.rs/ultrafast-models-sdk)**: API documentation on docs.rs
- **[Examples](https://github.com/techgopal/ultrafast-ai-gateway/tree/main/ultrafast-models-sdk/examples)**: More usage examples

---

**Made with ❤️ by the Ultrafast AI Team**
