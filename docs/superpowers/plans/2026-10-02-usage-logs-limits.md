# Usage, Logs, Limits, Budgets and Cache Implementation Plan (gateway plan 4 + console plan 3)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development with the project skill `uf-workflow` and the agents in `.claude/agents/`. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Every `/v1` call is logged and priced; admins, leads and members see usage and logs scoped to their role; rate limits and budgets are enforced in memory on the hot path; routes can cache exact-match responses; Prometheus metrics are exposed — backend and console for every feature.

**Architecture:** The `RequestSink` of plan 5 gets a real implementation: a bounded channel feeding one background writer that batches inserts into `request_logs` (a request is never delayed by logging; a full queue drops and counts). Prices live on models; cost is computed when the record is written. Limits, budgets and the cache are in-memory behind traits (`Limiter`, `Budgets`, `ResponseCache`) so a shared store can replace them later; budget counters are flushed to the database every 5 s and rebuilt from the logs at startup. The proxy pipeline gains steps 6 (limits, budgets), 7 (cache) and 9 (account) of spec section 7.

**Tech Stack:** Rust 1.94, axum 0.8, sqlx 0.9 SQLite (WAL), tokio; React, TypeScript, TanStack, shadcn/ui, Vitest + MSW, Playwright.

**Spec:** `docs/superpowers/specs/2026-09-28-gateway-v2-design.md` — sections 3, 6 (`/metrics`), 7 (steps 6, 7, 9, 10), 10, 11, 13. Conventions: `docs/CONVENTIONS.md` (binding). Previous plan's ledger: `docs/superpowers/plans/2026-10-01-models-and-routing-ledger.md`.

## Global Constraints

- The `/v1` hot path never waits on the database: logging goes through the bounded queue; limits, budgets and cache are checked in memory (spec 7, 10).
- Limits: the strictest of key, user, team and gateway applies; a refusal is 429 with `Retry-After` and a body naming the limit (`"rate limit 'tokens per minute' of team 'Platform' reached"`), in the ingress shape (OpenAI or Anthropic) (spec 7 step 6).
- Budgets: amount (USD), period daily/weekly/monthly (UTC calendar periods), action `block` (429 `budget_exceeded`) or `alert` (allow; one audit row `budget.alert` per budget per period). Spend counters flushed every 5 s and on shutdown; rebuilt from `request_logs` at startup (spec 10).
- Cache: exact match only; key over every output-affecting field with tagged fields; scope team (default), key or user always in the key; never across teams; temperature > 0.5 and streams not cached; bounded by entry count and total bytes with LRU (spec 10). Success criterion: **no cached response is ever served across teams** (spec 1).
- Logs store metadata only; prompt and response bodies are not stored in this plan (known limit). Retention: default 30 days, deleted in batches by a background task (spec 11).
- Visibility: admin all; team lead their teams' keys and members; member their own keys (spec 8, 13). The API enforces; the console hides only what the API refuses.
- Prices are per 1 million tokens, input and output, stored as integer micro-dollars per million; unknown price → cost 0 and the log says so (`priced: false`).
- Owner decision for this plan (controller ruling, recorded in the ledger): team leads **view** their team's limits and budgets; only admins create or change them (spec 8's "within limits an admin set" is satisfied by admins setting them; lead self-service is LATER).
- Everything in `docs/CONVENTIONS.md` applies (OpenAPI single source + `ROUTES` + role table; snapshot refresh after writes that change `/v1` behaviour; console patterns; tests first; production safety).

## Review Focus

1. Two teams send the identical request to a cached route → each team's first call reaches the provider; neither gets the other's answer. Pinned in Task 6.
2. A burst of concurrent calls against a concurrency limit of 2 → exactly 2 run, the rest get 429 at once, and the slots are released when calls end, fail or the caller disconnects (no leak). Pinned in Task 4.
3. The log queue is full (writer slow or DB locked) → calls still answer at full speed; dropped records are counted and visible in `/metrics`. Pinned in Task 1.
4. The gateway restarts mid-month → budgets resume from the logged spend; a block budget already exceeded keeps blocking after restart. Pinned in Task 5.
5. A lead or member asks for logs or usage outside their scope by query parameter (`?key_id=`, `?team_id=`) → empty or 404, never other teams' data. Pinned in Task 2 and 3.

---

### Task 1: Request logs — storage, prices, batched writer, retention

