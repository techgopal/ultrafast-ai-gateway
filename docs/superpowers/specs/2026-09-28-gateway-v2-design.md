# Ultrafast Gateway v2: Design

Date: 2026-09-28
Status: Draft for review
Prototype: https://claude.ai/artifact/UaWzRPnuMzfVVrZkFSceAk

## 1. Goal

Rebuild Ultrafast as an open-source, self-hosted AI gateway for small teams:
one small binary with a built-in admin console, no external services, and
multi-user access with admin control.

v2 is a clean rewrite in this repository. v1 code is reference only. The v1
review found that about 45% of the gateway crate is dead code and that auth,
rate limiting, caching, fallback and the circuit breaker are broken or not
connected.

### Success criteria

- A new user downloads one binary, runs it, and reaches a working console by
  setting only an admin email and password.
- Existing OpenAI and Anthropic SDKs work against the gateway by changing the
  base URL and key.
- An admin can add providers, enable models, create teams and users, and set
  budgets and limits entirely from the console, with no restart.
- No cached response is ever served across teams.
- Every feature listed in phase 1 has automated tests that run in CI.

### Out of scope for v2

Hosted SaaS, organizations above teams, SCIM, billing, and multi-node
deployment. The data model leaves room for them (section 12).

## 2. Components

| # | Component | Languages | Phase |
|---|---|---|---|
| 1 | **Gateway**: proxy, admin API, embedded console | Rust, TypeScript (UI) | 1 |
| 2 | **Client**: calls the gateway or a provider directly | Rust, Python, TypeScript | 1 |
| 3 | **Admin SDK**: manages the gateway | Python, TypeScript | 2 |

Build order: gateway core, console, clients (Python, TypeScript, Rust), then
the admin SDK. Each gets its own implementation plan.

Routing, fallback, retries, circuit breaking and caching live only in the
gateway. The client has none of them.

## 3. Phases

**Phase 1**

- Endpoints: OpenAI Chat Completions, Anthropic Messages, embeddings, model
  list. Streaming on both chat formats.
- Providers: OpenAI, Anthropic, Google Gemini, Azure OpenAI, Ollama, Groq,
  Mistral, OpenRouter, and any OpenAI-compatible endpoint.
- Routes with weighted targets, ordered fallbacks, retries, timeouts, circuit
  breaking.
- Models page: enable or disable models, grant access to teams and users.
- Email and password sign-in. Roles: Admin, Team lead, Member. Teams.
- Virtual keys with allowed routes and models, expiry, limits, budget, tags.
- Rate limits by requests, tokens and concurrency. Budgets per gateway, team,
  user and key.
- Request logs, overview dashboard, Prometheus metrics, audit log.
- Exact-match response cache, scoped per team, key or user.
- Playground.
- Configuration export and import.
- Clients in Rust, Python and TypeScript.

**Phase 2**

Guardrails (keyword and regex rules, PII redaction, external guardrail hooks),
single sign-on (OIDC), spend and error alerts, OpenTelemetry export, OpenAI
Responses format, images and audio endpoints, prompt templates, optional
Postgres, admin SDK.

**Phase 3**

MCP gateway with per-key tool allowlists, agent registry, semantic caching,
traffic splitting for experiments, latency- and cost-based routing, and an
interception mode that captures AI traffic without a base-URL change.

## 4. Repository layout

```
crates/
  translate/     Provider request/response/stream translation. No networking.
  gateway/       The binary: proxy, admin API, storage, embedded console.
  client/        Rust client. Uses translate plus reqwest.
clients/
  python/        PyO3 extension over crates/client. Built with maturin.
  typescript/    crates/translate compiled to WebAssembly, plus fetch.
ui/              Console. Vite, TanStack Router, Query, Table, Form.
openapi/         Generated admin API spec. Source for the admin SDK.
docs/
```

v1 crates are tagged `v1-final` and removed from `main` when the phase 1
gateway passes its test suite.

`Cargo.lock` is committed. There is one Dockerfile, pinned to a Rust version.

## 5. The translate crate

One implementation of provider formats, shared by the gateway and all three
clients.

- Input: a request in OpenAI Chat Completions or Anthropic Messages form, and
  a target provider.
- Output: the HTTP method, path, headers and body to send; functions that
  turn the provider's response, stream events and errors back into the
  caller's format.
- It performs no I/O and holds no runtime. This is what lets it compile to
  WebAssembly.
- Streaming parses bytes, not strings. It buffers until a complete event
  arrives, so events split or merged across network chunks, and multi-byte
  characters split across chunks, are handled. v1 got all three wrong.
- Provider errors map to one error type that records the HTTP status, whether
  a retry is sensible, and the provider's message.
- Content that a target format cannot express (for example a tool message for
  a provider without tools) is an error, never silently dropped.

Testing: recorded provider responses and streams as fixtures, replayed at
every possible chunk boundary.

## 6. Gateway architecture

One process. axum serves three things on one port:

| Path | Purpose | Auth |
|---|---|---|
| `/v1/*` | Model calls | Virtual key |
| `/api/*` | Admin API, used by the console and admin SDK | Session cookie or access token |
| `/` | Console static files, compiled into the binary | None for assets |
| `/health` | Liveness | None |
| `/metrics` | Prometheus | Metrics token |

