# Gateway Core, Plan 1: Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A working v2 gateway binary that authenticates a virtual key and proxies OpenAI-format chat requests, streaming and non-streaming, to OpenAI-compatible and Anthropic providers.

**Architecture:** A new Cargo workspace under `crates/`. `translate` converts between the OpenAI wire format, a common request form, and each provider's format, with no networking. `gateway` is an axum server that stores providers and keys in SQLite, authenticates by key hash, and uses `translate` plus reqwest to call providers.

**Tech Stack:** Rust 1.82+, axum 0.8, tokio, reqwest 0.12 (rustls), sqlx 0.8 (SQLite), serde, chacha20poly1305, sha2, clap 4, wiremock (tests).

**Spec:** `docs/superpowers/specs/2026-09-28-gateway-v2-design.md`

## Where this plan sits

The gateway core is too large for one plan. It is split into six, each ending in working software:

| Plan | Delivers |
|---|---|
| **1. Foundation (this plan)** | Workspace, `translate` for OpenAI and Anthropic, storage, key auth, chat proxy with streaming, CLI |
| 2. Identity and admin API | Users, teams, roles, sessions, access tokens, `/api`, in-memory snapshot, OpenAPI spec |
| 3. Catalog and routing | Models page backend, model grants, routes, fallback, retries, timeouts, circuit breaker |
| 4. Limits, budgets, cache | Rate limits, budgets, response cache |
| 5. Logs, audit, metrics | Request log queue, retention, audit log, Prometheus, config export/import |
| 6. Formats and providers | Anthropic Messages ingress, embeddings, tools, Gemini, Azure, remaining providers |

Known gaps in plan 1, closed by later plans: tools and images are rejected with a clear error (plan 6); keys and providers are read from SQLite on each request (plan 2 adds the in-memory snapshot); no routes, so the `model` field must be `provider/model` (plan 3).

## Global Constraints

- Workspace members live under `crates/`. v1 directories `ultrafast-gateway/` and `ultrafast-models-sdk/` stay on disk, excluded from the workspace, and are not edited.
- `Cargo.lock` is committed.
- `crates/translate` performs no I/O and depends on no async runtime or HTTP library.
- Streaming parses bytes, not strings, and buffers until a complete event arrives.
- Content a target format cannot express is an error, never silently dropped.
- Virtual keys start with `uf-sk-`, are stored as a SHA-256 hex hash, and the full key is shown once.
- Provider credentials are encrypted with the master key and never returned or logged.
- Every table has `org_id INTEGER NOT NULL DEFAULT 1`.
- `/v1` errors use the OpenAI error shape, built with `serde_json`, never string concatenation.
- Environment variables: `UF_DATA_DIR` (default `./data`), `UF_MASTER_KEY`, `UF_HOST`, `UF_PORT`. Flags win over environment variables.
- CI runs `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --all`.
- Commit messages end with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.

## Review Focus

1. A stream chunk boundary falls inside a multi-byte character or inside an event: output must be identical to the unsplit stream. Pinned in Task 3 and Task 4.
2. A provider returns a non-JSON error body (an HTML 502 page): the caller gets a JSON error with the provider status preserved as retryable, not a parse failure. Pinned in Task 4.
3. A missing, malformed, revoked or expired key: 401 in OpenAI error shape, and the provider is never called. Pinned in Task 8.
4. A request body over the size limit: 413 in OpenAI error shape. Pinned in Task 8.
5. A provider stream ends without its completion marker: the caller receives an error event and the stream closes, not a silent truncation. Pinned in Task 9.

## File Structure

```
Cargo.toml                          workspace root (rewritten)
.gitignore                          stop ignoring Cargo.lock
.github/workflows/ci.yml            fmt, clippy, test, docker
Dockerfile                          v2 image (rewritten in Task 10)
crates/translate/
  Cargo.toml
  src/lib.rs                        module list
  src/types.rs                      ChatRequest, ChatResponse, StreamEvent
  src/error.rs                      TranslateError
  src/sse.rs                        byte-level SSE parser
  src/ingress/mod.rs
  src/ingress/openai.rs             OpenAI wire format in and out
  src/provider/mod.rs               ProviderKind, Target, HttpRequest, StreamDecoder
  src/provider/openai.rs            OpenAI-compatible providers
  src/provider/anthropic.rs         Anthropic
crates/gateway/
  Cargo.toml
  migrations/0001_init.sql
  src/lib.rs                        module list
  src/main.rs                       CLI
  src/config.rs                     data dir, master key
  src/secrets.rs                    key generation, hashing, Cipher
  src/store.rs                      SQLite access
  src/errors.rs                     OpenAI-shaped error responses
  src/auth.rs                       bearer key check
  src/app.rs                        AppState, router
  src/proxy.rs                      chat completions handler
  tests/common/mod.rs               test harness
  tests/proxy.rs
  tests/stream.rs
```

---

### Task 1: Workspace reset

**Files:**
- Modify: `Cargo.toml`, `.gitignore`, `.github/workflows/ci.yml`
- Create: `crates/translate/Cargo.toml`, `crates/translate/src/lib.rs`, `crates/gateway/Cargo.toml`, `crates/gateway/src/lib.rs`, `crates/gateway/src/main.rs`

**Interfaces:**
- Consumes: nothing
- Produces: crates `ultrafast-translate` (lib name `ultrafast_translate`) and `ultrafast-gateway` (lib name `ultrafast_gateway`, binary `ultrafast`)

- [ ] **Step 1: Tag v1**

```bash
git tag v1-final main
```

- [ ] **Step 2: Replace root `Cargo.toml`**

```toml
[workspace]
resolver = "2"
members = ["crates/translate", "crates/gateway"]
exclude = ["ultrafast-gateway", "ultrafast-models-sdk"]

[workspace.package]
version = "2.0.0-alpha.1"
edition = "2021"
rust-version = "1.82"
license = "MIT"
repository = "https://github.com/techgopal/ultrafast-ai-gateway"

[workspace.dependencies]
anyhow = "1"
async-stream = "0.3"
axum = "0.8"
bytes = "1"
chacha20poly1305 = "0.10"
clap = { version = "4", features = ["derive", "env"] }
futures = "0.3"
hex = "0.4"
rand = "0.8"
reqwest = { version = "0.12", default-features = false, features = ["json", "stream", "rustls-tls"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sha2 = "0.10"
sqlx = { version = "0.8", default-features = false, features = ["runtime-tokio", "sqlite", "migrate"] }
tempfile = "3"
thiserror = "2"
tokio = { version = "1", features = ["full"] }
tower = { version = "0.5", features = ["util"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
wiremock = "0.6"
```

- [ ] **Step 3: Create `crates/translate/Cargo.toml`**

```toml
[package]
name = "ultrafast-translate"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
description = "Provider format translation for Ultrafast. No networking."

[dependencies]
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
```

- [ ] **Step 4: Create `crates/translate/src/lib.rs`**

```rust
//! Provider format translation. This crate performs no I/O.
```

- [ ] **Step 5: Create `crates/gateway/Cargo.toml`**

```toml
[package]
name = "ultrafast-gateway"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
description = "Ultrafast AI gateway"

[[bin]]
name = "ultrafast"
path = "src/main.rs"

[dependencies]
ultrafast-translate = { path = "../translate" }
anyhow.workspace = true
async-stream.workspace = true
axum.workspace = true
bytes.workspace = true
chacha20poly1305.workspace = true
clap.workspace = true
futures.workspace = true
hex.workspace = true
rand.workspace = true
reqwest.workspace = true
serde.workspace = true
serde_json.workspace = true
sha2.workspace = true
sqlx.workspace = true
thiserror.workspace = true
tokio.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true

[dev-dependencies]
tempfile.workspace = true
tower.workspace = true
wiremock.workspace = true
```

- [ ] **Step 6: Create `crates/gateway/src/lib.rs` and `crates/gateway/src/main.rs`**

`lib.rs`:

```rust
//! Ultrafast gateway library.
```

`main.rs`:

```rust
fn main() {
    println!("ultrafast {}", env!("CARGO_PKG_VERSION"));
}
```

- [ ] **Step 7: Stop ignoring the lockfile**

In `.gitignore`, delete the line `Cargo.lock` under `# Cargo`.

- [ ] **Step 8: Replace `.github/workflows/ci.yml`**

```yaml
name: CI

on:
  push:
    branches: [main, v2]
  pull_request:
    branches: [main, v2]

env:
  CARGO_TERM_COLOR: always
  RUST_BACKTRACE: 1

jobs:
  build-test:
    name: Lint and test
    runs-on: ubuntu-latest
    timeout-minutes: 25
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/registry
            ~/.cargo/git
            target
          key: ${{ runner.os }}-cargo-${{ hashFiles('Cargo.lock') }}
      - name: Format check
        run: cargo fmt --all -- --check
      - name: Clippy
        run: cargo clippy --all-targets --all-features -- -D warnings
      - name: Tests
        run: cargo test --all --no-fail-fast
```

- [ ] **Step 9: Verify**

Run: `cargo build --all && cargo run -p ultrafast-gateway`
Expected: build succeeds; prints `ultrafast 2.0.0-alpha.1`.

Run: `cargo fmt --all -- --check && cargo clippy --all-targets --all-features -- -D warnings`
Expected: no output, exit code 0.

- [ ] **Step 10: Commit**

```bash
git add Cargo.toml Cargo.lock .gitignore .github/workflows/ci.yml crates
git commit -m "chore: start v2 workspace under crates/"
```

---

### Task 2: Common types and OpenAI ingress

**Files:**
- Create: `crates/translate/src/types.rs`, `crates/translate/src/error.rs`, `crates/translate/src/ingress/mod.rs`, `crates/translate/src/ingress/openai.rs`
- Modify: `crates/translate/src/lib.rs`

**Interfaces:**
- Consumes: nothing
- Produces:
  - `types::{Role, Message, ChatRequest, ChatResponse, Usage, FinishReason, StreamEvent}`
  - `error::TranslateError`
  - `ingress::openai::parse_request(body: &[u8]) -> Result<ChatRequest, TranslateError>`
  - `ingress::openai::render_response(r: &ChatResponse, created: u64) -> serde_json::Value`
  - `ingress::openai::render_stream_event(ev: &StreamEvent, id: &str, model: &str, created: u64) -> String`
  - `ingress::openai::render_stream_error(message: &str) -> String`
  - `ingress::openai::render_error(kind: &str, message: &str) -> serde_json::Value`

- [ ] **Step 1: Write `crates/translate/src/lib.rs`**

```rust
//! Provider format translation. This crate performs no I/O.

pub mod error;
pub mod ingress;
pub mod types;
```

- [ ] **Step 2: Write `crates/translate/src/error.rs`**

```rust
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Error)]
pub enum TranslateError {
    /// The request is not valid in the caller's format.
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    /// The request is valid but uses something this target cannot express.
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// The provider answered with an error.
    #[error("provider error {status}: {message}")]
    Provider {
        status: u16,
        retryable: bool,
        message: String,
    },
    /// The provider answered with something that could not be read.
    #[error("malformed provider response: {0}")]
    Malformed(String),
}
```

- [ ] **Step 3: Write `crates/translate/src/types.rs`**

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub stop: Option<Vec<String>>,
    pub stream: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FinishReason {
    Stop,
    Length,
    ToolCalls,
    ContentFilter,
}

impl FinishReason {
    pub fn as_openai(self) -> &'static str {
        match self {
            FinishReason::Stop => "stop",
            FinishReason::Length => "length",
            FinishReason::ToolCalls => "tool_calls",
            FinishReason::ContentFilter => "content_filter",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatResponse {
    pub id: String,
    pub model: String,
    pub content: String,
    pub finish_reason: Option<FinishReason>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    Delta { text: String },
    Done {
        finish_reason: Option<FinishReason>,
        usage: Option<Usage>,
    },
}
```

- [ ] **Step 4: Write `crates/translate/src/ingress/mod.rs`**

```rust
pub mod openai;
```

- [ ] **Step 5: Write the failing tests in `crates/translate/src/ingress/openai.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::TranslateError;
    use crate::types::*;

    #[test]
    fn parses_minimal_request() {
        let body = br#"{"model":"openai/gpt-4o","messages":[{"role":"user","content":"hi"}]}"#;
        let req = parse_request(body).unwrap();
        assert_eq!(req.model, "openai/gpt-4o");
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.messages[0].role, Role::User);
        assert_eq!(req.messages[0].content, "hi");
        assert!(!req.stream);
    }

