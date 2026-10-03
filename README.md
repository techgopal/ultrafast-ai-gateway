<p align="center"><img src="docs/brand/banner.png" alt="ultrafast — one small binary between your apps and every model" width="100%"></p>

# Ultrafast Gateway 🚀

> **v2 is in development on this branch.** The v1 code under `ultrafast-gateway/`
> and `ultrafast-models-sdk/` is kept for reference and is tagged `v1-final`.
> Design: `docs/superpowers/specs/2026-09-28-gateway-v2-design.md`.

## v2 quickstart

```bash
cargo build --release -p ultrafast-gateway
export UF_DATA_DIR=./data

# Add a provider. Use kind "openai" for any OpenAI-compatible API.
UF_PROVIDER_API_KEY=sk-... ./target/release/ultrafast provider add \
  --name openai --kind openai --base-url https://api.openai.com/v1
UF_PROVIDER_API_KEY=sk-ant-... ./target/release/ultrafast provider add \
  --name anthropic --kind anthropic --base-url https://api.anthropic.com

# Put models in the catalog, enable them and let everyone call them.
# Without this a provider's models answer 404/403: nothing is callable until
# it is enabled and granted.
./target/release/ultrafast model add --provider openai --model gpt-4o --enable --everyone

# Create a key for your app. It is printed once.
./target/release/ultrafast key create --name my-app

./target/release/ultrafast serve
```

Then call it with the key (see below). Models can also be synced from a
provider, enabled and granted to teams or users in the console (Models page).

**Upgrading from alpha.1.** Existing providers keep working only after an
admin syncs, enables and grants their models: from the console (Models page)
or with `ultrafast model add --provider NAME --model ID --enable --everyone`.
Until then calls to `NAME/MODEL` are refused.

Changes made through the admin API under `/api` apply at once. Changes made
with the CLI reach a running gateway within 30 seconds.

Call it with any OpenAI SDK by setting the base URL to
`http://127.0.0.1:3000/v1` and the model to `provider/model`, for example
`anthropic/claude-sonnet-5`, or to the name of a route.

### Console

The web console is served by the same binary at `/`: a static app compiled
into the binary, with no Node process and no request to any other host at
runtime.

What is in it: setting up the first admin, signing in, accepting an invite;
an overview with a getting-started guide and the last 30 days of requests,
errors, tokens and spend; request logs (a list with filters and a detail of
each call, with the targets tried; no prompt or answer is stored); providers
(OpenAI-compatible, Anthropic, Gemini and Azure OpenAI: add, edit, sync
models, delete); models (enable, who may call each, add by name, set prices);
routing (routes with fallbacks, a response cache, who may use each, and the
health of their targets); virtual keys (create, shown once, limit to chosen
models and routes, revoke, filter); users (invite, role, status, teams, new
invite link, delete); teams (create, rename, delete, members added by email,
and leads); budgets and limits (admins set them; everyone sees what applies
to them); your account (name, password, access tokens); the audit log, for
admins; and a Settings page for admins (how long request logs are kept).
What a user sees depends on their role, and the API decides. Light and dark
themes, following the device until one is chosen, and a layout for phones.

Not yet: the playground, guardrails and MCP tools (shown as coming in the
navigation), and the rest of Settings (sign-in settings, backup,
configuration export and import).

#### Prices, cache, limits, budgets and retention

All of these are set by an admin in the console, or with the admin API under
`/api` (`openapi/admin.json` lists every route).

- **Prices.** Models page, Price on a model: dollars per one million input
  tokens and per one million output tokens. A model without a price is
  logged without a cost (and shown as Unpriced), so spend and budgets are a
  lower bound while any model in use has none. Prices are public (not secret).
- **Route cache.** Routing page, on a route: Cache answers, how long an
  answer is kept (TTL, 1 to 86 400 seconds) and whose calls share an answer
  (team, key or user). Streams and requests with a temperature above 0.5 are
  never cached. A cached answer costs nothing and calls no provider.
- **Limits.** Budgets and limits page, Set limit: requests per minute, tokens
  per minute and concurrent requests, for the gateway, a team, a user or a
  key. The strictest applies; a refused call gets 429 with `Retry-After`.
