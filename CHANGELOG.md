# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