    #[test]
    fn parses_text_parts_and_single_stop() {
        let body = br#"{"model":"m","stream":true,"stop":"END","max_completion_tokens":9,
            "messages":[{"role":"developer","content":[{"type":"text","text":"a"},{"type":"text","text":"b"}]}]}"#;
        let req = parse_request(body).unwrap();
        assert_eq!(req.messages[0].role, Role::System);
        assert_eq!(req.messages[0].content, "ab");
        assert_eq!(req.stop, Some(vec!["END".to_string()]));
        assert_eq!(req.max_tokens, Some(9));
        assert!(req.stream);
    }

    #[test]
    fn rejects_tools_and_images_instead_of_dropping_them() {
        let tools = br#"{"model":"m","messages":[{"role":"user","content":"x"}],"tools":[{"type":"function"}]}"#;
        assert!(matches!(parse_request(tools), Err(TranslateError::Unsupported(_))));
        let image = br#"{"model":"m","messages":[{"role":"user","content":[{"type":"image_url","image_url":{"url":"u"}}]}]}"#;
        assert!(matches!(parse_request(image), Err(TranslateError::Unsupported(_))));
        let tool_msg = br#"{"model":"m","messages":[{"role":"tool","content":"x"}]}"#;
        assert!(matches!(parse_request(tool_msg), Err(TranslateError::Unsupported(_))));
    }

    #[test]
    fn rejects_bad_json_and_empty_messages() {
        assert!(matches!(parse_request(b"{"), Err(TranslateError::InvalidRequest(_))));
        let empty = br#"{"model":"m","messages":[]}"#;
        assert!(matches!(parse_request(empty), Err(TranslateError::InvalidRequest(_))));
    }

    #[test]
    fn renders_response() {
        let r = ChatResponse {
            id: "id1".into(),
            model: "gpt-4o".into(),
            content: "hello".into(),
            finish_reason: Some(FinishReason::Stop),
            usage: Some(Usage { input_tokens: 3, output_tokens: 2 }),
        };
        let v = render_response(&r, 100);
        assert_eq!(v["object"], "chat.completion");
        assert_eq!(v["created"], 100);
        assert_eq!(v["choices"][0]["message"]["content"], "hello");
        assert_eq!(v["choices"][0]["finish_reason"], "stop");
        assert_eq!(v["usage"]["prompt_tokens"], 3);
        assert_eq!(v["usage"]["completion_tokens"], 2);
        assert_eq!(v["usage"]["total_tokens"], 5);
    }

    #[test]
    fn renders_stream_events() {
        let d = render_stream_event(&StreamEvent::Delta { text: "a\"b".into() }, "id1", "m", 1);
        assert!(d.starts_with("data: "));
        assert!(d.ends_with("\n\n"));
        let v: serde_json::Value = serde_json::from_str(d["data: ".len()..].trim()).unwrap();
        assert_eq!(v["object"], "chat.completion.chunk");
        assert_eq!(v["choices"][0]["delta"]["content"], "a\"b");

        let done = render_stream_event(
            &StreamEvent::Done { finish_reason: Some(FinishReason::Length), usage: None },
            "id1", "m", 1,
        );
        assert!(done.contains("\"finish_reason\":\"length\""));
        assert!(done.ends_with("data: [DONE]\n\n"));
    }

    #[test]
    fn error_text_is_escaped() {
        let v = render_error("invalid_request_error", "bad \"quote\"");
        assert_eq!(v["error"]["message"], "bad \"quote\"");
        assert_eq!(v["error"]["type"], "invalid_request_error");
        let s = render_stream_error("x\ny");
        let v: serde_json::Value = serde_json::from_str(s["data: ".len()..].trim()).unwrap();
        assert_eq!(v["error"]["message"], "x\ny");
    }
}
```

- [ ] **Step 6: Run the tests to see them fail**

Run: `cargo test -p ultrafast-translate ingress`
Expected: compile errors, `cannot find function parse_request`.

- [ ] **Step 7: Write the implementation at the top of `crates/translate/src/ingress/openai.rs`**

```rust
//! OpenAI Chat Completions wire format, as received from and returned to callers.

use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::TranslateError;
use crate::types::{ChatRequest, ChatResponse, Message, Role, StreamEvent};

#[derive(Deserialize)]
struct WireRequest {
    model: String,
    messages: Vec<WireMessage>,
    #[serde(default)]
    max_tokens: Option<u32>,
    #[serde(default)]
    max_completion_tokens: Option<u32>,
    #[serde(default)]
    temperature: Option<f32>,
    #[serde(default)]
    top_p: Option<f32>,
    #[serde(default)]
    stop: Option<StopField>,
    #[serde(default)]
    stream: bool,
    #[serde(default)]
    tools: Option<Value>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum StopField {
    One(String),
    Many(Vec<String>),
}

#[derive(Deserialize)]
struct WireMessage {
    role: String,
    #[serde(default)]
    content: Option<WireContent>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum WireContent {
    Text(String),
    Parts(Vec<Value>),
}

pub fn parse_request(body: &[u8]) -> Result<ChatRequest, TranslateError> {
    let wire: WireRequest =
        serde_json::from_slice(body).map_err(|e| TranslateError::InvalidRequest(e.to_string()))?;
    if wire.tools.is_some() {
        return Err(TranslateError::Unsupported("tools are not supported yet".into()));
    }
    if wire.messages.is_empty() {
        return Err(TranslateError::InvalidRequest("messages must not be empty".into()));
    }
    let mut messages = Vec::with_capacity(wire.messages.len());
    for m in wire.messages {
        let role = match m.role.as_str() {
            "system" | "developer" => Role::System,
            "user" => Role::User,
            "assistant" => Role::Assistant,
            other => {
                return Err(TranslateError::Unsupported(format!(
                    "message role '{other}' is not supported yet"
                )))
            }
        };
        let content = match m.content {
            None => String::new(),
            Some(WireContent::Text(t)) => t,
            Some(WireContent::Parts(parts)) => {
                let mut out = String::new();
                for p in parts {
                    match (p["type"].as_str(), p["text"].as_str()) {
                        (Some("text"), Some(t)) => out.push_str(t),
                        (kind, _) => {
                            return Err(TranslateError::Unsupported(format!(
                                "content part '{}' is not supported yet",
                                kind.unwrap_or("unknown")
                            )))
                        }
                    }
                }
                out
            }
        };
        messages.push(Message { role, content, name: m.name });
    }
    Ok(ChatRequest {
        model: wire.model,
        messages,
        max_tokens: wire.max_completion_tokens.or(wire.max_tokens),
        temperature: wire.temperature,
        top_p: wire.top_p,
        stop: wire.stop.map(|s| match s {
            StopField::One(s) => vec![s],
            StopField::Many(v) => v,
        }),
        stream: wire.stream,
    })
}

pub fn render_response(r: &ChatResponse, created: u64) -> Value {
    let mut v = json!({
        "id": r.id,
        "object": "chat.completion",
        "created": created,
        "model": r.model,
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": r.content },
            "finish_reason": r.finish_reason.map(|f| f.as_openai()),
        }],
    });
    if let Some(u) = r.usage {
        v["usage"] = json!({
            "prompt_tokens": u.input_tokens,
            "completion_tokens": u.output_tokens,
            "total_tokens": u.input_tokens + u.output_tokens,
        });
    }
    v
}

pub fn render_stream_event(ev: &StreamEvent, id: &str, model: &str, created: u64) -> String {
    match ev {
        StreamEvent::Delta { text } => {
            let v = json!({
                "id": id, "object": "chat.completion.chunk", "created": created, "model": model,
                "choices": [{ "index": 0, "delta": { "content": text }, "finish_reason": null }],
            });
            format!("data: {v}\n\n")
        }
        StreamEvent::Done { finish_reason, usage } => {
            let mut v = json!({
                "id": id, "object": "chat.completion.chunk", "created": created, "model": model,
                "choices": [{ "index": 0, "delta": {}, "finish_reason": finish_reason.map(|f| f.as_openai()) }],
            });
            if let Some(u) = usage {
                v["usage"] = json!({
                    "prompt_tokens": u.input_tokens,
                    "completion_tokens": u.output_tokens,
                    "total_tokens": u.input_tokens + u.output_tokens,
                });
            }
            format!("data: {v}\n\ndata: [DONE]\n\n")
        }
    }
}

pub fn render_error(kind: &str, message: &str) -> Value {
    json!({ "error": { "message": message, "type": kind, "param": null, "code": null } })
}

pub fn render_stream_error(message: &str) -> String {
    format!("data: {}\n\n", render_error("upstream_error", message))
}
```

- [ ] **Step 8: Run the tests**

Run: `cargo test -p ultrafast-translate ingress`
Expected: 7 passed.

- [ ] **Step 9: Commit**

```bash
git add crates/translate
git commit -m "feat(translate): common types and OpenAI ingress"
```

---

### Task 3: Byte-level SSE parser

**Files:**
- Create: `crates/translate/src/sse.rs`
- Modify: `crates/translate/src/lib.rs` (add `pub mod sse;`)

**Interfaces:**
- Consumes: nothing
- Produces: `sse::SseEvent { event: Option<String>, data: String }`, `sse::SseParser::new()`, `SseParser::feed(&mut self, chunk: &[u8]) -> Vec<SseEvent>`

- [ ] **Step 1: Add `pub mod sse;` to `lib.rs`, then write the failing tests in `crates/translate/src/sse.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn all(input: &[u8]) -> Vec<SseEvent> {
        SseParser::new().feed(input)
    }

    #[test]
    fn parses_events_with_lf_and_crlf() {
        let evs = all(b"event: a\ndata: 1\n\ndata: 2\r\n\r\n");
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[0], SseEvent { event: Some("a".into()), data: "1".into() });
        assert_eq!(evs[1], SseEvent { event: None, data: "2".into() });
    }

    #[test]
    fn joins_multiple_data_lines_and_skips_comments() {
        let evs = all(b": ping\n\ndata: a\ndata:b\n\n");
        assert_eq!(evs, vec![SseEvent { event: None, data: "a\nb".into() }]);
    }

    #[test]
    fn keeps_incomplete_event_until_more_arrives() {
        let mut p = SseParser::new();
        assert!(p.feed(b"data: par").is_empty());
        assert!(p.feed(b"tial\n").is_empty());
        assert_eq!(p.feed(b"\n"), vec![SseEvent { event: None, data: "partial".into() }]);
    }

    #[test]
    fn identical_output_for_every_split_point() {
        let input = "data: {\"t\":\"h\u{e9}llo \u{1f600}\"}\n\nevent: x\r\ndata: two\r\n\r\ndata: [DONE]\n\n".as_bytes();
        let expected = all(input);
        assert_eq!(expected.len(), 3);
        assert_eq!(expected[0].data, "{\"t\":\"h\u{e9}llo \u{1f600}\"}");
        for i in 0..=input.len() {
            let mut p = SseParser::new();
            let mut got = p.feed(&input[..i]);
            got.extend(p.feed(&input[i..]));
            assert_eq!(got, expected, "split at byte {i}");
        }
    }

    #[test]
    fn identical_output_one_byte_at_a_time() {
        let input = "data: \u{4f60}\u{597d}\n\ndata: b\n\n".as_bytes();
        let mut p = SseParser::new();
        let mut got = Vec::new();
        for b in input {
            got.extend(p.feed(&[*b]));
        }
        assert_eq!(got, all(input));
        assert_eq!(got[0].data, "\u{4f60}\u{597d}");
    }
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p ultrafast-translate sse`
Expected: compile errors, `cannot find type SseParser`.

- [ ] **Step 3: Write the implementation at the top of `crates/translate/src/sse.rs`**

```rust
//! Server-sent events parser that works on bytes.
//!
//! Bytes are buffered until a full event (terminated by a blank line) is
//! present. Only complete events are decoded as text, so a chunk boundary
//! inside a multi-byte character cannot corrupt it.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

#[derive(Debug, Default)]
pub struct SseParser {
    buf: Vec<u8>,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn feed(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        while let Some((end, sep_len)) = find_boundary(&self.buf) {
            let block: Vec<u8> = self.buf.drain(..end + sep_len).take(end).collect();
            if let Some(ev) = parse_block(&block) {
                out.push(ev);
            }
        }
        out
    }
}