- **Budgets.** Same page, Set budget: an amount per UTC day, week (from
  Monday) or month for the gateway, a team, a user or a key. `Block` refuses
  calls once the amount is spent (429, `budget_exceeded`); `Alert` allows them
  and writes one audit entry per period.
- **Who is counted.** A call counts against the limits and budgets of its key,
  the key's owner, the gateway and the team of the key; a key without a team
  counts against all of its owner's teams.
- **Retention.** Settings page: request logs older than the number of days
  (1 to 3 650, 30 at first) are deleted.

### Metrics

`GET /metrics` serves Prometheus metrics (text format 0.0.4). It exists only
when a token is set: start the gateway with `UF_METRICS_TOKEN` (or
`--metrics-token`; the variable is safer, a flag shows in the process list)
and scrape with `Authorization: Bearer <token>`. Without a token `/metrics`
is not served (the path is answered like any other the console does not know);
a missing or wrong token answers 401.

```yaml
scrape_configs:
  - job_name: ultrafast
    authorization: { credentials_file: /etc/prometheus/ultrafast-token }
    static_configs: [{ targets: ["127.0.0.1:3000"] }]
```

Metrics: `uf_requests_total{endpoint,status_class}` (endpoint `chat`,
`messages`, `embeddings`; class `2xx`, `4xx`, `5xx`, `499` for a caller that
went away, `other`), `uf_tokens_total{direction}` (answers from the cache are
not counted), `uf_cost_micros_total`, `uf_upstream_duration_seconds{provider}`
(histogram), `uf_log_records_dropped_total`, `uf_log_write_failures_total`,
`uf_cache_hits_total`, `uf_cache_misses_total`,
`uf_rate_limited_total{limit}` (`requests_per_minute`, `tokens_per_minute`,
`concurrent`), `uf_budget_blocked_total` and `uf_circuit_open{provider,model}`
(1 while a breaker is open). `uf_requests_total` counts authenticated chat, messages and embeddings calls
only (not `/v1/models`, not calls refused before authentication);
`uf_cost_micros_total` is the cost as priced by the log writer. Counters start at zero when the gateway starts.
No label names a key, user, team or prompt.

Build: the console is compiled into the binary from `ui/dist`, so build the
console first, then the gateway:

```bash
pnpm --dir ui install --frozen-lockfile && pnpm --dir ui build && cargo build --release -p ultrafast-gateway
```

This needs Node 22 and pnpm. `cargo build` alone never runs Node and works
without it: the binary then serves a page at `/` that says the console was not
built, and `/api`, `/v1`, `/health` and `/metrics` work as usual. The Docker image builds
both.

Develop: run a gateway of your own on a port that nothing else uses (here
3001; never the port of a gateway that is in use) with `--insecure-cookies`
(plain HTTP on your own machine only), and the Vite dev server next to it. The
dev server passes `/api`, `/v1` and `/health` on to `http://127.0.0.1:3001`,
or to the address in `UF_DEV_GATEWAY` when you use another port:

```bash
cargo run -p ultrafast-gateway -- serve --port 3001 --insecure-cookies
pnpm --dir ui dev
# another port: UF_DEV_GATEWAY=http://127.0.0.1:3002 pnpm --dir ui dev
```

Test: `pnpm --dir ui test` runs the unit and component tests. The browser
tests run the release binary built as above (or the one named by
`UF_E2E_BINARY`), each test with its own gateway on a free port and a new
temporary data directory, in Chromium at desktop and phone sizes:

```bash
pnpm --dir ui exec playwright install chromium
pnpm --dir ui test:e2e
```

Known limits:

- The console needs HTTPS, except on localhost: the session cookie is
  `Secure`, so over plain HTTP at any other address the browser drops it and
  the sign-in page says so. On a trusted network, start the gateway with
  `--insecure-cookies` instead.
- Rate limits, budgets, the response cache and the health of routing targets
  live in the memory of one gateway process: they start empty when it starts
  (budgets are counted again from the request logs) and are not shared
  between processes.
- A `block` budget can be overshot: spend is counted when the log writer has
  priced a call (batches of about a second), so calls already running, and
  concurrent ones, are not stopped, and a long call is charged when it ends.
- A stream whose caller left, or that failed after content was sent, is
  charged an estimate (the input at four characters to a token, and the
  streamed characters / 4 for the output), marked Estimated in the logs.
