# Docker Configuration Guide

This guide shows how to run the Ultrafast Gateway with different configuration files in Docker.

## Quick Examples

### Using Different Config Files

```bash
# Development with Ollama
docker run -d \
  --name ultrafast-gateway-ollama \
  -p 3000:3000 \
  -v $(pwd)/configs/development-ollama.toml:/app/config.toml:ro \
  ultrafast-gateway:latest

# Production with minimal providers
docker run -d \
  --name ultrafast-gateway-prod \
  -p 3000:3000 \
  -v $(pwd)/configs/production-minimal.toml:/app/config.toml:ro \
  -e OPENAI_API_KEY=your-key \
  -e ANTHROPIC_API_KEY=your-key \
  ultrafast-gateway:latest

# High performance configuration
docker run -d \
  --name ultrafast-gateway-perf \
  -p 3000:3000 \
  -v $(pwd)/configs/high-performance.toml:/app/config.toml:ro \
  ultrafast-gateway:latest
```

### Using Docker Compose with Different Configs

```bash
# Development environment
docker-compose -f docker-compose.yml -f docker-compose.dev.yml up -d

# Production environment
docker-compose -f docker-compose.yml -f docker-compose.prod.yml up -d

# Testing environment
docker-compose -f docker-compose.yml -f docker-compose.test.yml up -d
```

## Available Configuration Files

### Development Configurations

#### `configs/development-ollama.toml`
- **Purpose**: Local development with Ollama
- **Use Case**: Testing with local models
- **Docker Command**:
  ```bash
  docker run -d \
    --name ultrafast-gateway-dev \
    -p 3000:3000 \
    -v $(pwd)/configs/development-ollama.toml:/app/config.toml:ro \
    ultrafast-gateway:latest
  ```

#### `configs/development-anthropic.toml`
- **Purpose**: Development with Anthropic Claude
- **Use Case**: Testing Claude models
- **Docker Command**:
  ```bash
  docker run -d \
    --name ultrafast-gateway-anthropic \
    -p 3000:3000 \
    -v $(pwd)/configs/development-anthropic.toml:/app/config.toml:ro \
    -e ANTHROPIC_API_KEY=your-key \
    ultrafast-gateway:latest
  ```

#### `configs/development-gemini.toml`
- **Purpose**: Development with Google Gemini
- **Use Case**: Testing Gemini models
- **Docker Command**:
  ```bash
  docker run -d \
    --name ultrafast-gateway-gemini \
    -p 3000:3000 \
    -v $(pwd)/configs/development-gemini.toml:/app/config.toml:ro \
    -e GOOGLE_VERTEX_AI_API_KEY=your-key \
    ultrafast-gateway:latest
  ```

### Production Configurations

#### `configs/production.toml`
- **Purpose**: Full production setup
- **Use Case**: Production deployment with all providers
- **Docker Command**:
  ```bash
  docker run -d \
    --name ultrafast-gateway-prod \
    -p 3000:3000 \
    -v $(pwd)/configs/production.toml:/app/config.toml:ro \
    -e OPENAI_API_KEY=your-key \
    -e ANTHROPIC_API_KEY=your-key \
    -e AZURE_OPENAI_API_KEY=your-key \
    -e GOOGLE_VERTEX_AI_API_KEY=your-key \
    -e COHERE_API_KEY=your-key \
    -e GROQ_API_KEY=your-key \
    -e MISTRAL_API_KEY=your-key \
    -e PERPLEXITY_API_KEY=your-key \
    ultrafast-gateway:latest
  ```

#### `configs/production-minimal.toml`
- **Purpose**: Production with essential providers only
- **Use Case**: Cost-optimized production deployment
- **Docker Command**:
  ```bash
  docker run -d \
    --name ultrafast-gateway-prod-minimal \
    -p 3000:3000 \
    -v $(pwd)/configs/production-minimal.toml:/app/config.toml:ro \
    -e OPENAI_API_KEY=your-key \
    -e ANTHROPIC_API_KEY=your-key \
    ultrafast-gateway:latest
  ```