Modules, each with one job and a narrow interface:

| Module | Job |
|---|---|
| `proxy` | The request pipeline (section 7) |
| `routing` | Pick targets for a route; track health and circuit state |
| `limits` | Rate limits and budgets, in memory |
| `cache` | Response cache, in memory |
| `identity` | Users, teams, roles, sessions, keys, access checks |
| `catalog` | Providers, models, prices, model access |
| `logs` | Queue and batch-write request logs; retention |
| `audit` | Record every admin change |
| `store` | Storage interface and its SQLite implementation |
| `api` | Admin HTTP handlers; generates the OpenAPI spec |
| `web` | Serves the embedded console |

The proxy logic does not depend on axum types, so a second entry point
(phase 3 interception mode) can reuse it.

### Configuration

The database is the source of truth. Changes made in the console or admin API
take effect immediately: the gateway keeps an in-memory snapshot of providers,
models, routes, keys and access rules, and replaces it atomically after each
write.

Startup needs only:

- `UF_ADMIN_EMAIL` and `UF_ADMIN_PASSWORD` for the first admin, or the setup
  screen shown when no user exists.
- `UF_DATA_DIR` for the database (default `./data`).
- `UF_MASTER_KEY` to encrypt provider credentials. If unset, a key is
  generated on first start and written to the data directory with owner-only
  permissions.

Host and port come from `UF_HOST` and `UF_PORT` or the matching flags. Flags
win over environment variables.

Export produces one file with providers, models, routes, teams and limits.
Credentials, keys, users' passwords and logs are left out.

## 7. Request pipeline

For each `/v1` call, in this order:

1. **Authenticate.** Hash the presented key and look it up in the snapshot.
   Unknown, expired or revoked keys get 401. Nothing else runs first.
2. **Resolve identity.** Key, owning user, team.
3. **Parse** the body into the common request form. Reject bodies over the
   size limit while reading, not by trusting `Content-Length`.
4. **Resolve the target.** The `model` field names a route or a
   `provider/model`. Unknown names get 404.
5. **Check access** (section 8). Failures get 403.
6. **Check limits.** Rate limits, then budgets, across key, user, team and
   gateway. The strictest applies. Failures get 429 with a `Retry-After`
   header and a body naming the limit that was hit.
7. **Cache lookup**, if the route enables it.
8. **Dispatch.** Try targets in order with retries (section 9). Stream the
   response through as it arrives.
9. **Account.** Record tokens and cost against every budget, and update rate
   counters with actual token use.
10. **Log.** Put the record on the log queue. If the queue is full the record
    is dropped and a counter is incremented. A request is never delayed by
    logging.

## 8. Identity and access

### Roles

| Role | Can do |
|---|---|
| Admin | Everything: providers, models, routes, all users, teams, keys, budgets, settings, all logs |
| Team lead | Manage members, keys and budgets for their own team, within limits an admin set. See their team's logs and spend |
| Member | Create and revoke their own keys within team limits. See their own logs and spend. Use the playground |

Every admin API handler declares the role it needs and the scope (own, team,
all). This is enforced in one place, not per handler.

### Model access

A call to a model is allowed only when all of these hold:

1. The model is enabled. Newly synced models are disabled until an admin
   enables them.
2. The model is granted to everyone, to one of the user's teams, or to the
   user directly.
3. The key's allowlist includes the model or the route.
4. If called through a route, the user's team may use that route.

Admins can call any enabled model. A route skips a target whose model is
disabled or not granted to the caller, and moves to the next.

`GET /v1/models` lists only what the presented key can call.

### Credentials

| Secret | Storage |
|---|---|
| User password | Argon2id hash |
| Virtual key (`uf-sk-…`) | SHA-256 hash and a display prefix. The full key is shown once |
| Access token for the admin API | SHA-256 hash; carries its owner's role |
| Provider credential | Encrypted with the master key; never returned by any API |
| Session | Server-side row; cookie is HttpOnly, Secure, SameSite=Strict |

State-changing admin calls made with a session cookie require a CSRF token.
Sign-in attempts are rate limited per account and per address.

## 9. Routing and resilience

A route has primary targets with weights, an ordered list of fallbacks, and
settings for retries, timeouts, circuit breaking and caching.

- **Selection:** weighted random among healthy primary targets. If none
  succeed, fallbacks in order.
- **Retries:** per target, with exponential backoff and jitter. Retried:
  timeouts, connection errors, 429 and 5xx. Never retried: other 4xx.
- **Streaming:** once the first byte has been sent to the caller, the request
  is not retried or failed over. An error after that point ends the stream
  with an error event.
- **Timeouts:** one for the first token, one for the whole request.
- **Circuit breaker:** per provider and model. Opens after N failures in a
  window, stays open for a fixed period, then allows one trial request. Only
  retryable failures count, so bad requests from one caller cannot open it.
- **Health** shown in the console comes from real traffic. The gateway does
  not send billed requests to test providers.

Each attempt is recorded in the request log, which is what the console's
"routing attempts" panel shows.