- Any key created or revoked, and any change to what a cached answer depends
  on (teams and users created or deleted, routes with their targets and cache
  settings, providers, models, grants), clears the whole response cache.
  There is no single-flight: calls that miss together all go to the provider.
- Request logs keep metadata only: the prompt and the answer are not stored.
- Only admins set limits and budgets. Team leads see those of their team, and
  members see the gateway's and their teams' without what was spent (their
  own user's and keys' with it); nobody sees another person's spend.
- A budget alert is an audit entry, not an email or a webhook.
- Logs of a deleted user or team stay, with no owner; the id of a deleted
  user or team may be given out again and does not inherit them.
- Behind a reverse proxy, start the gateway with `--trusted-proxy CIDR` (or
  `UF_TRUSTED_PROXIES`) so sign-in limiting counts the client's address, not
  the proxy's. The proxy must set or overwrite `CF-Connecting-IP` and
  `X-Forwarded-For` itself and never pass on what the client sent; the
  gateway believes those headers from the listed networks. Never list a
  network that clients can reach directly. Without the flag, 20 failed
  sign-ins from anyone behind a proxy block sign-in for everyone for 15
  minutes.

What works today: `/v1/chat/completions`, `/v1/messages` (Anthropic format),
`/v1/embeddings` and `/v1/models`, streaming, OpenAI-compatible, Anthropic,
Gemini and Azure OpenAI providers, a model catalog with grants and prices,
routes with fallbacks, circuit breakers and a response cache, request logs
with retention, usage and spend reports, rate limits and budgets, Prometheus
metrics, and the console. Not yet: tools, images, the playground and
guardrails.

> **A high-performance AI gateway built in Rust** that provides a unified interface to 10+ LLM providers with advanced routing, caching, and monitoring capabilities.

[![Rust](https://img.shields.io/badge/Rust-1.94+-orange.svg)](LICENSE)
[![License](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Build Status](https://img.shields.io/github/actions/workflow/status/ultrafast-ai/ultrafast-gateway/ci.yml?branch=main)](https://github.com/ultrafast-ai/ultrafast-gateway/actions)


## Clients

Rust, Python and TypeScript clients for the gateway (or for a provider
directly). All three share one Rust core, `ultrafast-translate`: request
building and response parsing are the code the gateway itself uses, and error
classification and stream decoding are shared by the three clients (the
gateway does not use those two). The TypeScript client runs the core as
WebAssembly. One set of fixtures (`clients/fixtures/`) is run by all three
test suites, so the behaviour those fixtures cover is checked to be the same
in all three. They do not retry, route, cache or break circuits: an error says
whether trying again could help (`retryable`) and how long to wait
(`retry_after`). Every example below uses a key made with `ultrafast key
create` and a model written `provider/model`, or the name of a route.

Rust ([`crates/client`](crates/client/README.md)):

```rust
use ultrafast_client::{ChatRequest, Client, Target};

let client = Client::new(Target::gateway("http://127.0.0.1:3000", key));
let reply = client
    .chat(ChatRequest::new("anthropic/claude-sonnet-5").user("Say hi."))
    .await?;
println!("{}", reply.content);
```

Python ([`crates/client-py`](crates/client-py/README.md)):

```python
import ultrafast

client = ultrafast.Client(ultrafast.gateway("http://127.0.0.1:3000", key))
reply = client.chat("anthropic/claude-sonnet-5", [{"role": "user", "content": "Say hi."}])
print(reply.content)
```

TypeScript ([`clients/ts`](clients/ts/README.md)):

```ts
import { Client, gateway } from "@ultrafast/client";

const client = new Client(gateway({ baseUrl: "http://127.0.0.1:3000", key }));
const reply = await client.chat({
  model: "anthropic/claude-sonnet-5",
  messages: [{ role: "user", content: "Say hi." }],
});
console.log(reply.content);
```

Each client also streams (`chat_stream` / `chatStream`) and makes
embeddings; see its README. Wheels and an npm package are configured but not
published yet: build from source as each README says. Text content only, as
in the gateway; the optional `tags` are sent as `x-uf-tags` and ignored until
the gateway stores them.

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

- **Rust 1.94+** required
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