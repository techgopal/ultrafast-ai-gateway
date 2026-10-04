# Models and Routing Implementation Plan (gateway plan 3 + console plan 2)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. In this repository also use the project skill `uf-workflow` and the agents in `.claude/agents/`. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Callers reach models through an admin-controlled catalog and through routes with weighted targets, fallbacks, retries, timeouts and circuit breaking, on more providers and endpoints; the console gets Models and Routing pages and the identity gaps found in console plan 1 are closed — backend and frontend for every feature.

**Architecture:** New tables (`models`, `model_grants`, `routes`, `route_targets`, `route_grants`) are the source of truth; the `/v1` snapshot gains the catalog, grants and routes and every access decision is made from it without touching the database. A new `routing` module picks targets and keeps health and circuit state in memory behind a trait; the proxy reports each attempt to a `RequestSink` trait (no-op now, request logs in plan 6). The translate crate gains Gemini and Azure egress, Anthropic Messages ingress and embeddings. The console adds two pages on the existing patterns.

**Tech Stack:** Rust 1.94, axum 0.8, sqlx 0.9 SQLite, reqwest 0.13, utoipa 6, arc-swap, wiremock (tests); React, TypeScript, TanStack, shadcn/ui, Vitest + MSW, Playwright.

**Spec:** `docs/superpowers/specs/2026-09-28-gateway-v2-design.md` — sections 3 (phase 1), 6, 7, 8, 9, 13. Conventions: `docs/CONVENTIONS.md` (binding).

## Global Constraints

- Owner decisions (2026-10-01): team leads add members **by email** of an existing active user; **only admins change team roles** (leads add and remove members, never promote or demote; a lead may leave a team); plans ship backend and console together.
- Newly synced or added models are **disabled** until an admin enables them (spec 8).
- A call is allowed only when: the model is enabled; it is granted to everyone, to one of the user's teams, or to the user; the key's allowlist (if any) includes the model or the route; and, through a route, the user's team may use the route. Admins may call any enabled model; their key allowlist still applies (spec 8).
- Failures: unknown model or route → 404 `not_found_error`; not allowed → 403 `permission_error`; both in the shape of the ingress format (OpenAI or Anthropic).
- `/v1` never reads the database. Every admin write that changes what `/v1` sees calls `api::refresh_snapshot` after commit.
- Retried: timeouts, connection errors, 429, 5xx. Never retried and never failed over: other 4xx. After the first byte of a stream reaches the caller there is no retry or failover; a later error ends the stream with an error event (spec 9).
- Health and circuit state come from real traffic only; the gateway never sends test requests to providers (spec 9).
- Provider credentials never leave the gateway: not in errors, logs, API answers, or attempt records.
- Every new admin route: declared once with utoipa, unique `operationId`, added to `ROUTES` in `api/openapi.rs`, to `tests/api_roles.rs`, and `openapi/admin.json` regenerated; the console regenerates `ui/src/api/schema.d.ts` (`pnpm --dir ui gen:api`).
- Console: everything in `docs/CONVENTIONS.md` "Console" applies to the new pages (FormDialog, `can()`, one `main`/`h1`, 44 px, 390 px, toasts identify nothing, tests first).

## Review Focus

1. A model granted to a team, then the user leaves the team → the next `/v1` call is refused (snapshot refreshed on membership changes; today team writes do not refresh). Pinned in Task 4.
2. All targets of a route have open circuits → the caller gets a 503 `upstream_error` naming no provider, quickly, not a hang. Pinned in Task 6.
3. A provider is deleted while routes point at its models → routes skip the gone targets; a route with no target left answers 503; the console shows the route as broken. Pinned in Tasks 3 and 10.
4. A stream that fails before its first byte fails over; one that fails after does not (the caller sees one error event, no duplicated text). Pinned in Task 6.
5. Model names with characters providers use (`gpt-4o-2024-08-06`, `models/gemini-2.0-flash`, `meta-llama/Llama-3.3-70B`, `llama3.2:3b`) round-trip through sync, grants, `provider/model` parsing (split at the FIRST `/` only) and `/v1/models`. Pinned in Tasks 2 and 4.

---

### Task 1: Identity API gaps

**Files:**
- Modify: `crates/gateway/src/api/teams.rs`, `api/users.rs`, `api/auth.rs` (`UserView`), `api/tokens.rs` (`TokenView`), `api/mod.rs` (`ClientAddr`), `identity/policy.rs`, `src/main.rs`, `src/app.rs`, `api/openapi.rs`
- Test: `crates/gateway/tests/api_teams.rs`, `api_users.rs`, `api_tokens.rs`, `api_auth.rs`, `api_roles.rs`

