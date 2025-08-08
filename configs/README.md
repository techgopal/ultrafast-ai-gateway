# Configuration Files

This directory contains various configuration files for the ultrafast-gateway.

## 📁 Configuration Files

### **Production Configs**
- `production.toml` - Full production configuration with all providers
- `production-minimal.toml` - Production config with essential providers only
- `production-secure.toml` - Production config with enhanced security

### **Development Configs**
- `development.toml` - Development environment with local providers
- `development-ollama.toml` - Development with Ollama only
- `development-testing.toml` - Development config for testing

### **Provider-Specific Configs**
- `ollama-only.toml` - Ollama provider only
- `openai-only.toml` - OpenAI provider only
- `anthropic-only.toml` - Anthropic provider only
- `multi-provider.toml` - Multiple providers with load balancing

### **Specialized Configs**
- `high-performance.toml` - Optimized for high throughput
- `low-latency.toml` - Optimized for low latency
- `secure.toml` - Enhanced security configuration
- `monitoring.toml` - Configuration with extensive monitoring

## 🎯 Usage

### **Start with specific config:**
```bash
cargo run --bin ultrafast-gateway -- --config configs/development-ollama.toml
```

### **Environment-specific configs:**
```bash
# Development
cargo run --bin ultrafast-gateway -- --config configs/development.toml

# Production
cargo run --bin ultrafast-gateway -- --config configs/production.toml

# Testing
cargo run --bin ultrafast-gateway -- --config configs/development-testing.toml
```

## 🔧 Configuration Structure

### **Server Configuration**
```toml
[server]
host = "127.0.0.1"          # Server host
port = 3000                  # Server port
timeout = "30s"              # Request timeout
max_body_size = 10485760     # Max request body size (10MB)
cors = { enabled = true, ... } # CORS settings
```

### **Provider Configuration**
```toml
[providers.provider_name]
name = "provider_name"        # Provider identifier
api_key = "your-api-key"     # API key (empty for local providers)
base_url = "https://api.example.com" # Provider API URL
timeout = "30s"              # Provider timeout
max_retries = 3              # Retry attempts
retry_delay = "1s"           # Delay between retries
enabled = true               # Enable/disable provider
model_mapping = {}           # Model name mappings
headers = {}                 # Custom headers
```

### **Routing Configuration**
```toml
[routing]
strategy = { Single = {} }   # Routing strategy
health_check_interval = "30s" # Health check frequency
failover_threshold = 0.8     # Failover threshold
```

### **Authentication Configuration** ⚠️ **UPDATED**
```toml
[auth]
enabled = true               # Enable authentication
api_keys = [                 # API key configurations
    { 
        key = "sk-key", 
        name = "default", 
        enabled = true,
        rate_limit = { requests_per_minute = 100, ... }, # Per-key rate limits
        metadata = {}
    }
]
rate_limiting = { requests_per_minute = 1000, ... } # Global rate limits (fallback)
```
**⚠️ Important**: Rate limiting is now configured here, not as a plugin!

### **Cache Configuration**
```toml
[cache]
enabled = true               # Enable caching
backend = "Memory"           # Cache backend (Memory/Redis)
ttl = "1h"                  # Cache TTL
max_size = 1000             # Max cache size
```

### **Logging Configuration**
```toml
[logging]
level = "info"               # Log level
format = "Pretty"            # Log format (Pretty/Json/Compact)
output = "Stdout"            # Log output (Stdout/File)
```

### **Plugin Configuration** ⚠️ **UPDATED**
```toml
[[plugins]]
name = "plugin_name"         # Plugin name (cost_tracking, content_filtering, logging)
enabled = true               # Enable plugin
config = { "key" = "value" } # Plugin configuration
```
**⚠️ Important**: `rate_limiting` plugin is **deprecated** - use `[auth]` section instead!

**Available Plugins:**
- `cost_tracking` - Track API costs and token usage
- `content_filtering` - Filter inappropriate content  
- `logging` - Enhanced request/response logging

## 🚀 Quick Start

1. **Choose a configuration file** based on your needs
2. **Update API keys** in the selected config file
3. **Start the gateway** with the config:
   ```bash
   cargo run --bin ultrafast-gateway -- --config configs/your-config.toml
   ```

## 🔒 Security Notes

- **Never commit API keys** to version control
- **Use environment variables** for sensitive data in production
- **Rotate API keys** regularly
- **Use secure configs** for production deployments

## 📊 Performance Tuning

- **High Performance**: Use `high-performance.toml`
- **Low Latency**: Use `low-latency.toml`
- **Development**: Use `development-*.toml` files
- **Production**: Use `production-*.toml` files 