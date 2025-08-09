# Ultrafast Gateway 🚀

> **A high-performance AI gateway built in Rust** that provides a unified interface to 10+ LLM providers with advanced routing, caching, and monitoring capabilities.

[![Rust](https://img.shields.io/badge/Rust-1.75+-orange.svg)](LICENSE)
[![License](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Build Status](https://img.shields.io/github/actions/workflow/status/ultrafast-ai/ultrafast-gateway/ci.yml?branch=main)](https://github.com/ultrafast-ai/ultrafast-gateway/actions)


## ✨ Features

### 🎯 **Dual Mode Operation**
- **Standalone Mode**: Direct provider calls with built-in routing and load balancing
- **Gateway Mode**: Centralized server with unified OpenAI-compatible API endpoints

### 🔌 **Provider Support (100+ Models)**
- **OpenAI** 
- **Anthropic** 
- **Azure OpenAI** 
- **Google Vertex AI** 
- **Cohere** 
- **Groq** 
- **Mistral AI** 
- **Perplexity AI** 
- **Together AI** 
- **Ollama** 
- **Custom HTTP providers**

### ⚡ **Performance & Scalability**
- **<1ms** request routing overhead
- **10,000+ requests/second** throughput
- **100,000+ concurrent connections** supported
- **<1GB memory** usage under normal load
- **99.9% uptime** with automatic failover
- **Zero-copy** deserialization
- **Async I/O** throughout the stack
- **Connection pooling** for optimal resource utilization

### 🛡️ **Enterprise Features**
- **Authentication** with virtual API keys and JWT tokens
- **Rate limiting** per user/provider with sliding windows
- **Request validation** with comprehensive schemas
- **Content filtering** with plugin system
- **Cost tracking** and analytics
- **Real-time metrics** and monitoring
- **Circuit breakers** for fault tolerance and automatic failover
- **Horizontal scaling** with Redis-based session storage

### 🎛️ **Advanced Routing**
- **Single Provider**: Direct calls to specific provider
- **Load Balancing**: Distribute requests across multiple providers
- **Fallback**: Automatic failover to backup providers
- **Conditional**: Route based on request parameters (model, region, size)
- **A/B Testing**: Split traffic between providers
- **Round Robin**: Even distribution across providers
- **Least Used**: Route to least busy provider
- **Lowest Latency**: Route to fastest provider

## 🖥️ Dashboard

Below is a preview of the built-in monitoring dashboard showing live metrics, costs, and provider breakdowns.

![Ultrafast Gateway Dashboard](./docs/images/dashboard.png)

> Tip: Open the dashboard at `/dashboard` when the gateway is running.

## 🚀 Quick Start

### Installation

```bash
# Clone the repository
git clone https://github.com/ultrafast-ai/ultrafast-gateway.git
cd ultrafast-gateway

# Build the project
cargo build --release

# Run the gateway server
cargo run --bin ultrafast-gateway -- --config config.toml
```

### Environment Setup

```bash
# Set your API keys
export OPENAI_API_KEY="sk-your-openai-key"
export ANTHROPIC_API_KEY="sk-ant-your-anthropic-key"
export GATEWAY_API_KEYS='[{"key":"sk-ultrafast-gateway-key","name":"default","enabled":true}]'

# Optional: Set JWT secret for stateless authentication
export GATEWAY_JWT_SECRET="your-secret-key"
```

## 📖 Usage Examples

### Standalone Mode (Direct Provider Calls)

```rust
use ultrafast_models_sdk::{UltrafastClient, ChatRequest, Message, RoutingStrategy};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create standalone client with multiple providers
    let client = UltrafastClient::standalone()
        .with_openai("sk-your-openai-key")
        .with_anthropic("sk-ant-your-anthropic-key")
        .with_azure_openai("your-azure-key", "gpt-4-deployment")
        .with_google_vertex_ai("your-google-key", "your-project-id")
        .with_routing_strategy(RoutingStrategy::LoadBalance {
            weights: vec![0.4, 0.3, 0.2, 0.1],
        })
        .build()?;

    // Make a request (automatically routed to best available provider)
    let response = client.chat_completion(ChatRequest {
        model: "gpt-4".to_string(),
        messages: vec![Message::user("Hello! What is the capital of France?")],
        max_tokens: Some(100),
        temperature: Some(0.7),
        ..Default::default()
    }).await?;

    println!("Response: {}", response.choices[0].message.content);
    Ok(())
}
```

### Gateway Mode (Server Calls)

```rust
use ultrafast_models_sdk::{UltrafastClient, ChatRequest, Message};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create gateway client
    let client = UltrafastClient::gateway("http://localhost:3000".to_string())
        .with_api_key("sk-ultrafast-gateway-key")
        .with_timeout(Duration::from_secs(30))
        .build()?;

    // Make requests through the gateway
    let response = client.chat_completion(ChatRequest {
        model: "gpt-4".to_string(),
        messages: vec![Message::user("Hello! What is the capital of France?")],
        max_tokens: Some(100),
        temperature: Some(0.7),
        ..Default::default()
    }).await?;

    println!("Response: {}", response.choices[0].message.content);
    Ok(())
}
```

### Streaming Responses

```rust
use futures::StreamExt;

// Streaming chat completion
let mut stream = client
    .stream_chat_completion(ChatRequest {
        model: "gpt-4".to_string(),
        messages: vec![Message::user("Write a short story about a robot learning to paint")],
        max_tokens: Some(300),
        temperature: Some(0.8),
        stream: Some(true),
        ..Default::default()
    })
    .await?;

print!("Streaming response: ");
while let Some(chunk_result) = stream.next().await {
    match chunk_result {
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

let embedding_response = client
    .embedding(EmbeddingRequest {
        model: "text-embedding-ada-002".to_string(),
        input: EmbeddingInput::String("This is a test sentence for embeddings.".to_string()),
        ..Default::default()
    })
    .await?;

println!(
    "Embedding dimensions: {}",
    embedding_response.data[0].embedding.len()
);
```

## ⚙️ Configuration

### Basic Configuration

```toml
[server]
host = "127.0.0.1"
port = 3000
timeout = "30s"
max_body_size = 10485760  # 10MB
cors = { enabled = true, allowed_origins = ["*"] }

# OpenAI Provider
[providers.openai]
name = "openai"
api_key = ""  # Loaded from OPENAI_API_KEY environment variable
base_url = "https://api.openai.com/v1"
timeout = "30s"
max_retries = 3
retry_delay = "1s"
enabled = true
model_mapping = {}
headers = {}

# Anthropic Provider
[providers.anthropic]
name = "anthropic"
api_key = ""  # Loaded from ANTHROPIC_API_KEY environment variable
base_url = "https://api.anthropic.com"
timeout = "30s"
max_retries = 3
retry_delay = "1s"
enabled = true
model_mapping = {}
headers = {}

# Authentication
[auth]
enabled = true
api_keys = []  # Loaded from GATEWAY_API_KEYS environment variable
rate_limiting = { 
    requests_per_minute = 1000, 
    requests_per_hour = 10000, 
    tokens_per_minute = 100000 
}

# Caching
[cache]
enabled = true
backend = "Memory"  # or "Redis"
ttl = "1h"
max_size = 1000

# Routing
[routing]
strategy = { LoadBalance = { weights = [1.0, 1.0] } }
health_check_interval = "30s"
failover_threshold = 0.8

# Metrics
[metrics]
enabled = true
max_requests = 10000
retention_duration = "1h"
cleanup_interval = "5m"

# Logging
[logging]
level = "info"
format = "Pretty"  # Pretty, Json, or Compact
output = "Stdout"  # Stdout or File
```

### Advanced Configuration

```toml
# Circuit Breaker Configuration
[providers.openai]
name = "openai"
api_key = "sk-your-key"
base_url = "https://api.openai.com/v1"
timeout = "30s"
enabled = true

# Circuit breaker configuration
circuit_breaker = { 
    failure_threshold = 5,        # Number of failures before opening circuit
    recovery_timeout = "60s",     # Time to wait before trying recovery
    request_timeout = "30s",      # Timeout for individual requests
    half_open_max_calls = 3       # Max calls in half-open state
}

# Rate Limiting per Provider
[providers.openai]
rate_limit = { 
    requests_per_minute = 1000, 
    tokens_per_minute = 100000 
}

# Model Mapping
[providers.azure-openai]
name = "azure-openai"
api_key = "your-azure-key"
base_url = "https://your-resource.openai.azure.com"
model_mapping = { 
    "gpt-4" = "gpt-4-deployment",
    "gpt-3.5-turbo" = "gpt-35-turbo-deployment"
}
headers = { "api-version" = "2024-02-15-preview" }

# Plugins
[[plugins]]
name = "cost_tracking"
enabled = true
config = { "track_costs" = true, "detailed_tracking" = true }

[[plugins]]
name = "content_filtering"
enabled = true
config = { 
    "filter_level" = "medium",
    "blocked_words" = ["spam", "inappropriate"],
    "max_input_length" = 10000 
}
```

## 🔌 API Reference

### Authentication

#### API Key Authentication
```bash
curl -X POST http://localhost:3000/v1/chat/completions \
  -H "Authorization: Bearer sk-ultrafast-gateway-key" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

#### JWT Token Authentication
```bash
# First, get a JWT token (if your gateway supports it)
curl -X POST http://localhost:3000/auth/token \
  -H "Authorization: Bearer sk-ultrafast-gateway-key"

# Use the JWT token for subsequent requests
curl -X POST http://localhost:3000/v1/chat/completions \
  -H "Authorization: Bearer eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9..." \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4",
    "messages": [{"role": "user", "content": "Hello!"}]
  }'
```

### Chat Completions

```bash
curl -X POST http://localhost:3000/v1/chat/completions \
  -H "Authorization: Bearer sk-ultrafast-gateway-key" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4",
    "messages": [
      {"role": "system", "content": "You are a helpful assistant."},
      {"role": "user", "content": "What is the capital of France?"}
    ],
    "max_tokens": 100,
    "temperature": 0.7,
    "stream": false
  }'
```

### Streaming Responses

```bash
curl -X POST http://localhost:3000/v1/chat/completions \
  -H "Authorization: Bearer sk-ultrafast-gateway-key" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4",
    "messages": [{"role": "user", "content": "Tell me a story"}],
    "stream": true
  }'
```

### Embeddings

```bash
curl -X POST http://localhost:3000/v1/embeddings \
  -H "Authorization: Bearer sk-ultrafast-gateway-key" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "text-embedding-ada-002",
    "input": "This is a test sentence for embeddings."
  }'