**Interfaces:**
- Produces: `POST /api/teams/{id}/members` body `AddMemberRequest { email: String }` → 201 `TeamMemberView { user_id, email, name, role }` (role always `member`), operationId `teams_member_add`. `PUT /api/teams/{id}/members/{user_id}` becomes admin-only (role changes). `UserView.teams: Vec<UserTeamView { team_id, name, role }>`. `TokenView.status: "active" | "expired" | "revoked"`. `AppState.trusted_proxies: Vec<IpNet>`; CLI `--trusted-proxy <CIDR>` (repeatable), env `UF_TRUSTED_PROXIES` (comma-separated).

**Rules:**
1. `POST /api/teams/{id}/members`: admin or a lead of that team. The email is normalized; unknown email or a user who is not `active` → 404 `user_not_found` "No active user with that email." (same answer for both, so leads cannot probe for disabled accounts); already in the team → 409 `already_member` "Already in this team." with no change. Audit `team.member_add`.
2. `PUT /api/teams/{id}/members/{user_id}`: admin only (`Action::PutMember` → Forbidden for every non-admin). It still adds-or-sets the role for admins.
3. `DELETE /api/teams/{id}/members/{user_id}`: admin; a lead of the team may remove a **member** or themselves, never another lead (403 `forbidden`).
4. Every team membership write (add, put, remove, team delete) calls `refresh_snapshot` (needed by Task 4's team grants).
5. `UserView.teams` lists the user's teams ordered by name, in every response that returns a `UserView` (list, view, me's `user`, update). The list endpoint gets the teams in one query, not one per user.
6. `TokenView.status`: `revoked` if `revoked_at` is set; else `expired` if `expires_at <= now`; else `active` — computed by the gateway at answer time.
7. Trusted proxies: when the TCP peer is inside a trusted CIDR, the client address is `CF-Connecting-IP` if present and valid, else the **last** address of `X-Forwarded-For` that is not itself trusted; otherwise the TCP peer. Untrusted peers' headers are ignored. Invalid CIDR at startup → exit with an error naming the value.

**Tests (Rust):**

| Test | Assertion |
|---|---|
| `lead_adds_member_by_email` | 201, role `member`, audit row, user can see the team |
| `add_by_email_unknown_or_disabled_is_404` | both give the same code and message |
| `add_existing_member_is_409_and_changes_nothing` | a co-lead added again stays lead |
| `only_admins_change_roles` | lead PUT on any member → 403; admin PUT → 204 |
| `lead_cannot_remove_another_lead` | 403; lead removing self → 204 |
| `membership_writes_refresh_the_snapshot` | after add/remove, `state.snapshot` reflects it without `refresh()` |
| `user_view_lists_teams` | list/view/me include `teams`; the list's teams come from one store call (`Store::teams_of_users(&[i64])`, asserted by a unit test of the handler's store use, not by timing) |
| `token_status` | active / expired / revoked |
| `trusted_proxy_uses_forwarded_address` | peer in CIDR + `CF-Connecting-IP` → limiter keyed on it; untrusted peer's header ignored; XFF chain picks last untrusted |
| roles table | new route added for Admin/Lead/Member/Nobody/Token |

- [ ] Step 1: write the tests; run `cargo test -p ultrafast-gateway`; paste the RED output.
- [ ] Step 2: implement; regenerate `openapi/admin.json`; update `ROUTES`.
- [ ] Step 3: gates (fmt, clippy, `cargo test --all`).
- [ ] Step 4: commit `feat(gateway): leads add members by email; only admins change roles` (and smaller commits per rule as fits).

---

### Task 2: Model catalog — storage, sync and admin API

**Files:**
- Create: `crates/gateway/migrations/0003_catalog.sql`, `src/store/models.rs`, `src/catalog/mod.rs`, `src/catalog/sync.rs`, `src/api/models.rs`
- Modify: `src/store/mod.rs`, `src/api/mod.rs`, `src/api/openapi.rs`, `src/identity/policy.rs`, `src/main.rs` (`mod catalog`)
- Test: `crates/gateway/tests/api_models.rs`, `tests/api_roles.rs`

**Interfaces:**
- Migration `0003_catalog.sql` creates:
  - `models(id INTEGER PRIMARY KEY, org_id INTEGER NOT NULL DEFAULT 1, provider_id INTEGER NOT NULL REFERENCES providers(id) ON DELETE CASCADE, name TEXT NOT NULL, enabled INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL DEFAULT (datetime('now')), UNIQUE(provider_id, name))`
  - `model_grants(id INTEGER PRIMARY KEY, org_id INTEGER NOT NULL DEFAULT 1, model_id INTEGER NOT NULL REFERENCES models(id) ON DELETE CASCADE, team_id INTEGER REFERENCES teams(id) ON DELETE CASCADE, user_id INTEGER REFERENCES users(id) ON DELETE CASCADE, CHECK (team_id IS NULL OR user_id IS NULL))` — both NULL means everyone; unique index on `(model_id, IFNULL(team_id,0), IFNULL(user_id,0))`.
  - `ALTER TABLE virtual_keys ADD COLUMN allowed TEXT` (JSON array of model/route names; NULL = no allowlist).
  - `ALTER TABLE providers ADD COLUMN api_version TEXT` (Azure; Task 7).
