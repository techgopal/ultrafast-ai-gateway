# Conventions

Binding rules for everyone who changes this repository, human or agent. Plans
and briefs add to these; they do not repeat them.

## Safety on the maintainer's machine

- A production instance runs as the systemd user service `ultrafast-gateway`
  on 127.0.0.1:3900, binary and data under `~/.local/share/ultrafast-gateway`,
  config under `~/.config/ultrafast-gateway`. Never stop, restart, call or read
  it; never use port 3900; never run `systemctl`; never copy over its binary.
  Only the controller deploys, and only with the owner's consent.
- A gateway you start for a test uses a port the OS gives out, a fresh
  temporary data directory, and an environment written out in full (no
  inherited `UF_*`). Stop it by PID. `pgrep -a ultrafast` shows only the
  production process when you are done.
- Scratch copies (`git archive <sha> | tar -x -C <dir>`) install their own
  `node_modules`; never link or share the checkout's.
- Remove anything a browser tool writes into the repository (`.playwright-mcp/`).
- Secrets (passwords, provider API keys, virtual keys, access tokens, invite
  links) never reach logs, reports, test output, caches, storage, URLs or toasts.

## Git

- Commit with `git -c user.name=techgopal -c user.email=44522021+techgopal@users.noreply.github.com commit`.
  Never change git config. Stage specific paths; never `git add -A`.
- The owner is the only author: no `Co-Authored-By` or `Claude-Session` lines,
  or any other trailer naming an assistant, in commit messages.
- Never push. Never commit `ui/node_modules`, `ui/dist`, test reports or `target/`.
- After committing, read `git log --oneline` yourself; never quote SHAs from memory.

## Tests and gates

- Tests first: write the test, run it, paste the actual RED output into your
  report, then implement.
- A commit is gated on a green run of what you changed:
  - Console: `pnpm --dir ui lint`, `pnpm --dir ui typecheck`, `pnpm --dir ui test`;
    `pnpm --dir ui build` before E2E or at the end.
  - Rust: `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`,
    `cargo test --all`.
  - E2E: `pnpm --dir ui build`, `cargo build --release -p ultrafast-gateway`
    (the binary embeds the console), `pnpm --dir ui test:e2e`.
- Before reporting done: `git status --ignored` shows no source file ignored, and a
  clean `git archive HEAD` extracted to a temp dir builds (`cargo check`), so what
  is committed is what was tested.
- Disk is limited: build with `CARGO_INCREMENTAL=0` in scratch copies and delete
  every scratch target dir when done. Put scratch copies, target dirs, venvs and
  node_modules under `~/.cache/`, not `/tmp` (a small tmpfs quota that a single
  target dir fills).
- `sqlx::migrate!` embeds migrations at compile time and does not rebuild when only a
  migration file changes: touch `crates/gateway/src/store/mod.rs` after adding one.
- If a full run fails even once, do not commit over it: record the output,
  find the cause. Other sessions load this machine; note `uptime` with runs.
  Load timeouts are not proof of correctness or of a defect.
- Existing tests keep passing unchanged unless the brief changes the behaviour
  they pin. List every changed test and every removed assertion.
- No test may depend on today's date or the machine's time zone; "future"
  fixture dates are 2999-01-01.

## Gateway (Rust)

- axum 0.8, sqlx 0.9 (SQLite in WAL mode, and optionally PostgreSQL), tokio. Rust 1.94 (`rust-toolchain.toml`).
- The database is the source of truth; `/v1` reads an in-memory `ArcSwap`
  snapshot and never waits on the database. Writes on the hot path go through a
  background writer.
- Every admin route is declared once with utoipa; `openapi/admin.json` is
  generated from it and every operation has a unique `operationId`.
- Access to models and routes is decided only by the shared predicates in
  `crates/gateway/src/access.rs` (`model_callable`, `route_usable`); /v1 and the
  admin API's lists use the same functions. Every admin write that can change
  access (grants, enabled, routes, membership, role, status, key allowlist,
  provider delete) calls `refresh_snapshot` after commit.
- Anything that talks to a third party (trace export, alert delivery) runs on a
  background task fed by a bounded queue: the request path only does a
  non-blocking send, a full queue drops and counts (a metric, and a record
  where one exists), memory per destination is bounded, and shutdown waits for
  the task for a fixed cap (5 s) before abandoning what is left. Nothing on
  `/v1` ever waits for it.
- Stateful features (rate limits, budgets, cache) sit behind traits so a shared
  store can replace the in-memory one.
- A write transaction that reads before it writes starts with `BEGIN IMMEDIATE`
  (as the configuration import does); a deferred one fails at once with SQLite
  code 517 when another writer commits in between.
  On PostgreSQL `begin_immediate` takes an advisory lock that is per database, not per
  schema: it serializes these transactions across every gateway process (and every test
  schema) sharing that database. Code that reads, decides and writes (a last-admin
  check, a first-user check) must use it; a plain transaction is READ COMMITTED there.