### Specialized Configurations

#### `configs/high-performance.toml`
- **Purpose**: Optimized for high throughput
- **Use Case**: High-traffic production environments
- **Docker Command**:
  ```bash
  docker run -d \
    --name ultrafast-gateway-perf \
    -p 3000:3000 \
    --memory=2g \
    --cpus=2.0 \
    -v $(pwd)/configs/high-performance.toml:/app/config.toml:ro \
    ultrafast-gateway:latest
  ```

#### `configs/secure.toml`
- **Purpose**: Enhanced security configuration
- **Use Case**: High-security environments
- **Docker Command**:
  ```bash
  docker run -d \
    --name ultrafast-gateway-secure \
    -p 3000:3000 \
    -v $(pwd)/configs/secure.toml:/app/config.toml:ro \
    -e GATEWAY_JWT_SECRET=your-secure-secret \
    ultrafast-gateway:latest
  ```

### Provider-Specific Configurations

#### `configs/ollama-only.toml`
- **Purpose**: Ollama provider only
- **Use Case**: Local model deployment
- **Docker Command**:
  ```bash
  docker run -d \
    --name ultrafast-gateway-ollama \
    -p 3000:3000 \
    -v $(pwd)/configs/ollama-only.toml:/app/config.toml:ro \
    ultrafast-gateway:latest
  ```

#### `configs/openai-only.toml`
- **Purpose**: OpenAI provider only
- **Use Case**: OpenAI-only deployment
- **Docker Command**:
  ```bash
  docker run -d \
    --name ultrafast-gateway-openai \
    -p 3000:3000 \
    -v $(pwd)/configs/openai-only.toml:/app/config.toml:ro \
    -e OPENAI_API_KEY=your-key \
    ultrafast-gateway:latest
  ```

## Docker Compose Overrides

### Development Override

Create `docker-compose.dev.yml`:
```yaml
version: '3.8'

services:
  gateway:
    volumes:
      - ./configs/development-ollama.toml:/app/config.toml:ro
    environment:
      - RUST_LOG=debug
      - RUST_BACKTRACE=1
    ports:
      - "3000:3000"
```

### Production Override

Create `docker-compose.prod.yml`:
```yaml
version: '3.8'

services:
  gateway:
    volumes:
      - ./configs/production-minimal.toml:/app/config.toml:ro
    environment:
      - RUST_LOG=warn
      - GATEWAY_JWT_SECRET=${GATEWAY_JWT_SECRET}
    deploy:
      resources:
        limits:
          memory: 1G
          cpus: '1.0'
    restart: unless-stopped
```

### Testing Override

Create `docker-compose.test.yml`:
```yaml
version: '3.8'

services:
  gateway:
    volumes:
      - ./configs/development-testing.toml:/app/config.toml:ro
    environment:
      - RUST_LOG=debug
      - RUST_BACKTRACE=1
    ports:
      - "3000:3000"
```

## Environment-Specific Configurations

### Development Environment

```bash
# Use development config with debug logging
docker run -d \
  --name ultrafast-gateway-dev \
  -p 3000:3000 \
  -v $(pwd)/configs/development.toml:/app/config.toml:ro \
  -e RUST_LOG=debug \
  -e RUST_BACKTRACE=1 \
  ultrafast-gateway:latest
```

### Staging Environment

```bash
# Use production config with staging settings
docker run -d \
  --name ultrafast-gateway-staging \
  -p 3000:3000 \
  -v $(pwd)/configs/production-minimal.toml:/app/config.toml:ro \
  -e RUST_LOG=info \
  -e OPENAI_API_KEY=your-staging-key \
  -e ANTHROPIC_API_KEY=your-staging-key \
  ultrafast-gateway:latest
```

### Production Environment