**Files:** Create `crates/gateway/migrations/0006_logs.sql`, `src/logs/mod.rs`, `src/logs/writer.rs`, `src/logs/retention.rs`, `src/store/logs.rs`, `src/store/settings.rs`; Modify `src/app.rs`, `src/main.rs`, `src/telemetry.rs` (only to add fields if needed), `src/api/models.rs` (prices), `src/store/models.rs`. Tests: `tests/logs.rs`, unit tests.

**Interfaces:**
- `0006_logs.sql`: `request_logs(id INTEGER PRIMARY KEY, org_id INTEGER NOT NULL DEFAULT 1, at TEXT NOT NULL, key_id INTEGER, user_id INTEGER, team_id INTEGER, requested TEXT NOT NULL, endpoint TEXT NOT NULL, stream INTEGER NOT NULL, status INTEGER NOT NULL, provider TEXT, model TEXT, input_tokens INTEGER, output_tokens INTEGER, cost_micros INTEGER NOT NULL DEFAULT 0, priced INTEGER NOT NULL DEFAULT 0, cached INTEGER NOT NULL DEFAULT 0, duration_ms INTEGER NOT NULL, attempts TEXT NOT NULL /*JSON*/)`; indexes on `at`, `(key_id, at)`, `(user_id, at)`, `(team_id, at)`. `ALTER TABLE models ADD COLUMN input_price_micros INTEGER` and `output_price_micros INTEGER` (per 1M tokens; NULL = unknown). `settings(key TEXT PRIMARY KEY, value TEXT NOT NULL)` with `log_retention_days` default 30.
- `logs::LogSink` implements `RequestSink`: `try_send` into a `tokio::sync::mpsc` channel of 10 000; full → drop + `AtomicU64 dropped`. `logs::writer::spawn(store, rx, prices: Arc<ArcSwap<Prices>> or snapshot access)` batches up to 500 records or 1 s, one transaction per batch; `provider`/`model` = the attempt that answered (last `Ok`, else last attempt); cost = tokens × price / 1e6 rounded half-up; writer drains the queue on shutdown.
- `retention::spawn` every hour deletes rows older than the setting in batches of 1 000 with a short pause between batches.
- Admin API: `PATCH /api/models/{id}` accepts optional `input_price_micros`, `output_price_micros` (≥ 0 or null); `ModelView` gains both. `GET /api/settings` / `PATCH /api/settings` `{ log_retention_days: 1..=3650 }` admin only (`settings_view`, `settings_update`).

**Tests:** records written in batches; cost computed (priced and unpriced); a full queue drops and counts without blocking (Review Focus 3: sink with a channel of 1 and a stalled writer; 1 000 calls finish quickly); shutdown drains; retention deletes only old rows in batches; price and settings APIs (validation, admin-only, audit); role table; OpenAPI.

- [ ] Tests RED → implement → gates → commit(s).

---

### Task 2: Logs API

**Files:** Create `src/api/logs.rs`; Modify `src/api/mod.rs`, `src/api/openapi.rs`, `src/identity/policy.rs`, `src/store/logs.rs`. Tests: `tests/api_logs.rs`, `tests/api_roles.rs`.

**Interfaces:** `GET /api/logs?before=<id>&limit=<1..200, default 50>&from=&to=&key_id=&user_id=&team_id=&model=&status=` (`logs_list`) → `{ logs: [LogView { id, at, key_id, key_name, user_id, user_email, team_id, team_name, requested, endpoint, stream, status, provider, model, input_tokens, output_tokens, cost_micros, priced, cached, duration_ms }] }` newest first; `GET /api/logs/{id}` (`logs_view`) → `LogView` + `attempts: [{ provider, model, outcome, status, duration_ms }]`. Scope: admin all; a lead the rows whose `team_id` is a team they lead or whose `user_id` is a member of such a team or themselves; a member rows with their own `user_id`. Filters intersect with the scope (Review Focus 5); a log outside the scope is 404.

**Tests:** scope matrix for admin/lead/member; filters; cursor; out-of-scope id 404; filter parameters cannot widen scope; role table; OpenAPI.

- [ ] Tests RED → implement → gates → commit.

---

### Task 3: Usage API

**Files:** Create `src/api/usage.rs`; Modify `src/store/logs.rs`, `src/api/mod.rs`, `src/api/openapi.rs`. Tests: `tests/api_usage.rs`.

**Interfaces:** `GET /api/usage?from=YYYY-MM-DD&to=YYYY-MM-DD&group=day|model|key|user|team` (`usage_view`) → `{ from, to, total: UsageRow, rows: [UsageRow { group: string, label: string, requests, errors /*status >= 400*/, input_tokens, output_tokens, cost_micros, unpriced_requests }] }`. Same scope rules as Task 2. Range at most 366 days; default last 30 days; dates UTC. `group=user|team` for members returns only themselves / their teams. Aggregation by SQL over the indexes, one query per call.

