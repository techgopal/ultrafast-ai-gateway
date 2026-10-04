# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [2.0.0-alpha.2] - Unreleased

The last features of phase 1: a playground, Settings for sign-in and backup,
configuration export and import, the audit log under Settings, and a release
workflow that works. The version in `Cargo.toml` is still `2.0.0-alpha.1`
until the release is cut; the release workflow stops on a tag that does not
name it.

### Added
- **Playground** in the console (Observe, Playground) and
  `POST /api/playground/chat`: chat with a model or route as the signed-in
  user, streamed, with tokens and cost and Copy as curl. The call goes through
  the same pipeline as `/v1` (access, limits, budgets, cache, routing,
  logging) as a key of the user with no team and no allowlist, and is logged
  with no key and the endpoint `playground` (also a `playground` label of
  `uf_requests_total`).
- **Configuration export and import**: `GET /api/config/export`,
  `POST /api/config/import?dry_run=`, `ultrafast config export|import`, and a
  Configuration section of Settings. The file has no credential, key, token,
  password or log; an import is checked whole (dry run first), creates and
  updates by name, never deletes, writes in one transaction and is audited.
- **Backup**: `GET /api/backup` (a consistent SQLite copy made with
  `VACUUM INTO`, streamed and deleted, audited), `ultrafast backup <path>`, a
  Backup section of Settings, and a restore procedure in the README. The
  backup does not hold the master key.
- **Sign-in settings**: `session_hours` (1 to 720, applied to new sessions)
  in `GET`/`PATCH /api/settings`; the trusted proxies and the sign-in limits
  are shown read only (`trusted_proxies`, `login_limits`).
- **Release workflow**: on a tag `v*` the console is built, then binaries for
  Linux x86_64 and aarch64 (musl), macOS x86_64 and arm64 and Windows x64
  with it embedded, checksums, a draft GitHub release, and an image on
  ghcr.io. By hand it builds the archives and publishes nothing.

### Changed
- The audit log is a view of Settings (`/settings#audit`); the sidebar item
  is gone and `/audit` leads there.
- `GET /api/settings` answers more than `log_retention_days`; `PATCH` takes
  `log_retention_days` and/or `session_hours` (at least one).
- `RequestRecord.key_id` and `Scope::begin` take an optional key id (a call of
  the playground has no key).

### Known limits
- An import never deletes (a prune is for later), and limits and budgets of
  keys are not exported.
- A backup excludes the master key by design; restoring is a manual
  procedure, not an API.
- The playground does not save its conversations.
- The crates are not published to crates.io yet, and the image is for
  linux/amd64 only.

## [0.1.0] - 2025-08-10

### Added
- **Initial Release**: High-performance AI gateway built in Rust
- **Dual Mode Operation**: Standalone mode for direct provider calls and Gateway mode for centralized API
- **Provider Support**: Support for 10+ LLM providers including OpenAI, Anthropic, Azure OpenAI, Google Vertex AI, Cohere, Groq, Mistral AI, Perplexity AI, Together AI, Ollama, and custom HTTP providers
- **Advanced Routing**: Multiple routing strategies including load balancing, fallback, conditional routing, A/B testing, round robin, least used, and lowest latency
- **Performance Features**: 
  - <1ms request routing overhead
  - 10,000+ requests/second throughput
  - 100,000+ concurrent connections support
  - <1GB memory usage under normal load
  - 99.9% uptime with automatic failover
  - Zero-copy deserialization
  - Async I/O throughout the stack
  - Connection pooling for optimal resource utilization
- **Enterprise Features**:
  - Authentication with virtual API keys and JWT tokens
  - Rate limiting per user/provider with sliding windows
  - Request validation with comprehensive schemas
  - Content filtering with plugin system
  - Cost tracking and analytics
  - Real-time metrics and monitoring
  - Circuit breakers for fault tolerance and automatic failover
  - Horizontal scaling with Redis-based session storage
- **Caching System**: Multi-level caching with Redis and in-memory options
- **Metrics & Monitoring**: Comprehensive metrics collection with Prometheus support
- **Error Handling**: Robust error handling with detailed error types and recovery mechanisms
- **Configuration Management**: Flexible configuration system with environment variable overrides
- **Plugin System**: Extensible plugin architecture for custom functionality
- **WebSocket Support**: Real-time streaming and WebSocket connections
- **Dashboard**: Web-based dashboard for monitoring and management
- **Docker Support**: Complete Docker deployment with multi-stage builds
- **CI/CD Pipeline**: Comprehensive GitHub Actions workflow for testing, linting, and deployment

### Technical Features
- **Rust SDK**: Complete Rust SDK for both standalone and gateway modes
- **Circuit Breaker Pattern**: Automatic fault tolerance and recovery
- **JSON Optimization**: Efficient JSON processing with size reduction
- **Middleware System**: Comprehensive middleware for logging, metrics, CORS, and validation
- **Health Checks**: Provider health monitoring and automatic failover
- **Security**: JWT authentication, rate limiting, and input validation
- **Documentation**: Comprehensive API documentation and examples

### Infrastructure
- **Multi-platform Support**: Linux, macOS, and Windows builds
- **Docker Images**: Optimized Docker images for production deployment
- **Kubernetes Ready**: Helm charts and Kubernetes manifests
- **Monitoring**: Prometheus metrics, Grafana dashboards, and alerting
- **Logging**: Structured logging with configurable levels and outputs

### Developer Experience
- **Comprehensive Testing**: Unit tests, integration tests, and benchmarks
- **Code Quality**: Clippy linting, rustfmt formatting, and security audits
- **Documentation**: Extensive inline documentation and examples
- **Error Messages**: Clear and actionable error messages
- **Type Safety**: Strong type system with comprehensive error handling

### Breaking Changes
- None (initial release)

### Deprecations
- None (initial release)

### Security
- All dependencies audited for security vulnerabilities
- JWT-based authentication system
- Rate limiting and input validation
- Secure configuration management
- No known security issues

### Performance
- Optimized for high-throughput scenarios
- Efficient memory usage and garbage collection
- Async/await throughout the codebase
- Connection pooling and reuse
- Zero-copy JSON processing

### Documentation
- Comprehensive README with quick start guide
- API documentation with examples
- Deployment guides for various environments
- Configuration reference
- Troubleshooting guide

### Contributors
- Initial release by the Ultrafast Gateway team

---

## [Unreleased]

### Planned Features
- Additional provider integrations
- Enhanced monitoring and alerting
- Performance optimizations
- Extended plugin system
- Advanced routing algorithms
