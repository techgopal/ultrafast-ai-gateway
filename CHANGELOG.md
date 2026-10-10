# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [2.0.0-beta.4] - 2026-10-10

Hardening: guardrails for teams and users, single sign-on password links, a
fallback that skips targets which cannot express a request, console branding,
and the clients ready to publish.

### Added
- **Team and user guardrails.** Guardrails attach to teams and to users as
  well as to defaults, routes and keys. They apply in this order: defaults,
  the caller's team, the teams its owner leads, the user, the route, the key.
  A personal key is covered by the teams of its owner.
- **Password links for single sign-on users.** `POST /api/users/{id}/password-link`
  (admin) makes a single-use link, valid 24 hours, that sets a password and
  keeps the identity link, role and sessions. The console shows it once to
  copy; an SSO-only user who opens the password page is told to ask an admin.
- **Skip to the fallback on unsupported features.** A target that cannot
  express a feature of the request (for example audio input) is skipped and
  the next target is tried. The log's attempt has outcome `skipped` and a
  `skipped` reason, `unsupported:<feature>`, shown in the console's call
  detail.
- **Console branding.** The brand mark and name in the sidebar, the mobile
  header and the sign-in pages.
- **Clients on the registries.** `.github/workflows/publish.yml` (tag `v*`)
  publishes `ultrafast-translate` and `ultrafast-client` to crates.io,
  `ultrafast` and `ultrafast-admin` to PyPI (trusted publishing, no stored
  tokens) and `@ultrafast/client` and `@ultrafast/admin` to npm (with
  provenance). It stays a dry run until the repository variable
  `PUBLISH_LIVE` is `true`; the one-time owner steps are in
  `docs/RELEASING.md`. Package metadata (description, license, repository,
  homepage, keywords, readme) is complete, and the READMEs show the install
  commands. The packages appear once the owner has registered the trusted publishers and
  set `PUBLISH_LIVE`; until then this release publishes nothing.

### Changed
- A failed single sign-on attempt returns to
  `/sign-in?sso_error=<code>&next=<page>`, so the deep link is kept.
- Team leads read the alerts and alert events of their teams (admins read all).
- The dependency `source-map-js` is 1.2.2 (build and test tooling only).

### Fixed
- Alert heartbeats use the database clock, so processes on a shared PostgreSQL
  agree.
- The CI Postgres job starts a TLS server, so the TLS tests run.

## [2.0.0-beta.3] - 2026-10-10

Phase 2: tracing and alerts, single sign-on, optional PostgreSQL, guardrails,
the Responses API, structured outputs, images, audio and prompt templates, and
admin SDKs for TypeScript and Python.

### Added
- **Admin SDKs.** `@ultrafast/admin` (`clients/admin-ts`, TypeScript) and
  `ultrafast-admin` (`clients/admin-py`, Python 3.11 or newer, sync and async)
  for the admin API (`/api`), generated from `openapi/admin.json`, with an
  access-token wrapper, a total timeout per call and a typed `AdminApiError`
  (`status`, `code`, `message`, `fields`). Build from source; not published to
  npm or PyPI. CI regenerates both and fails when the committed copy differs,
  and runs their tests against the real gateway (Python 3.11 and 3.14). The
  Python calls are imported from `ultrafast_admin.api` and
  `ultrafast_admin.models`.
- **Structured outputs.** `response_format` (`text`, `json_object`, `json_schema`)
  on `/v1/chat/completions`, `text.format` on `/v1/responses`, and
  `output_config.format` on `/v1/messages`. OpenAI and Azure get it as it
  came; Anthropic gets `output_config.format` (generally available, no beta
  header; `json_object` is the schema `{"type":"object"}`, which may be
  answered with `{}`); Gemini gets `responseMimeType` and
  `responseJsonSchema`. `strict`, `name` and `description` are not sent to
  Anthropic and Gemini. The clients take it (`responseFormat` with
  `jsonSchema` in TypeScript); the playground has a *Response format* control.
  A schema given as `output_config.format` on `/v1/messages` is sent to
  OpenAI and Azure as `strict: true`, as Anthropic always enforces it.
- **Responses API.** `POST /v1/responses` over any provider, stateless, with
  streaming (no `[DONE]`), function tools, images, `text.format`,
  `reasoning.effort` and `prompt`. `store`, `previous_response_id`,
  conversations, background mode, built-in tools and an array
  `function_call_output` are a 400; `reasoning` items and
  `include: ["reasoning.encrypted_content"]` are accepted and ignored.
  `reasoning_effort` is also accepted on chat completions (OpenAI and Azure;
  a 400 elsewhere).
- **Images.** `POST /v1/images/generations` for OpenAI, Azure and compatible
  providers. A call that was sent is never repeated (504 on a timeout, no
  fallback); timeouts of at least 180 s and 300 s; answers up to 128 MiB.
  Playground *Images* mode.
- **Audio.** `POST /v1/audio/transcriptions`, `/v1/audio/translations` and
  `/v1/audio/speech` for OpenAI, Azure and compatible providers. Uploads are
  capped by `UF_MAX_AUDIO_BYTES` (25 MiB by default), must arrive within 60 s
  (15 s idle), and at most `UF_MAX_CONCURRENT_UPLOADS` (8 by default) are
  being received at once per process (a place is given back when the body has
  arrived); send `model` before `file` to be refused early. Guardrails check
  speech input, the transcription `prompt` and the transcript (subtitles by
  cue). Playground *Audio* mode, which checks a file against the gateway's cap
  (`GET /api/playground/config`).