**Tests:** aggregates exact over seeded logs; each group; scope (Review Focus 5); range validation; role table; OpenAPI.

- [ ] Tests RED → implement → gates → commit.

---

### Task 4: Rate limits

**Files:** Create `migrations/0007_limits.sql`, `src/limits/mod.rs`, `src/limits/window.rs`, `src/store/limits.rs`, `src/api/limits.rs`; Modify `src/snapshot.rs`, `src/proxy.rs`, `src/app.rs`, `src/errors.rs`, `src/api/mod.rs`, `src/api/openapi.rs`, `src/identity/policy.rs`. Tests: `tests/limits.rs`, `tests/api_limits.rs`, unit tests.

**Interfaces:**
- `rate_limits(id, org_id, scope TEXT CHECK IN ('gateway','team','user','key'), scope_id INTEGER /*NULL for gateway*/, requests_per_minute INTEGER, tokens_per_minute INTEGER, concurrent INTEGER, UNIQUE(org_id, scope, scope_id))` — each limit nullable (no limit).
- `trait Limiter: Send + Sync { fn acquire(&self, who: &Subjects, estimate_tokens: u64, now: Instant) -> Result<Permit, Refusal>; }` where `Subjects { key, user, teams, gateway }`; `Permit` releases concurrency on Drop and `Permit::settle(actual_tokens)` corrects the token window; `Refusal { limit_name, scope_label, retry_after: Duration }`. In-memory sliding window (per-second buckets over 60 s). Limits come from the snapshot.
- Pipeline: after access (step 5), before dispatch. Estimate = `max_tokens` (or 1 000 when absent) + input estimate (characters / 4). The permit lives for the whole call including the stream (released on end, error, caller drop).
- API: `GET /api/limits` (admin all; lead/member: those applying to them), `PUT /api/limits` `{scope, scope_id, requests_per_minute?, tokens_per_minute?, concurrent?}` (upsert, admin), `DELETE /api/limits/{id}` (admin). Names for errors: "requests per minute", "tokens per minute", "concurrent requests"; scope labels "gateway", "team '<name>'", "user '<email>'", "key '<name>'".

**Tests:** each limit at each scope; strictest applies; 429 body and Retry-After in both ingress shapes; concurrency burst (Review Focus 2: 20 parallel calls, limit 2, slow mock → 2 succeed, 18 refused immediately; slots released after completion, after upstream failure, after caller drop); token correction after the call; snapshot refresh on limit writes; API validation, role table, OpenAPI.

- [ ] Tests RED → implement → gates → commit(s).

---

### Task 5: Budgets

**Files:** Create `migrations/0008_budgets.sql`, `src/budgets/mod.rs`, `src/store/budgets.rs`, `src/api/budgets.rs`; Modify `src/proxy.rs`, `src/logs/writer.rs` (account spend), `src/app.rs`, `src/main.rs` (flush on shutdown), `src/errors.rs`, API wiring. Tests: `tests/budgets.rs`, `tests/api_budgets.rs`.

**Interfaces:**
- `budgets(id, org_id, scope CHECK IN ('gateway','team','user','key'), scope_id, amount_micros INTEGER NOT NULL CHECK > 0, period CHECK IN ('daily','weekly','monthly'), action CHECK IN ('block','alert'), created_at, UNIQUE(org_id, scope, scope_id, period))`; `budget_usage(budget_id, period_start TEXT, spent_micros INTEGER, alerted INTEGER, PRIMARY KEY(budget_id, period_start))`.
- `trait Budgets: Send + Sync { fn check(&self, who: &Subjects, now) -> Result<(), BudgetRefusal>; fn spend(&self, who: &Subjects, micros: u64, now); }` in memory; `spend` called when the log writer prices a record (step 9 "account"; a call already running is not stopped mid-way). Weekly periods start Monday 00:00 UTC.
- Flush every 5 s and on graceful shutdown; at startup rebuild current-period spend from `request_logs` (sum of cost by scope since period start) — the logs are the source of truth, `budget_usage` is a cache.
- `alert`: when spend crosses the amount, one audit row `budget.alert` "Budget '<scope label> <period>' reached $X of $Y" per budget per period; the call is allowed.
- API: `GET /api/budgets` (scoped like limits; each with `spent_micros` for the current period and `period_start`), `PUT /api/budgets` (upsert, admin), `DELETE /api/budgets/{id}` (admin).