- `catalog::sync::fetch_model_names(http: &reqwest::Client, provider: &SnapProvider) -> Result<Vec<String>, SyncError>`: OpenAI kind `GET {base}/models` (Bearer) → `data[].id`; Anthropic `GET {base}/v1/models` (x-api-key, anthropic-version) → `data[].id`, following `has_more`/`last_id` up to 10 pages; Gemini and Azure are added in Task 7. 10 s timeout; response capped at 4 MiB; provider errors mapped to `SyncError::Provider { status }` with no provider body echoed.
- Admin API (all admin-only for writes):
  - `GET /api/models` → `ModelList { models: Vec<ModelView> }`; `ModelView { id, provider_id, provider_name, name, enabled, grants: GrantsView { everyone: bool, team_ids: Vec<i64>, user_ids: Vec<i64> }, created_at }`. Admin sees all; others see only models they may call (enabled + granted to them), with `grants` omitted (empty) for non-admins. operationId `models_list`.
  - `POST /api/models` `{ provider_id, name }` → 201 `ModelView` (disabled). `models_create`. 409 `model_exists`.
  - `PATCH /api/models/{id}` `{ enabled: bool }` → `ModelView`. `models_update`.
  - `PUT /api/models/{id}/grants` `GrantsView` → `ModelView`. Unknown team/user ids → 422 `fields.team_ids`/`fields.user_ids`. `models_grants_put`.
  - `DELETE /api/models/{id}` → 204. `models_delete`.
  - `POST /api/providers/{id}/sync` → `SyncResult { added: Vec<String>, existing: u32 }`; new names inserted disabled; never deletes. Upstream failure → 502 `sync_failed` "The provider did not return its models." `providers_sync`.
- Policy: `Action::ListModels` (everyone), `Action::ManageModels` (admin only).

**Rules:**
1. Model `name` is the provider's model id verbatim: 1–200 chars, no whitespace, no control characters; may contain `/`, `:`, `.`, `-`, `_`, `@`.
2. All writes are audited (`model.create`, `model.update` "Enabled …"/"Disabled …", `model.grants`, `model.delete`, `provider.sync` "Synced N new models from <provider>") and refresh the snapshot.
3. Deleting a provider cascades to its models and grants (FK); audit says how many models went with it.

**Tests:** `sync_adds_new_models_disabled` (wiremock OpenAI and Anthropic list answers; second sync adds nothing), `sync_keeps_names_verbatim` (the five names of Review Focus 5), `sync_upstream_error_is_502_without_body`, `create_update_grants_delete`, `grants_validation`, `non_admin_list_shows_only_callable`, `provider_delete_cascades`, roles table rows, openapi `ROUTES`.

- [ ] Step 1: tests, RED pasted. Step 2: implement. Step 3: gates. Step 4: commit `feat(gateway): model catalog with sync and grants`.

---

### Task 3: Routes — storage and admin API

**Files:**
- Create: `crates/gateway/migrations/0004_routes.sql`, `src/store/routes.rs`, `src/api/routes.rs`
- Modify: `src/store/mod.rs`, `src/api/mod.rs`, `src/api/openapi.rs`, `src/identity/policy.rs`
- Test: `crates/gateway/tests/api_routes.rs`, `tests/api_roles.rs`

**Interfaces:**
- `0004_routes.sql`:
  - `routes(id INTEGER PRIMARY KEY, org_id INTEGER NOT NULL DEFAULT 1, name TEXT NOT NULL, retries INTEGER NOT NULL DEFAULT 2, first_token_timeout_ms INTEGER NOT NULL DEFAULT 30000, total_timeout_ms INTEGER NOT NULL DEFAULT 300000, breaker_failures INTEGER NOT NULL DEFAULT 5, breaker_window_s INTEGER NOT NULL DEFAULT 60, breaker_open_s INTEGER NOT NULL DEFAULT 30, created_at TEXT NOT NULL DEFAULT (datetime('now')), UNIQUE(org_id, name))`
  - `route_targets(id INTEGER PRIMARY KEY, route_id INTEGER NOT NULL REFERENCES routes(id) ON DELETE CASCADE, model_id INTEGER NOT NULL REFERENCES models(id) ON DELETE CASCADE, tier TEXT NOT NULL CHECK (tier IN ('primary','fallback')), weight INTEGER NOT NULL DEFAULT 1, position INTEGER NOT NULL)`
  - `route_grants(route_id INTEGER NOT NULL REFERENCES routes(id) ON DELETE CASCADE, team_id INTEGER NOT NULL REFERENCES teams(id) ON DELETE CASCADE, PRIMARY KEY(route_id, team_id))` — no rows: every team (and users without a team) may use the route.