/// Returns the index where the first event ends and the separator length.
fn find_boundary(buf: &[u8]) -> Option<(usize, usize)> {
    let lf = buf.windows(2).position(|w| w == b"\n\n").map(|i| (i, 2));
    let crlf = buf.windows(4).position(|w| w == b"\r\n\r\n").map(|i| (i, 4));
    match (lf, crlf) {
        (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
        (a, b) => a.or(b),
    }
}

fn parse_block(block: &[u8]) -> Option<SseEvent> {
    let text = String::from_utf8_lossy(block);
    let mut event = None;
    let mut data: Vec<&str> = Vec::new();
    for line in text.lines() {
        if line.starts_with(':') {
            continue;
        }
        if let Some(v) = line.strip_prefix("data:") {
            data.push(v.strip_prefix(' ').unwrap_or(v));
        } else if let Some(v) = line.strip_prefix("event:") {
            event = Some(v.trim().to_string());
        }
    }
    if data.is_empty() && event.is_none() {
        return None;
    }
    Some(SseEvent { event, data: data.join("\n") })
}
```

Note on `find_boundary`: `\r\n\r\n` never contains `\n\n`, so the two searches cannot match the same separator.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ultrafast-translate sse`
Expected: 5 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/translate
git commit -m "feat(translate): byte-level SSE parser"
```

---

### Task 4: OpenAI-compatible provider

**Files:**
- Create: `crates/translate/src/provider/mod.rs`, `crates/translate/src/provider/openai.rs`
- Modify: `crates/translate/src/lib.rs` (add `pub mod provider;`)

**Interfaces:**
- Consumes: `types::*`, `error::TranslateError`, `sse::{SseParser, SseEvent}`
- Produces:
  - `provider::ProviderKind` (variant `OpenAi`), with `ProviderKind::parse(s: &str) -> Option<ProviderKind>` and `as_str(self) -> &'static str`
  - `provider::Target { kind: ProviderKind, base_url: String, api_key: Option<String>, model: String }`
  - `provider::HttpRequest { method: &'static str, url: String, headers: Vec<(String, String)>, body: Vec<u8> }`
  - `provider::build_request(target: &Target, req: &ChatRequest) -> Result<HttpRequest, TranslateError>`
  - `provider::parse_response(kind: ProviderKind, status: u16, body: &[u8]) -> Result<ChatResponse, TranslateError>`
  - `provider::StreamDecoder::new(kind: ProviderKind)`, `StreamDecoder::feed(&mut self, chunk: &[u8]) -> Result<Vec<StreamEvent>, TranslateError>`
  - `pub(crate) provider::StreamState { finish: Option<FinishReason>, input_tokens: Option<u32>, output_tokens: Option<u32> }` with `usage(&self) -> Option<Usage>`
  - `pub(crate) provider::provider_error(status: u16, body: &[u8]) -> TranslateError`

- [ ] **Step 1: Add `pub mod provider;` to `lib.rs` and write `crates/translate/src/provider/mod.rs`**

```rust
//! Outbound side: turning a common request into a provider call and back.

mod openai;

use serde_json::Value;

use crate::error::TranslateError;
use crate::sse::SseParser;
use crate::types::{ChatRequest, ChatResponse, FinishReason, StreamEvent, Usage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    /// OpenAI and every OpenAI-compatible API (Groq, Mistral, OpenRouter, Ollama).
    OpenAi,
}

impl ProviderKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "openai" => Some(ProviderKind::OpenAi),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ProviderKind::OpenAi => "openai",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub kind: ProviderKind,
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HttpRequest {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

pub fn build_request(target: &Target, req: &ChatRequest) -> Result<HttpRequest, TranslateError> {
    match target.kind {
        ProviderKind::OpenAi => openai::build(target, req),
    }
}

pub fn parse_response(
    kind: ProviderKind,
    status: u16,
    body: &[u8],
) -> Result<ChatResponse, TranslateError> {
    if status >= 400 {
        return Err(provider_error(status, body));
    }
    match kind {
        ProviderKind::OpenAi => openai::parse(body),
    }
}

/// Builds a provider error from any body, JSON or not.
pub(crate) fn provider_error(status: u16, body: &[u8]) -> TranslateError {
    let message = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| {
            v["error"]["message"]
                .as_str()
                .or_else(|| v["message"].as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| {
            let text = String::from_utf8_lossy(body);
            text.chars().take(500).collect()
        });
    TranslateError::Provider {
        status,
        retryable: status == 408 || status == 429 || status >= 500,
        message,
    }
}

#[derive(Debug, Default)]
pub(crate) struct StreamState {
    pub finish: Option<FinishReason>,
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
}

impl StreamState {
    pub fn usage(&self) -> Option<Usage> {
        match (self.input_tokens, self.output_tokens) {
            (None, None) => None,
            (i, o) => Some(Usage {
                input_tokens: i.unwrap_or(0),
                output_tokens: o.unwrap_or(0),
            }),
        }
    }
}

pub struct StreamDecoder {
    kind: ProviderKind,
    sse: SseParser,
    state: StreamState,
}

impl StreamDecoder {
    pub fn new(kind: ProviderKind) -> Self {
        Self { kind, sse: SseParser::new(), state: StreamState::default() }
    }

    pub fn feed(&mut self, chunk: &[u8]) -> Result<Vec<StreamEvent>, TranslateError> {
        let mut out = Vec::new();
        for ev in self.sse.feed(chunk) {
            match self.kind {
                ProviderKind::OpenAi => openai::decode(&mut self.state, &ev, &mut out)?,
            }
        }
        Ok(out)
    }
}
```

- [ ] **Step 2: Write the failing tests in `crates/translate/src/provider/openai.rs`**

```rust
#[cfg(test)]
mod tests {
    use crate::error::TranslateError;
    use crate::provider::*;
    use crate::types::*;

    fn target() -> Target {
        Target {
            kind: ProviderKind::OpenAi,
            base_url: "https://api.example.com/v1/".into(),
            api_key: Some("sk-x".into()),
            model: "gpt-4o".into(),
        }
    }

    fn request(stream: bool) -> ChatRequest {
        ChatRequest {
            model: "openai/gpt-4o".into(),
            messages: vec![Message { role: Role::User, content: "hi".into(), name: None }],
            max_tokens: Some(5),
            temperature: None,
            top_p: None,
            stop: None,
            stream,
        }
    }

    #[test]
    fn builds_request_with_target_model_and_auth() {
        let r = build_request(&target(), &request(false)).unwrap();
        assert_eq!(r.method, "POST");
        assert_eq!(r.url, "https://api.example.com/v1/chat/completions");
        assert!(r.headers.contains(&("authorization".into(), "Bearer sk-x".into())));
        let v: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(v["model"], "gpt-4o");
        assert_eq!(v["max_tokens"], 5);
        assert_eq!(v["messages"][0]["content"], "hi");
        assert!(v.get("stream").is_none());
        assert!(v.get("temperature").is_none());
    }

    #[test]
    fn stream_request_asks_for_usage() {
        let r = build_request(&target(), &request(true)).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(v["stream"], true);
        assert_eq!(v["stream_options"]["include_usage"], true);
    }

    #[test]
    fn omits_auth_header_without_key() {
        let mut t = target();
        t.api_key = None;
        let r = build_request(&t, &request(false)).unwrap();
        assert!(!r.headers.iter().any(|(k, _)| k == "authorization"));
    }

    #[test]
    fn parses_response() {
        let body = br#"{"id":"c1","model":"gpt-4o","choices":[{"message":{"role":"assistant","content":"yo"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":2}}"#;
        let r = parse_response(ProviderKind::OpenAi, 200, body).unwrap();
        assert_eq!(r.content, "yo");
        assert_eq!(r.finish_reason, Some(FinishReason::Stop));
        assert_eq!(r.usage, Some(Usage { input_tokens: 1, output_tokens: 2 }));
    }

    #[test]
    fn json_error_body_keeps_message() {
        let body = br#"{"error":{"message":"bad key","type":"auth"}}"#;
        let e = parse_response(ProviderKind::OpenAi, 401, body).unwrap_err();
        assert_eq!(e, TranslateError::Provider { status: 401, retryable: false, message: "bad key".into() });
    }

    #[test]
    fn html_error_body_is_a_retryable_provider_error() {
        let e = parse_response(ProviderKind::OpenAi, 502, b"<html>Bad Gateway</html>").unwrap_err();
        assert_eq!(
            e,
            TranslateError::Provider { status: 502, retryable: true, message: "<html>Bad Gateway</html>".into() }
        );
    }

    #[test]
    fn success_status_with_unreadable_body_is_malformed() {
        let e = parse_response(ProviderKind::OpenAi, 200, b"not json").unwrap_err();
        assert!(matches!(e, TranslateError::Malformed(_)));
    }

    const STREAM: &str = concat!(
        "data: {\"id\":\"c\",\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"c\",\"choices\":[{\"delta\":{\"content\":\"h\u{e9}\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"c\",\"choices\":[{\"delta\":{\"content\":\"y \u{1f600}\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"c\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: {\"id\":\"c\",\"choices\":[],\"usage\":{\"prompt_tokens\":4,\"completion_tokens\":6}}\n\n",
        "data: [DONE]\n\n",
    );

    fn expected() -> Vec<StreamEvent> {
        vec![
            StreamEvent::Delta { text: "h\u{e9}".into() },
            StreamEvent::Delta { text: "y \u{1f600}".into() },
            StreamEvent::Done {
                finish_reason: Some(FinishReason::Stop),
                usage: Some(Usage { input_tokens: 4, output_tokens: 6 }),
            },
        ]
    }

    #[test]
    fn decodes_stream() {
        let mut d = StreamDecoder::new(ProviderKind::OpenAi);
        assert_eq!(d.feed(STREAM.as_bytes()).unwrap(), expected());
    }

    #[test]
    fn decodes_stream_identically_at_every_split_point() {
        let bytes = STREAM.as_bytes();
        for i in 0..=bytes.len() {
            let mut d = StreamDecoder::new(ProviderKind::OpenAi);
            let mut got = d.feed(&bytes[..i]).unwrap();
            got.extend(d.feed(&bytes[i..]).unwrap());
            assert_eq!(got, expected(), "split at byte {i}");
        }
    }

    #[test]
    fn error_inside_stream_is_reported() {
        let mut d = StreamDecoder::new(ProviderKind::OpenAi);
        let e = d.feed(b"data: {\"error\":{\"message\":\"overloaded\"}}\n\n").unwrap_err();
        assert!(matches!(e, TranslateError::Provider { message, .. } if message == "overloaded"));
    }
}
```

- [ ] **Step 3: Run to see them fail**

Run: `cargo test -p ultrafast-translate provider::openai`
Expected: compile errors, `cannot find function build in module openai`.

- [ ] **Step 4: Write the implementation at the top of `crates/translate/src/provider/openai.rs`**

```rust
use serde::Deserialize;
use serde_json::{json, Value};

use super::{HttpRequest, StreamState, Target};
use crate::error::TranslateError;
use crate::sse::SseEvent;
use crate::types::{ChatRequest, ChatResponse, FinishReason, Role, StreamEvent, Usage};

fn role_str(r: Role) -> &'static str {
    match r {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
    }
}

fn finish(s: &str) -> Option<FinishReason> {
    match s {
        "stop" => Some(FinishReason::Stop),
        "length" => Some(FinishReason::Length),
        "tool_calls" => Some(FinishReason::ToolCalls),
        "content_filter" => Some(FinishReason::ContentFilter),
        _ => None,
    }
}

pub(crate) fn build(target: &Target, req: &ChatRequest) -> Result<HttpRequest, TranslateError> {
    let messages: Vec<Value> = req
        .messages
        .iter()
        .map(|m| {
            let mut o = json!({ "role": role_str(m.role), "content": m.content });
            if let Some(n) = &m.name {
                o["name"] = json!(n);
            }
            o
        })
        .collect();
    let mut body = json!({ "model": target.model, "messages": messages });
    if let Some(v) = req.max_tokens {
        body["max_tokens"] = json!(v);
    }
    if let Some(v) = req.temperature {
        body["temperature"] = json!(v);
    }
    if let Some(v) = req.top_p {
        body["top_p"] = json!(v);
    }
    if let Some(v) = &req.stop {
        body["stop"] = json!(v);
    }
    if req.stream {
        body["stream"] = json!(true);
        body["stream_options"] = json!({ "include_usage": true });
    }
    let mut headers = vec![("content-type".to_string(), "application/json".to_string())];
    if let Some(k) = &target.api_key {
        headers.push(("authorization".to_string(), format!("Bearer {k}")));
    }
    Ok(HttpRequest {
        method: "POST",
        url: format!("{}/chat/completions", target.base_url.trim_end_matches('/')),
        headers,
        body: serde_json::to_vec(&body).map_err(|e| TranslateError::InvalidRequest(e.to_string()))?,
    })
}

#[derive(Deserialize)]
struct WireResponse {
    id: String,
    model: String,
    choices: Vec<WireChoice>,
    #[serde(default)]
    usage: Option<WireUsage>,
}

#[derive(Deserialize)]
struct WireChoice {
    message: WireMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct WireMessage {
    #[serde(default)]
    content: Option<String>,
}

#[derive(Deserialize)]
struct WireUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

pub(crate) fn parse(body: &[u8]) -> Result<ChatResponse, TranslateError> {
    let w: WireResponse =
        serde_json::from_slice(body).map_err(|e| TranslateError::Malformed(e.to_string()))?;
    let choice = w
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| TranslateError::Malformed("response has no choices".into()))?;
    Ok(ChatResponse {
        id: w.id,
        model: w.model,
        content: choice.message.content.unwrap_or_default(),
        finish_reason: choice.finish_reason.as_deref().and_then(finish),
        usage: w.usage.map(|u| Usage {
            input_tokens: u.prompt_tokens,
            output_tokens: u.completion_tokens,
        }),
    })
}

pub(crate) fn decode(
    state: &mut StreamState,
    ev: &SseEvent,
    out: &mut Vec<StreamEvent>,
) -> Result<(), TranslateError> {
    if ev.data == "[DONE]" {
        out.push(StreamEvent::Done { finish_reason: state.finish, usage: state.usage() });
        return Ok(());
    }
    let v: Value =
        serde_json::from_str(&ev.data).map_err(|e| TranslateError::Malformed(e.to_string()))?;
    if let Some(msg) = v["error"]["message"].as_str() {
        return Err(TranslateError::Provider {
            status: 502,
            retryable: false,
            message: msg.to_string(),
        });
    }
    if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
        state.input_tokens = u["prompt_tokens"].as_u64().map(|n| n as u32);
        state.output_tokens = u["completion_tokens"].as_u64().map(|n| n as u32);
    }
    let choice = &v["choices"][0];
    if let Some(f) = choice["finish_reason"].as_str() {
        state.finish = finish(f);
    }
    if let Some(t) = choice["delta"]["content"].as_str() {
        if !t.is_empty() {
            out.push(StreamEvent::Delta { text: t.to_string() });
        }
    }
    Ok(())
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p ultrafast-translate provider::openai`
Expected: 10 passed.

- [ ] **Step 6: Commit**

```bash
git add crates/translate
git commit -m "feat(translate): OpenAI-compatible provider"
```

---

### Task 5: Anthropic provider

**Files:**
- Create: `crates/translate/src/provider/anthropic.rs`
- Modify: `crates/translate/src/provider/mod.rs`

**Interfaces:**
- Consumes: everything Task 4 produced
- Produces: `ProviderKind::Anthropic`, parsed from and rendered as `"anthropic"`. No new public functions.

- [ ] **Step 1: Extend `crates/translate/src/provider/mod.rs`**

Add `mod anthropic;` under `mod openai;`. Add the variant and extend every match:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    /// OpenAI and every OpenAI-compatible API (Groq, Mistral, OpenRouter, Ollama).
    OpenAi,
    Anthropic,
}