**Tests:** block at each scope; alert once per period; period rollover (paused clock); restart rebuild (Review Focus 4: spend logged, new AppState from the same store → still blocks); flush timing; 429 `budget_exceeded` body naming the budget in both shapes; API; role table; OpenAPI.

- [ ] Tests RED → implement → gates → commit(s).

---

### Task 6: Response cache

**Files:** Create `src/cache/mod.rs`, `src/cache/key.rs`, `migrations/0009_route_cache.sql`; Modify `src/proxy.rs`, `src/snapshot.rs`, `src/api/routes.rs` (+ RouteView/Request fields), `src/telemetry.rs` (`AttemptOutcome::Cached` or record flag). Tests: `tests/cache.rs`, unit tests.

**Interfaces:**
- `0009`: `ALTER TABLE routes ADD COLUMN cache_enabled INTEGER NOT NULL DEFAULT 0`, `cache_ttl_s INTEGER NOT NULL DEFAULT 300`, `cache_scope TEXT NOT NULL DEFAULT 'team' CHECK IN ('team','key','user')`.
- `trait ResponseCache: Send + Sync { fn get(&self, key: &CacheKey, now) -> Option<Cached>; fn put(&self, key: CacheKey, value: Cached, ttl, now); }`; in-memory LRU bounded by 10 000 entries and 64 MiB (constants; configurable later).
- `CacheKey` = SHA-256 over a canonical, length-prefixed, field-tagged encoding of: endpoint, route name, model list of the plan, every request field that affects output (messages with role, name and content; system; max_tokens; temperature; top_p; stop; and any field the request types carry), plus the scope tag and scope id. A key with no team under scope `team` uses scope `user`, then `key`.
- Only non-stream calls to a route with `cache_enabled`, temperature absent or ≤ 0.5, and a 200 answer are stored. A hit answers without contacting any provider, still passes access, limits and budgets, and is logged with `cached = 1`, zero cost.
- Route API and console fields: "Cache answers" switch, TTL seconds (1–86 400), scope.

**Tests:** hit and miss; Review Focus 1 (two teams, identical request → two provider calls); scope key/user; streams and temperature 0.6 not cached; TTL expiry (paused clock); LRU eviction by count and by bytes; key distinguishes every field (property-style table: change one field → different key); route API validation.

- [ ] Tests RED → implement → gates → commit.

---

### Task 7: Prometheus metrics

**Files:** Create `src/metrics.rs`; Modify `src/app.rs`, `src/main.rs`, `src/proxy.rs`, `src/logs/mod.rs`. Tests: `tests/metrics.rs`.

**Interfaces:** `GET /metrics` in the Prometheus text format, guarded by `UF_METRICS_TOKEN` (`--metrics-token`, `Authorization: Bearer <token>`); unset → 404. Metrics: `uf_requests_total{endpoint,status_class}`, `uf_tokens_total{direction}`, `uf_cost_micros_total`, `uf_upstream_duration_seconds` histogram `{provider}`, `uf_log_records_dropped_total`, `uf_cache_hits_total`, `uf_cache_misses_total`, `uf_rate_limited_total{limit}`, `uf_budget_blocked_total`, `uf_circuit_open{provider,model}` gauge. No labels with user, key or prompt data. Hand-written exposition (no new dependency) unless a small, well-known crate is clearly better — state why.

**Tests:** token required; 404 when unset; format parses (line-by-line check); counters move with traffic; no key names or emails in the output.

- [ ] Tests RED → implement → gates → commit.

---

### Task 8: Console — Logs page and Overview usage

**Files:** Create `ui/src/pages/Logs.tsx`, `LogsDetail.tsx` (`/logs/$id`), `LogsFilters.tsx`, `ui/src/lib/usage.ts` (formatting money/tokens, grouping), `ui/src/components/Sparkline.tsx` (inline SVG, no chart library); Modify `Overview.tsx`, `AppSidebar.tsx` (Logs live), `router.tsx`, `queries.ts`, `guards.ts`, fixtures/handlers/schema. Tests: `logs.test.tsx`, `overview.test.tsx`, `lib/usage.test.ts`.