## 10. Limits, budgets and cache

All three are held in memory and are checked without touching the database.

**Rate limits:** requests per minute, tokens per minute, and concurrent
requests. Sliding window. Token limits reserve an estimate before dispatch
and correct it afterwards.

**Budgets:** an amount, a reset period (daily, weekly, monthly), and an
action when reached: block, or allow and alert. Spend counters are written to
the database every few seconds and on shutdown. On startup they are rebuilt
from the request log, so an unclean stop loses nothing that was logged.

**Cache:** exact match only in phase 1.

- The key covers every field that affects the output: model, messages
  including names and tool calls, tools, tool choice, sampling settings, stop
  sequences, response format. Each field is tagged so two different requests
  cannot collide.
- The key always includes the scope: team (default), key, or user.
- Requests with temperature above 0.5 and streaming requests are not cached
  in phase 1.
- Bounded by entry count and total size, with least-recently-used eviction.

## 11. Storage

SQLite in WAL mode, behind the `store` interface. Migrations are embedded and
run at startup.

Main tables: `users`, `teams`, `team_members`, `sessions`, `access_tokens`,
`providers`, `models`, `model_grants`, `routes`, `route_targets`,
`virtual_keys`, `rate_limits`, `budgets`, `budget_usage`, `request_logs`,
`audit_log`, `settings`.

Request logs store metadata always. Prompt and response bodies are stored
only when an admin turns that on; it is off by default. A background task
deletes logs past the retention period in small batches.

## 12. Room for later

- Every team-owned table has an `org_id` column, fixed to one default
  organization in v2.
- Users have an `auth_provider` and `external_id`, unused until OIDC arrives.
- The `store` interface has no SQLite-specific types, so Postgres is a second
  implementation.
- In-memory limits and cache sit behind interfaces, so a shared backend can
  replace them for multi-node deployment.

## 13. Console

A static single-page app compiled into the binary. TanStack Router, Query,
Table and Form, built with Vite. No server rendering, so no Node runtime is
needed.

Pages, as in the prototype:

| Section | Pages |
|---|---|
| Observe | Overview, Logs, Playground |
| Configure | Providers, Models, Routing, Virtual keys |
| Govern | Users and teams, Budgets and limits |
| | Settings (retention, sign-in, backup, audit log) |

Guardrails and MCP tools are shown as upcoming and are not built in phase 1.

What a user sees depends on role: Members see their own keys, logs and spend
and the playground; Team leads also see their team; Admins see everything.
The API enforces this. The console only hides what the API would refuse.

The console talks to the gateway through types generated from the OpenAPI
spec.

## 14. Clients

All three expose the same surface: chat, streaming chat, and embeddings,
against either the gateway or a provider directly.

| Language | Built from | Networking |
|---|---|---|
| Rust | `translate` | reqwest |
| Python | The Rust client through PyO3 | reqwest, inside the extension |
| TypeScript | `translate` compiled to WebAssembly | The runtime's `fetch` |

- Python ships sync and async interfaces and prebuilt wheels for Linux, macOS
  and Windows on x86-64 and arm64.
- TypeScript runs in Node, Bun, Deno, browsers and edge runtimes.
- When pointed at the gateway, a client can attach tags to a request for cost
  attribution.
- Clients do not retry, route, cache or break circuits. They return typed
  errors that say whether a retry is sensible.

The admin SDK (phase 2) is generated from the OpenAPI spec for Python and
TypeScript.

## 15. Errors

`/v1` errors are returned in the format the caller used: OpenAI error shape on
OpenAI endpoints, Anthropic error shape on the Messages endpoint. Error text
is always serialized as JSON, never concatenated into it.

`/api` errors use one shape: a stable code, a message, and field details for
validation failures.

Provider credentials and virtual keys never appear in errors or logs.

## 16. Testing

| Layer | Approach |
|---|---|
| `translate` | Fixture replay per provider, including streams cut at every byte |
| Pipeline | Integration tests against mock providers: auth, access, limits, budgets, cache scope, fallback, retries, circuit breaker, streaming |
| Access control | A table-driven test that calls every admin endpoint as each role and checks the result |
| Storage | Migration tests and store interface tests on a temporary database |
| Console | Component tests, plus a small end-to-end suite against a real gateway |
| Clients | One shared set of cases run in all three languages against a mock server |

Integration tests live where Cargo compiles them. CI runs format, lint with
warnings as errors, all tests, and the Docker build.

Binary size and gateway overhead per request are measured in CI and reported.
No target is promised until there are measurements.

## 17. Decisions made

| Decision | Choice |
|---|---|
| Audience | Small teams self-hosting; open source |
| Source of truth | Database, edited through console and API |
| Roles | Admin, Team lead, Member |
| New models | Disabled until an admin enables them |
| Model access | Granted to teams and to individual users |
| Web framework | axum. Rama reconsidered for phase 3 interception mode |
| TypeScript client | WebAssembly translation layer plus `fetch` |
| Admin SDK | Separate from the client; Python and TypeScript; phase 2 |
| Bodies in logs | Off by default |
| Cache scope | Team by default; never across teams |