```

### Image Generation

```bash
curl -X POST http://localhost:3000/v1/images/generations \
  -H "Authorization: Bearer sk-ultrafast-gateway-key" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "dall-e-3",
    "prompt": "A beautiful sunset over the ocean",
    "n": 1,
    "size": "1024x1024"
  }'
```

### Audio Transcription

```bash
curl -X POST http://localhost:3000/v1/audio/transcriptions \
  -H "Authorization: Bearer sk-ultrafast-gateway-key" \
  -F "file=@audio.mp3" \
  -F "model=whisper-1"
```

### Health & Monitoring

```bash
# Health check
curl http://localhost:3000/health

# JSON metrics
curl http://localhost:3000/metrics

# Prometheus metrics
curl http://localhost:3000/metrics/prometheus

# Circuit breaker metrics
curl http://localhost:3000/admin/circuit-breakers

# List providers
curl http://localhost:3000/admin/providers

# Get configuration
curl http://localhost:3000/admin/config
```


## 🧪 Testing

### Running Tests

```bash
# Run all tests
cargo test

# Run specific test modules
cargo test --test integration_tests
cargo test --test unit_tests

# Run with coverage
cargo tarpaulin

# Run benchmarks
cargo bench
```

### Test Coverage

The project includes comprehensive test coverage for:
- **Unit tests** for all core components
- **Integration tests** for provider connectivity
- **Performance benchmarks** for critical paths
- **Error scenario testing** for fault tolerance
- **Circuit breaker testing** for reliability

## 🤝 Contributing

We welcome contributions! Please see our [Contributing Guide](CONTRIBUTING.md) for details.

### Development Setup

```bash
# Clone and setup
git clone https://github.com/ultrafast-ai/ultrafast-gateway.git
cd ultrafast-gateway

# Install dependencies
cargo build

# Run tests
cargo test

# Run benchmarks
cargo bench

# Format code
cargo fmt

# Check code quality
cargo clippy

# Run comprehensive tests
./run_comprehensive_tests.sh
```

### Code Quality

- **Rust 1.75+** required
- **Clippy** for linting
- **rustfmt** for code formatting
- **Tarpaulin** for test coverage
- **Criterion** for benchmarking


## 📈 Roadmap

### v0.2.0 
- [ ] GraphQL API support
- [ ] WebSocket streaming
- [ ] Advanced caching strategies
- [ ] Plugin marketplace
- [ ] Multi-region deployment support

### v0.3.0
- [ ] Advanced analytics dashboard
- [ ] Custom model fine-tuning support
- [ ] Enterprise SSO integration
- [ ] Real-time collaboration features


## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.


---

**Made with ❤️ by the Ultrafast AI Team** 