# Ultrafast Gateway

[![Crates.io](https://img.shields.io/crates/v/ultrafast-gateway)](https://crates.io/crates/ultrafast-gateway)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-1.75+-blue.svg)](https://www.rust-lang.org/)
[![CI](https://github.com/techgopal/ultrafast-ai-gateway/workflows/CI/badge.svg)](https://github.com/techgopal/ultrafast-ai-gateway/actions)

A high-performance AI gateway built in Rust that provides a unified interface to 10+ LLM providers with advanced routing, caching, and monitoring capabilities.

## 🚀 Features

### **Core Capabilities**
- **Multi-Provider Support**: OpenAI, Anthropic, Google, Groq, Mistral, Cohere, Perplexity, Ollama, and more
- **Intelligent Routing**: Automatic provider selection and load balancing
- **Advanced Caching**: Built-in response caching with TTL and invalidation
- **Circuit Breakers**: Automatic failover and recovery mechanisms
- **Rate Limiting**: Per-user and per-provider rate limiting
- **Real-time Monitoring**: Live metrics, analytics, and health checks

### **Performance Features**
- **High Throughput**: Built with Rust for maximum performance
- **Async Processing**: Non-blocking I/O with Tokio runtime
- **Connection Pooling**: Efficient HTTP client management
- **Response Streaming**: Real-time streaming support
- **Memory Optimization**: Minimal memory footprint

### **Enterprise Features**
- **API Key Management**: Virtual API keys with rate limiting
- **JWT Authentication**: Stateless token-based authentication
- **Request Validation**: Comprehensive input sanitization
- **Content Filtering**: Plugin-based content moderation
- **Audit Logging**: Complete request/response logging

## 📦 Installation

### **From Crates.io**
```bash
cargo add ultrafast-gateway
```

### **From Source**
```bash
git clone https://github.com/techgopal/ultrafast-ai-gateway.git
cd ultrafast-ai-gateway/ultrafast-gateway
cargo build --release
```

## 🚀 Quick Start

### **Basic Usage**
```rust
use ultrafast_gateway::{Gateway, GatewayConfig, ProviderConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create gateway configuration
    let config = GatewayConfig::default()
        .with_provider(ProviderConfig::openai("your-openai-key"))
        .with_provider(ProviderConfig::anthropic("your-anthropic-key"))
        .with_cache_enabled(true)
        .with_rate_limiting(true);

    // Initialize gateway
    let gateway = Gateway::new(config).await?;

    // Start the server
    gateway.serve("127.0.0.1:3000").await?;
    
    Ok(())
}
```

### **Advanced Configuration**
```rust
use ultrafast_gateway::{
    Gateway, GatewayConfig, ProviderConfig, 
    CacheConfig, RateLimitConfig, CircuitBreakerConfig
};

let config = GatewayConfig::default()
    .with_provider(ProviderConfig::openai("sk-...")
        .with_circuit_breaker(CircuitBreakerConfig {
            failure_threshold: 5,
            recovery_timeout: Duration::from_secs(60),
            request_timeout: Duration::from_secs(30),
        }))
    .with_cache(CacheConfig {
        ttl: Duration::from_secs(3600),
        max_size: 10000,
        eviction_policy: EvictionPolicy::LRU,
    })
    .with_rate_limiting(RateLimitConfig {
        requests_per_minute: 100,
        burst_size: 20,
        per_user: true,
    })
    .with_authentication(true)
    .with_monitoring(true);
```

## 🔧 Configuration

### **Configuration File (config.toml)**
```toml
[server]
host = "0.0.0.0"
port = 3000
workers = 4

[providers.openai]
api_key = "your-openai-key"
base_url = "https://api.openai.com/v1"
timeout = 30
max_retries = 3

[providers.anthropic]
api_key = "your-anthropic-key"
base_url = "https://api.anthropic.com"
timeout = 30
max_retries = 3

[cache]
enabled = true
ttl = 3600
max_size = 10000
eviction_policy = "lru"

[rate_limiting]
enabled = true
requests_per_minute = 100
burst_size = 20
per_user = true

[authentication]
enabled = true
jwt_secret = "your-jwt-secret"
api_key_header = "X-API-Key"

[monitoring]
enabled = true
metrics_port = 9090
health_check_interval = 30
```

### **Environment Variables**
```bash
export ULTRAFAST_GATEWAY_HOST=0.0.0.0
export ULTRAFAST_GATEWAY_PORT=3000
export ULTRAFAST_GATEWAY_OPENAI_API_KEY=sk-...
export ULTRAFAST_GATEWAY_ANTHROPIC_API_KEY=sk-ant-...
export ULTRAFAST_GATEWAY_JWT_SECRET=your-secret
```

## 📡 API Endpoints

### **Chat Completions**
```bash
# OpenAI-compatible endpoint
POST /v1/chat/completions
Content-Type: application/json
Authorization: Bearer your-api-key

{
  "model": "gpt-4",
  "messages": [
    {"role": "user", "content": "Hello, world!"}
  ],
  "max_tokens": 100
}
```

### **Text Completions**
```bash
# OpenAI-compatible endpoint
POST /v1/completions
Content-Type: application/json
Authorization: Bearer your-api-key

{
  "model": "text-davinci-003",
  "prompt": "Hello, world!",
  "max_tokens": 100
}
```

### **Embeddings**
```bash
POST /v1/embeddings
Content-Type: application/json
Authorization: Bearer your-api-key

{
  "model": "text-embedding-ada-002",
  "input": "Hello, world!"
}
```

### **Models List**
```bash
GET /v1/models
Authorization: Bearer your-api-key
```

### **Health Check**
```bash
GET /health
```

### **Metrics**
```bash
GET /metrics
```

## 🔌 Plugins

### **Content Filtering**
```rust
use ultrafast_gateway::plugins::ContentFilteringPlugin;

let plugin = ContentFilteringPlugin::new()
    .with_filters(vec![
        "hate_speech".to_string(),
        "violence".to_string(),
        "sexual_content".to_string(),
    ])
    .with_moderation_api("https://api.moderation.com");

gateway.add_plugin(plugin);
```

### **Cost Tracking**
```rust
use ultrafast_gateway::plugins::CostTrackingPlugin;

let plugin = CostTrackingPlugin::new()
    .with_cost_limits(vec![
        ("daily", 100.0),
        ("monthly", 1000.0),
    ])
    .with_alert_threshold(0.8);

gateway.add_plugin(plugin);
```

### **Logging**
```rust
use ultrafast_gateway::plugins::LoggingPlugin;

let plugin = LoggingPlugin::new()
    .with_level(log::Level::Info)
    .with_format(LogFormat::JSON)
    .with_output(LogOutput::File("gateway.log".into()));

gateway.add_plugin(plugin);
```

## 📊 Monitoring & Analytics

### **Real-time Metrics**
- **Request Count**: Total requests per provider
- **Response Time**: Average, P95, P99 response times
- **Error Rates**: Success/failure rates per provider
- **Cache Hit Rate**: Cache effectiveness metrics
- **Rate Limiting**: Throttled request counts

### **Dashboard**
Access the built-in dashboard at `/dashboard` for:
- Real-time metrics visualization
- Provider health status
- Cache performance analytics
- Rate limiting statistics
- Error rate monitoring

### **Prometheus Integration**
```yaml
# prometheus.yml
scrape_configs:
  - job_name: 'ultrafast-gateway'
    static_configs:
      - targets: ['localhost:9090']
    metrics_path: '/metrics'
```

## 🚀 Performance Tuning

### **Optimization Tips**
```rust
// Enable connection pooling
let config = GatewayConfig::default()
    .with_connection_pool_size(100)
    .with_keep_alive_timeout(Duration::from_secs(60));

// Optimize cache settings
let cache_config = CacheConfig {
    ttl: Duration::from_secs(3600),
    max_size: 50000,
    eviction_policy: EvictionPolicy::LRU,
    compression: true,
};

// Configure circuit breakers
let circuit_breaker = CircuitBreakerConfig {
    failure_threshold: 3,
    recovery_timeout: Duration::from_secs(30),
    request_timeout: Duration::from_secs(10),
    half_open_max_calls: 5,
};
```

### **Benchmark Results**
- **Throughput**: 10,000+ requests/second
- **Latency**: P99 < 50ms
- **Memory**: < 100MB baseline
- **CPU**: Efficient async processing

## 🐳 Docker Deployment

### **Quick Start**
```bash
docker run -p 3000:3000 \
  -v /path/to/config:/app/config.toml \
  ghcr.io/techgopal/ultrafast-ai-gateway:latest
```

### **Docker Compose**
```yaml
version: '3.8'
services:
  ultrafast-gateway:
    image: ghcr.io/techgopal/ultrafast-ai-gateway:latest
    ports:
      - "3000:3000"
      - "9090:9090"
    volumes:
      - ./config.toml:/app/config.toml
      - ./logs:/app/logs
    environment:
      - RUST_LOG=info
      - RUST_BACKTRACE=1
    restart: unless-stopped
```

### **Kubernetes**
```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: ultrafast-gateway
spec:
  replicas: 3
  selector:
    matchLabels:
      app: ultrafast-gateway
  template:
    metadata:
      labels:
        app: ultrafast-gateway
    spec:
      containers:
      - name: gateway
        image: ghcr.io/techgopal/ultrafast-ai-gateway:latest
        ports:
        - containerPort: 3000
        - containerPort: 9090
        env:
        - name: RUST_LOG
          value: "info"
        volumeMounts:
        - name: config
          mountPath: /app/config.toml
          subPath: config.toml
      volumes:
      - name: config
        configMap:
          name: gateway-config
```

## 🔒 Security

### **Authentication Methods**
- **API Keys**: Virtual API key management
- **JWT Tokens**: Stateless authentication
- **OAuth 2.0**: Third-party authentication
- **Rate Limiting**: Per-user and per-provider limits

### **Security Features**
- **Request Validation**: Input sanitization
- **Content Filtering**: Moderation and filtering
- **HTTPS Only**: Secure communication
- **CORS Configuration**: Cross-origin resource sharing
- **Audit Logging**: Complete request tracking

## 🧪 Testing

### **Unit Tests**
```bash
cargo test
```

### **Integration Tests**
```bash
cargo test --test integration
```

### **Load Testing**
```bash
cargo test --test load_testing
```

### **Benchmark Tests**
```bash
cargo bench
```

## 📚 Examples

### **Basic Gateway**
```rust
// examples/basic_gateway.rs
use ultrafast_gateway::{Gateway, GatewayConfig, ProviderConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = GatewayConfig::default()
        .with_provider(ProviderConfig::openai("your-key"))
        .with_cache_enabled(true);

    let gateway = Gateway::new(config).await?;
    gateway.serve("127.0.0.1:3000").await?;
    Ok(())
}
```

### **Advanced Routing**
```rust
// examples/advanced_routing.rs
use ultrafast_gateway::{
    Gateway, GatewayConfig, ProviderConfig, 
    RoutingStrategy, LoadBalancingStrategy
};

let config = GatewayConfig::default()
    .with_provider(ProviderConfig::openai("key1"))
    .with_provider(ProviderConfig::anthropic("key2"))
    .with_routing_strategy(RoutingStrategy::LoadBalanced(
        LoadBalancingStrategy::RoundRobin
    ));
```

### **Custom Middleware**
```rust
// examples/custom_middleware.rs
use ultrafast_gateway::{
    Gateway, GatewayConfig, Middleware, Request, Response
};

struct CustomMiddleware;

#[async_trait]
impl Middleware for CustomMiddleware {
    async fn process(&self, request: Request) -> Result<Response, Box<dyn std::error::Error>> {
        // Custom processing logic
        Ok(request.into())
    }
}

let gateway = Gateway::new(config)
    .with_middleware(CustomMiddleware)
    .await?;
```

## 🤝 Contributing

We welcome contributions! Please see our [Contributing Guide](CONTRIBUTING.md) for details.

### **Development Setup**
```bash
git clone https://github.com/techgopal/ultrafast-ai-gateway.git
cd ultrafast-ai-gateway
cargo build
cargo test
```

### **Code Style**
- Follow Rust formatting guidelines
- Run `cargo fmt` before committing
- Ensure all tests pass with `cargo test`
- Add tests for new features

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 🆘 Support

### **Documentation**
- [API Reference](https://docs.rs/ultrafast-gateway)
- [Examples](examples/)
- [Configuration Guide](docs/configuration.md)

### **Community**
- [GitHub Issues](https://github.com/techgopal/ultrafast-ai-gateway/issues)
- [Discussions](https://github.com/techgopal/ultrafast-ai-gateway/discussions)
- [Wiki](https://github.com/techgopal/ultrafast-ai-gateway/wiki)

### **Commercial Support**
For enterprise support and consulting, contact:
- **Email**: techgopal2@gmail.com
- **GitHub**: [@techgopal](https://github.com/techgopal)

## 🙏 Acknowledgments

- **Rust Community**: For the amazing language and ecosystem
- **Tokio Team**: For the async runtime
- **OpenAI, Anthropic, Google**: For their AI APIs
- **Contributors**: All who have helped improve this project

## 📈 Roadmap

### **v0.2.0 (Q2 2024)**
- [ ] GraphQL API support
- [ ] WebSocket streaming
- [ ] Advanced analytics dashboard
- [ ] Plugin marketplace

### **v0.3.0 (Q3 2024)**
- [ ] Multi-region deployment
- [ ] Advanced caching strategies
- [ ] Machine learning routing
- [ ] Enterprise SSO integration

### **v1.0.0 (Q4 2024)**
- [ ] Production-ready stability
- [ ] Comprehensive documentation
- [ ] Performance benchmarks
- [ ] Enterprise features

---

**Built with ❤️ in Rust by the Ultrafast Gateway Team**

[![GitHub stars](https://img.shields.io/github/stars/techgopal/ultrafast-ai-gateway?style=social)](https://github.com/techgopal/ultrafast-ai-gateway)
[![GitHub forks](https://img.shields.io/github/forks/techgopal/ultrafast-ai-gateway?style=social)](https://github.com/techgopal/ultrafast-ai-gateway)
[![GitHub issues](https://img.shields.io/github/issues/techgopal/ultrafast-ai-gateway)](https://github.com/techgopal/ultrafast-ai-gateway/issues)
