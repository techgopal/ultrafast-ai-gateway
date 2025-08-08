# Ultrafast Gateway Deployment

This folder contains all the Docker-related files for deploying the Ultrafast Gateway.

## Files

- `Dockerfile` - Multi-stage Docker build for the gateway
- `docker-compose.yml` - Docker Compose configuration for the full stack
- `build.sh` - Build script with various options
- `config.dev.toml` - Development configuration
- `config.production.toml` - Production configuration template
- `env.development` - Development environment variables
- `.dockerignore` - Files to exclude from Docker build

## Quick Start

### Development Mode

```bash
# Navigate to deployment directory
cd deployment

# Build and run with docker-compose
./build.sh -c -r

# Or manually
docker-compose up -d
```

### Production Mode

```bash
# Copy production config
cp config.production.toml config.toml

# Set environment variables
export OPENAI_API_KEY="your-key"
export ANTHROPIC_API_KEY="your-key"
# ... other API keys

# Build and run
docker-compose up -d
```

## Configuration

### Development Configuration (`config.dev.toml`)
- Only Ollama provider enabled (no API keys required)
- Authentication disabled
- Memory cache backend
- Pretty logging format

### Production Configuration (`config.production.toml`)
- All providers enabled (requires API keys)
- Authentication enabled
- Redis cache backend
- JSON logging format

## Environment Variables

Create a `.env` file or set environment variables:

```bash
# Development
DEVELOPMENT_MODE=true
RUST_LOG=info
REDIS_URL=redis://redis:6379

# API Keys (for production)
OPENAI_API_KEY=your-key
ANTHROPIC_API_KEY=your-key
AZURE_OPENAI_API_KEY=your-key
GOOGLE_VERTEX_AI_API_KEY=your-key
COHERE_API_KEY=your-key
GROQ_API_KEY=your-key
MISTRAL_API_KEY=your-key
PERPLEXITY_API_KEY=your-key

# Gateway API Keys
GATEWAY_API_KEYS='[{"key":"sk-gateway-key","name":"default","enabled":true}]'
```

## Build Script Options

```bash
./build.sh [OPTIONS]

Options:
  -t, --tag TAG         Docker image tag (default: latest)
  -b, --build-type TYPE  Build type: debug or release (default: release)
  -p, --push            Push image to registry after build
  -r, --run             Run container after build
  -c, --compose         Use docker-compose instead of docker build
  -h, --help            Show this help message

Examples:
  ./build.sh                    # Build release image with latest tag
  ./build.sh -t v1.0.0         # Build with specific tag
  ./build.sh -r                # Build and run container
  ./build.sh -c                # Use docker-compose
  ./build.sh -c -r             # Use docker-compose and run
```

## Services

### Gateway
- **Port**: 3000
- **Health Check**: `http://localhost:3000/health`
- **Admin Endpoints**:
  - `GET /admin/providers` - List configured providers
  - `GET /admin/config` - Get configuration
  - `GET /metrics` - Get metrics

### Redis
- **Port**: 6380 (mapped from 6379)
- **Purpose**: Caching and rate limiting

### Prometheus (Optional)
- **Port**: 9090
- **Profile**: monitoring
- **Usage**: `docker-compose --profile monitoring up -d`

### Grafana (Optional)
- **Port**: 3001
- **Profile**: monitoring
- **Usage**: `docker-compose --profile monitoring up -d`

## Monitoring

### Health Check
```bash
curl http://localhost:3000/health
```

### Metrics
```bash
curl http://localhost:3000/metrics
```

### Logs
```bash
# Gateway logs
docker-compose logs -f gateway

# Redis logs
docker-compose logs -f redis
```

## Troubleshooting

### Common Issues

1. **Port conflicts**: Change ports in `docker-compose.yml`
2. **API key errors**: Ensure all required API keys are set
3. **Build failures**: Check that you're in the deployment directory
4. **Permission errors**: Ensure build script is executable: `chmod +x build.sh`

### Debug Mode

```bash
# Run with debug logging
RUST_LOG=debug docker-compose up

# Build with debug symbols
./build.sh -b debug
```

## Production Deployment

1. Copy `config.production.toml` to `config.toml`
2. Set all required environment variables
3. Configure API keys for all providers
4. Enable authentication
5. Use Redis for caching
6. Set up monitoring with Prometheus/Grafana

```bash
# Production deployment
cp config.production.toml config.toml
export OPENAI_API_KEY="your-key"
export ANTHROPIC_API_KEY="your-key"
# ... set all API keys
docker-compose up -d
```