impl ProviderKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "openai" => Some(ProviderKind::OpenAi),
            "anthropic" => Some(ProviderKind::Anthropic),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ProviderKind::OpenAi => "openai",
            ProviderKind::Anthropic => "anthropic",
        }
    }
}
```

In `build_request` add `ProviderKind::Anthropic => anthropic::build(target, req),`.
In `parse_response` add `ProviderKind::Anthropic => anthropic::parse(body),`.
In `StreamDecoder::feed` add `ProviderKind::Anthropic => anthropic::decode(&mut self.state, &ev, &mut out)?,`.

- [ ] **Step 2: Write the failing tests in `crates/translate/src/provider/anthropic.rs`**

```rust
#[cfg(test)]
mod tests {
    use crate::error::TranslateError;
    use crate::provider::*;
    use crate::types::*;

    fn target() -> Target {
        Target {
            kind: ProviderKind::Anthropic,
            base_url: "https://api.anthropic.com".into(),
            api_key: Some("sk-ant".into()),
            model: "claude-sonnet-5".into(),
        }
    }

    fn msg(role: Role, content: &str) -> Message {
        Message { role, content: content.into(), name: None }
    }

    fn request(messages: Vec<Message>) -> ChatRequest {
        ChatRequest {
            model: "anthropic/claude-sonnet-5".into(),
            messages,
            max_tokens: None,
            temperature: Some(0.5),
            top_p: None,
            stop: Some(vec!["END".into()]),
            stream: false,
        }
    }

    #[test]
    fn builds_request_with_system_field_and_default_max_tokens() {
        let req = request(vec![
            msg(Role::System, "be brief"),
            msg(Role::Assistant, "earlier"),
            msg(Role::System, "be kind"),
            msg(Role::User, "hi"),
        ]);
        let r = build_request(&target(), &req).unwrap();
        assert_eq!(r.url, "https://api.anthropic.com/v1/messages");
        assert!(r.headers.contains(&("x-api-key".into(), "sk-ant".into())));
        assert!(r.headers.contains(&("anthropic-version".into(), "2023-06-01".into())));
        let v: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(v["model"], "claude-sonnet-5");
        assert_eq!(v["system"], "be brief\n\nbe kind");
        assert_eq!(v["max_tokens"], 4096);
        assert_eq!(v["stop_sequences"][0], "END");
        assert_eq!(v["messages"].as_array().unwrap().len(), 2);
        assert_eq!(v["messages"][0]["role"], "assistant");
        assert_eq!(v["messages"][1]["role"], "user");
    }

    #[test]
    fn request_with_only_system_messages_is_invalid() {
        let e = build_request(&target(), &request(vec![msg(Role::System, "x")])).unwrap_err();
        assert!(matches!(e, TranslateError::InvalidRequest(_)));
    }

    #[test]
    fn parses_response_joining_text_blocks() {
        let body = br#"{"id":"m1","model":"claude-sonnet-5","content":[{"type":"text","text":"a"},{"type":"text","text":"b"}],"stop_reason":"max_tokens","usage":{"input_tokens":7,"output_tokens":3}}"#;
        let r = parse_response(ProviderKind::Anthropic, 200, body).unwrap();
        assert_eq!(r.content, "ab");
        assert_eq!(r.finish_reason, Some(FinishReason::Length));
        assert_eq!(r.usage, Some(Usage { input_tokens: 7, output_tokens: 3 }));
    }

    #[test]
    fn error_body_keeps_message() {
        let body = br#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#;
        let e = parse_response(ProviderKind::Anthropic, 529, body).unwrap_err();
        assert_eq!(e, TranslateError::Provider { status: 529, retryable: true, message: "Overloaded".into() });
    }

    const STREAM: &str = concat!(
        "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"m1\",\"model\":\"claude-sonnet-5\",\"usage\":{\"input_tokens\":9,\"output_tokens\":1}}}\n\n",
        "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: ping\ndata: {\"type\":\"ping\"}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"h\u{e9}\"}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"llo\"}}\n\n",
        "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":12}}\n\n",
        "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
    );

    fn expected() -> Vec<StreamEvent> {
        vec![
            StreamEvent::Delta { text: "h\u{e9}".into() },
            StreamEvent::Delta { text: "llo".into() },
            StreamEvent::Done {
                finish_reason: Some(FinishReason::Stop),
                usage: Some(Usage { input_tokens: 9, output_tokens: 12 }),
            },
        ]
    }

    #[test]
    fn decodes_stream_identically_at_every_split_point() {
        let bytes = STREAM.as_bytes();
        for i in 0..=bytes.len() {
            let mut d = StreamDecoder::new(ProviderKind::Anthropic);
            let mut got = d.feed(&bytes[..i]).unwrap();
            got.extend(d.feed(&bytes[i..]).unwrap());
            assert_eq!(got, expected(), "split at byte {i}");
        }
    }

    #[test]
    fn error_event_inside_stream_is_reported() {
        let mut d = StreamDecoder::new(ProviderKind::Anthropic);
        let e = d
            .feed(b"event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n")
            .unwrap_err();
        assert!(matches!(e, TranslateError::Provider { message, retryable: true, .. } if message == "Overloaded"));
    }
}
```

- [ ] **Step 3: Run to see them fail**

Run: `cargo test -p ultrafast-translate provider::anthropic`
Expected: compile errors, `cannot find function build in module anthropic`.

- [ ] **Step 4: Write the implementation at the top of `crates/translate/src/provider/anthropic.rs`**

```rust
use serde_json::{json, Value};

use super::{HttpRequest, StreamState, Target};
use crate::error::TranslateError;
use crate::sse::SseEvent;
use crate::types::{ChatRequest, ChatResponse, FinishReason, Role, StreamEvent, Usage};

const API_VERSION: &str = "2023-06-01";
/// Anthropic requires max_tokens. Used when the caller sets none.
const DEFAULT_MAX_TOKENS: u32 = 4096;

fn finish(s: &str) -> Option<FinishReason> {
    match s {
        "end_turn" | "stop_sequence" => Some(FinishReason::Stop),
        "max_tokens" => Some(FinishReason::Length),
        "tool_use" => Some(FinishReason::ToolCalls),
        "refusal" => Some(FinishReason::ContentFilter),
        _ => None,
    }
}

pub(crate) fn build(target: &Target, req: &ChatRequest) -> Result<HttpRequest, TranslateError> {
    let system: Vec<&str> = req
        .messages
        .iter()
        .filter(|m| m.role == Role::System)
        .map(|m| m.content.as_str())
        .collect();
    let messages: Vec<Value> = req
        .messages
        .iter()
        .filter(|m| m.role != Role::System)
        .map(|m| {
            let role = if m.role == Role::Assistant { "assistant" } else { "user" };
            json!({ "role": role, "content": m.content })
        })
        .collect();
    if messages.is_empty() {
        return Err(TranslateError::InvalidRequest(
            "at least one user or assistant message is required".into(),
        ));
    }
    let mut body = json!({
        "model": target.model,
        "messages": messages,
        "max_tokens": req.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
    });
    if !system.is_empty() {
        body["system"] = json!(system.join("\n\n"));
    }
    if let Some(v) = req.temperature {
        body["temperature"] = json!(v);
    }
    if let Some(v) = req.top_p {
        body["top_p"] = json!(v);
    }
    if let Some(v) = &req.stop {
        body["stop_sequences"] = json!(v);
    }
    if req.stream {
        body["stream"] = json!(true);
    }
    let mut headers = vec![
        ("content-type".to_string(), "application/json".to_string()),
        ("anthropic-version".to_string(), API_VERSION.to_string()),
    ];
    if let Some(k) = &target.api_key {
        headers.push(("x-api-key".to_string(), k.clone()));
    }
    Ok(HttpRequest {
        method: "POST",
        url: format!("{}/v1/messages", target.base_url.trim_end_matches('/')),
        headers,
        body: serde_json::to_vec(&body).map_err(|e| TranslateError::InvalidRequest(e.to_string()))?,
    })
}

pub(crate) fn parse(body: &[u8]) -> Result<ChatResponse, TranslateError> {
    let v: Value =
        serde_json::from_slice(body).map_err(|e| TranslateError::Malformed(e.to_string()))?;
    let blocks = v["content"]
        .as_array()
        .ok_or_else(|| TranslateError::Malformed("response has no content".into()))?;
    let mut content = String::new();
    for b in blocks {
        match b["type"].as_str() {
            Some("text") => content.push_str(b["text"].as_str().unwrap_or("")),
            Some("thinking") | Some("redacted_thinking") => {}
            other => {
                return Err(TranslateError::Unsupported(format!(
                    "response content block '{}' is not supported yet",
                    other.unwrap_or("unknown")
                )))
            }
        }
    }
    let usage = v.get("usage").filter(|u| u.is_object()).map(|u| Usage {
        input_tokens: u["input_tokens"].as_u64().unwrap_or(0) as u32,
        output_tokens: u["output_tokens"].as_u64().unwrap_or(0) as u32,
    });
    Ok(ChatResponse {
        id: v["id"].as_str().unwrap_or_default().to_string(),
        model: v["model"].as_str().unwrap_or_default().to_string(),
        content,
        finish_reason: v["stop_reason"].as_str().and_then(finish),
        usage,
    })
}

