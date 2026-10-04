# SDD ledger — plan: docs/superpowers/plans/2026-10-04-playground-settings.md

Base: e6388fa. Owner run mode: ONE implementer, all 7 tasks in order, no per-task reviews; one final review + one fix wave.
Task 1 done: 59b6901 (playground tests 9; cargo test --all green; fmt/clippy green). Readings: Actor<'a>{access: Cow<SnapKey>, key_id: Option} instead of plan's Option<SnapKey>; Scope::begin/RequestRecord.key_id/budgets_of now Option<i64> (test edits mechanical); tokens may also call playground (any Authed); cache scope Key -> User for keyless calls; non-2xx of playground documented in schema PlaygroundErrorBody (openapi test relaxed for that path); role-table row 59 (404 unknown model, no /api code).
Task 2 done: a127789 (ui vitest 1804 passed; lint/typecheck green). Changed tests: client.test.ts own-props list (+retryAfter), shell.test.tsx (Playground now a link; 'coming' test uses Guardrails), guards.test.ts (+usePlayground row). Readings: playground always streams; sends only parameters typed; tokens may also call playground.
Task 3 done: ae8b3f1 (cargo test --all 975 passed, 5 ignored; config_io 14, cli_config 2). Readings: dry_run defaults true; 422 body is the report (also for dry run); settings/limits/budgets identity = scope+name(+period); action ManageSettings reused for admin gate; export is audited; created/updated cleared in a report with errors; import body limit 8 MiB (raw body, not the 64 KiB /api limit); existing provider kind change = error.
Task 4 done: 59d1902 (cargo test --all 983 passed, 5 ignored; backup 4, api_settings 6). Changed tests: api_settings (GET/PATCH shape), api_roles (world on disk; rows 59-62; send() tolerates non-JSON), config_io/cli_config (counts incl. session_hours), openapi.rs counts. Readings: in-memory store cannot be backed up (VACUUM INTO writes nothing) -> role table on disk; backup temp unlinked on unix right after open; PATCH needs at least one field.
Task 5 done: e922637 (ui vitest 1852 passed; lint/typecheck green). Changed tests: settings.test (button 'Save retention', form 'Retention', typed responses), audit tests moved to settings-audit.test.tsx (route /settings#audit, h2, part errors), shell.test (no Audit item), client.test (+retryAfter earlier), queries.test (hook count 55, +useImportConfig). Readings: backup/export are plain download links (streamed by the browser), import via client importConfig (422 body = report); audit is a view '#audit' of Settings (not tabs widget).
Task 6 done: ac301b7 (cargo test --all 990 passed; release_workflow 6). Readings: workspace version NOT bumped (release cut decision; workflow checks tag == Cargo.toml version); docker image amd64 only; macos runners macos-15-intel/macos-15 (unrun); playground added to metrics ENDPOINTS.
Task 7 done: 6c1f9e1 (E2E 80 passed, 4 skipped, x3 green; ui vitest 1855; release binary 23.20 MB, console assets 0.88 MB). Found+fixed: Stop/Send button reuse (real browser resend), '1 minutes'.
Implementer report: DONE_WITH_CONCERNS (8 commits e6388fa..4e2a0f4; cargo 991 green, ui 1855, E2E 80/4 skipped x3). Concerns: release workflow unrun; no mid-apply failure test for import; concurrent imports may 500 (deferred tx); version not bumped. Dispatched ONE uf-final-reviewer on review-e6388fa..4e2a0f4.diff.
Final review: ready after fixes — 0 Critical, 2 Important, 10 Minor (final-review.md). Readings accepted except tokens-on-playground (I2) and plain download links (M1).
Ruling: fix wave takes I1, I2, M1-M10 (M5 = document that the image is public before the draft release is published) — all small, one wave — cost if wrong: a few extra commits.
Ruling: playground is session-only (plan says session + CSRF) — a token would bypass key expiry/revocation/allowlist — cost if wrong: owner asks to reopen it later.
LATER: BEGIN IMMEDIATE across the store; release smoke step for the embedded console; first manual workflow run by owner before the first tag; import prune; crates.io; arm64 image.
Dispatched fix wave (uf-implementer).
Fix wave commit 2ecd96a: import BEGIN IMMEDIATE, file built once, M10 base_url warning, mid-apply atomicity test (I1, M8a, M10; cargo 999 green at wave end).
Fix wave commit c984900: playground refuses access tokens (403 forbidden), openapi/schema regenerated, role table row 59 token column, budget-through-log-writer tests (I2, M8b).
Fix wave commit f41bd7c: backup file created 0600 by the store (M7).
Fix wave commit a671b82: version check first job, publish only on tag push, README image/restore notes (M3, M4, M5, M6).
Fix wave commit 5ebfff2: DownloadLink session check, playground prices called model, audit gated by viewAudit (M1, M2, M9; vitest 1861; E2E 80 passed, 4 skipped).
Re-review: APPROVED (re-review.md), no remaining fixes. Plan 7 closed at 5ebfff2.

## Final review (summary)
# Final review: plan 7 (playground, settings, config, backup, release), e6388fa..4e2a0f4

Reviewer: final whole-plan reviewer (read-only on the checkout; probes in a scratch
`git archive 4e2a0f4` copy, CARGO_INCREMENTAL=0, target deleted afterwards).

## Verdict: READY AFTER FIXES

The security core is sound. The playground is not a side door for a session
user: it runs the shared `run`/`dispatch`, gets access from `access.rs`, and
charges limits and budgets through `subjects_of`/`budgets_of`. Its cache scope is
never shared between keyless callers. The export holds no secret. The import is
fully validated before it writes and is atomic. Backup and export/import are
admin-only. The temp file is in the 0700 data dir, has a random name and is
unlinked. `session_hours` drives both `expires_at` and the cookie `Max-Age`. The
release jobs publish only on `refs/tags/v*` and build the console first.

Two Important defects need the fix wave: the import fails with a 500 when another
connection writes while it plans, and access tokens can call the playground.

Passes, in this order:
1. Playground and the proxy `Actor` refactor (proxy.rs, snapshot.rs, telemetry,
   logs, access.rs, api/playground.rs, the Authed extractor).
2. Config export and import (portable.rs, store/portable.rs, api/config.rs, the
   validators it borrows from api/routes, providers, teams, limits, budgets).
3. Backup, settings, sessions, CLI (api/backup.rs, store/backup.rs,
   store/sessions.rs, store/settings.rs, api/settings.rs, main.rs).
4. Release workflow, README, CHANGELOG, Dockerfile.
5. Console (guards, client, queries, router, sidebar, Playground*, Settings*,
   lib/playground.ts).
6. Tests and probes (playground.rs, config_io.rs, backup.rs; scratch probes below).

## Probes (scratch copy of 4e2a0f4)

- **Import while another connection writes.** A writer committed one audit row
  per transaction, and `portable::import` ran 100 times, each creating one team.
  - Tight write loop: 98/100 imports failed.
  - One write every 100 ms: 8/100 failed.
  - One write per second (the log writer's `max_wait` cadence): 1/100 failed.
  - Every failure was `(code: 517) database is locked`, which is
    SQLITE_BUSY_SNAPSHOT. The busy timeout does not retry this error.
- **Two imports at once.** 30/60 failed (one of each pair), with the same 517.
- **Failure partway through an apply.** A `BEFORE INSERT ON routes` trigger
  raised ABORT, and the file created a provider, a team, a model, a route and a
  retention change. The import returned an error. Afterwards providers, teams,
  models and audit_log each had 0 rows and retention was still 30. The import
  is atomic.
- **Full suite.** `cargo test -p ultrafast-gateway` in scratch: see "Test health"
  below.

## Findings

Counts: Critical 0, Important 2, Minor 10.

### Critical
None.

### Important

**I1. Config import answers 500 whenever anything else commits during its read phase.**
- Where: `crates/gateway/src/portable.rs:1584-1592` (`import`: `store.begin()`,
  then `config_state()`, `plan()`, then the first write).
- Cause: the transaction is DEFERRED. It reads the whole configuration and
  plans, then upgrades to a write lock. If any other connection committed in
  between (the log writer, a session or token touch, another admin), SQLite
  returns SQLITE_BUSY_SNAPSHOT (517) at once, and the busy timeout does not
  retry it.
- What a person meets: on a gateway under traffic, an admin who clicks Apply,
  or runs `ultrafast config import` against a running gateway, sometimes gets
  "Something went wrong". Nothing is written (atomicity holds); retrying usually
  works. Two concurrent imports: one always fails.
- Fix: run the import (dry run included, so the report matches what the apply
  sees) in `BEGIN IMMEDIATE`. Add `Store::begin_immediate()` using
  `pool.begin_with("BEGIN IMMEDIATE")`; the busy timeout then serialises
  writers. Add a regression test with a concurrent writer, as in the probe.
- Also compute `file_of(state)` once rather than once per existing route
  (`portable.rs:940`). Under IMMEDIATE, the planning time holds the write lock
  and blocks the log writer.

**I2. Access tokens can call the playground: a second programmatic inference credential.**
- Where: `crates/gateway/src/api/playground.rs:97-104` accepts any `Authed`, and
  the OpenAPI `security` lists `("token" = [])`. Ledger, Task 1: "tokens may
  also call playground (any Authed)".
- The plan's Task 1 says "session auth + CSRF". An access token carries its
  owner's full role and is meant for the admin SDK. With this, a token becomes a
  long-lived `/v1` credential that skips everything that applies only to virtual
  keys: expiry, revocation, allowlist, and key-scoped limits and budgets. It is
  also logged as `playground` rather than as an API call.
- What a person meets: an admin who limits a member to one budgeted, expiring
  key finds the member's scripts still calling models through
  `/api/playground/chat` with an access token. A leaked admin SDK token can now
  spend on every model.
- Fix: in `chat`, refuse `AuthVia::Token` (403 `forbidden`, or 401). Drop
  `("token" = [])` from the path's security, regenerate `openapi/admin.json`, and
  add a test: a token call is refused and nothing is logged.
- Ruling on the reading "tokens may also call playground": REQUIRE FIX.

### Minor

**M1. Backup and export are plain `<a download>` links.**
- Where: `ui/src/pages/SettingsBackup.tsx:25-28` and
  `ui/src/pages/SettingsConfig.tsx:181-185`.
- What a person meets: after the session ends, or on a 500 (disk full during
  `VACUUM INTO`), the page shows nothing. Depending on the browser the download
  fails in the download bar, or the error JSON is saved where the admin expects
  a backup.
- Fix: before following the link, check the session through the client (a
  `GET /api/me` that goes through `request`, so a 401 runs session-over
  handling). Or fetch `HEAD`/`GET` with `credentials` and only then navigate.
  Streaming by the browser stays.
- Ruling on the concern: FIX (cheap). Not Important, because Content-Length is
  set and Chrome fails a non-2xx download visibly.

**M2. The playground prices the last call by the model picked now, not the one called.**
- Where: `ui/src/pages/Playground.tsx:113-116`. `pricesOf(target, …)` uses the
  current `target`.
- What a person meets: after a call to model A, choosing model B in the picker
  changes the shown cost of A's call to B's price.
- Fix: store the called target in `Finished`, from `call.model` in
  `PlaygroundRun.ts`, and price by that. Add a unit test.

**M3. The release version check runs after the five 45-minute builds.**
- Where: `.github/workflows/release.yml:153-161`, inside `release`.
- What a person meets: a mistyped tag burns about an hour of runners before it
  fails.
- Fix: move "The tag names the version" into a tiny first job that `console`
  `needs` (tag pushes only).

**M4. A manual dispatch on a tag ref publishes.**
- Where: `.github/workflows/release.yml:139,176`. `if:
  startsWith(github.ref,'refs/tags/v')` is also true for a `workflow_dispatch`
  run on a tag.
- The header and README say "By hand … nothing is published".
- Fix: `if: github.event_name == 'push' && startsWith(github.ref, 'refs/tags/v')`
  on `release` and `docker-publish`.

**M5. The image goes public while the GitHub release is still a draft.**
- Where: `.github/workflows/release.yml:173-205`.
- What a person meets: `ghcr.io/...:2.0.0-alpha.2` (and `latest` for a stable
  tag) is pullable before a person has reviewed and published the draft.
- Fix: either say so in the README, or move `docker-publish` to
  `on: release: types: [published]`.

**M6. The README restore procedure can corrupt the "kept, in case" copy.**
- Where: `README.md`, Backup and restore, step 2.
- What a person meets: `mv gateway.db gateway.db.before` followed by
  `rm -f gateway.db-wal gateway.db-shm` deletes the WAL that belongs to the moved
  file. After an unclean stop, `.before` silently loses its last transactions.
- Fix: move `-wal` and `-shm` alongside (`mv gateway.db-wal
  gateway.db.before-wal`, and the same for `-shm`), or say "after a clean stop".

**M7. The backup file is world-readable while it is being written, on the CLI path.**
- Where: `crates/gateway/src/main.rs` `backup_command`. `VACUUM INTO` creates the
  file with the process umask, and `restrict_file` runs only afterwards.
- The API path is safe (it writes inside the 0700 data dir). The CLI writes to
  any path the user names, so a large database is readable by other local users
  for seconds.
- Fix: pre-create the file with `OpenOptions::new().create_new(true).mode(0o600)`
  then `VACUUM INTO` it (SQLite accepts an empty existing file). Or set the
  umask to 077 for the command.

**M8. Two test gaps.**
- No end-to-end test shows that a playground call's cost is charged to a budget.
  `tests/playground.rs:343-357` calls `budgets_of`/`spend` by hand. Add one test
  through the log writer (`org_with_sink` or the real writer) that a priced
  playground call moves the user's and the team's spend.
- Add the mid-apply failure test from the probe (trigger plus import, all counts
  0) to `tests/config_io.rs`, so atomicity is pinned.

**M9. The audit view is gated by `manageSettings` only.**
- Where: `ui/src/pages/Settings.tsx` (`AuditSection` is reached through
  `can(manageSettings)`), whereas the gateway decides the audit log by
  `viewAudit`.
- The two are equal today (admin only). The convention, though, is that each
  control is decided by its own action.
- Fix: render the "Audit log" nav link and the section only when
  `can(me, {type:"viewAudit"})`.

**M10. An import can point a provider's existing credential at a new host without saying so.**
- Where: `crates/gateway/src/portable.rs` `UpdateProvider`, for a `base_url`
  change.
- The API allows the same change, so this is not an escalation. The dry-run
  report shows only "base_url".
- Fix: when the provider has a credential and its `base_url` changes, add a
  warning: "the stored credential will be sent to the new base URL".

## Rulings on the implementer's readings (ledger)

Task 1
- `Actor { access: Cow<SnapKey>, key_id: Option }` instead of `Option<SnapKey>`: ACCEPT.
- `Option<i64>` key ids through Scope, RequestRecord and budgets_of: ACCEPT.
  Mechanical; `logs.rs` uses a LEFT JOIN, so keyless rows list.
- Tokens may call the playground: REQUIRE FIX (I2).
- Cache scope Key becomes User for keyless calls: ACCEPT. Correct: id 0 would
  otherwise be shared by every keyless caller.
- `PlaygroundErrorBody` with the openapi test relaxed for that path: ACCEPT.
- Role-table row 59 (404 for an unknown model): ACCEPT.

Task 2
- The playground always streams: ACCEPT.
- Only typed parameters are sent: ACCEPT.

Task 3
- `dry_run` defaults to true: ACCEPT (the safe default).
- The 422 body is the report, including for a dry run: ACCEPT.
- Identity of limits and budgets is scope + name (+ period): ACCEPT.
- `ManageSettings` gates export and import: ACCEPT (admin only in policy and in
  `can`).
- The export is audited: ACCEPT.
- created/updated are cleared when the report has errors: ACCEPT.
- 8 MiB raw body: ACCEPT. Tested; the 64 KiB `DefaultBodyLimit` does not apply
  to a raw `Body`.
- A kind change on an existing provider is an error: ACCEPT.

Task 4
- The in-memory store cannot be backed up, so the role table is on disk: ACCEPT.
- The temp file is unlinked right after open on unix: ACCEPT. On Windows Rust
  opens with FILE_SHARE_DELETE, so the Drop removal works there too.
- PATCH needs at least one field: ACCEPT.

Task 5
- Plain download links: FIX (M1).
- Audit as the `#audit` view of Settings: ACCEPT, with M9.

Task 6
- Workspace version not bumped: ACCEPT. Bumping is part of cutting the release,
  the CHANGELOG says so, and the guard stops a mismatched tag. Move the guard
  first (M3).
- Docker image amd64 only: ACCEPT (LATER).
- macOS runner labels unrun: see "Declined to judge".
- `playground` added to metrics ENDPOINTS: ACCEPT.

## Rulings on the implementer's concerns

1. **Release workflow never run on GitHub.** LATER, with a plan, and before the
   first tag. The owner runs it once by hand (workflow_dispatch) on v2 or main
   and checks the five archives. M4 must be fixed first so that run cannot
   publish. Not a merge blocker; it is a release blocker.
2. **No test forces a mid-apply import failure.** The probe proves atomicity
   holds. Add the test (M8).
3. **Concurrent imports may answer 500.** Confirmed, and worse than stated: any
   concurrent writer triggers it, not just another import. FIX now (I1, BEGIN
   IMMEDIATE).
4. **Workspace version not bumped.** ACCEPT (see Task 6).
5. **Backup and export as plain download links.** FIX, Minor (M1).

## Parked items: triage

- "PARKED/LATER" in the plan: an import `--prune`. LATER, correct; a separate
  plan with its own dry-run deletion report.
- Key-scoped limits and budgets not exported. ACCEPT, by design: keys are not
  portable.
- Restore is CLI/manual only. ACCEPT, by spec. Fix the README step (M6).
- crates.io publishing. LATER, as planned.
- arm64 Docker image. LATER. Use a matrix on `ubuntu-24.04-arm` plus a manifest.
- Playground history not saved. ACCEPT, by plan.
- New, for LATER: the DEFERRED-transaction pattern is project-wide. Every
  `/api` handler that reads and then writes in `store.begin()` (route create
  with `check_ids`, and others) can return 517 under write load, with a smaller
  window. Plan: make the write-transaction helper `BEGIN IMMEDIATE` across the
  store, with one load test. I1 fixes only the import, where the window is
  largest.
- New, for LATER: a smoke step in the release build that the binary embeds the
  real console and not the build.rs placeholder (for example, grep the binary
  for the Vite asset hash). Today a missing `ui/dist` falls back silently,
  although download-artifact failing makes that unlikely.

## Test health

- Scratch run of `cargo test -p ultrafast-gateway` at 4e2a0f4: 795 passed,
  0 failed, 5 ignored. Uptime load was 2.7 to 6.7 during the run.
- The scratch copy and its target dir were deleted afterwards. `pgrep -a
  ultrafast` shows only the production process.
- The changed tests listed in the ledger are mechanical (`Option` key ids,
  settings shape, hook count, moved audit tests). I found no removed assertion
  that weakens a pinned behaviour.
- Gaps: M8 (budget spend end to end; the mid-apply atomicity test), a
  token-refused playground test (I2), and a concurrent-writer import test (I1).

## Declined to judge

- Whether the release matrix actually builds. This includes aarch64-musl on
  `ubuntu-24.04-arm` with `aws-lc-sys`, the `macos-15-intel` and `macos-15`
  labels, and Windows `7z` packaging. It cannot be judged without running it on
  GitHub (concern 1).
- E2E and console Vitest suites. Not rerun here (no `node_modules` in scratch);
  the ledger reports E2E 80/4 skipped three times and 1855 Vitest tests green.
  The one-main/one-h1 and 390 px checks were read in the specs, not executed.
- Browser-specific behaviour of `<a download>` on a non-2xx answer (M1 severity
  depends on it).

## Re-review
# Re-review: plan 7 fix wave 4e2a0f4..5ebfff2

## Verdict: APPROVED

Probe: scratch copy of 5ebfff2, CARGO_INCREMENTAL=0. `cargo test -p ultrafast-gateway`: 803 passed, 0 failed, 5 ignored (uptime load 4 to 0.6).
backup + config_io run 3 more times in a row: 5 and 19 passed each time. Reverting `begin_immediate` to `begin` in portable::import:
`an_import_survives_another_connection_writing_meanwhile` fails ("30 of 30 failed ... database is locked"). Scratch deleted.
UI suites were not rerun (no node_modules in scratch); the UI diff was read in full.

## Findings
- I1 ADDRESSED: store/mod.rs `begin_immediate` (`begin_with("BEGIN IMMEDIATE")`), used by portable::import (portable.rs:1588), dry run included. Planner builds `file_of` once. Tests: concurrent writer, 8 simultaneous imports, mid-apply abort leaves everything unchanged.
- I2 ADDRESSED: api/playground.rs refuses non-Session with 403 `forbidden` before the policy and the pipeline; OpenAPI security is session only, admin.json/schema.d.ts updated; test shows 403, no upstream call, no record, token still works on /api/auth/me; api_roles row 59 token column updated.
- M1 ADDRESSED: DownloadLink checks through the client, shows the error in place, ignores double click, then clicks a programmatic anchor.
- M2 ADDRESSED: `Finished.called` from call.model; priced by it; test.
- M3, M4 ADDRESSED: first `version` job (push only step), console needs it; release and docker-publish need push + v* tag; release_workflow test updated.
- M5, M6 ADDRESSED: README notes (read in the diff).
- M7 ADDRESSED: create_private 0600 create_new before VACUUM INTO; file removed if VACUUM fails; existing file still refused.
- M8 ADDRESSED: mid-apply atomicity test; user and team budget tests through the real writer.
- M9 ADDRESSED: audit link and view gated by viewAudit, `#audit` falls back to General.
- M10 ADDRESSED: warning when a provider with a credential changes base_url; test.

No regression found in the fix diff.

## Rulings on the concerns
(a) GET /api/settings as the session check: acceptable. It is admin-only like both downloads, goes through `request` so a 401 ends the session as elsewhere, and /api/auth/me is excluded from that handling. It cannot catch a 500 raised only by the download itself; that is stated and was ruled browser territory.
(b) umask in the M7 test: not a flakiness risk. It sets 022 (the usual default) and restores before the unwrap; only this test touches the umask; it can only narrow modes (it clears bits, never adds); no test asserts a mode that depends on the umask (config.rs mode tests are in the lib unit-test binary, a separate process, and chmod explicitly; the API backup temp file is chmodded explicitly). Seven consecutive green runs here. No fix required. Optional (Minor): note in the test that it must stay the only umask user in the binary.
(c) M9 test mocking only viewAudit: acceptable and the right way to prove the gate is by its own action; it fails with Settings.tsx stashed (implementer's RED).

## Out of scope
The release workflow is still unrun on GitHub (already LATER).