- **Prompt templates.** Named, versioned messages with `{{variables}}`: a
  `prompt` object (`id`, `version`, `variables`) on `/v1/chat/completions` and
  `/v1/responses`; versions never change; the latest is kept in memory (a
  refresh reads only what changed; a team lead may make 100 templates). A
  Prompts page (versions, a line diff between versions, variables found as you
  type, *Open in Playground*), a template picker in the playground,
  `/api/prompts/*`, templates in configuration export and import, and the
  `name@version` of a call in the logs (migrations: SQLite 0018, PostgreSQL
  0004).
- The logs filter by endpoint (`GET /api/logs?endpoint=`) and show each call's
  endpoint and prompt. Calls are logged, counted and traced under `responses`,
  `images`, `transcriptions`, `translations` and `speech` too, and an external
  guardrail is told these endpoint names.
- **PostgreSQL (optional).** `UF_DATABASE_URL` (a `postgres://` URL, TLS by
  `sslmode`; `UF_DATABASE_MAX_CONNECTIONS`, default 10) runs the gateway on
  PostgreSQL instead of SQLite, so several processes can share one database.
  `UF_MASTER_KEY` is required there. Budgets are shared (each process flushes
  its spend about every 5 s); rate limits, the response cache, single-flight,
  breaker health, the setup code and alert error windows stay per process.
  Back up with `pg_dump`: the console's Backup panel says so, `GET /api/backup`
  answers 409 `backup_unsupported`, and `settings` reports `database`. A
  Settings panel shows which database is in use. Docker Compose example in
  `docs/compose/postgres.yml`; README section "Using PostgreSQL", including
  what a configuration export and import moves from SQLite. The browser tests
  run on PostgreSQL with `UF_E2E_DATABASE_URL`. No online migration from
  SQLite. Tested on PostgreSQL 14 and 17 (the Rust suite, and for this release
  the whole browser suite on 14.24 and 17: 111 passed, the 7 SQLite-only tests
  skipped); the gateway logs `database: sqlite`
  or `database: postgres` at start. Revocations (keys, users, grants) and
  single sign-on settings reach the other processes within about 30 s. An
  error-rate or circuit alert episode belongs to the process that opened it
  (migrations: SQLite 0016, PostgreSQL 0002).
- **Guardrails.** Block, redact or flag what goes to models and what comes
  back. Rules that run in the gateway (keywords, regular expressions, PII types
  `EMAIL`, `PHONE`, `CREDIT_CARD`, `IBAN`, `US_SSN`, `IPV4`, `IPV6`, `SECRET`)
  with an action and a direction each, or an external signed webhook (timeout,
  fail open or closed, `x-uf-signature`). They apply to every call, to a route
  or to a key, in order; built-in rules run before external ones. Inputs
  (text parts, system, tool results, tool-call arguments, names, embeddings
  inputs) and outputs (answers and tool-call arguments), streams included, are
  checked with a 256-character hold-back; a block is a 400 `guardrail_blocked`
  on input and a `content_filter` ending on output. The logs carry a badge,
  details and a filter (`guardrail=`); a console page with a rule editor, *Try
  it* and attachment on routes and keys; `/api/guardrails/*` (admin only);
  guardrails in configuration export and import; metrics
  `uf_guardrail_actions_total` and `uf_guardrail_external_errors_total`; span
  attribute `uf.guardrail.action` (migrations: SQLite 0017, PostgreSQL 0003).
  README section "Guardrails" lists the detectors' known misses and the limits.
  The rate limits run before the guardrails (a blocked call gives its request
  and tokens back), large scans are bounded to as many at once as CPUs, and
  only admins see which guardrail or rule acted in the logs.
- Multiple Anthropic `system` blocks are now joined with a newline (they were
  joined with nothing), so a guardrail sees, and the provider receives, the
  blocks apart.
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
- Alert history is now deleted with the request logs, after the log retention
  period (it was kept for ever).
- A circuit alert resolves only after its breaker has stayed closed for
  5 minutes, so a flapping provider is one episode. Budget alerts of a past
  period no longer show as firing. The `requested` name in logs and traces is
  cut at 256 bytes.
- **Single sign-on.** Sign in with one OpenID Connect provider (Google,
  Microsoft Entra ID, Okta, Keycloak, any with discovery): authorization code
  flow with PKCE, ID token checks (RS/PS256-512, ES256/384), settings in the
  console (Settings, Single sign-on) and `GET/PUT /api/settings/oidc`, a test
  endpoint that fetches the issuer's discovery and key set (admin only).
  `UF_PUBLIC_URL` / `--public-url` names the gateway's address (no path; plain
  http only for localhost unless `--insecure-cookies`). Users are linked by
  `<issuer>|<sub>`, by verified email, or created for allowed domains; an
  optional admin group sets the role at each sign-in (a missing groups claim
  leaves the role; the last admin is never demoted). Starting a sign-in and the
  callback each have a rate limit of their own, apart from password failures;
  `sso_error` codes on the sign-in page; metric
  `uf_oidc_signins_total{result}`. Users show how they sign in (Password, SSO only
  or Password and SSO; `has_password` in the user view), and the Account page
  tells a user without a password so. Migration 0015.

### Changed
- The admin API description (`openapi/admin.json`) names the keywords variant of
  a guardrail matcher (`KeywordsMatcher`), declares alert rule `params` as an
  open object, and types the backup and playground speech downloads as binary
  (`string`, `format: binary`, also under `application/octet-stream`).
- The console's Settings configuration export is fetched first: a refusal
  (409 `export_blocked`, with the name of the prompt template that cannot be
  read) is shown in place, and a file is saved only when there is one.
- OpenAI (base URL host `api.openai.com`) and Azure targets are sent
  `max_completion_tokens` in place of `max_tokens`, which the o-series and
  GPT-5 models require; chat completions accept `max_completion_tokens`. Other
  OpenAI-compatible hosts keep `max_tokens`.

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