pub(crate) fn decode(
    state: &mut StreamState,
    ev: &SseEvent,
    out: &mut Vec<StreamEvent>,
) -> Result<(), TranslateError> {
    let v: Value =
        serde_json::from_str(&ev.data).map_err(|e| TranslateError::Malformed(e.to_string()))?;
    match v["type"].as_str() {
        Some("message_start") => {
            let u = &v["message"]["usage"];
            state.input_tokens = u["input_tokens"].as_u64().map(|n| n as u32);
            state.output_tokens = u["output_tokens"].as_u64().map(|n| n as u32);
        }
        Some("content_block_delta") => {
            if v["delta"]["type"] == "text_delta" {
                if let Some(t) = v["delta"]["text"].as_str() {
                    if !t.is_empty() {
                        out.push(StreamEvent::Delta { text: t.to_string() });
                    }
                }
            }
        }
        Some("message_delta") => {
            if let Some(s) = v["delta"]["stop_reason"].as_str() {
                state.finish = finish(s);
            }
            if let Some(n) = v["usage"]["output_tokens"].as_u64() {
                state.output_tokens = Some(n as u32);
            }
        }
        Some("message_stop") => {
            out.push(StreamEvent::Done { finish_reason: state.finish, usage: state.usage() });
        }
        Some("error") => {
            let kind = v["error"]["type"].as_str().unwrap_or("");
            return Err(TranslateError::Provider {
                status: 502,
                retryable: kind == "overloaded_error" || kind == "api_error",
                message: v["error"]["message"].as_str().unwrap_or("provider error").to_string(),
            });
        }
        _ => {}
    }
    Ok(())
}
```

- [ ] **Step 5: Run all translate tests**

Run: `cargo test -p ultrafast-translate`
Expected: 28 passed (7 ingress, 5 sse, 10 openai, 6 anthropic).

- [ ] **Step 6: Commit**

```bash
git add crates/translate
git commit -m "feat(translate): Anthropic provider"
```

---

### Task 6: Secrets

**Files:**
- Create: `crates/gateway/src/secrets.rs`
- Modify: `crates/gateway/src/lib.rs`

**Interfaces:**
- Consumes: nothing
- Produces:
  - `secrets::KEY_PREFIX: &str` = `"uf-sk-"`
  - `secrets::NewKey { full: String, hash: String, display: String }`
  - `secrets::generate_key() -> NewKey`
  - `secrets::hash_key(key: &str) -> String`
  - `secrets::Cipher`, `Cipher::from_hex(master: &str) -> anyhow::Result<Cipher>`, `Cipher::generate_master_hex() -> String`, `Cipher::encrypt(&self, plain: &[u8]) -> Vec<u8>`, `Cipher::decrypt(&self, data: &[u8]) -> anyhow::Result<Vec<u8>>`

- [ ] **Step 1: Set `crates/gateway/src/lib.rs`**

```rust
//! Ultrafast gateway library.

pub mod secrets;
```

- [ ] **Step 2: Write the failing tests in `crates/gateway/src/secrets.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_keys_are_unique_prefixed_and_hash_consistently() {
        let a = generate_key();
        let b = generate_key();
        assert_ne!(a.full, b.full);
        assert!(a.full.starts_with(KEY_PREFIX));
        assert_eq!(a.full.len(), KEY_PREFIX.len() + 64);
        assert_eq!(a.hash, hash_key(&a.full));
        assert_eq!(a.hash.len(), 64);
        assert!(a.display.ends_with(&a.full[a.full.len() - 4..]));
        assert!(!a.display.contains(&a.full[KEY_PREFIX.len()..a.full.len() - 4]));
    }

    #[test]
    fn cipher_round_trips_and_uses_fresh_nonces() {
        let c = Cipher::from_hex(&Cipher::generate_master_hex()).unwrap();
        let one = c.encrypt(b"sk-provider");
        let two = c.encrypt(b"sk-provider");
        assert_ne!(one, two);
        assert_eq!(c.decrypt(&one).unwrap(), b"sk-provider");
    }

    #[test]
    fn cipher_rejects_tampering_wrong_key_and_bad_input() {
        let c = Cipher::from_hex(&Cipher::generate_master_hex()).unwrap();
        let mut data = c.encrypt(b"secret");
        let last = data.len() - 1;
        data[last] ^= 1;
        assert!(c.decrypt(&data).is_err());
        let other = Cipher::from_hex(&Cipher::generate_master_hex()).unwrap();
        assert!(other.decrypt(&c.encrypt(b"secret")).is_err());
        assert!(c.decrypt(b"short").is_err());
        assert!(Cipher::from_hex("abcd").is_err());
        assert!(Cipher::from_hex("not hex").is_err());
    }
}
```

- [ ] **Step 3: Run to see them fail**

Run: `cargo test -p ultrafast-gateway secrets`
Expected: compile errors, `cannot find function generate_key`.

- [ ] **Step 4: Write the implementation at the top of `crates/gateway/src/secrets.rs`**

```rust
//! Virtual key generation and hashing, and encryption of provider credentials.

use anyhow::{anyhow, bail, Result};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::{Digest, Sha256};

pub const KEY_PREFIX: &str = "uf-sk-";
const NONCE_LEN: usize = 12;

pub struct NewKey {
    /// Shown to the user once. Never stored.
    pub full: String,
    pub hash: String,
    /// Safe to store and show, for example `uf-sk-…7d2f`.
    pub display: String,
}

pub fn generate_key() -> NewKey {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let full = format!("{KEY_PREFIX}{}", hex::encode(bytes));
    let display = format!("{KEY_PREFIX}\u{2026}{}", &full[full.len() - 4..]);
    NewKey { hash: hash_key(&full), full, display }
}

pub fn hash_key(key: &str) -> String {
    hex::encode(Sha256::digest(key.as_bytes()))
}

pub struct Cipher(ChaCha20Poly1305);

impl Cipher {
    pub fn generate_master_hex() -> String {
        let mut bytes = [0u8; 32];
        OsRng.fill_bytes(&mut bytes);
        hex::encode(bytes)
    }

    pub fn from_hex(master: &str) -> Result<Self> {
        let bytes = hex::decode(master.trim()).map_err(|_| anyhow!("master key is not hex"))?;
        if bytes.len() != 32 {
            bail!("master key must be 32 bytes (64 hex characters)");
        }
        Ok(Self(ChaCha20Poly1305::new(Key::from_slice(&bytes))))
    }

    /// Output is the 12-byte nonce followed by the ciphertext.
    pub fn encrypt(&self, plain: &[u8]) -> Vec<u8> {
        let mut nonce = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce);
        let ct = self
            .0
            .encrypt(Nonce::from_slice(&nonce), plain)
            .expect("encryption with a valid key and nonce cannot fail");
        let mut out = nonce.to_vec();
        out.extend(ct);
        out
    }

    pub fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>> {
        if data.len() <= NONCE_LEN {
            bail!("encrypted value is too short");
        }
        let (nonce, ct) = data.split_at(NONCE_LEN);
        self.0
            .decrypt(Nonce::from_slice(nonce), ct)
            .map_err(|_| anyhow!("could not decrypt: wrong master key or corrupted value"))
    }
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p ultrafast-gateway secrets`
Expected: 3 passed.

- [ ] **Step 6: Commit**

```bash
git add crates/gateway
git commit -m "feat(gateway): key generation, hashing and credential encryption"
```

---

### Task 7: Storage

**Files:**
- Create: `crates/gateway/migrations/0001_init.sql`, `crates/gateway/src/store.rs`
- Modify: `crates/gateway/src/lib.rs` (add `pub mod store;`)

**Interfaces:**
- Consumes: nothing
- Produces:
  - `store::Store` (cheap to clone), `Store::open(path: &std::path::Path) -> anyhow::Result<Store>`, `Store::open_in_memory() -> anyhow::Result<Store>`
  - `store::ProviderRow { id: i64, name: String, kind: String, base_url: String, credential: Option<Vec<u8>> }`
  - `store::KeyRow { id: i64, name: String, display: String }`
  - `Store::insert_provider(&self, name: &str, kind: &str, base_url: &str, credential: Option<&[u8]>) -> anyhow::Result<i64>`
  - `Store::provider_by_name(&self, name: &str) -> anyhow::Result<Option<ProviderRow>>`
  - `Store::insert_key(&self, name: &str, hash: &str, display: &str, expires_at: Option<&str>) -> anyhow::Result<i64>`
  - `Store::active_key_by_hash(&self, hash: &str) -> anyhow::Result<Option<KeyRow>>`
  - `Store::revoke_key(&self, id: i64) -> anyhow::Result<()>`

`expires_at` is a UTC timestamp in SQLite's `YYYY-MM-DD HH:MM:SS` form.

- [ ] **Step 1: Write `crates/gateway/migrations/0001_init.sql`**

```sql
CREATE TABLE providers (
    id          INTEGER PRIMARY KEY,
    org_id      INTEGER NOT NULL DEFAULT 1,
    name        TEXT    NOT NULL,
    kind        TEXT    NOT NULL,
    base_url    TEXT    NOT NULL,
    credential  BLOB,
    created_at  TEXT    NOT NULL DEFAULT (datetime('now')),
    UNIQUE (org_id, name)
);

CREATE TABLE virtual_keys (
    id          INTEGER PRIMARY KEY,
    org_id      INTEGER NOT NULL DEFAULT 1,
    name        TEXT    NOT NULL,
    key_hash    TEXT    NOT NULL UNIQUE,
    display     TEXT    NOT NULL,
    expires_at  TEXT,
    revoked_at  TEXT,
    created_at  TEXT    NOT NULL DEFAULT (datetime('now'))
);
```

- [ ] **Step 2: Write the failing tests in `crates/gateway/src/store.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn provider_round_trip_and_unique_name() {
        let s = Store::open_in_memory().await.unwrap();
        s.insert_provider("openai", "openai", "https://api.openai.com/v1", Some(b"enc")).await.unwrap();
        let p = s.provider_by_name("openai").await.unwrap().unwrap();
        assert_eq!(p.kind, "openai");
        assert_eq!(p.base_url, "https://api.openai.com/v1");
        assert_eq!(p.credential.as_deref(), Some(&b"enc"[..]));
        assert!(s.provider_by_name("missing").await.unwrap().is_none());
        assert!(s.insert_provider("openai", "openai", "x", None).await.is_err());
    }

    #[tokio::test]
    async fn key_lookup_honours_revocation_and_expiry() {
        let s = Store::open_in_memory().await.unwrap();
        let live = s.insert_key("live", "h1", "uf-sk-…aaaa", None).await.unwrap();
        s.insert_key("future", "h2", "uf-sk-…bbbb", Some("2999-01-01 00:00:00")).await.unwrap();
        s.insert_key("past", "h3", "uf-sk-…cccc", Some("2000-01-01 00:00:00")).await.unwrap();

        assert_eq!(s.active_key_by_hash("h1").await.unwrap().unwrap().name, "live");
        assert!(s.active_key_by_hash("h2").await.unwrap().is_some());
        assert!(s.active_key_by_hash("h3").await.unwrap().is_none());
        assert!(s.active_key_by_hash("nope").await.unwrap().is_none());

        s.revoke_key(live).await.unwrap();
        assert!(s.active_key_by_hash("h1").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn data_survives_reopen_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gateway.db");
        {
            let s = Store::open(&path).await.unwrap();
            s.insert_key("k", "h", "d", None).await.unwrap();
        }
        let s = Store::open(&path).await.unwrap();
        assert!(s.active_key_by_hash("h").await.unwrap().is_some());
    }
}
```

- [ ] **Step 3: Run to see them fail**

Run: `cargo test -p ultrafast-gateway store`
Expected: compile errors, `cannot find type Store`.

- [ ] **Step 4: Write the implementation at the top of `crates/gateway/src/store.rs`**

```rust
//! SQLite storage. Nothing outside this module writes SQL.

use std::path::Path;
use std::str::FromStr;

use anyhow::Result;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions};
use sqlx::Row;

#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
}

#[derive(Debug, Clone)]
pub struct ProviderRow {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub base_url: String,
    pub credential: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct KeyRow {
    pub id: i64,
    pub name: String,
    pub display: String,
}

impl Store {
    pub async fn open(path: &Path) -> Result<Self> {
        let opts = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .foreign_keys(true);
        Self::connect(opts, 8).await
    }

    /// One connection only: every in-memory connection is its own database.
    pub async fn open_in_memory() -> Result<Self> {
        let opts = SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true);
        Self::connect(opts, 1).await
    }

    async fn connect(opts: SqliteConnectOptions, max: u32) -> Result<Self> {
        let pool = SqlitePoolOptions::new().max_connections(max).connect_with(opts).await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    pub async fn insert_provider(
        &self,
        name: &str,
        kind: &str,
        base_url: &str,
        credential: Option<&[u8]>,
    ) -> Result<i64> {
        let r = sqlx::query(
            "INSERT INTO providers (name, kind, base_url, credential) VALUES (?, ?, ?, ?)",
        )
        .bind(name)
        .bind(kind)
        .bind(base_url)
        .bind(credential)
        .execute(&self.pool)
        .await?;
        Ok(r.last_insert_rowid())
    }

    pub async fn provider_by_name(&self, name: &str) -> Result<Option<ProviderRow>> {
        let row = sqlx::query(
            "SELECT id, name, kind, base_url, credential FROM providers WHERE name = ?",
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| ProviderRow {
            id: r.get("id"),
            name: r.get("name"),
            kind: r.get("kind"),
            base_url: r.get("base_url"),
            credential: r.get("credential"),
        }))
    }

    pub async fn insert_key(
        &self,
        name: &str,
        hash: &str,
        display: &str,
        expires_at: Option<&str>,
    ) -> Result<i64> {
        let r = sqlx::query(
            "INSERT INTO virtual_keys (name, key_hash, display, expires_at) VALUES (?, ?, ?, ?)",
        )
        .bind(name)
        .bind(hash)
        .bind(display)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;
        Ok(r.last_insert_rowid())
    }

