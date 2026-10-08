# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **Tracing.** `UF_OTEL_ENDPOINT` (also `UF_OTEL_HEADERS`, `UF_OTEL_SERVICE_NAME`,
  `UF_OTEL_SAMPLE_RATIO`): every `/v1` call is exported as an OpenTelemetry
  trace over OTLP/HTTP (JSON), one server span per call and one span per
  provider attempt, with `gen_ai.*` attributes; an incoming `traceparent` is
  honored. Best effort: bounded queue, drops counted, 5 s cap at shutdown.
  Metrics `uf_otel_spans_exported_total`, `uf_otel_spans_dropped_total`,
  `uf_otel_export_failures_total`.
- **Alerts.** Budget, error-rate and circuit-open rules, delivered to signed
  webhook or Slack-compatible channels (`x-uf-signature`, retries at now, +5 s,
  +30 s, per-channel limits); a console page, `/api/alerts/*` (admin only),
  and alert channels and rules in configuration export and import. Metric
  `uf_alert_deliveries_total{result}`.

## [2.0.0-beta.2] - 2026-10-05

Tool calling and image input on both chat endpoints, single-flight for the
response cache, and the same in the playground and the clients.

### Added
- **Tools** on `/v1/chat/completions` and `/v1/messages`, for every provider
  kind (OpenAI, Azure, Anthropic, Gemini, OpenAI-compatible), streaming and
  not. Tool arguments pass through unchanged between OpenAI-format callers
  and OpenAI-format providers; otherwise they are converted from or to the
  provider's JSON object. With `tools` empty or absent, `tool_choice` `auto` /
  `none` and `parallel_tool_calls` are ignored; `required` or a named tool is a
  400, and so is a named tool that is not in `tools`. `function.strict` is sent
  to OpenAI and Azure (Anthropic and Gemini ignore it). A `tool` message may
  carry `name`. Gemini tool call ids are `call_<8 hex>_<n>` (the hex from the
  response id, so they differ from answer to answer), tool schemas are sent as
  `parametersJsonSchema` (full JSON Schema), and earlier tool calls carry
  Google's placeholder thought signature `skip_thought_signature_validator`
  (Gemini 3 refuses history without one).
- Anthropic-format streams never interleave content blocks: the first tool
  call streams live, later calls and text after a call are sent whole after it.
- OpenAI-format answers with tool calls that end as `stop` are reported as
  `tool_calls`.
- **Images** in user messages: PNG, JPEG, GIF and WebP as `http(s)` or `data:`
  URLs (Anthropic `image` blocks with a `base64` or `url` source). The gateway
  never fetches an image URL; Gemini takes `data:` URLs only (`https` is a 400).
  Images count toward the 10 MiB request body limit (413 above it).
- **Single-flight** in the response cache: concurrent identical cacheable calls
  reach the provider once. Waiters hold their concurrency slot while waiting; if
  the first call fails, waiters call the provider themselves, concurrently.
  Metric `uf_cache_flight_waits_total`.
- **Playground**: images (5 MB each, 9 MiB per request including the history),
  tools as JSON, a tool choice, and tool calls and results in the thread.
- **Clients**: tools and images in the Rust, Python (flat tool dicts, `ToolCall`
  round trip) and TypeScript clients, with tool-call events in streams.

### Changed
- README: "Known limits" and the role rule for spend are accurate now: members
  see their own usage and budgets, team leads their teams', admins all; only
  admins set limits and budgets.

### Known limits
- No Responses API, image or audio output, or `response_format` / structured
  outputs yet (phase 2).
- Gemini thought signatures are not carried; the documented placeholder is
  sent instead.
- A tool result's `is_error` flag is not carried.
- Single-flight is per process; clearing the cache on configuration changes
  stays.
- The Python wheels, the npm package and the crates are still not published.

## [2.0.0-beta.1] - 2026-10-04

The first release of Ultrafast v2, a rewrite of the gateway. Phase 1 is
complete: one binary with an embedded web console, an OpenAI-compatible `/v1`
proxy (chat in the OpenAI and Anthropic formats, embeddings, streaming) over
OpenAI, Anthropic, Gemini, Azure OpenAI and other OpenAI-compatible
providers, virtual keys, teams and roles, rate limits and budgets, a response
cache, routing with fallbacks, request logs and usage, a playground,
configuration export and import, backup, and clients for Rust, TypeScript and
Python. It is a beta: the API may still change before 2.0.0, and the limits
below and in the README's "Known limits" apply. v2 is a fresh install;
nothing from v1 is migrated.

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

## v1

v1 (0.1.0, August 2025) was a different codebase, removed from `main` and kept
at the tag `v1-final`. Its changelog is not carried over: it described
features (Redis, plugins, WebSockets, Kubernetes charts) that v2 does not have.
