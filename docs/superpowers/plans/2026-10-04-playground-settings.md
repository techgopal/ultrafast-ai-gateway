# Playground, Settings and Release Implementation Plan (gateway plan 5 + console plan 4)

> **For agentic workers:** Owner's run mode for this plan: ONE implementer does all tasks in order (no parallel worktrees, no per-task reviews); then one final whole-plan review and one fix wave. Use the project skill `uf-workflow` and `docs/CONVENTIONS.md`.

**Goal:** The last phase-1 features: a Playground in the console, Settings for sign-in and backup, configuration export/import, the audit log moved under Settings, and a working release workflow — backend and console for each.

**Spec:** `docs/superpowers/specs/2026-09-28-gateway-v2-design.md` sections 3 (phase 1: playground, config export/import), 6 (Configuration: export file contents), 8 (members use the playground; sign-in limits), 11 (backup), 13 (Settings: retention, sign-in, backup, audit log). Conventions: `docs/CONVENTIONS.md`. Ledgers of plans 5-6 in `docs/superpowers/plans/`.

## Global Constraints

- Playground calls go through the SAME `/v1` pipeline as API calls (access, limits, budgets, cache, routing, logging), on behalf of the signed-in user — never a side door. A playground call is logged like any call, attributed to the user, with `key_id` NULL and endpoint marked as playground (`requested` as usual).
- Export file (spec 6): providers (name, kind, base_url, api_version — NO credential), models (enabled, prices, grants by team NAME / user EMAIL), routes (all settings, targets by `provider/model`, teams by name), teams (name), limits and budgets (scope by name/email; key-scoped ones excluded), settings. Never: credentials, keys, tokens, passwords, sessions, users' password hashes, logs, audit. Format: one JSON document `{ "format": "ultrafast-config", "version": 1, ... }`.
- Import is admin-only, validated in full before any write (dry-run report first), applied in one transaction, audited, then snapshot refresh. Policy: create missing, update existing by name, never delete (a `--prune` is LATER). Providers imported without a credential stay credential-less until an admin sets one; the report says so.
- Backup: a consistent SQLite online backup (`VACUUM INTO` a temp file) streamed to an admin as a download, and a CLI `ultrafast backup <path>`; the master key is NOT in the backup and the UI/README say the backup is useless without it. Restore is a documented CLI procedure (stop, replace file), not an API.
- Sign-in settings: trusted proxies (existing CLI flag) shown read-only; session lifetime (hours, 1–720, default as today) editable; sign-in limiter numbers shown read-only. Stored in `settings`.
- Production safety and all console/gateway conventions apply.

## Review Focus

1. A member's playground call is refused exactly as their key would be for a model they may not use, and is charged to their budgets and limits.
2. An exported file contains no secret (scan the JSON for every credential, key hash, token, password hash in the test DB).
3. Import of a file referencing an unknown team/user/model fails validation with a precise report and writes nothing.
4. A backup taken during heavy writes restores to a consistent database that passes migrations and serves.
5. The release workflow builds the console before the binary and publishes nothing without a tag.

---

### Task 1: Playground API
`POST /api/playground/chat` (session auth + CSRF, any signed-in user; body: model, messages, max_tokens, temperature, top_p, stop, stream) → same answers as `/v1/chat/completions` (OpenAI shape, SSE when stream). Internally builds a synthetic principal for the user and runs the shared proxy `dispatch` (refactor so dispatch takes an `Actor { key: Option<SnapKey>, user_id, team_id }`); access as for a key owned by the user with no allowlist and no key team (owner's teams count). Logged with key_id NULL, endpoint `playground`. Tests: access matrix parity with a key, limits/budgets apply, logging, streaming, CSRF required, member/lead/admin, disabled user refused.

### Task 2: Console Playground page
`/playground` (sidebar Observe → Playground live): model/route picker (from `GET /api/models` + `GET /api/routes` callable for the viewer), system prompt, message thread, parameters (max tokens, temperature, top_p, stop), Send / Stop (abort stream), streaming render, token usage and cost of the call (from the answer usage + model price), "Copy as curl" (gateway URL, `Authorization: Bearer <your key>` placeholder), errors in place (403/429 with Retry-After text). No message history persisted (memory only). Tests incl. stream rendering, abort, error states, 390 px, one main/h1.

### Task 3: Configuration export and import (API + CLI)
`GET /api/config/export` (admin) → JSON per Global Constraints; `POST /api/config/import?dry_run=true|false` (admin) → `{ created: [...], updated: [...], unchanged: n, warnings: [...], errors: [...] }`; 422 when errors (nothing written). CLI `ultrafast config export <file>` / `ultrafast config import <file> [--dry-run]`. Tests: round-trip (export → fresh DB import → export equal), secret scan, unknown references, idempotent second import, audit rows, snapshot refreshed.

### Task 4: Backup and sign-in settings (API + CLI)
`GET /api/backup` (admin) → `application/vnd.sqlite3` download via `VACUUM INTO` a temp file in the data dir, streamed and deleted; filename `ultrafast-<UTC timestamp>.db`; audited. CLI `ultrafast backup <path>`. Settings: `session_hours` (1–720) applied to new sessions; GET /api/settings also returns read-only `trusted_proxies`, `login_limits`. Tests: backup during concurrent writes opens and migrates; non-admin 403; session_hours applies; README restore procedure.

### Task 5: Console Settings page
Settings page sections: Retention (exists), Sign-in (session hours editable; trusted proxies and limits read-only with explanation), Backup (Download backup button; note on the master key), Configuration (Export download; Import: file picker → dry-run report table → Apply with confirm), Audit log moved here as a section/tab (sidebar footer "Audit log" link removed or redirects to `/settings#audit`). Tests per section.

### Task 6: Release workflow and docs
Rewrite `.github/workflows/release.yml`: on tag `v*`: build console, build release binaries (linux x86_64/aarch64 musl or gnu, macOS x86_64/arm64, windows x64) with the console embedded, checksums, GitHub Release draft with artifacts; Docker image build and push to ghcr only on tag; no crates.io publish yet (LATER). `workflow_dispatch` builds artifacts without publishing. README: Playground, Settings (backup/restore, export/import), release/install instructions. CHANGELOG entry for 2.0.0-alpha.2.

### Task 7: E2E and release check
Specs: playground (stream, member refused on ungranted model, cost shown), config (export → import into a fresh gateway via the launcher → models/routes identical), backup (download is a valid SQLite file with the expected tables), settings (session hours). Three E2E runs green; all gates; release build sizes.

## Known limits after this plan
- Import never deletes (prune LATER); key-scoped limits/budgets are not exported.
- Backup excludes the master key by design; restore is a CLI/manual procedure.
- Playground history is not saved.