    pub async fn active_key_by_hash(&self, hash: &str) -> Result<Option<KeyRow>> {
        let row = sqlx::query(
            "SELECT id, name, display FROM virtual_keys
             WHERE key_hash = ?
               AND revoked_at IS NULL
               AND (expires_at IS NULL OR expires_at > datetime('now'))",
        )
        .bind(hash)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| KeyRow {
            id: r.get("id"),
            name: r.get("name"),
            display: r.get("display"),
        }))
    }

    pub async fn revoke_key(&self, id: i64) -> Result<()> {
        sqlx::query("UPDATE virtual_keys SET revoked_at = datetime('now') WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p ultrafast-gateway store`
Expected: 3 passed.

- [ ] **Step 6: Commit**

```bash
git add crates/gateway Cargo.lock
git commit -m "feat(gateway): SQLite store for providers and virtual keys"
```

---

### Task 8: Authenticated chat proxy (non-streaming)

**Files:**
- Create: `crates/gateway/src/errors.rs`, `crates/gateway/src/auth.rs`, `crates/gateway/src/app.rs`, `crates/gateway/src/proxy.rs`, `crates/gateway/tests/common/mod.rs`, `crates/gateway/tests/proxy.rs`
- Modify: `crates/gateway/src/lib.rs`

**Interfaces:**
- Consumes: `Store`, `KeyRow`, `ProviderRow`, `Cipher`, `hash_key`, `KEY_PREFIX`, all of `ultrafast_translate`
- Produces:
  - `app::AppState { store: Store, cipher: Cipher, http: reqwest::Client, max_body_bytes: usize }`
  - `app::DEFAULT_MAX_BODY_BYTES: usize` = `10 * 1024 * 1024`
  - `app::router(state: std::sync::Arc<AppState>) -> axum::Router`
  - `errors::error_response(status: StatusCode, kind: &str, message: &str) -> Response`
  - `errors::translate_error_response(e: &TranslateError) -> Response`
  - `auth::authenticate(store: &Store, headers: &HeaderMap) -> Result<KeyRow, Response>`
  - `proxy::chat_completions` (axum handler)
  - `proxy::stream_response(upstream: reqwest::Response, kind: ProviderKind, model: String) -> Response` — returns 501 in this task; Task 9 replaces it
  - Test harness `common::Harness { app: Router, upstream: MockServer, key: String, store: Store }`, `common::harness(kind: &str) -> Harness`, `common::post_chat(app: &Router, key: Option<&str>, body: &str) -> (StatusCode, String)`

Status mapping for provider errors: provider 5xx becomes 502; provider 4xx is passed through unchanged.

- [ ] **Step 1: Set `crates/gateway/src/lib.rs`**

```rust
//! Ultrafast gateway library.

pub mod app;
pub mod auth;
pub mod errors;
pub mod proxy;
pub mod secrets;
pub mod store;
```

- [ ] **Step 2: Write the test harness `crates/gateway/tests/common/mod.rs`**

```rust
#![allow(dead_code)]

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use tower::ServiceExt;
use ultrafast_gateway::app::{router, AppState, DEFAULT_MAX_BODY_BYTES};
use ultrafast_gateway::secrets::{generate_key, Cipher};
use ultrafast_gateway::store::Store;
use wiremock::MockServer;

pub struct Harness {
    pub app: Router,
    pub upstream: MockServer,
    pub key: String,
    pub store: Store,
}

/// A gateway with one provider named "p" of the given kind, pointing at a mock server.
pub async fn harness(kind: &str) -> Harness {
    harness_with_limit(kind, DEFAULT_MAX_BODY_BYTES).await
}

pub async fn harness_with_limit(kind: &str, max_body_bytes: usize) -> Harness {
    let upstream = MockServer::start().await;
    let store = Store::open_in_memory().await.unwrap();
    let cipher = Cipher::from_hex(&Cipher::generate_master_hex()).unwrap();
    let credential = cipher.encrypt(b"provider-secret");
    store.insert_provider("p", kind, &upstream.uri(), Some(&credential)).await.unwrap();
    let key = generate_key();
    store.insert_key("test", &key.hash, &key.display, None).await.unwrap();
    let state = Arc::new(AppState {
        store: store.clone(),
        cipher,
        http: reqwest::Client::new(),
        max_body_bytes,
    });
    Harness { app: router(state), upstream, key: key.full, store }
}

pub async fn post_chat(app: &Router, key: Option<&str>, body: &str) -> (StatusCode, String) {
    let mut req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json");
    if let Some(k) = key {
        req = req.header("authorization", format!("Bearer {k}"));
    }
    let resp = app.clone().oneshot(req.body(Body::from(body.to_string())).unwrap()).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}
```

- [ ] **Step 3: Write the failing tests `crates/gateway/tests/proxy.rs`**

```rust
mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{harness, harness_with_limit, post_chat};
use serde_json::{json, Value};
use tower::ServiceExt;
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, ResponseTemplate};

const BODY: &str = r#"{"model":"p/gpt-4o","messages":[{"role":"user","content":"hi"}]}"#;

fn openai_ok() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "c1", "model": "gpt-4o",
        "choices": [{ "message": { "role": "assistant", "content": "hello" }, "finish_reason": "stop" }],
        "usage": { "prompt_tokens": 1, "completion_tokens": 2 }
    }))
}

fn error_message(body: &str) -> String {
    let v: Value = serde_json::from_str(body).expect("error body must be JSON");
    v["error"]["message"].as_str().expect("error.message must be a string").to_string()
}

#[tokio::test]
async fn health_needs_no_key() {
    let h = harness("openai").await;
    let resp = h
        .app
        .clone()
        .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn proxies_to_openai_provider_with_decrypted_credential() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("authorization", "Bearer provider-secret"))
        .and(body_partial_json(json!({ "model": "gpt-4o" })))
        .respond_with(openai_ok())
        .expect(1)
        .mount(&h.upstream)
        .await;
    let (status, body) = post_chat(&h.app, Some(&h.key), BODY).await;
    assert_eq!(status, StatusCode::OK);
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["choices"][0]["message"]["content"], "hello");
    assert_eq!(v["usage"]["total_tokens"], 3);
    assert!(!body.contains("provider-secret"));
}

#[tokio::test]
async fn proxies_to_anthropic_provider_and_returns_openai_shape() {
    let h = harness("anthropic").await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", "provider-secret"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "m1", "model": "claude-sonnet-5",
            "content": [{ "type": "text", "text": "bonjour" }],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 4, "output_tokens": 5 }
        })))
        .expect(1)
        .mount(&h.upstream)
        .await;
    let body = r#"{"model":"p/claude-sonnet-5","messages":[{"role":"user","content":"hi"}]}"#;
    let (status, out) = post_chat(&h.app, Some(&h.key), body).await;
    assert_eq!(status, StatusCode::OK);
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["object"], "chat.completion");
    assert_eq!(v["choices"][0]["message"]["content"], "bonjour");
    assert_eq!(v["choices"][0]["finish_reason"], "stop");
}

#[tokio::test]
async fn bad_keys_get_401_and_never_reach_the_provider() {
    let h = harness("openai").await;
    Mock::given(method("POST")).respond_with(openai_ok()).expect(0).mount(&h.upstream).await;

    let revoked = ultrafast_gateway::secrets::generate_key();
    let id = h.store.insert_key("r", &revoked.hash, &revoked.display, None).await.unwrap();
    h.store.revoke_key(id).await.unwrap();
    let expired = ultrafast_gateway::secrets::generate_key();
    h.store
        .insert_key("e", &expired.hash, &expired.display, Some("2000-01-01 00:00:00"))
        .await
        .unwrap();

    let cases: Vec<Option<String>> = vec![
        None,
        Some(String::new()),
        Some("sk-not-ours".into()),
        Some("uf-sk-unknown".into()),
        Some(revoked.full),
        Some(expired.full),
    ];
    for key in cases {
        let (status, body) = post_chat(&h.app, key.as_deref(), BODY).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "key {key:?}");
        assert!(!error_message(&body).is_empty());
    }
}

#[tokio::test]
async fn non_bearer_authorization_header_gets_401() {
    let h = harness("openai").await;
    let resp = h
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("authorization", format!("Basic {}", h.key))
                .body(Body::from(BODY))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn invalid_requests_get_400_and_unknown_targets_get_404() {
    let h = harness("openai").await;
    let (s, b) = post_chat(&h.app, Some(&h.key), "{not json").await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(!error_message(&b).is_empty());

    let tools = r#"{"model":"p/m","messages":[{"role":"user","content":"x"}],"tools":[{}]}"#;
    let (s, b) = post_chat(&h.app, Some(&h.key), tools).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(error_message(&b).contains("tools"));

    let no_slash = r#"{"model":"gpt-4o","messages":[{"role":"user","content":"x"}]}"#;
    assert_eq!(post_chat(&h.app, Some(&h.key), no_slash).await.0, StatusCode::NOT_FOUND);

    let unknown = r#"{"model":"nope/gpt-4o","messages":[{"role":"user","content":"x"}]}"#;
    assert_eq!(post_chat(&h.app, Some(&h.key), unknown).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn oversized_body_gets_413_in_json() {
    let h = harness_with_limit("openai", 64).await;
    let big = format!(
        r#"{{"model":"p/m","messages":[{{"role":"user","content":"{}"}}]}}"#,
        "x".repeat(200)
    );
    let (s, b) = post_chat(&h.app, Some(&h.key), &big).await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
    assert!(!error_message(&b).is_empty());
}

#[tokio::test]
async fn provider_errors_are_mapped() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(502).set_body_string("<html>Bad Gateway</html>"))
        .mount(&h.upstream)
        .await;
    let (s, b) = post_chat(&h.app, Some(&h.key), BODY).await;
    assert_eq!(s, StatusCode::BAD_GATEWAY);
    assert!(error_message(&b).contains("Bad Gateway"));

    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(429).set_body_json(json!({ "error": { "message": "slow down" } })),
        )
        .mount(&h.upstream)
        .await;
    let (s, b) = post_chat(&h.app, Some(&h.key), BODY).await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(error_message(&b), "slow down");
}

#[tokio::test]
async fn unreachable_provider_gets_502() {
    let h = harness("openai").await;
    h.store.insert_provider("dead", "openai", "http://127.0.0.1:1", None).await.unwrap();
    let body = r#"{"model":"dead/m","messages":[{"role":"user","content":"x"}]}"#;
    let (s, b) = post_chat(&h.app, Some(&h.key), body).await;
    assert_eq!(s, StatusCode::BAD_GATEWAY);
    assert!(!error_message(&b).is_empty());
}
```

- [ ] **Step 4: Run to see them fail**

Run: `cargo test -p ultrafast-gateway --test proxy`
Expected: compile errors, `could not find app in ultrafast_gateway`.

- [ ] **Step 5: Write `crates/gateway/src/errors.rs`**

```rust
//! Error responses for `/v1`, in the OpenAI error shape.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use ultrafast_translate::error::TranslateError;
use ultrafast_translate::ingress::openai::render_error;

pub fn error_response(status: StatusCode, kind: &str, message: &str) -> Response {
    (status, Json(render_error(kind, message))).into_response()
}

pub fn translate_error_response(e: &TranslateError) -> Response {
    match e {
        TranslateError::InvalidRequest(m) | TranslateError::Unsupported(m) => {
            error_response(StatusCode::BAD_REQUEST, "invalid_request_error", m)
        }
        TranslateError::Provider { status, message, .. } => {
            let code = if *status >= 500 {
                StatusCode::BAD_GATEWAY
            } else {
                StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_GATEWAY)
            };
            error_response(code, "upstream_error", message)
        }
        TranslateError::Malformed(m) => {
            error_response(StatusCode::BAD_GATEWAY, "upstream_error", m)
        }
    }
}
```

- [ ] **Step 6: Write `crates/gateway/src/auth.rs`**

```rust
//! Virtual key check. Runs before anything else on `/v1`.

use axum::http::{header::AUTHORIZATION, HeaderMap, StatusCode};
use axum::response::Response;

use crate::errors::error_response;
use crate::secrets::{hash_key, KEY_PREFIX};
use crate::store::{KeyRow, Store};

fn unauthorized() -> Response {
    error_response(
        StatusCode::UNAUTHORIZED,
        "authentication_error",
        "Missing or invalid API key.",
    )
}

pub async fn authenticate(store: &Store, headers: &HeaderMap) -> Result<KeyRow, Response> {
    let key = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|k| k.starts_with(KEY_PREFIX))
        .ok_or_else(unauthorized)?;
    match store.active_key_by_hash(&hash_key(key)).await {
        Ok(Some(row)) => Ok(row),
        Ok(None) => Err(unauthorized()),
        Err(e) => {
            tracing::error!(error = %e, "key lookup failed");
            Err(error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "server_error",
                "Could not verify the API key.",
            ))
        }
    }
}
```

- [ ] **Step 7: Write `crates/gateway/src/app.rs`**

```rust
//! Shared state and the route table.

use std::sync::Arc;

use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;

use crate::proxy;
use crate::secrets::Cipher;
use crate::store::Store;

pub const DEFAULT_MAX_BODY_BYTES: usize = 10 * 1024 * 1024;

pub struct AppState {
    pub store: Store,
    pub cipher: Cipher,
    pub http: reqwest::Client,
    pub max_body_bytes: usize,
}

pub fn router(state: Arc<AppState>) -> Router {
    let limit = state.max_body_bytes;
    Router::new()
        .route("/health", get(|| async { Json(json!({ "status": "ok" })) }))
        .route("/v1/chat/completions", post(proxy::chat_completions))
        .layer(DefaultBodyLimit::max(limit))
        .with_state(state)
}
```

- [ ] **Step 8: Write `crates/gateway/src/proxy.rs`**

```rust
//! The `/v1/chat/completions` handler.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::rejection::BytesRejection;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use bytes::Bytes;
use ultrafast_translate::ingress::openai::{parse_request, render_response};
use ultrafast_translate::provider::{
    build_request, parse_response, HttpRequest, ProviderKind, Target,
};

use crate::app::AppState;
use crate::auth::authenticate;
use crate::errors::{error_response, translate_error_response};

pub(crate) fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn not_found(model: &str) -> Response {
    error_response(
        StatusCode::NOT_FOUND,
        "not_found_error",
        &format!("Unknown model '{model}'. Use the form provider/model."),
    )
}

fn server_error(message: &str) -> Response {
    error_response(StatusCode::INTERNAL_SERVER_ERROR, "server_error", message)
}

pub async fn chat_completions(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    // 1. Authenticate before looking at anything else.
    if let Err(resp) = authenticate(&state.store, &headers).await {
        return resp;
    }

    // 2. Read and parse the body.
    let body = match body {
        Ok(b) => b,
        Err(_) => {
            return error_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                "invalid_request_error",
                "Request body is too large or could not be read.",
            )
        }
    };
    let req = match parse_request(&body) {
        Ok(r) => r,
        Err(e) => return translate_error_response(&e),
    };

    // 3. Resolve provider/model.
    let Some((provider_name, model)) = req.model.split_once('/') else {
        return not_found(&req.model);
    };
    if provider_name.is_empty() || model.is_empty() {
        return not_found(&req.model);
    }
    let provider = match state.store.provider_by_name(provider_name).await {
        Ok(Some(p)) => p,
        Ok(None) => return not_found(&req.model),
        Err(e) => {
            tracing::error!(error = %e, "provider lookup failed");
            return server_error("Could not load the provider.");
        }
    };
    let Some(kind) = ProviderKind::parse(&provider.kind) else {
        tracing::error!(provider = %provider.name, kind = %provider.kind, "unknown provider kind");
        return server_error("The provider is misconfigured.");
    };
    let api_key = match provider.credential.as_deref().map(|c| state.cipher.decrypt(c)) {
        None => None,
        Some(Ok(bytes)) => match String::from_utf8(bytes) {
            Ok(s) => Some(s),
            Err(_) => return server_error("The provider credential is unreadable."),
        },
        Some(Err(e)) => {
            tracing::error!(provider = %provider.name, error = %e, "credential decrypt failed");
            return server_error("The provider credential is unreadable.");
        }
    };
    let target = Target { kind, base_url: provider.base_url, api_key, model: model.to_string() };

    // 4. Call the provider.
    let out = match build_request(&target, &req) {
        Ok(o) => o,
        Err(e) => return translate_error_response(&e),
    };
    let upstream = match send(&state.http, out).await {
        Ok(r) => r,
        Err(e) => {
            // `without_url` keeps credentials in query strings out of logs and replies.
            let e = e.without_url();
            tracing::warn!(provider = %provider.name, error = %e, "provider unreachable");
            return error_response(
                StatusCode::BAD_GATEWAY,
                "upstream_error",
                &format!("Could not reach provider '{}'.", provider.name),
            );
        }
    };

    let status = upstream.status().as_u16();
    if req.stream && status < 400 {
        return stream_response(upstream, kind, target.model);
    }
    let bytes = match upstream.bytes().await {
        Ok(b) => b,
        Err(_) => {
            return error_response(
                StatusCode::BAD_GATEWAY,
                "upstream_error",
                "The provider response could not be read.",
            )
        }
    };
    match parse_response(kind, status, &bytes) {
        Ok(r) => Json(render_response(&r, now_secs())).into_response(),
        Err(e) => translate_error_response(&e),
    }
}

async fn send(http: &reqwest::Client, out: HttpRequest) -> Result<reqwest::Response, reqwest::Error> {
    let mut rb = http.post(&out.url);
    for (k, v) in &out.headers {
        rb = rb.header(k, v);
    }
    rb.body(out.body).send().await
}

pub fn stream_response(_upstream: reqwest::Response, _kind: ProviderKind, _model: String) -> Response {
    error_response(
        StatusCode::NOT_IMPLEMENTED,
        "invalid_request_error",
        "Streaming is not available in this build.",
    )
}
```

- [ ] **Step 9: Run the tests**

Run: `cargo test -p ultrafast-gateway --test proxy`
Expected: 9 passed.

- [ ] **Step 10: Lint and commit**

Run: `cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings`
Expected: no warnings.

```bash
git add crates/gateway Cargo.lock
git commit -m "feat(gateway): authenticated chat completions proxy"
```

---

### Task 9: Streaming

**Files:**
- Create: `crates/gateway/tests/stream.rs`
- Modify: `crates/gateway/src/proxy.rs` (replace `stream_response`)

**Interfaces:**
- Consumes: `StreamDecoder`, `StreamEvent`, `render_stream_event`, `render_stream_error`, `proxy::now_secs`
- Produces: `proxy::stream_response(upstream: reqwest::Response, kind: ProviderKind, model: String) -> Response` that streams `text/event-stream`

Behavior: every upstream chunk is decoded and forwarded as it arrives. If the provider reports an error mid-stream, or the connection ends before the provider's completion marker, one error event is sent and the stream closes. No `[DONE]` follows an error.

- [ ] **Step 1: Write the failing tests `crates/gateway/tests/stream.rs`**

```rust
mod common;

use axum::http::StatusCode;
use common::{harness, post_chat};
use serde_json::Value;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, ResponseTemplate};

const BODY: &str = r#"{"model":"p/m","stream":true,"messages":[{"role":"user","content":"hi"}]}"#;

/// Returns the JSON payload of every `data:` line except `[DONE]`.
fn payloads(body: &str) -> Vec<Value> {
    body.lines()
        .filter_map(|l| l.strip_prefix("data: "))
        .filter(|d| *d != "[DONE]")
        .map(|d| serde_json::from_str(d).expect("every data line must be JSON"))
        .collect()
}

fn text(body: &str) -> String {
    payloads(body)
        .iter()
        .filter_map(|v| v["choices"][0]["delta"]["content"].as_str().map(str::to_string))
        .collect()
}

fn sse(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(body.to_string(), "text/event-stream")
}

#[tokio::test]
async fn streams_openai_provider() {
    let h = harness("openai").await;
    let upstream = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"h\u{e9}l\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"lo \u{1f600}\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":3}}\n\n",
        "data: [DONE]\n\n",
    );
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_partial_json(serde_json::json!({ "stream": true })))
        .respond_with(sse(upstream))
        .mount(&h.upstream)
        .await;
    let (status, body) = post_chat(&h.app, Some(&h.key), BODY).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(text(&body), "h\u{e9}llo \u{1f600}");
    let all = payloads(&body);
    let last = all.last().unwrap();
    assert_eq!(last["choices"][0]["finish_reason"], "stop");
    assert_eq!(last["usage"]["total_tokens"], 5);
    assert!(body.ends_with("data: [DONE]\n\n"));
    assert!(all.iter().all(|v| v["model"] == "m"));
    let first_id = all[0]["id"].as_str().unwrap();
    assert!(first_id.starts_with("chatcmpl-"));
    assert!(all.iter().all(|v| v["id"] == first_id));
}

#[tokio::test]
async fn streams_anthropic_provider_in_openai_shape() {
    let h = harness("anthropic").await;
    let upstream = concat!(
        "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":3,\"output_tokens\":0}}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"sal\"}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"ut\"}}\n\n",
        "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"max_tokens\"},\"usage\":{\"output_tokens\":4}}\n\n",
        "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
    );
    Mock::given(method("POST")).and(path("/v1/messages")).respond_with(sse(upstream)).mount(&h.upstream).await;
    let (status, body) = post_chat(&h.app, Some(&h.key), BODY).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(text(&body), "salut");
    let all = payloads(&body);
    assert_eq!(all.last().unwrap()["choices"][0]["finish_reason"], "length");
    assert_eq!(all.last().unwrap()["usage"]["total_tokens"], 7);
    assert!(body.ends_with("data: [DONE]\n\n"));
}

#[tokio::test]
async fn response_has_event_stream_content_type() {
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;
    let h = harness("openai").await;
    Mock::given(method("POST")).respond_with(sse("data: [DONE]\n\n")).mount(&h.upstream).await;
    let resp = h
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("authorization", format!("Bearer {}", h.key))
                .body(Body::from(BODY))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.headers()["content-type"], "text/event-stream");
    assert_eq!(resp.headers()["cache-control"], "no-cache");
}

#[tokio::test]
async fn stream_that_ends_early_sends_an_error_event_and_no_done() {
    let h = harness("openai").await;
    let upstream = "data: {\"choices\":[{\"delta\":{\"content\":\"par\"},\"finish_reason\":null}]}\n\n";
    Mock::given(method("POST")).respond_with(sse(upstream)).mount(&h.upstream).await;
    let (status, body) = post_chat(&h.app, Some(&h.key), BODY).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(text(&body), "par");
    let last = payloads(&body).pop().unwrap();
    assert!(last["error"]["message"].as_str().unwrap().contains("ended before"));
    assert!(!body.contains("[DONE]"));
}

#[tokio::test]
async fn provider_error_inside_stream_is_forwarded_as_error_event() {
    let h = harness("openai").await;
    let upstream = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"a\"},\"finish_reason\":null}]}\n\n",
        "data: {\"error\":{\"message\":\"over \\\"loaded\\\"\"}}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"never\"},\"finish_reason\":null}]}\n\n",
    );
    Mock::given(method("POST")).respond_with(sse(upstream)).mount(&h.upstream).await;
    let (_, body) = post_chat(&h.app, Some(&h.key), BODY).await;
    assert_eq!(text(&body), "a");
    let last = payloads(&body).pop().unwrap();
    assert!(last["error"]["message"].as_str().unwrap().contains("over \"loaded\""));
    assert!(!body.contains("[DONE]"));
}

#[tokio::test]
async fn provider_error_before_stream_starts_is_a_normal_json_error() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503).set_body_string("unavailable"))
        .mount(&h.upstream)
        .await;
    let (status, body) = post_chat(&h.app, Some(&h.key), BODY).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"]["message"], "unavailable");
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p ultrafast-gateway --test stream`
Expected: 5 of 6 fail with status 501; `provider_error_before_stream_starts_is_a_normal_json_error` passes.

- [ ] **Step 3: Replace `stream_response` in `crates/gateway/src/proxy.rs`**

Add these imports at the top of the file:

```rust
use std::convert::Infallible;

use axum::body::Body;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use futures::StreamExt;
use ultrafast_translate::ingress::openai::{render_stream_error, render_stream_event};
use ultrafast_translate::provider::StreamDecoder;
use ultrafast_translate::types::StreamEvent;
```

Replace the function:

```rust
pub fn stream_response(upstream: reqwest::Response, kind: ProviderKind, model: String) -> Response {
    let created = now_secs();
    let id = format!("chatcmpl-{}", &crate::secrets::generate_key().hash[..24]);
    let body = async_stream::stream! {
        let mut decoder = StreamDecoder::new(kind);
        let mut chunks = upstream.bytes_stream();
        let mut finished = false;
        while let Some(chunk) = chunks.next().await {
            let bytes = match chunk {
                Ok(b) => b,
                Err(_) => {
                    yield Ok::<String, Infallible>(render_stream_error(
                        "The connection to the provider was lost.",
                    ));
                    return;
                }
            };
            match decoder.feed(&bytes) {
                Ok(events) => {
                    for ev in events {
                        let done = matches!(ev, StreamEvent::Done { .. });
                        yield Ok(render_stream_event(&ev, &id, &model, created));
                        if done {
                            finished = true;
                        }
                    }
                }
                Err(e) => {
                    yield Ok(render_stream_error(&e.to_string()));
                    return;
                }
            }
            if finished {
                return;
            }
        }
        if !finished {
            yield Ok(render_stream_error("The provider stream ended before completion."));
        }
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "text/event-stream")
        .header(CACHE_CONTROL, "no-cache")
        .body(Body::from_stream(body))
        .expect("static headers are valid")
}
```

- [ ] **Step 4: Run all gateway tests**

Run: `cargo test -p ultrafast-gateway`
Expected: 21 passed (3 secrets, 3 store, 9 proxy, 6 stream).

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings`
Expected: no warnings.

```bash
git add crates/gateway
git commit -m "feat(gateway): streaming chat completions"
```

---

### Task 10: Binary, CLI and image

**Files:**
- Create: `crates/gateway/src/config.rs`
- Modify: `crates/gateway/src/lib.rs` (add `pub mod config;`), `crates/gateway/src/main.rs`, `Dockerfile`, `.dockerignore`, `.github/workflows/ci.yml`, `README.md`
- Delete: `deployment/Dockerfile`, `DOCKER_CONFIG.md`

**Interfaces:**
- Consumes: `Store`, `Cipher`, `generate_key`, `AppState`, `router`, `DEFAULT_MAX_BODY_BYTES`, `ProviderKind::parse`
- Produces:
  - `config::load_master_key(data_dir: &Path, from_env: Option<&str>) -> anyhow::Result<String>`
  - `config::db_path(data_dir: &Path) -> PathBuf`
  - Binary `ultrafast` with subcommands `serve`, `provider add`, `key create`

- [ ] **Step 1: Write the failing tests in `crates/gateway/src/config.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::Cipher;

    #[test]
    fn env_value_wins_and_nothing_is_written() {
        let dir = tempfile::tempdir().unwrap();
        let master = Cipher::generate_master_hex();
        assert_eq!(load_master_key(dir.path(), Some(&master)).unwrap(), master);
        assert!(!dir.path().join("master.key").exists());
    }

    #[test]
    fn invalid_env_value_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_master_key(dir.path(), Some("too-short")).is_err());
    }

    #[test]
    fn generates_once_then_reuses() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a/b");
        let first = load_master_key(&nested, None).unwrap();
        assert!(Cipher::from_hex(&first).is_ok());
        assert_eq!(load_master_key(&nested, None).unwrap(), first);
    }

    #[cfg(unix)]
    #[test]
    fn key_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        load_master_key(dir.path(), None).unwrap();
        let mode = std::fs::metadata(dir.path().join("master.key")).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn corrupt_key_file_is_an_error_not_a_silent_regenerate() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("master.key"), "garbage").unwrap();
        assert!(load_master_key(dir.path(), None).is_err());
    }
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p ultrafast-gateway config`
Expected: compile errors, `cannot find function load_master_key`.

- [ ] **Step 3: Write the implementation at the top of `crates/gateway/src/config.rs`**

```rust
//! Data directory layout and the master key.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::secrets::Cipher;

