# Multi-stage Dockerfile for Ultrafast Gateway
# Stage 1: Build stage
FROM rust:1.75-slim AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Set working directory
WORKDIR /app

# Copy workspace files first for better caching
COPY Cargo.toml ./
COPY ultrafast-gateway/Cargo.toml ultrafast-gateway/
COPY ultrafast-models-sdk/Cargo.toml ultrafast-models-sdk/

# Create dummy source files for dependency resolution
RUN mkdir -p ultrafast-gateway/src ultrafast-models-sdk/src && \
    echo "fn main() {}" > ultrafast-gateway/src/main.rs && \
    echo "fn main() {}" > ultrafast-models-sdk/src/lib.rs

# Download and cache dependencies
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release -p ultrafast-models-sdk && \
    cargo build --release -p ultrafast-gateway

# Remove dummy files and copy real source
RUN rm -rf ultrafast-gateway/src ultrafast-models-sdk/src
COPY ultrafast-gateway/ ultrafast-gateway/
COPY ultrafast-models-sdk/ ultrafast-models-sdk/

# Build the application with real source
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release -p ultrafast-models-sdk && \
    cargo build --release -p ultrafast-gateway

# Stage 2: Runtime stage
FROM debian:bookworm-slim AS runtime

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    && rm -rf /var/lib/apt/lists/* \
    && apt-get clean

# Create non-root user
RUN groupadd -r gateway && useradd -r -g gateway gateway

# Set working directory
WORKDIR /app

# Copy binary from builder stage
COPY --from=builder /app/target/release/ultrafast-gateway /app/ultrafast-gateway

# Copy configuration files
RUN echo '[server]' > /app/config.toml && \
    echo 'host = "0.0.0.0"' >> /app/config.toml && \
    echo 'port = 3000' >> /app/config.toml && \
    echo 'timeout = "30s"' >> /app/config.toml && \
    echo 'max_body_size = 10485760' >> /app/config.toml && \
    echo 'cors = { enabled = true, allowed_origins = ["*"], allowed_methods = ["GET", "POST", "PUT", "DELETE"], allowed_headers = ["*"] }' >> /app/config.toml && \
    echo '' >> /app/config.toml && \
    echo 'development_mode = true' >> /app/config.toml && \
    echo '' >> /app/config.toml && \
    echo '[providers.ollama]' >> /app/config.toml && \
    echo 'name = "ollama"' >> /app/config.toml && \
    echo 'api_key = ""' >> /app/config.toml && \
    echo 'base_url = "http://localhost:11434"' >> /app/config.toml && \
    echo 'timeout = "30s"' >> /app/config.toml && \
    echo 'max_retries = 3' >> /app/config.toml && \
    echo 'retry_delay = "1s"' >> /app/config.toml && \
    echo 'enabled = true' >> /app/config.toml && \
    echo 'model_mapping = {}' >> /app/config.toml && \
    echo 'headers = {}' >> /app/config.toml && \
    echo '' >> /app/config.toml && \
    echo '[routing]' >> /app/config.toml && \
    echo 'strategy = { Single = {} }' >> /app/config.toml && \
    echo 'health_check_interval = "30s"' >> /app/config.toml && \
    echo 'failover_threshold = 0.8' >> /app/config.toml && \
    echo '' >> /app/config.toml && \
    echo '[auth]' >> /app/config.toml && \
    echo 'enabled = false' >> /app/config.toml && \
    echo 'api_keys = []' >> /app/config.toml && \
    echo 'rate_limiting = { requests_per_minute = 1000, requests_per_hour = 10000, tokens_per_minute = 100000 }' >> /app/config.toml && \
    echo '' >> /app/config.toml && \
    echo '[cache]' >> /app/config.toml && \
    echo 'enabled = true' >> /app/config.toml && \
    echo 'backend = "Memory"' >> /app/config.toml && \
    echo 'ttl = "1h"' >> /app/config.toml && \
    echo 'max_size = 1000' >> /app/config.toml && \
    echo '' >> /app/config.toml && \
    echo '[logging]' >> /app/config.toml && \
    echo 'level = "info"' >> /app/config.toml && \
    echo 'format = "Pretty"' >> /app/config.toml && \
    echo 'output = "Stdout"' >> /app/config.toml && \
    echo '' >> /app/config.toml && \
    echo '[metrics]' >> /app/config.toml && \
    echo 'enabled = true' >> /app/config.toml && \
    echo 'port = 9090' >> /app/config.toml && \
    echo 'path = "/metrics"' >> /app/config.toml && \
    echo 'max_requests = 1000' >> /app/config.toml && \
    echo 'retention_duration = "24h"' >> /app/config.toml && \
    echo 'cleanup_interval = "1h"' >> /app/config.toml && \
    echo '' >> /app/config.toml && \
    echo '[[plugins]]' >> /app/config.toml && \
    echo 'name = "cost_tracking"' >> /app/config.toml && \
    echo 'enabled = true' >> /app/config.toml && \
    echo 'config = { "track_costs" = true }' >> /app/config.toml

COPY configs/ /app/configs/

# Create directories for logs and data
RUN mkdir -p /app/logs /app/data && \
    chown -R gateway:gateway /app

# Switch to non-root user
USER gateway

# Expose default port
EXPOSE 3000

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=30s --retries=5 \
    CMD curl -f http://localhost:3000/health || exit 1

# Default command
CMD ["/app/ultrafast-gateway", "--config", "/app/config.toml", "--host", "0.0.0.0", "--port", "3000"]

# Environment variables for configuration
ENV RUST_LOG=info
ENV RUST_BACKTRACE=1
ENV DEVELOPMENT_MODE=true

# Labels for metadata (will be overridden by build args)
LABEL org.opencontainers.image.title="Ultrafast Gateway"
LABEL org.opencontainers.image.description="High-performance LLM gateway with advanced routing and caching"
LABEL org.opencontainers.image.vendor="Ultrafast AI"
LABEL org.opencontainers.image.licenses="MIT"
LABEL maintainer="techgopal <techgopal2@gmail.com>"
LABEL description="Ultrafast Gateway - A high-performance AI gateway built in Rust that provides a unified interface to 10+ LLM providers with advanced routing, caching, and monitoring capabilities."
LABEL usage="docker run -p 3000:3000 -v /path/to/config:/app/config.toml ultrafast-ai-gateway:latest"