- API: `GET /api/routes` (`routes_list`; non-admins see routes they may use, name and targets' model names only), `POST /api/routes` (`routes_create`), `GET /api/routes/{id}` (`routes_view`), `PUT /api/routes/{id}` (`routes_update`, full replace), `DELETE /api/routes/{id}` (`routes_delete`).
  - `RouteRequest { name, primaries: Vec<{ model_id, weight }>, fallbacks: Vec<i64 /*model_id, in order*/>, retries, first_token_timeout_ms, total_timeout_ms, breaker_failures, breaker_window_s, breaker_open_s, team_ids: Vec<i64> }`.
  - `RouteView { id, name, primaries: Vec<{ model_id, model: "provider/model", weight, enabled }>, fallbacks: Vec<{ model_id, model, enabled }>, retries, …timeouts…, …breaker…, team_ids, broken: bool, created_at }`. `broken` = no enabled target left.
- Policy: `ListRoutes` (everyone), `ManageRoutes` (admin).

**Rules:**
1. Name: 1–64 chars `[a-z0-9][a-z0-9._-]*`, **no `/`** (so a route never collides with `provider/model`). 409 `route_exists`.
2. Validation (422 with `fields`): at least one primary; weights 1–1000; a model at most once per route; retries 0–5; first token 1 000–300 000 ms; total 1 000–3 600 000 ms and ≥ first token; breaker failures 1–100, window 5–3 600 s, open 5–3 600 s; unknown model/team ids.
3. Audited (`route.create`, `route.update`, `route.delete`), snapshot refreshed.

**Tests:** `create_view_update_delete`, `validation_errors_on_fields`, `name_with_slash_is_refused`, `model_delete_removes_target_and_marks_broken` (Review Focus 3), `non_admin_sees_only_usable_routes`, roles table, openapi.

- [ ] Steps: tests RED → implement → gates → commit `feat(gateway): routes with targets, fallbacks and team grants`.

---

### Task 4: Access checks on `/v1` and `GET /v1/models`

**Files:**
- Modify: `crates/gateway/src/snapshot.rs`, `src/proxy.rs`, `src/auth.rs`, `src/app.rs`, `src/api/keys.rs`, `src/store/keys.rs`
- Create: `crates/gateway/src/access.rs`
- Test: `crates/gateway/tests/access.rs`, `tests/snapshot.rs`, `tests/api_keys.rs`

**Interfaces:**
- Snapshot gains: `models: HashMap<(String /*provider*/, String /*model*/), SnapModel { id, provider: String, name, enabled, everyone: bool, team_ids: HashSet<i64>, user_ids: HashSet<i64> }>`, `routes: HashMap<String, SnapRoute>` (Task 6 uses targets/settings), `users: HashMap<i64, SnapUser { role: Role, team_ids: HashSet<i64> }>`; `SnapKey.allowed: Option<HashSet<String>>`.
- `access::resolve(snapshot: &Snapshot, key: &SnapKey, requested: &str) -> Result<Resolved, Denied>` where `Resolved::Model(&SnapModel) | Resolved::Route(&SnapRoute)` and `Denied::Unknown | Denied::Forbidden`. `access::may_call_model(snapshot, key, &SnapModel) -> bool` (used by routing to skip targets).
- `GET /v1/models` → `{ "object": "list", "data": [{ "id": "provider/model" | "route-name", "object": "model", "created": 0, "owned_by": "<provider>" | "ultrafast-route" }] }`, sorted by id.
- `CreateKeyRequest.allowed: Option<Vec<String>>`; `KeyView.allowed: Option<Vec<String>>`. Unknown names → 422 `fields.allowed`.

**Rules:**
1. `requested` with a `/` → split at the FIRST `/` into provider and model; else it is a route name. Unknown → `Denied::Unknown`.
2. Model checks in order: enabled; grant (everyone, a team of the key's owner, the owner) — skipped for admin owners; key allowlist contains `provider/model` (or, through a route, the route name). A key with **no owner** (CLI-created) may call only models granted to everyone. Failures → `Denied::Forbidden`.
3. Route checks: route grants include a team of the owner (or no grants); key allowlist contains the route name. A route call is allowed if at least one target is callable by this caller; otherwise 403.
4. Messages: 404 "Unknown model '{m}'." ; 403 "You do not have access to model '{m}'." — in OpenAI shape here (Anthropic shape in Task 8).
5. `/v1/models` lists exactly what this key can call (models and routes).
6. Owner disabled / removed from team / grant removed → next call reflects it (snapshot refresh on all those writes: users PATCH already refreshes; Task 1 adds team writes; Task 2 adds grants).

**Tests:** a matrix test `access_matrix` (admin/lead/member/ownerless × enabled/disabled × everyone/team/user/none × allowlist yes/no/absent → expected status), `left_team_loses_team_grant` (Review Focus 1), `route_access`, `v1_models_lists_only_callable`, `names_split_at_first_slash` (Review Focus 5), `key_allowlist_validation`.

- [ ] Steps: tests RED → implement → gates → commit `feat(gateway): model access on /v1 and GET /v1/models`.

---

### Task 5: Request sink and attempt records (interfaces for plan 6)

**Files:**
- Create: `crates/gateway/src/telemetry.rs`
- Modify: `src/app.rs`, `src/proxy.rs`
- Test: unit tests in `telemetry.rs`, `tests/proxy.rs`

**Interfaces:**
- `pub struct Attempt { pub provider: String, pub model: String, pub outcome: AttemptOutcome, pub status: Option<u16>, pub duration_ms: u64 }`; `pub enum AttemptOutcome { Ok, Retryable, Fatal, CircuitOpen, Skipped }`.
- `pub struct RequestRecord { pub key_id: i64, pub user_id: Option<i64>, pub team_id: Option<i64>, pub requested: String, pub endpoint: &'static str /* "chat" | "messages" | "embeddings" */, pub stream: bool, pub status: u16, pub usage: Option<Usage>, pub attempts: Vec<Attempt>, pub started_at: String, pub duration_ms: u64 }`.
- `pub trait RequestSink: Send + Sync { fn record(&self, record: RequestRecord); }` — must not block; `NoopSink`. `AppState.sink: Arc<dyn RequestSink>`, default `NoopSink`.
- The proxy builds and records one `RequestRecord` per `/v1` call that passed authentication, including streams (recorded when the stream ends or the caller drops it).

**Rules:** no prompt, response text, or credential in the record. A test sink (`Mutex<Vec<RequestRecord>>`) proves records for success, upstream error, stream end and caller disconnect.

- [ ] Steps: tests RED → implement → gates → commit `feat(gateway): one record per request, with its attempts`.

---

### Task 6: Routing engine — selection, retries, timeouts, circuit breaker

**Files:**
- Create: `crates/gateway/src/routing/mod.rs`, `routing/select.rs`, `routing/breaker.rs`, `routing/health.rs`, `src/api/health.rs`
- Modify: `src/proxy.rs`, `src/snapshot.rs`, `src/app.rs`, `src/api/mod.rs`, `src/api/openapi.rs`
- Test: `crates/gateway/tests/routing.rs`, unit tests in `routing/*.rs`

**Interfaces:**
- `SnapRoute { id, name, primaries: Vec<(TargetRef, u32 /*weight*/)>, fallbacks: Vec<TargetRef>, retries: u32, first_token_timeout: Duration, total_timeout: Duration, breaker: BreakerSettings, team_ids: HashSet<i64> }`, `TargetRef { provider: String, model: String, model_id: i64 }`.
- `pub trait HealthStore: Send + Sync { fn allow(&self, t: &TargetRef, now: Instant, s: &BreakerSettings) -> bool; fn report(&self, t: &TargetRef, ok: bool, retryable_failure: bool, now: Instant, s: &BreakerSettings); fn view(&self) -> Vec<TargetHealth>; }` with `InMemoryHealth` (Mutex<HashMap<(provider, model), State>>). `AppState.health: Arc<dyn HealthStore>`.
- `routing::plan(route: &SnapRoute, rng: &mut impl Rng) -> Vec<TargetRef>`: weighted random order of primaries (each once), then fallbacks in order.
- A direct `provider/model` call uses the same engine with a one-target plan and default settings (retries 2, timeouts 30 s / 300 s, breaker 5/60/30).
- `GET /api/routing/health` (admin) → `{ targets: Vec<TargetHealth { provider, model, state: "closed"|"open"|"half_open", successes: u64, failures: u64, last_failure_at: Option<String>, last_status: Option<u16> }> }`, operationId `routing_health`.

**Rules:**
1. For each target in plan order: skip (attempt `Skipped`) if the caller may not call it (Task 4) or it is disabled; skip (`CircuitOpen`) if the breaker refuses; else try up to `1 + retries` times with exponential backoff (base 250 ms, factor 2, max 4 s, full jitter) on retryable failures; a fatal 4xx returns that error to the caller at once.
2. Timeouts: until the first response byte (non-stream: until headers+body start; stream: until the first event) `first_token_timeout`; whole request `total_timeout` (a stream past it ends with an error event). Timeouts are retryable.
3. Streaming: the first successful event commits the target; later errors end the stream with `render_stream_error` and are reported to the breaker; no retry.
4. Breaker per (provider, model): counts only retryable failures in a sliding `window`; opens at `failures`; after `open` seconds allows one trial (half-open); success closes, failure re-opens.
5. All targets exhausted → 503 `upstream_error` "No provider could serve this request." (fatal 4xx keeps its own status). Nothing names a provider credential.
6. The record (Task 5) carries every attempt.

**Tests:** weighted selection distribution (seeded rng, 10 000 draws within ±3 %), fallback order, `retries_on_429_and_5xx_not_on_400`, `backoff_bounded` (paused tokio time), `first_token_timeout_fails_over`, `stream_fails_over_before_first_byte_not_after` (Review Focus 4; caller sees no duplicated text), `breaker_opens_half_opens_closes`, `bad_requests_do_not_open_breaker`, `all_open_is_fast_503` (Review Focus 2: under 100 ms with tokio paused time), `skips_targets_caller_may_not_call`, `health_endpoint`, roles table.

- [ ] Steps: tests RED → implement → gates → commit `feat(gateway): routing with fallbacks, retries, timeouts and circuit breaking`.

---

### Task 7: Providers — Gemini and Azure OpenAI

**Files:**
- Create: `crates/translate/src/provider/gemini.rs`, `provider/azure.rs`
- Modify: `crates/translate/src/provider/mod.rs` (`ProviderKind::{Gemini, Azure}`, `Target.api_version: Option<String>`), `crates/gateway/src/catalog/sync.rs`, `src/api/providers.rs` (`api_version` in create/update/view; kind values), `src/main.rs` (`provider add --kind`), `src/config.rs`
- Test: translate unit tests with recorded JSON fixtures in `crates/translate/tests/fixtures/{gemini,azure}/`, `crates/gateway/tests/proxy.rs`, `tests/api_providers.rs`

**Interfaces / rules:**
1. Kinds stored as `"gemini"` and `"azure"`.
2. Gemini: `POST {base}/v1beta/models/{model}:generateContent`, stream `:streamGenerateContent?alt=sse`; header `x-goog-api-key`; system messages → `systemInstruction`; roles user/model; `generationConfig { maxOutputTokens, temperature, topP, stopSequences }`; usage from `usageMetadata { promptTokenCount, candidatesTokenCount }`; finish reasons STOP→stop, MAX_TOKENS→length, SAFETY/RECITATION→content_filter. Default base URL offered by the console: `https://generativelanguage.googleapis.com`. Sync: `GET {base}/v1beta/models` (pages via `nextPageToken`), names with the `models/` prefix stripped, only those whose `supportedGenerationMethods` contain `generateContent` or `embedContent`.
3. Azure: `POST {base}/openai/deployments/{model}/chat/completions?api-version={api_version}`; header `api-key`; body and answers in OpenAI format (`model` omitted from the body). `api_version` required for Azure (default `2024-10-21` when not given), stored in `providers.api_version`, validated `^\d{4}-\d{2}-\d{2}(-preview)?$`. Sync: none (deployments are named by the admin; `POST /api/providers/{id}/sync` answers 422 `sync_unsupported` "Add Azure deployments as models by name.").
4. Provider errors of both map through the existing `TranslateError::Provider` rules.

**Tests:** request building and response/stream parsing from fixtures for both; gateway proxy tests through wiremock for both kinds (non-stream and stream); provider API accepts the new kinds and `api_version`; sync for Gemini.

- [ ] Steps: tests RED → implement → gates → commit `feat: Gemini and Azure OpenAI providers`.

---

### Task 8: Anthropic Messages ingress and embeddings

**Files:**
- Create: `crates/translate/src/ingress/anthropic.rs`, `crates/translate/src/embeddings.rs`
- Modify: `crates/translate/src/ingress/mod.rs`, `src/types.rs` (only if needed), provider modules (embeddings), `crates/gateway/src/proxy.rs`, `src/auth.rs`, `src/app.rs` (routes), `src/errors.rs`
- Test: translate unit tests, `crates/gateway/tests/messages.rs`, `tests/embeddings.rs`

**Interfaces / rules:**
1. `POST /v1/messages` accepts the Anthropic Messages request (`model`, `max_tokens` required, `system` string or text blocks, `messages` with string or text-block content, `temperature`, `top_p`, `stop_sequences`, `stream`); unknown fields rejected except `metadata`; non-text blocks → 400 `invalid_request_error` "Only text content is supported." Answers in Anthropic format (`message` object with `content: [{type:"text",text}]`, `stop_reason` end_turn/max_tokens/stop_sequence, `usage {input_tokens, output_tokens}`); streams as Anthropic SSE events (`message_start`, `content_block_start`, `content_block_delta`, `content_block_stop`, `message_delta`, `message_stop`; errors as `event: error`). Errors in Anthropic shape `{"type":"error","error":{"type","message"}}` with types `authentication_error`, `permission_error`, `not_found_error`, `invalid_request_error`, `rate_limit_error`, `api_error`, `overloaded_error` (503 → `overloaded_error`).
2. Authentication on `/v1` accepts `Authorization: Bearer uf-sk-…` and `x-api-key: uf-sk-…` (both endpoints); `anthropic-version` header accepted and ignored.
3. `/v1/messages` uses the same access checks and routing engine as chat completions; any provider kind may serve it.
4. `POST /v1/embeddings` (OpenAI format: `model`, `input` string or array of strings, `dimensions` optional, `encoding_format` "float" only) → OpenAI embeddings answer with `usage.prompt_tokens`. Served by OpenAI, Azure (`/openai/deployments/{m}/embeddings`) and Gemini (`:batchEmbedContents`); an Anthropic target → skipped as unsupported; no other target → 400 "This model does not support embeddings." Same access checks and routing (no streaming).
5. Records (Task 5) use endpoint `"messages"` / `"embeddings"`.

**Tests:** Anthropic SDK-shaped requests and answers (non-stream and stream) served by an OpenAI-kind mock and by an Anthropic-kind mock; error shapes per status; `x-api-key` auth; embeddings for the three kinds; unsupported target.

- [ ] Steps: tests RED → implement → gates → commit `feat: Anthropic Messages ingress and embeddings`.

---

### Task 9: Console — Models page

**Files:**
- Create: `ui/src/pages/Models.tsx`, `ModelsAccess.tsx` (grants dialog), `ModelsAdd.tsx`, `ui/src/lib/models.ts` (filters, access summary), tests `ui/src/pages/models.test.tsx`, `ui/src/lib/models.test.ts`
- Modify: `ui/src/router.tsx` (`/models`), `components/AppSidebar.tsx` (Models no longer "Coming"; add to `Path`), `api/queries.ts` (`useModels`, `useCreateModel`, `useUpdateModel`, `usePutModelGrants`, `useDeleteModel`, `useSyncProvider`; hook-count pin), `auth/guards.ts` (`manageModels`), `test/fixtures.ts` + `fixtures.test.ts`, `test/handlers.ts`, `src/api/schema.d.ts` (regenerated)

**Rules:**
1. Admin view: table of models — name (mono), provider, status switch "Enabled"/"Disabled", access summary ("Everyone", "No one", "2 teams, 1 user"), actions Edit access, Delete. Filters (client-side): text over name and provider, provider, status. Default sort provider then name.
2. Header actions (admin): "Sync models" (choose a provider; shows "Added N models. They start disabled." or "No new models."; Azure shows the gateway's `sync_unsupported` message), "Add model" (provider + model name; hint "The provider's model ID, for example gpt-4o-mini.").
3. Edit access dialog (FormDialog): "Everyone" switch; when off, team and user multi-selects (from `useTeams`/`useUsers`); sends the full `GrantsView`.
4. Enabling a model with no grant shows a hint in the row: "Enabled, but nobody has access yet." (admins can still call it).
5. Non-admins: read-only list "Models you can use" — name `provider/model` (mono) with a copy button; empty state "No models are available to you yet. Ask an admin."
6. Delete confirms with "Routes that use this model lose this target. Calls to it fail at once."

**Tests:** list and filters; enable/disable sends PATCH and the row follows; sync result texts (added / none / unsupported / 502); add model with field errors (`model_exists`); access dialog sends exact grants; member view read-only and copy; delete confirm; roles (member sees no admin controls); one `main`/`h1`; 390 px layout via existing helpers.

- [ ] Steps: tests RED → implement → gates (lint, typecheck, test, build) → commit `feat(console): models page`.

---

### Task 10: Console — Routing page

**Files:**
- Create: `ui/src/pages/Routes.tsx`, `RoutesEdit.tsx` (create/edit form dialog or full page `/routes/$id`), `RoutesHealth.tsx`, `ui/src/lib/routes.ts` (form ↔ request mapping, validation mirror), tests `routes.test.tsx`, `lib/routes.test.ts`
- Modify: `router.tsx` (`/routes`, `/routes/$id`), `AppSidebar.tsx` (Routing live), `queries.ts` (`useRoutes`, `useRoute`, `useCreateRoute`, `useUpdateRoute`, `useDeleteRoute`, `useRoutingHealth`), `guards.ts` (`manageRoutes`, `viewRoutingHealth`), fixtures/handlers/schema

**Rules:**
1. List: name (mono), primaries (`provider/model ×weight`), fallbacks count, teams ("All teams" or count), status badge "Ready" / "Broken" (`broken`), actions Edit, Delete (admin). Non-admins: name and its models only, with a copy button for the name.
2. Editor (admin, page `/routes/$id` and `/routes/new`): name; Primary targets (model select from enabled models + weight 1–1000, add/remove rows); Fallbacks (ordered list, add, remove, move up/down with buttons — no drag); Teams ("All teams" or chosen); Advanced (collapsed): retries, first-token timeout (s), total timeout (s), breaker failures / window (s) / open (s), defaults shown. Client-side validation mirrors the gateway's rules; gateway field errors shown on their fields (map `primaries[i]` etc. to the row).
3. Health panel on the route page and an admin "Target health" section on the list: per target state badge "Healthy" (closed) / "Failing" (open) / "Testing" (half_open), successes, failures, last failure time. Text above it: "From real traffic only. The gateway does not send test requests."
4. Delete confirms with "Calls that use this route fail at once."

**Tests:** list (ready/broken), editor create and edit round-trip (exact request body), move fallback up/down, validation and gateway field errors on rows, health states, member read-only view, delete confirm, one `main`/`h1`, 390 px.

- [ ] Steps: tests RED → implement → gates → commit `feat(console): routing page`.

---

### Task 11: Console — identity changes, providers and keys

**Files:**
- Modify: `ui/src/pages/TeamDetail.tsx`, `TeamDetailAddMember.tsx`, `Users.tsx`, `UserDetail.tsx`, `KeysCreate.tsx`, `Keys.tsx`, `AccountTokens.tsx`, `ProvidersAdd.tsx`, `ProvidersEdit.tsx`, `Providers.tsx`, `lib/keys.ts`, `lib/token-status.ts` (remove), `lib/providers.ts`, `auth/guards.ts`, `api/queries.ts` (`useAddTeamMember`; drop `useTeamDetails` if unused), tests of each, fixtures/handlers/schema

**Rules:**
1. Add member: a single "Email" field for leads AND admins (admins may still pick from the user list as a shortcut that fills the email); POST by email; `user_not_found` and `already_member` on the field. The "User ID" field and its hint go.
2. Role changes ("Make lead"/"Make member") are shown to admins only; a lead sees "Remove" for members and "Leave team" for themselves, nothing on other leads.
3. Users list gains a "Teams" column (names, "—" when none); user detail lists teams with roles.
4. Create key: the owner's teams come from `UserView.teams` (no per-team reads); new "Allowed models" field: "All models I can use" (default) or a multi-select of callable models and routes (from `GET /api/models` + `GET /api/routes` for the owner — for another owner the admin's lists, the gateway validates); key list shows "All" or the count.
5. Access tokens use `TokenView.status`; `lib/token-status.ts` and its clock dependence go.
6. Providers: kinds "Gemini" and "Azure OpenAI" with known base URLs (Gemini `https://generativelanguage.googleapis.com`); Azure shows an "API version" field (default `2024-10-21`) and the hint "Base URL is your resource endpoint, for example https://my-resource.openai.azure.com."; a "Sync models" action per provider row for admins (same texts as Task 9); after adding a provider the "how to call it" notice also says "Enable its models on the Models page first." with a link.
7. README "Known limits" and "Not yet" updated: the three API gaps of plan 1 are gone; Models and Routing are live.

**Tests:** for each rule, in the existing test files; E2E impact handled in Task 12.

- [ ] Steps: tests RED → implement → gates → commit(s) `feat(console): …`.

---

### Task 12: End-to-end flows and release check

**Files:**
- Create: `ui/e2e/models.spec.ts`, `ui/e2e/routing.spec.ts`
- Modify: `ui/e2e/keys.spec.ts`, `roles.spec.ts`, `ui/e2e/mock-provider.ts` (failure modes: status, delay before first byte, error after first chunk; a `/models` list), `ui/e2e/steps.ts`

**Flows:**

| Spec | Flow |
|---|---|
| `keys.spec` (updated) | admin adds a provider, syncs models, enables one and grants it to everyone, creates a key, calls `/v1/chat/completions` and `/v1/messages` (Anthropic shape) through it; revoke → 401 |
| `models.spec` | a member's key cannot call a disabled model (403) nor an enabled one not granted to them; after a grant to their team it can; `/v1/models` lists exactly the granted ones; the member's Models page lists the same |
| `routing.spec` | two mock providers; route with primary A and fallback B; A answers 500 → the call succeeds from B; A down for N calls → the health panel shows A "Failing"; route editor round-trip in the console; a member without the route's team gets 403 |
| `roles.spec` (updated) | a lead adds a member by email, sees no Make lead, cannot remove another lead |

Then: three full E2E runs in a row green; `pnpm --dir ui lint && typecheck && test && build`; Rust fmt, clippy, `cargo test --all`; `cargo build --release -p ultrafast-gateway`; report binary size and gzipped JS/CSS.

- [ ] Steps: specs → run (RED where the flow is new) → fix console defects found (one commit each) → three runs → final checks → commit `test(console): models and routing flows`.

---

## Known limits after this plan

- Request logs, usage, spend, budgets, rate limits and the cache arrive in plan 6; until then attempt records go to a no-op sink and health resets on restart.
- Content is text only (no images, tools or audio) on every endpoint.
- Azure models are added by deployment name; there is no Azure sync.

## Spec coverage

| Spec item | Task |
|---|---|
| Phase 1 endpoints: chat completions, Anthropic Messages, embeddings, model list, streaming | 4, 6, 8 |
| Providers: OpenAI, Anthropic, Gemini, Azure, Ollama/Groq/Mistral/OpenRouter (OpenAI-compatible) | 7 (others already via OpenAI kind) |
| Routes: weights, fallbacks, retries, timeouts, circuit breaking; health from real traffic; attempts recorded | 3, 5, 6 |
| Models page: enable/disable, grants to teams and users; new models disabled | 2, 9 |
| Key allowlist of models and routes | 4, 11 |
| `/v1/models` lists what the key can call | 4 |
| Console Routing page | 10 |
| Identity gaps: add by email, admin-only roles, user's teams, token status, trusted proxy | 1, 11 |