const MASTER_KEY_FILE: &str = "master.key";

pub fn db_path(data_dir: &Path) -> PathBuf {
    data_dir.join("gateway.db")
}

/// Returns the master key as hex. Uses `from_env` when given. Otherwise reads
/// `master.key` in the data directory, creating it on first use.
pub fn load_master_key(data_dir: &Path, from_env: Option<&str>) -> Result<String> {
    if let Some(v) = from_env {
        Cipher::from_hex(v).context("UF_MASTER_KEY is not valid")?;
        return Ok(v.trim().to_string());
    }
    std::fs::create_dir_all(data_dir)
        .with_context(|| format!("could not create {}", data_dir.display()))?;
    let path = data_dir.join(MASTER_KEY_FILE);
    if path.exists() {
        let v = std::fs::read_to_string(&path)
            .with_context(|| format!("could not read {}", path.display()))?;
        Cipher::from_hex(&v).with_context(|| format!("{} is not a valid master key", path.display()))?;
        return Ok(v.trim().to_string());
    }
    let v = Cipher::generate_master_hex();
    write_owner_only(&path, &v)?;
    Ok(v)
}

#[cfg(unix)]
fn write_owner_only(path: &Path, contents: &str) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("could not create {}", path.display()))?;
    f.write_all(contents.as_bytes())?;
    Ok(())
}