```bash
# Use production config with all security measures
docker run -d \
  --name ultrafast-gateway-prod \
  -p 3000:3000 \
  -v $(pwd)/configs/production.toml:/app/config.toml:ro \
  -e RUST_LOG=warn \
  -e GATEWAY_JWT_SECRET=your-secure-secret \
  -e OPENAI_API_KEY=your-prod-key \
  -e ANTHROPIC_API_KEY=your-prod-key \
  --memory=1g \
  --cpus=1.0 \
  --restart=unless-stopped \
  ultrafast-gateway:latest
```

## Custom Configuration Creation

### Creating a Custom Config

1. **Copy an existing config**:
   ```bash
   cp configs/development.toml configs/my-custom.toml
   ```

2. **Modify the config** for your needs:
   ```toml
   [server]
   host = "0.0.0.0"
   port = 3000
   timeout = "60s"
   max_body_size = 20971520  # 20MB

   [providers.openai]
   name = "openai"
   api_key = ""
   base_url = "https://api.openai.com/v1"
   timeout = "60s"
   max_retries = 5
   retry_delay = "2s"
   enabled = true
   ```

3. **Use with Docker**:
   ```bash
   docker run -d \
     --name ultrafast-gateway-custom \
     -p 3000:3000 \
     -v $(pwd)/configs/my-custom.toml:/app/config.toml:ro \
     -e OPENAI_API_KEY=your-key \
     ultrafast-gateway:latest
   ```

## Configuration Validation

### Validate Config Before Running

```bash
# Check if config is valid
docker run --rm \
  -v $(pwd)/configs/my-custom.toml:/app/config.toml:ro \
  ultrafast-gateway:latest \
  /app/ultrafast-gateway --config /app/config.toml --validate-only
```

### Test Configuration

```bash
# Test config with health check
docker run -d \
  --name ultrafast-gateway-test \
  -p 3000:3000 \
  -v $(pwd)/configs/my-custom.toml:/app/config.toml:ro \
  ultrafast-gateway:latest

# Wait for startup and test
sleep 10
curl http://localhost:3000/health
```

## Best Practices

### 1. **Use Volume Mounting**
Always mount config files as volumes rather than copying them into the image:
```bash
# ✅ Good
-v $(pwd)/configs/production.toml:/app/config.toml:ro

# ❌ Avoid
COPY configs/production.toml /app/config.toml
```

### 2. **Environment-Specific Configs**
Use different configs for different environments:
- `development-*.toml` for development
- `production-*.toml` for production
- `testing-*.toml` for testing

### 3. **Security Considerations**
- Never commit API keys to config files
- Use environment variables for sensitive data
- Use read-only volume mounts (`:ro`)

### 4. **Resource Allocation**
Match Docker resources to your config:
```bash
# High-performance config
docker run -d \
  --memory=2g \
  --cpus=2.0 \
  -v $(pwd)/configs/high-performance.toml:/app/config.toml:ro \
  ultrafast-gateway:latest
```

### 5. **Monitoring and Logging**
Use appropriate log levels:
```bash
# Development
-e RUST_LOG=debug

# Production
-e RUST_LOG=warn
```

## Troubleshooting

### Common Issues

1. **Config file not found**:
   ```bash
   # Check if file exists
   ls -la configs/
   
   # Verify mount path
   docker exec ultrafast-gateway ls -la /app/config.toml
   ```

2. **Invalid config syntax**:
   ```bash
   # Validate config
   docker run --rm \
     -v $(pwd)/configs/my-config.toml:/app/config.toml:ro \
     ultrafast-gateway:latest \
     /app/ultrafast-gateway --config /app/config.toml --validate-only
   ```

3. **Missing environment variables**:
   ```bash
   # Check required env vars
   docker logs ultrafast-gateway | grep -i "missing\|error"
   ```

### Debug Configuration

```bash
# Run with debug logging
docker run -d \
  --name ultrafast-gateway-debug \
  -p 3000:3000 \
  -v $(pwd)/configs/my-config.toml:/app/config.toml:ro \
  -e RUST_LOG=debug \
  -e RUST_BACKTRACE=1 \
  ultrafast-gateway:latest

# View logs
docker logs -f ultrafast-gateway-debug
```