- The store speaks two databases (SQLite by default, PostgreSQL with
  `UF_DATABASE_URL`):
  - No SQLite-only (or Postgres-only) SQL outside `Dialect` (`store/mod.rs`):
    `last_insert_rowid`, `INSERT OR IGNORE`, `rowid`, `?` binds the database must
    infer (cast or type them), integer booleans, `BLOB` columns are all
    spelled through the dialect helpers.
  - Every migration exists in both directories: `migrations/sqlite/` (next
    number, today 0017) and `migrations/postgres/` (its own next number, today
    0003: the Postgres baseline `0001_baseline.sql` folds SQLite 0001-0015 into
    one, so the numbers differ). Pinned files never change. A schema change in
    only one fails the baseline parity test in `store/mod.rs`, which runs only
    with `UF_TEST_DATABASE_URL` set.
  - Run the whole suite on Postgres before a store change is done: build with
    `--features test-support` and set `UF_TEST_DATABASE_URL` to a throwaway
    server; each test gets a schema of its own. The Rust suite and, once for a
    release, the browser tests (`UF_E2E_DATABASE_URL`) pass on both.
  - On Postgres a failed statement aborts the whole transaction (every later
    statement fails until rollback): do not catch an error inside a transaction
    and carry on; check first (`ON CONFLICT DO NOTHING`, a `SELECT`) or roll
    back.
  - State shared by several processes belongs in the database; anything kept
    in memory (rate limits, cache, breaker health, alert error windows) is per
    process and documented as such in the README's Known limits.
- Sign-in methods other than the password sit behind the `SignInProvider` trait
  (`identity/external.rs`): `begin(return_to)` returns the redirect and a flow
  cookie value, `complete(&CallbackParams, flow_cookie)` returns
  `Completed { identity, return_to }`. Both return boxed futures (`BoxFuture`)
  so the trait stays object safe for `Arc<dyn SignInProvider>`;
  `CallbackParams` is transport neutral (name/value pairs, from a query or a
  form post; a name sent twice reads as absent). A provider only proves
  who the person is: mapping to a user, roles, the session and audit are done
  once in `api/sso.rs`, and the caller re-checks `return_to`. That mapping
  reads `OidcSettings` and the callback is a GET today, so a SAML provider would
  still need a provider-neutral mapping policy, an issuer or realm on the
  identity, its own cookie attributes and a POST callback route. Nothing a
  provider sent (claims, codes, tokens, error text) is logged or reflected;
  failures are fixed codes.
- Errors to clients use the gateway's error shape; never leak provider keys or
  internal paths.

## Console (`ui/`)

- React, TypeScript strict, Vite, Tailwind, shadcn/ui components committed in
  `src/components/ui/` (add with `pnpm dlx shadcn@4.21.0 add -y <c>`, then make
  `cn` come from `@/lib/utils`; the `shadcn` and `cn` packages are never deps).
- No external URL at runtime; CSP is strict with a per-response nonce. No inline
  styles, no colour literals.
- API types come from `src/api/schema.d.ts` (generated). All requests go through
  `src/api/client.ts`. Fixtures in `src/test/fixtures.ts` and error bodies in
  `src/test/errors.ts` are the gateway's real forms, pinned by tests.
- Hooks live in `src/api/queries.ts` (`useApiMutation`, `queryKeys`, detail keys
  via `detailOf`); a test pins the hook count.
- What a viewer may do is decided only by `can(me, action)` in
  `src/auth/guards.ts`, mirroring the gateway's policy; its table test lists every
  action and role. Never offer a control the gateway would refuse.
- Pages are flat files under `src/pages/` (`Keys.tsx`, `KeysCreate.tsx`, …); a
  page imports only from its own area. Shared pieces live in `components/`, pure
  logic in `lib/` (with unit tests); `lib/` imports nothing from `components/`
  or `pages/`. Shared class names in `components/classes.ts` (`control`).
- Exactly one `main` and one `h1` per screen in every state
  (`expectOneMain()`, `expectOneH1()`).
- Every form dialog is a `FormDialog`, submitted through the shared handler in
  `components/form.ts`: it stays open from the moment of the submit until the
  request settles, refuses a second submit, and can be closed after a failure.
  Only `FormDialog`, `ConfirmDialog` and `SecretDialog` use dialog primitives.
- A secret is shown once with `SecretDialog`/`useSecretOnce`; afterwards it is
  nowhere (`expectNoSecret`). A typed secret stays in its field after a refusal
  and is cleared on success and on every close.
- Errors: what the gateway answered is an `ApiError`; what the console refuses
  itself is a `ConsoleRefusal`; messages go through `messageOfError`. A gateway
  field error shows on its field (`applyApiError`, `onField`).
- A 404 on a detail key drops its data; a deleted/left object is marked `gone` in
  the hook and the page only navigates (no flash of skeleton or not-found).
- Mutations and queries use `networkMode: "always"`.
- Toasts identify nothing (no names, ids, secrets).
- A choice no longer offered is never sent: derive the effective choice from
  what is offered.
- Touch targets are 44 x 44 px below 768 px; every page fits 390 px with no
  horizontal scroll.
- Tests: Vitest + Testing Library + MSW; `afterEach(forgetToasts)` where toasts
  can appear; a "list shows it afterwards" test must fail without the
  invalidation; one session-over test per form. Fill forms with `paste` where
  typing is not what is tested; one test per form types for real.
- E2E (`ui/e2e/`): an expected API refusal is allowed per test with
  `rules.expectRefusal(status, pathRegex)`, which fails if it never happens.
- E2E (`ui/e2e/`): every spec fails on a console error, a CSP violation or a
  request to another origin; the launcher in `e2e/gateway.ts` is the only way to
  start a gateway.