**Rules:**
1. Logs list: time, key, user (email), model (`provider/model`, or the requested name when none answered), status badge, tokens in/out, cost, duration, "Cached" badge; filters (time range presets: last hour / 24 h / 7 days / 30 days / custom; key; user; team; model; status: all / errors only); "Load older" by cursor; auto-refresh off by default with a "Refresh" button. Members see only their own; leads their teams'.
2. Detail: all fields, plus the "Routing attempts" panel (provider/model, outcome badge — "Answered", "Retried", "Failed", "Circuit open", "Skipped", "Cached" — status, duration) in order.
3. Overview: replaces the "Usage, spend and request logs arrive with a later release." note with tiles for the last 30 days: Requests, Errors (rate), Tokens (in/out), Spend — each with a 30-day sparkline (`group=day`); a "Top models" table (`group=model`, top 5) and for admins/leads "Top keys". An unpriced share shows "Some models have no price; spend is a lower bound." Money formatted `$1,234.56`; below $0.01 shows `<$0.01`.
4. Sparkline: inline SVG with `<title>` text, `role="img"`, accessible name with the min/max; respects the theme tokens (no colour literals).

**Tests:** list/filter/cursor/scope; detail attempts; overview tiles from fixture usage; unpriced note; formatting; one `main`/`h1`; 390 px; member/lead/admin variants.

- [ ] Tests RED → implement → gates → commit(s).

---

### Task 9: Console — Budgets and limits page, prices, settings

**Files:** Create `ui/src/pages/Limits.tsx` (`/limits`, "Budgets and limits"), `LimitsEdit.tsx`, `BudgetsEdit.tsx`, `ui/src/pages/Settings.tsx` (`/settings`); Modify `Models.tsx` (price columns + edit in the access/edit dialog), `Routes`/`RoutesEdit` (cache fields), `AppSidebar.tsx`, router, queries, guards, fixtures/handlers/schema. Tests accordingly.

**Rules:**
1. Budgets and limits page: two sections. Limits table (scope, requests/min, tokens/min, concurrent) with "Set limit" (admin; FormDialog: scope select gateway/team/user/key + target select, three optional numbers). Budgets table (scope, period, amount, spent this period with a progress bar, action badge "Blocks"/"Alerts") with "Set budget" (admin). Leads and members see what applies to them, read-only. Delete with confirm "Calls are no longer limited by this." / "Spend is no longer capped by this budget."
2. Models: "Input $/1M" and "Output $/1M" columns; admin edits prices in an "Edit price" dialog (dollars with up to 6 decimals ↔ micros).
3. Routing editor: "Cache answers" switch, TTL (s), scope (Team/Key/User) under Advanced, with the hint "Streams and requests with temperature above 0.5 are never cached."
4. Settings page (admin): "Keep request logs for N days" (1–3650). Sidebar: Settings in the footer for admins.

**Tests:** for each rule, incl. scope-target selection, money ↔ micros conversion, read-only views, field errors, one `main`/`h1`, 390 px.

- [ ] Tests RED → implement → gates → commit(s).

---

### Task 10: End-to-end flows and release check

**Files:** `ui/e2e/usage.spec.ts`, `ui/e2e/limits.spec.ts`, `ui/e2e/cache.spec.ts`; Modify `mock-provider.ts` (usage numbers in answers), `api.ts`.

| Spec | Flow |
|---|---|
| `usage.spec` | admin sets prices, makes calls; Logs page shows them with cost; detail shows attempts; Overview tiles show the requests and spend; a member sees only their own calls |
| `limits.spec` | a key limit of 2 requests/min → third call 429 with Retry-After and the named limit; a block budget reached → 429 `budget_exceeded`; the Budgets page shows spent and the bar |
| `cache.spec` | a cached route: same request twice from team A → one provider call, second log "Cached"; same request from team B → a provider call |

Then three E2E runs green; ui and Rust gates; release build; sizes.

- [ ] Specs → runs → fix console defects (one commit each) → three runs → final checks → commit.

## Known limits after this plan

- Prompt and response bodies are not stored in logs.
- Limits, budgets, cache and health are per process (single instance).
- Budget alerts are audit rows only; email/webhook delivery is phase 2.
- Team leads cannot set their own team's limits or budgets yet.

## Spec coverage

| Spec item | Task |
|---|---|
| Request logs (metadata), retention, background batch writer, drop counter | 1, 2 |
| Overview dashboard (usage, spend) | 3, 8 |
| Rate limits by requests, tokens, concurrency; strictest across scopes; 429 with Retry-After naming the limit | 4 |
| Budgets per gateway, team, user, key; block or alert; flush + rebuild from logs | 5 |
| Exact-match cache, scoped, never across teams, LRU bounded | 6 |
| Prometheus metrics with token | 7 |
| Console Logs, Budgets and limits, Settings (retention) | 8, 9 |
