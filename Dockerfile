# Multi-stage Dockerfile for Ultrafast Gateway
# Stage 1: Build stage
FROM rust:latest AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Set working directory
WORKDIR /app

# Copy workspace files
COPY Cargo.toml ./
COPY ultrafast-gateway/Cargo.toml ultrafast-gateway/
COPY ultrafast-models-sdk/Cargo.toml ultrafast-models-sdk/

# Copy full source code (no dummy files)
COPY ultrafast-gateway/ ultrafast-gateway/
COPY ultrafast-models-sdk/ ultrafast-models-sdk/

# Build the application
RUN cargo build --release -p ultrafast-models-sdk && cargo build --release -p ultrafast-gateway

# Stage 2: Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN groupadd -r gateway && useradd -r -g gateway gateway

# Set working directory
WORKDIR /app

# Copy binary from builder stage
COPY --from=builder /app/target/release/ultrafast-gateway /app/ultrafast-gateway

# Copy configuration files
COPY deployment/config.dev.toml /app/config.toml
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

# Labels for metadata
LABEL org.opencontainers.image.title="Ultrafast Gateway"
LABEL org.opencontainers.image.description="High-performance LLM gateway with advanced routing and caching"
LABEL org.opencontainers.image.version="0.1.0"
LABEL org.opencontainers.image.source="https://github.com/ultrafast-ai/ultrafast-gateway"