#[cfg(not(unix))]
fn write_owner_only(path: &Path, contents: &str) -> Result<()> {
    std::fs::write(path, contents).with_context(|| format!("could not create {}", path.display()))
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p ultrafast-gateway config`
Expected: 5 passed (4 on non-Unix).

- [ ] **Step 5: Replace `crates/gateway/src/main.rs`**

```rust
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use ultrafast_gateway::app::{router, AppState, DEFAULT_MAX_BODY_BYTES};
use ultrafast_gateway::config::{db_path, load_master_key};
use ultrafast_gateway::secrets::{generate_key, Cipher};
use ultrafast_gateway::store::Store;
use ultrafast_translate::provider::ProviderKind;

#[derive(Parser)]
#[command(name = "ultrafast", version, about = "Ultrafast AI gateway")]
struct Cli {
    /// Directory for the database and master key.
    #[arg(long, env = "UF_DATA_DIR", default_value = "./data", global = true)]
    data_dir: PathBuf,
    /// 64 hex characters. Generated into the data directory when unset.
    #[arg(long, env = "UF_MASTER_KEY", hide_env_values = true, global = true)]
    master_key: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the gateway.
    Serve {
        #[arg(long, env = "UF_HOST", default_value = "127.0.0.1")]
        host: String,
        #[arg(long, env = "UF_PORT", default_value_t = 3000)]
        port: u16,
    },
    /// Manage providers.
    Provider {
        #[command(subcommand)]
        command: ProviderCommand,
    },
    /// Manage virtual keys.
    Key {
        #[command(subcommand)]
        command: KeyCommand,
    },
}

#[derive(Subcommand)]
enum ProviderCommand {
    /// Add a provider. Call its models as NAME/MODEL.
    Add {
        #[arg(long)]
        name: String,
        /// "openai" (also for OpenAI-compatible APIs) or "anthropic".
        #[arg(long)]
        kind: String,
        #[arg(long)]
        base_url: String,
        #[arg(long, env = "UF_PROVIDER_API_KEY", hide_env_values = true)]
        api_key: Option<String>,
    },
}

#[derive(Subcommand)]
enum KeyCommand {
    /// Create a virtual key. The key is printed once.
    Create {
        #[arg(long)]
        name: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();
    let master = load_master_key(&cli.data_dir, cli.master_key.as_deref())?;
    let cipher = Cipher::from_hex(&master)?;
    let store = Store::open(&db_path(&cli.data_dir)).await.context("could not open the database")?;

    match cli.command {
        Command::Serve { host, port } => {
            let addr: SocketAddr = format!("{host}:{port}")
                .parse()
                .with_context(|| format!("'{host}:{port}' is not a valid address"))?;
            let state = Arc::new(AppState {
                store,
                cipher,
                http: reqwest::Client::new(),
                max_body_bytes: DEFAULT_MAX_BODY_BYTES,
            });
            let listener = tokio::net::TcpListener::bind(addr)
                .await
                .with_context(|| format!("could not listen on {addr}"))?;
            tracing::info!(%addr, "gateway listening");
            axum::serve(listener, router(state))
                .with_graceful_shutdown(async {
                    let _ = tokio::signal::ctrl_c().await;
                })
                .await?;
        }
        Command::Provider { command: ProviderCommand::Add { name, kind, base_url, api_key } } => {
            if name.is_empty() || name.contains('/') {
                bail!("provider name must not be empty or contain '/'");
            }
            if ProviderKind::parse(&kind).is_none() {
                bail!("unknown kind '{kind}'. Use 'openai' or 'anthropic'");
            }
            if !(base_url.starts_with("http://") || base_url.starts_with("https://")) {
                bail!("base URL must start with http:// or https://");
            }
            let credential = api_key.as_deref().map(|k| cipher.encrypt(k.as_bytes()));
            store
                .insert_provider(&name, &kind, &base_url, credential.as_deref())
                .await
                .with_context(|| format!("could not add provider '{name}' (is the name taken?)"))?;
            println!("Added provider '{name}'. Call its models as {name}/<model>.");
        }
        Command::Key { command: KeyCommand::Create { name } } => {
            let key = generate_key();
            store.insert_key(&name, &key.hash, &key.display, None).await?;
            println!("Created key '{name}'. Copy it now; it is not shown again:");
            println!("{}", key.full);
        }
    }
    Ok(())
}
```

- [ ] **Step 6: Verify by hand against a local mock**

```bash
export UF_DATA_DIR=$(mktemp -d)
cargo run -q -p ultrafast-gateway -- provider add --name local --kind openai --base-url http://127.0.0.1:9
KEY=$(cargo run -q -p ultrafast-gateway -- key create --name smoke | tail -1)
cargo run -q -p ultrafast-gateway -- serve --port 3999 &
sleep 2
curl -s localhost:3999/health
curl -s -o /dev/null -w '%{http_code}\n' localhost:3999/v1/chat/completions -d '{}'
curl -s -w '\n%{http_code}\n' localhost:3999/v1/chat/completions -H "Authorization: Bearer $KEY" \
  -d '{"model":"local/m","messages":[{"role":"user","content":"hi"}]}'
kill %1
```

Expected, in order: `{"status":"ok"}`; `401`; a JSON error saying the provider could not be reached, then `502`.

- [ ] **Step 7: Replace `Dockerfile`**

```dockerfile
FROM rust:1.82-slim AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release --locked -p ultrafast-gateway

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --create-home --home-dir /var/lib/ultrafast ultrafast
COPY --from=builder /app/target/release/ultrafast /usr/local/bin/ultrafast
USER ultrafast
ENV UF_DATA_DIR=/var/lib/ultrafast UF_HOST=0.0.0.0 UF_PORT=3000
VOLUME /var/lib/ultrafast
EXPOSE 3000
ENTRYPOINT ["ultrafast"]
CMD ["serve"]
```

- [ ] **Step 8: Replace `.dockerignore`**

```
target
.git
data
docs
ultrafast-gateway
ultrafast-models-sdk
deployment
configs
```

- [ ] **Step 9: Remove the superseded Docker files**

```bash
git rm deployment/Dockerfile DOCKER_CONFIG.md
```

- [ ] **Step 10: Add the image build to `.github/workflows/ci.yml`**

Append to the `steps` list:

```yaml
      - name: Build image
        run: docker build -t ultrafast:ci .
      - name: Report binary size
        run: |
          cargo build --release --locked -p ultrafast-gateway
          ls -l target/release/ultrafast | awk '{printf "binary size: %.1f MB\n", $5/1048576}'
```

- [ ] **Step 11: Add a v2 section at the top of `README.md`**

Insert directly under the first heading:

````markdown
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

# Create a key for your app. It is printed once.
./target/release/ultrafast key create --name my-app

./target/release/ultrafast serve
```

Call it with any OpenAI SDK by setting the base URL to `http://127.0.0.1:3000/v1`
and the model to `provider/model`, for example `anthropic/claude-sonnet-5`.

What works today: chat completions, streaming, OpenAI-compatible and Anthropic
providers. Not yet: tools, images, the console, users and teams, routing,
limits and budgets.
````

- [ ] **Step 12: Run everything**

Run: `cargo fmt --all -- --check && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all`
Expected: no warnings; 54 tests pass (28 translate, 26 gateway).

Run: `docker build -t ultrafast:ci .`
Expected: image builds.

- [ ] **Step 13: Commit**

```bash
git add -A crates Dockerfile .dockerignore .github README.md Cargo.lock
git commit -m "feat(gateway): ultrafast binary with serve, provider and key commands"
```

---

## Spec coverage

| Spec section | Covered here | Deferred to |
|---|---|---|
| 4 Repository layout | `crates/translate`, `crates/gateway`, lockfile, one Dockerfile | `client`, `clients/`, `ui/`, `openapi/` in their own plans |
| 5 translate crate | No I/O, byte-level streams, error mapping, no silent drops, split-point fixtures | Anthropic Messages ingress, tools (plan 6) |
| 6 Gateway architecture | `/v1`, `/health`, modules `proxy`, `store`; startup variables | `/api`, `/metrics`, console, snapshot (plans 2, 5) |
| 7 Pipeline | Steps 1 to 4 and 8 | Access, limits, cache, accounting, logs (plans 3 to 5) |
| 8 Credentials | Virtual key hash, provider credential encryption, master key file | Passwords, sessions, tokens, roles (plan 2) |
| 9 Streaming rule | No retry after first byte; error event on failure | Retries, fallback, circuit breaker (plan 3) |
| 11 Storage | SQLite WAL, embedded migrations, `org_id` | Remaining tables as their plans need them |
| 15 Errors | OpenAI error shape, JSON-serialized, no secrets | Anthropic error shape (plan 6) |
| 16 Testing | Fixture replay at every split, integration tests with mock providers, CI | Role table tests (plan 2) |
