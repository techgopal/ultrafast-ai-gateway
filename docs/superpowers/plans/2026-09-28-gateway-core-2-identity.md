# Gateway Core, Plan 2: Identity and Admin API Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Users, teams and roles with sign-in, access tokens, and an `/api` admin API that manages users, teams, keys and providers live, with `/v1` served from an in-memory snapshot.

**Architecture:** Identity data lives in SQLite behind the store. A pure policy module decides what each principal may do; every `/api` handler goes through it. After each write the gateway rebuilds an in-memory snapshot of keys and providers and swaps it atomically, so `/v1` requests never touch the database.

**Tech Stack:** Rust 1.88, axum 0.8, sqlx 0.8 (SQLite), argon2 0.5, arc-swap 1, utoipa 5, time 0.3. Existing: tokio, reqwest, chacha20poly1305, sha2, clap, wiremock.

**Spec:** `docs/superpowers/specs/2026-09-28-gateway-v2-design.md` (sections 6, 7 steps 1-2, 8, 11, 12, 15, 16)

## Where this plan sits

| Plan | Delivers | Status |
|---|---|---|
| 1. Foundation | Workspace, translate, storage, key auth, chat proxy, CLI | Done |
| **2. Identity and admin API (this plan)** | Users, teams, roles, sessions, access tokens, `/api`, snapshot, OpenAPI | |
| 3. Catalog and routing | Models, model grants, routes, fallback, retries, circuit breaker | |
| 4. Limits, budgets, cache | | |
| 5. Logs, audit views, metrics | | |
| 6. Formats and providers | | |

Deferred by this plan: model access and route access (plan 3), budgets and limits on keys (plan 4), request logs and the audit log *viewer* endpoints beyond a simple list (plan 5), OIDC (phase 2), email delivery of invites (the API returns the invite link; an admin passes it on).

## How to read the tasks

Plan 1 showed that implementation code written without compiling it needs correcting, while exact interfaces, rules and tests hold up. So each task here gives:

- **Interfaces:** exact names, types and signatures. Binding.
- **Rules:** numbered, exact behavior with every constant and message. Binding.
- **Tests:** either test code, or a case table where every row is one assertion the test file must make. Binding; add more where you see a gap.
- **Implementation notes:** guidance and code for the non-obvious parts. Adapt as needed to compile, keeping interfaces and rules.

Follow TDD in every task: write the tests, run them to see them fail for the expected reason, implement, run them to see them pass, then `cargo fmt --all`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --all`, and commit.

## Global Constraints

- Everything in plan 1's Global Constraints still holds.
- `rust-version` is `1.88`.
- No secret ever appears in a log line, an error message, a response body (except a newly created key, token or invite link, shown once), an audit entry, or `Debug` output. Secrets are: passwords, password hashes, session ids, CSRF tokens, access tokens, invite tokens, virtual keys, provider credentials, the master key.
- Every type that holds a secret has a redacting `Debug` impl or none.
- Every `/api` handler obtains its `Principal` through the one extractor and calls `policy::authorize` before doing any work. No handler checks roles by hand.
- Every state-changing `/api` call writes one audit entry in the same database transaction as the change.
- `/api` errors use one JSON shape: `{"error":{"code":"<stable_code>","message":"<text>","fields":{"<field>":"<text>"}}}`. `fields` is present only for validation errors.
- `/v1` requests read keys and providers from the snapshot only, never from the database.
- Every new table has `org_id INTEGER NOT NULL DEFAULT 1`. Every query that reads or writes a row filters by `org_id = 1` through one constant, `store::DEFAULT_ORG`.
- Emails are stored trimmed and lowercased; comparisons use the stored form.
- All timestamps stored as UTC text `YYYY-MM-DD HH:MM:SS`.
- Commit with `git -c user.name=techgopal -c user.email=techgopal2@gmail.com commit`; messages end with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.

## Fixed values

| Name | Value |
|---|---|
| Password length | 12 to 256 characters |
| Password hash | Argon2id, m=19456 KiB, t=2, p=1, PHC string |
| Session cookie | `uf_session`; `HttpOnly; SameSite=Strict; Path=/`; `Secure` unless insecure cookies are enabled |
| Session lifetime | 12 hours from sign-in, not extended by use |
| CSRF header | `x-csrf-token` |
| Access token prefix | `uf-at-` followed by 64 hex characters |
| Invite token prefix | `uf-inv-` followed by 64 hex characters; valid 7 days; single use |
| Login limit per email | 5 failures in 15 minutes |
| Login limit per address | 20 failures in 15 minutes |
| Insecure cookies | flag `--insecure-cookies`, env `UF_INSECURE_COOKIES=true`; default off |
| First admin | env `UF_ADMIN_EMAIL` and `UF_ADMIN_PASSWORD`, used only when no user exists |

## Roles

- `users.role` is global: `admin` or `member`.
- `team_members.role` is per team: `lead` or `member`.
- "Team lead" in the spec means a user who is `lead` of the team in question.
- `users.status`: `active`, `invited` (no password yet), `disabled`.

## Review Focus

1. A disabled user, or one whose role was lowered, still holds a live session or access token: the very next request must see the new state. Pinned in Task 6.
2. Sign-in with an unknown email must take the same path as a wrong password (same status, same message, a password hash still computed) so accounts cannot be enumerated. Pinned in Task 6.
3. The last active admin is demoted, disabled or deleted, including by themselves: refused with 409. Pinned in Task 7.
4. A team lead acts on a user, key or team outside their team by guessing an id: 404, the same as a missing id, never 403 that confirms it exists. Pinned in Tasks 7 and 8.
5. A key is revoked or expires, or a provider is changed, while the gateway runs: the next `/v1` request sees it without a restart. Pinned in Task 9.

## File Structure

```
crates/gateway/
  migrations/0002_identity.sql
  src/store/mod.rs            pool, open, DEFAULT_ORG, timestamp check   (was store.rs)
  src/store/providers.rs      provider rows
  src/store/keys.rs           virtual key rows
  src/store/users.rs          users, invites
  src/store/teams.rs          teams, membership
  src/store/sessions.rs       sessions, access tokens
  src/store/audit.rs          audit entries
  src/identity/mod.rs         Role, TeamRole, UserStatus, Principal
  src/identity/password.rs    hashing and policy
  src/identity/policy.rs      authorize()
  src/identity/limiter.rs     sign-in attempt limiter
  src/snapshot.rs             in-memory keys and providers
  src/api/mod.rs              /api router, ApiError, Authed extractor
  src/api/auth.rs             setup, login, logout, me, accept-invite
  src/api/users.rs
  src/api/teams.rs
  src/api/keys.rs
  src/api/providers.rs
  src/api/tokens.rs
  src/api/audit.rs
  src/api/openapi.rs          spec assembly
  tests/common/mod.rs         extended harness
  tests/api_auth.rs  api_users.rs  api_teams.rs  api_keys.rs
  tests/api_providers.rs  api_tokens.rs  api_roles.rs  snapshot.rs
openapi/admin.json            generated, committed
```

---

### Task 1: Store restructure and identity schema

**Files:**
- Create: `crates/gateway/migrations/0002_identity.sql`, `crates/gateway/src/store/mod.rs`, `store/providers.rs`, `store/keys.rs`
- Delete: `crates/gateway/src/store.rs` (its content moves)
- Modify: root `Cargo.toml` (add `time = { version = "0.3", features = ["parsing", "formatting", "macros"] }`), `crates/gateway/Cargo.toml`

**Interfaces:**
- Consumes: plan 1's `Store`, `ProviderRow`, `KeyRow`
- Produces:
  - `store::DEFAULT_ORG: i64 = 1`
  - `store::check_timestamp(value: &str) -> anyhow::Result<()>` (public; replaces `check_expires_at`)
  - `store::now() -> String` returning the current UTC time as `YYYY-MM-DD HH:MM:SS`
  - `store::after(seconds: i64) -> String` returning now plus the offset in the same form
  - `Store::pool(&self) -> &SqlitePool` (crate-visible, `pub(crate)`)
  - `KeyRow` gains `pub user_id: Option<i64>`, `pub team_id: Option<i64>`, `pub expires_at: Option<String>`, `pub revoked_at: Option<String>`, `pub created_at: String`
  - `Store::revoke_key(&self, id: i64) -> anyhow::Result<bool>` returns whether a live key was revoked
  - All existing `Store` methods keep their signatures otherwise

**Migration `0002_identity.sql`:**

```sql
CREATE TABLE users (
    id             INTEGER PRIMARY KEY,
    org_id         INTEGER NOT NULL DEFAULT 1,
    email          TEXT    NOT NULL,
    name           TEXT    NOT NULL,
    role           TEXT    NOT NULL CHECK (role IN ('admin', 'member')),
    status         TEXT    NOT NULL CHECK (status IN ('active', 'invited', 'disabled')),
    password_hash  TEXT,
    auth_provider  TEXT    NOT NULL DEFAULT 'password',
    external_id    TEXT,
    created_at     TEXT    NOT NULL DEFAULT (datetime('now')),
    last_active_at TEXT,
    UNIQUE (org_id, email)
);

CREATE TABLE invites (
    id          INTEGER PRIMARY KEY,
    org_id      INTEGER NOT NULL DEFAULT 1,
    user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash  TEXT    NOT NULL UNIQUE,
    expires_at  TEXT    NOT NULL,
    used_at     TEXT,
    created_at  TEXT    NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE teams (
    id          INTEGER PRIMARY KEY,
    org_id      INTEGER NOT NULL DEFAULT 1,
    name        TEXT    NOT NULL,
    created_at  TEXT    NOT NULL DEFAULT (datetime('now')),
    UNIQUE (org_id, name)
);

CREATE TABLE team_members (
    org_id   INTEGER NOT NULL DEFAULT 1,
    team_id  INTEGER NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    user_id  INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role     TEXT    NOT NULL CHECK (role IN ('lead', 'member')),
    PRIMARY KEY (team_id, user_id)
);

CREATE TABLE sessions (
    id          INTEGER PRIMARY KEY,
    org_id      INTEGER NOT NULL DEFAULT 1,
    user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    id_hash     TEXT    NOT NULL UNIQUE,
    csrf_token  TEXT    NOT NULL,
    expires_at  TEXT    NOT NULL,
    created_at  TEXT    NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE access_tokens (
    id           INTEGER PRIMARY KEY,
    org_id       INTEGER NOT NULL DEFAULT 1,
    user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name         TEXT    NOT NULL,
    token_hash   TEXT    NOT NULL UNIQUE,
    display      TEXT    NOT NULL,
    expires_at   TEXT,
    revoked_at   TEXT,
    last_used_at TEXT,
    created_at   TEXT    NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE audit_log (
    id             INTEGER PRIMARY KEY,
    org_id         INTEGER NOT NULL DEFAULT 1,
    at             TEXT    NOT NULL DEFAULT (datetime('now')),
    actor_user_id  INTEGER,
    actor_email    TEXT    NOT NULL,
    action         TEXT    NOT NULL,
    target_type    TEXT    NOT NULL,
    target_id      INTEGER,
    summary        TEXT    NOT NULL
);

ALTER TABLE virtual_keys ADD COLUMN user_id INTEGER REFERENCES users(id) ON DELETE SET NULL;
ALTER TABLE virtual_keys ADD COLUMN team_id INTEGER REFERENCES teams(id) ON DELETE SET NULL;

CREATE INDEX idx_keys_user ON virtual_keys(user_id);
CREATE INDEX idx_keys_team ON virtual_keys(team_id);
CREATE INDEX idx_sessions_user ON sessions(user_id);
CREATE INDEX idx_tokens_user ON access_tokens(user_id);
CREATE INDEX idx_audit_at ON audit_log(at);
```

`actor_email` is copied into the audit row so the entry survives deletion of the user.

**Rules:**
1. `store.rs` becomes `store/mod.rs` plus `providers.rs` and `keys.rs`; every plan 1 test in the old file moves with the code it tests and passes unchanged, except where a rule below changes behavior.
2. `check_timestamp` accepts only real calendar dates: parse with `time::PrimitiveDateTime` and the format `[year]-[month]-[day] [hour]:[minute]:[second]`, and also require the input to be exactly 19 bytes. `2999-02-31 00:00:00` is rejected.
3. `open_in_memory` builds its pool with `.min_connections(1).idle_timeout(None).max_lifetime(None)` so the single connection, and the database with it, is never reaped.
4. Every query on `providers` and `virtual_keys` adds `AND org_id = ?` bound to `DEFAULT_ORG`; inserts set `org_id` explicitly.
5. `revoke_key` updates only `WHERE id = ? AND org_id = ? AND revoked_at IS NULL` and returns `rows_affected() == 1`. A second revoke does not change `revoked_at`.
6. Migrations run on an existing plan 1 database without data loss: existing keys get `user_id` and `team_id` NULL.

**Tests** (in `store/mod.rs`, `store/keys.rs`, `store/providers.rs`):

| Test | Assertion |
|---|---|
| `timestamp_rejects_impossible_dates` | `2999-02-31 00:00:00`, `2023-02-29 00:00:00`, `2999-04-31 00:00:00` are errors |
| `timestamp_accepts_boundaries` | `2999-12-31 23:59:59`, `2024-02-29 00:00:00`, `0001-01-01 00:00:00` are ok |
| all 16 malformed values from plan 1's test | still errors |
| `now_and_after_are_well_formed` | `check_timestamp(&now())` ok; `after(3600) > now()` as strings; `after(-1) < now()` |
| `revoke_reports_whether_it_revoked` | first call `true`; second `false`; unknown id `false`; `revoked_at` unchanged by the second call |
| `in_memory_database_survives_idle` | open, insert a key, acquire and release the connection 50 times with `tokio::task::yield_now()` between, key still found |
| `plan1_database_migrates` | create a temp file db, apply only migration 1 by running its SQL text through `sqlx::query`, insert a provider and a key, close, then `Store::open` the same path: both rows still present, key has `user_id == None` |
| `key_row_carries_new_fields` | insert with `expires_at`, read back via `active_key_by_hash`: fields populated |

For `plan1_database_migrates`, include migration 1's text with `include_str!("../../migrations/0001_init.sql")`. Because sqlx records applied migrations in `_sqlx_migrations`, a database built by raw SQL has no record of migration 1 and `Store::open` would try to re-run it. Build the plan 1 database through sqlx's migrator restricted to version 1 instead: `let mut m = sqlx::migrate!("./migrations"); m.migrations.to_mut().retain(|x| x.version == 1); m.run(&pool).await`.

- [ ] Step 1: Write the tests above. Run `cargo test -p ultrafast-gateway store`. Expected: compile errors (`check_timestamp`, `now` not found).
- [ ] Step 2: Move the code into `store/`, add the migration, implement the rules.
- [ ] Step 3: Run `cargo test --all`. Expected: all pass, including every plan 1 test.
- [ ] Step 4: fmt, clippy, commit `refactor(gateway): split the store and add the identity schema`.

---

### Task 2: Identity types and passwords

**Files:**
- Create: `crates/gateway/src/identity/mod.rs`, `identity/password.rs`
- Modify: `crates/gateway/src/lib.rs` (add `pub mod identity;`), root and gateway `Cargo.toml` (add `argon2 = "0.5"`)

**Interfaces (produces):**

```rust
// identity/mod.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role { Admin, Member }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TeamRole { Lead, Member }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UserStatus { Active, Invited, Disabled }

impl Role       { pub fn as_str(self) -> &'static str; pub fn parse(s: &str) -> Option<Self>; }
impl TeamRole   { pub fn as_str(self) -> &'static str; pub fn parse(s: &str) -> Option<Self>; }
impl UserStatus { pub fn as_str(self) -> &'static str; pub fn parse(s: &str) -> Option<Self>; }

/// Who is making an /api request, as of this request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    pub user_id: i64,
    pub email: String,
    pub role: Role,
    /// Teams the user belongs to, with their role in each.
    pub teams: Vec<(i64, TeamRole)>,
}

impl Principal {
    pub fn is_admin(&self) -> bool;
    pub fn team_role(&self, team_id: i64) -> Option<TeamRole>;
    pub fn leads(&self, team_id: i64) -> bool;
    pub fn led_teams(&self) -> Vec<i64>;
}

pub fn normalize_email(raw: &str) -> Result<String, &'static str>;

// identity/password.rs
pub const MIN_PASSWORD_LEN: usize = 12;
pub const MAX_PASSWORD_LEN: usize = 256;
pub fn check_password_policy(password: &str) -> Result<(), &'static str>;
pub fn hash_password(password: &str) -> anyhow::Result<String>;
pub fn verify_password(password: &str, phc: &str) -> bool;
/// Spends the same effort as a real verification. Used when the account
/// does not exist, so timing does not reveal that.
pub fn verify_dummy(password: &str);
```

**Rules:**
1. `normalize_email` trims and lowercases. It returns `Err("email is not valid")` unless the result has exactly one `@`, a non-empty part on each side, at least one `.` in the domain part that is neither first nor last, no whitespace or control characters, and is at most 254 bytes.
2. `check_password_policy` counts `chars()`, not bytes. Errors: `"password must be at least 12 characters"`, `"password must be at most 256 characters"`.
3. `hash_password` uses Argon2id with `Params::new(19456, 2, 1, None)`, a fresh random salt from `OsRng`, and returns the PHC string. It does not check policy.
4. `verify_password` returns `false` for a wrong password and for a malformed hash; it never panics and never returns an error that could carry the hash.
5. `verify_dummy` verifies against a fixed PHC string computed once (a `std::sync::LazyLock<String>` holding `hash_password("dummy-password-for-timing")`).

**Tests:**

| Test | Assertion |
|---|---|
| `roles_round_trip` | for each variant of the three enums, `parse(as_str())` is the variant; `parse("ADMIN")` and `parse("")` are `None` |
| `enums_serialize_lowercase` | `serde_json::to_string(&Role::Admin)` is `"\"admin\""` |
| `principal_helpers` | a principal with teams `[(1, Lead), (2, Member)]`: `leads(1)`, `!leads(2)`, `!leads(3)`, `team_role(2) == Some(Member)`, `led_teams() == [1]`, `is_admin()` follows `role` |
| `email_is_normalized` | `"  Maya@Example.COM "` gives `maya@example.com` |
| `invalid_emails_are_rejected` | `""`, `"a"`, `"a@"`, `"@b.co"`, `"a@b"`, `"a@.co"`, `"a@b."`, `"a b@c.co"`, `"a@b@c.co"`, `"a\n@b.co"`, and a 255-byte address are errors |
| `password_policy` | 11 chars error; 12 ok; 256 ok; 257 error; 12 multi-byte characters (for example 12 of `é`) ok |
| `hash_and_verify` | hash verifies the same password; rejects another; two hashes of one password differ; hash starts with `$argon2id$v=19$m=19456,t=2,p=1$` |
| `verify_handles_bad_hashes` | `verify_password("x", "")`, `("x", "not-a-hash")`, `("x", "$argon2id$broken")` are all `false` |
| `dummy_verification_runs` | `verify_dummy("anything")` returns without panicking |

- [ ] Step 1: Write the tests. Run `cargo test -p ultrafast-gateway identity`. Expected: compile errors.
- [ ] Step 2: Implement.
- [ ] Step 3: Run the tests. Expected: 9 passed.
- [ ] Step 4: fmt, clippy, `cargo test --all`, commit `feat(gateway): identity types and password hashing`.

---

### Task 3: Users, invites, teams and audit in the store

**Files:**
- Create: `crates/gateway/src/store/users.rs`, `store/teams.rs`, `store/audit.rs`
- Modify: `crates/gateway/src/store/mod.rs`, `crates/gateway/src/secrets.rs`

**Interfaces (produces):**

```rust
// secrets.rs additions
pub const TOKEN_PREFIX: &str = "uf-at-";
pub const INVITE_PREFIX: &str = "uf-inv-";
/// A secret with the given prefix and 32 random bytes as hex.
pub fn generate_secret(prefix: &str) -> NewKey;   // same NewKey type: full, hash, display

// store/users.rs
#[derive(Clone, PartialEq, Eq)]
pub struct UserRow {
    pub id: i64, pub email: String, pub name: String,
    pub role: Role, pub status: UserStatus,
    pub password_hash: Option<String>,
    pub created_at: String, pub last_active_at: Option<String>,
}
// manual Debug: password_hash prints as "<present>" or "<none>"

pub struct NewUser<'a> { pub email: &'a str, pub name: &'a str, pub role: Role,
                         pub status: UserStatus, pub password_hash: Option<&'a str> }

impl Store {
    pub async fn count_users(&self) -> Result<i64>;
    pub async fn count_active_admins(&self) -> Result<i64>;
    pub async fn user_by_id(&self, id: i64) -> Result<Option<UserRow>>;
    pub async fn user_by_email(&self, email: &str) -> Result<Option<UserRow>>;
    pub async fn list_users(&self) -> Result<Vec<UserRow>>;                // ordered by email
    pub async fn touch_user(&self, id: i64) -> Result<()>;                 // last_active_at = now
    pub async fn invite_by_hash(&self, hash: &str) -> Result<Option<InviteRow>>; // live only
}
pub struct InviteRow { pub id: i64, pub user_id: i64, pub expires_at: String }

// store/teams.rs
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamRow { pub id: i64, pub name: String, pub created_at: String }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberRow { pub team_id: i64, pub user_id: i64, pub role: TeamRole }

impl Store {
    pub async fn team_by_id(&self, id: i64) -> Result<Option<TeamRow>>;
    pub async fn list_teams(&self) -> Result<Vec<TeamRow>>;                // ordered by name
    pub async fn members_of(&self, team_id: i64) -> Result<Vec<MemberRow>>;
    pub async fn memberships_of(&self, user_id: i64) -> Result<Vec<MemberRow>>;
}

// store/audit.rs
pub struct AuditEntry<'a> {
    pub actor_user_id: Option<i64>, pub actor_email: &'a str,
    pub action: &'a str, pub target_type: &'a str,
    pub target_id: Option<i64>, pub summary: &'a str,
}
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AuditRow { pub id: i64, pub at: String, pub actor_email: String,
                      pub action: String, pub target_type: String,
                      pub target_id: Option<i64>, pub summary: String }
impl Store {
    pub async fn list_audit(&self, limit: i64, before_id: Option<i64>) -> Result<Vec<AuditRow>>; // newest first
}

// store/mod.rs: writes go through a transaction that carries the audit entry
pub struct Tx<'c> { /* wraps sqlx::Transaction<'c, Sqlite> */ }
impl Store { pub async fn begin(&self) -> Result<Tx<'_>>; }
impl Tx<'_> {
    pub async fn commit(self) -> Result<()>;
    pub async fn audit(&mut self, e: AuditEntry<'_>) -> Result<()>;

    pub async fn insert_user(&mut self, u: NewUser<'_>) -> Result<i64>;
    pub async fn set_user_name(&mut self, id: i64, name: &str) -> Result<bool>;
    pub async fn set_user_role(&mut self, id: i64, role: Role) -> Result<bool>;
    pub async fn set_user_status(&mut self, id: i64, status: UserStatus) -> Result<bool>;
    pub async fn set_user_password(&mut self, id: i64, hash: &str) -> Result<bool>;
    pub async fn delete_user(&mut self, id: i64) -> Result<bool>;
    pub async fn count_active_admins(&mut self) -> Result<i64>;

    pub async fn insert_invite(&mut self, user_id: i64, token_hash: &str, expires_at: &str) -> Result<i64>;
    pub async fn use_invite(&mut self, invite_id: i64) -> Result<bool>;   // false if already used
    pub async fn delete_invites_of(&mut self, user_id: i64) -> Result<()>;

    pub async fn insert_team(&mut self, name: &str) -> Result<i64>;
    pub async fn rename_team(&mut self, id: i64, name: &str) -> Result<bool>;
    pub async fn delete_team(&mut self, id: i64) -> Result<bool>;
    pub async fn put_member(&mut self, team_id: i64, user_id: i64, role: TeamRole) -> Result<()>; // insert or update
    pub async fn remove_member(&mut self, team_id: i64, user_id: i64) -> Result<bool>;
}
```

`Result` is `anyhow::Result`. Boolean returns report whether a row was changed.

**Rules:**
1. A `Tx` that is dropped without `commit` rolls back, including its audit entries.
2. `insert_user` and `insert_team` fail with an error whose `downcast_ref::<StoreError>()` is `StoreError::Duplicate` when the unique constraint is hit. Add `#[derive(Debug, thiserror::Error)] pub enum StoreError { #[error("already exists")] Duplicate }` to `store/mod.rs`, detected through `sqlx::Error::Database(e)` with `e.is_unique_violation()`.
3. `invite_by_hash` returns only invites with `used_at IS NULL AND expires_at > datetime('now')`.
4. `use_invite` sets `used_at` only `WHERE used_at IS NULL`.
5. `count_active_admins` counts `role = 'admin' AND status = 'active'`.
6. `list_audit` returns at most `limit` rows (clamp to 1..=200), with `id < before_id` when given, ordered `id DESC`.
7. `audit` summaries are written by callers; the store does not inspect them.
8. Move plan 1's key and provider writes onto `Tx` as well, keeping the existing `Store::insert_provider`, `insert_key`, `revoke_key` as thin wrappers that open a transaction, call the `Tx` method and commit, so plan 1 callers and tests are unchanged:
   `Tx::insert_provider`, `Tx::insert_key(name, hash, display, expires_at, user_id: Option<i64>, team_id: Option<i64>)`, `Tx::revoke_key`.

**Tests:**

| Test | Assertion |
|---|---|
| `generate_secret_uses_prefix` | `generate_secret(TOKEN_PREFIX)`: `full` starts with `uf-at-`, length 6 + 64; `hash == hash_key(&full)`; `display` ends with the last 4 of `full`; two calls differ; Debug output hides `full` |
| `user_round_trip` | insert, read by id and by email, fields equal; `count_users` 1 |
| `duplicate_email_is_reported` | second insert with the same email gives `StoreError::Duplicate` |
| `user_debug_hides_password_hash` | `format!("{:?}", row)` contains `<present>` and not the hash |
| `user_updates_report_changes` | each `set_*` returns `true` for an existing id and `false` for an unknown one; values read back |
| `active_admin_count` | admin active, admin disabled, admin invited, member active: count is 1 |
| `dropped_transaction_rolls_back` | insert a user and an audit entry in a `Tx`, drop it; `count_users` 0 and `list_audit` empty |
| `invite_lifecycle` | live invite is found; after `use_invite` it is not found and a second `use_invite` is `false`; an invite with `expires_at` in the past is not found |
| `deleting_a_user_removes_dependents` | user with an invite, a team membership and a key: after `delete_user`, invite and membership are gone and the key remains with `user_id == None` |
| `team_round_trip_and_duplicate` | insert, list ordered by name, duplicate name gives `Duplicate` |
| `membership` | `put_member` inserts, then updates the role on a second call; `members_of` and `memberships_of` agree; `remove_member` returns `true` then `false` |
| `deleting_a_team_keeps_keys` | key with `team_id`: after `delete_team` the key remains with `team_id == None` |
| `audit_is_newest_first_and_paged` | insert 5; `list_audit(2, None)` returns ids 5, 4; `list_audit(2, Some(4))` returns 3, 2; limit 0 behaves as 1; limit 1000 behaves as 200 |

- [ ] Step 1: Write the tests. Run `cargo test -p ultrafast-gateway store`. Expected: compile errors.
- [ ] Step 2: Implement.
- [ ] Step 3: Run `cargo test --all`. Expected: all pass, plan 1 tests unchanged.
- [ ] Step 4: fmt, clippy, commit `feat(gateway): users, invites, teams and audit in the store`.

---

### Task 4: Sessions and access tokens in the store

**Files:**
- Create: `crates/gateway/src/store/sessions.rs`
- Modify: `crates/gateway/src/store/mod.rs`

**Interfaces (produces):**

```rust
pub const SESSION_SECONDS: i64 = 12 * 60 * 60;

pub struct NewSession { pub id: String /* cookie value */, pub csrf_token: String }
// no Debug

#[derive(Clone, PartialEq, Eq)]
pub struct SessionRow { pub id: i64, pub user_id: i64, pub csrf_token: String, pub expires_at: String }
// manual Debug: csrf_token redacted

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenRow { pub id: i64, pub user_id: i64, pub name: String, pub display: String,
                      pub expires_at: Option<String>, pub revoked_at: Option<String>,
                      pub last_used_at: Option<String>, pub created_at: String }

impl Store {
    /// Creates a session valid for SESSION_SECONDS.
    pub async fn create_session(&self, user_id: i64) -> Result<NewSession>;
    pub async fn live_session(&self, cookie_value: &str) -> Result<Option<SessionRow>>;
    pub async fn delete_session(&self, cookie_value: &str) -> Result<bool>;
    pub async fn delete_sessions_of(&self, user_id: i64) -> Result<u64>;
    pub async fn delete_expired_sessions(&self) -> Result<u64>;

    pub async fn live_token(&self, token: &str) -> Result<Option<TokenRow>>;
    pub async fn touch_token(&self, id: i64) -> Result<()>;
    pub async fn token_by_id(&self, id: i64) -> Result<Option<TokenRow>>;
    pub async fn list_tokens_of(&self, user_id: i64) -> Result<Vec<TokenRow>>;  // newest first
}
impl Tx<'_> {
    pub async fn insert_token(&mut self, user_id: i64, name: &str, hash: &str,
                              display: &str, expires_at: Option<&str>) -> Result<i64>;
    pub async fn revoke_token(&mut self, id: i64) -> Result<bool>;
    pub async fn delete_sessions_of(&mut self, user_id: i64) -> Result<u64>;
    pub async fn revoke_tokens_of(&mut self, user_id: i64) -> Result<u64>;
}
```

**Rules:**
1. The session cookie value is 32 random bytes as hex. Only its SHA-256 hex (`hash_key`) is stored, in `id_hash`. The CSRF token is another 32 random bytes as hex, stored as is.
2. `live_session` hashes the given value, and returns the row only when `expires_at > datetime('now')`. Values that are not 64 lowercase hex characters return `Ok(None)` without querying.
3. `live_token` returns `Ok(None)` without querying unless the value starts with `TOKEN_PREFIX`; otherwise it looks up the hash and requires `revoked_at IS NULL AND (expires_at IS NULL OR expires_at > datetime('now'))`.
4. `insert_token` validates `expires_at` with `check_timestamp`.
5. Neither function checks the user's status; that is the API layer's job (Task 6), because it must produce a `Principal` from current data.

**Tests:**

| Test | Assertion |
|---|---|
| `session_round_trip` | created session is found by its cookie value; `csrf_token` matches; `expires_at` is between `after(SESSION_SECONDS - 5)` and `after(SESSION_SECONDS + 5)` |
| `session_value_is_not_stored` | after `create_session`, a raw `SELECT id_hash FROM sessions` returns `hash_key(cookie)` and no column holds the cookie value |
| `malformed_session_values_find_nothing` | `""`, `"abc"`, 64 uppercase hex, 64 chars with a `g`, 65 hex |
| `expired_session_is_not_live` | set `expires_at` to the past with a raw UPDATE; `live_session` is `None`; `delete_expired_sessions` returns 1 |
| `delete_session` | returns `true` then `false`; session no longer live |
| `delete_sessions_of_user` | two sessions for one user and one for another: returns 2, the other user's session survives |
| `session_debug_hides_csrf` | Debug output does not contain the CSRF token |
| `token_round_trip` | inserted token found by its full value; not found by its hash, by another token, or by a value without the prefix |
| `revoked_and_expired_tokens_are_not_live` | both cases `None`; `revoke_token` returns `true` then `false` |
| `token_expiry_is_validated` | `insert_token` with `"tomorrow"` is an error and stores nothing |
| `deleting_a_user_removes_sessions_and_tokens` | after `delete_user`, neither is found |

- [ ] Step 1: Write the tests. Run. Expected: compile errors.
- [ ] Step 2: Implement.
- [ ] Step 3: Run `cargo test --all`. Expected: all pass.
- [ ] Step 4: fmt, clippy, commit `feat(gateway): sessions and access tokens in the store`.

---

### Task 5: Authorization policy

**Files:**
- Create: `crates/gateway/src/identity/policy.rs`
- Modify: `crates/gateway/src/identity/mod.rs`

**Interfaces (produces):**

```rust
/// What a request wants to do, with the facts needed to decide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    // users
    ListUsers,
    ViewUser   { user_id: i64, shares_led_team: bool },
    InviteUser { role: Role },
    UpdateUser { user_id: i64, changes_role_or_status: bool },
    DeleteUser { user_id: i64 },
    // teams
    ListTeams,
    CreateTeam,
    ViewTeam     { team_id: i64 },
    RenameTeam   { team_id: i64 },
    DeleteTeam   { team_id: i64 },
    PutMember    { team_id: i64, role: TeamRole },
    RemoveMember { team_id: i64 },
    // virtual keys
    ListKeys,
    CreateKey { owner_id: i64, team_id: Option<i64> },
    ViewKey   { owner_id: Option<i64>, team_id: Option<i64> },
    RevokeKey { owner_id: Option<i64>, team_id: Option<i64> },
    // access tokens: always the caller's own
    ManageOwnTokens,
    // providers
    ListProviders,
    ManageProviders,
    // audit
    ViewAudit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    /// Answer 403.
    Forbidden,
    /// Answer 404, so the caller cannot learn that the target exists.
    Hidden,
}

pub fn authorize(p: &Principal, action: &Action) -> Decision;

/// Which rows a list call may return.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope { All, Teams { team_ids: Vec<i64>, own_user_id: i64 }, Own { user_id: i64 } }
pub fn list_scope(p: &Principal) -> Scope;
```

**Rules** (A = admin, L = lead of the team in question, M = everyone else signed in):

| Action | A | L | M |
|---|---|---|---|
| `ListUsers` | Allow | Allow (rows filtered by `list_scope`) | Allow (own row only) |
| `ViewUser` | Allow | Allow if own id or `shares_led_team`; else Hidden | Allow if own id; else Hidden |
| `InviteUser` | Allow | Forbidden | Forbidden |
| `UpdateUser` | Allow | own id and not `changes_role_or_status`: Allow; own id and changes: Forbidden; other: Hidden | same as L |
| `DeleteUser` | Allow | Hidden unless own id, then Forbidden | same |
| `ListTeams` | Allow | Allow (filtered) | Allow (filtered) |
| `CreateTeam` | Allow | Forbidden | Forbidden |
| `ViewTeam` | Allow | Allow if a member of it in any role; else Hidden | same |
| `RenameTeam` | Allow | Allow if lead of it; member of it: Forbidden; else Hidden | member: Forbidden; else Hidden |
| `DeleteTeam` | Allow | lead or member of it: Forbidden; else Hidden | same |
| `PutMember` | Allow | lead of it and `role == Member`: Allow; lead of it and `role == Lead`: Forbidden; member of it: Forbidden; else Hidden | member: Forbidden; else Hidden |
| `RemoveMember` | Allow | lead of it: Allow; member of it: Forbidden; else Hidden | same |
| `ListKeys` | Allow | Allow (filtered) | Allow (filtered) |
| `CreateKey` | Allow | `owner_id` is own and `team_id` is `None` or a team they belong to: Allow; `owner_id` is another user and `team_id` is `Some(t)` they lead: Allow; else Forbidden | own and `team_id` `None` or a team they belong to: Allow; else Forbidden |
| `ViewKey`, `RevokeKey` | Allow | `owner_id == Some(own)`: Allow; `team_id == Some(t)` they lead: Allow; else Hidden | `owner_id == Some(own)`: Allow; else Hidden |
| `ManageOwnTokens` | Allow | Allow | Allow |
| `ListProviders` | Allow | Allow | Allow |
| `ManageProviders` | Allow | Forbidden | Forbidden |
| `ViewAudit` | Allow | Forbidden | Forbidden |

`list_scope`: admin gives `All`; a user who leads at least one team gives `Teams { team_ids: led_teams(), own_user_id }`; otherwise `Own`.

`authorize` is a pure function: no I/O, no clock.

**Tests:** one table-driven test, `policy_matrix`, with a fixture of four principals:

- `admin`: role Admin, no teams
- `lead`: role Member, teams `[(10, Lead), (20, Member)]`, id 2
- `member`: role Member, teams `[(10, Member)]`, id 3
- `loner`: role Member, no teams, id 4

The test asserts every cell of the rules table, with at least these rows per action where they apply: target inside a team the principal leads (10), inside a team they only belong to (20), in an unrelated team (30), own id, another user's id. It is written as a `Vec<(&str /*case name*/, &Principal, Action, Decision)>` so a failure prints the case name. It must contain at least 70 cases. Plus:

| Test | Assertion |
|---|---|
| `list_scope_by_role` | admin `All`; lead `Teams { team_ids: [10], own_user_id: 2 }`; member `Own { user_id: 3 }`; loner `Own { user_id: 4 }` |
| `admin_is_always_allowed` | for every `Action` value used in `policy_matrix`, `authorize(&admin, a) == Allow` |
| `nobody_else_manages_providers_or_audit` | lead, member, loner get `Forbidden` for `ManageProviders`, `ViewAudit`, `InviteUser`, `CreateTeam` |

- [ ] Step 1: Write the tests. Run `cargo test -p ultrafast-gateway policy`. Expected: compile errors.
- [ ] Step 2: Implement.
- [ ] Step 3: Run. Expected: 4 passed.
- [ ] Step 4: fmt, clippy, `cargo test --all`, commit `feat(gateway): authorization policy`.

---

### Task 6: `/api` foundation and sign-in

**Files:**
- Create: `crates/gateway/src/api/mod.rs`, `api/auth.rs`, `crates/gateway/src/identity/limiter.rs`, `crates/gateway/tests/api_auth.rs`
- Modify: `crates/gateway/src/lib.rs`, `src/app.rs`, `src/main.rs`, `tests/common/mod.rs`

**Interfaces (produces):**

```rust
// app.rs: AppState gains
pub limiter: crate::identity::limiter::LoginLimiter,
pub cookie_secure: bool,
// and a constructor so call sites stop listing every field:
impl AppState { pub fn new(store: Store, cipher: Cipher) -> Self; }   // defaults for everything else
// router() nests the /api router.

// api/mod.rs
pub fn router() -> axum::Router<std::sync::Arc<AppState>>;

#[derive(Debug)]
pub struct ApiError { pub status: StatusCode, pub code: &'static str,
                      pub message: String, pub fields: Option<BTreeMap<String, String>> }
impl ApiError {
    pub fn bad_request(message: impl Into<String>) -> Self;       // 400 bad_request
    pub fn validation(fields: BTreeMap<String, String>) -> Self;  // 422 validation_failed
    pub fn unauthenticated() -> Self;                             // 401 unauthenticated
    pub fn csrf() -> Self;                                        // 403 csrf_failed
    pub fn forbidden() -> Self;                                   // 403 forbidden
    pub fn not_found() -> Self;                                   // 404 not_found
    pub fn conflict(code: &'static str, message: impl Into<String>) -> Self;  // 409
    pub fn too_many_attempts() -> Self;                           // 429 too_many_attempts
    pub fn internal() -> Self;                                    // 500 internal_error
}
impl IntoResponse for ApiError;
impl From<anyhow::Error> for ApiError;   // logs the error, returns internal()

/// The authenticated caller. Extracting it authenticates the request and,
/// for cookie sessions on non-GET/HEAD requests, checks the CSRF header.
pub struct Authed { pub principal: Principal, pub via: AuthVia }
pub enum AuthVia { Session { session_id: i64, csrf_token: String }, Token { token_id: i64 } }
impl FromRequestParts<Arc<AppState>> for Authed;

/// Turns a policy decision into a result.
pub fn require(p: &Principal, action: &Action) -> Result<(), ApiError>;
//   Allow -> Ok, Forbidden -> forbidden(), Hidden -> not_found()

// identity/limiter.rs
pub struct LoginLimiter { /* Mutex<HashMap<String, VecDeque<Instant>>> */ }
impl LoginLimiter {
    pub fn new() -> Self;
    /// True when this email or address has used up its failures.
    pub fn is_blocked(&self, email: &str, addr: IpAddr, now: Instant) -> bool;
    pub fn record_failure(&self, email: &str, addr: IpAddr, now: Instant);
    pub fn record_success(&self, email: &str);   // clears the email's failures only
}
```

**Endpoints:**

| Method and path | Auth | Request | Success |
|---|---|---|---|
| `GET /api/setup` | none | | 200 `{"needs_setup": bool}` |
| `POST /api/setup` | none | `{"email","name","password"}` | 201 user object; creates the first admin, active |
| `POST /api/auth/login` | none | `{"email","password"}` | 200 `{"user": user, "csrf_token": "..."}` and `Set-Cookie` |
| `POST /api/auth/logout` | session | | 204, cookie cleared |
| `GET /api/auth/me` | any | | 200 `{"user": user, "teams": [{"team_id","name","role"}], "csrf_token": "..."|null}` |
| `POST /api/auth/accept-invite` | none | `{"token","password"}` | 204; user becomes active |
| `POST /api/auth/password` | any | `{"current_password","new_password"}` | 204; every other session and all access tokens of the user are ended |

User object: `{"id","email","name","role","status","created_at","last_active_at"}`. Never the password hash.

**Rules:**
1. `Authed` looks for `Authorization: Bearer <token>` first (scheme case-insensitive). If the header is present the cookie is ignored: a bad token is 401 even with a good cookie. Otherwise it reads the `uf_session` cookie.
2. After finding a live session or token, `Authed` loads the user and their memberships from the database on every request and builds the `Principal` from that. If the user is missing or `status != active`, the answer is 401 and, for a session, the session is deleted.
3. For a session on any method other than GET or HEAD, the `x-csrf-token` header must equal the session's CSRF token, compared in constant time; otherwise `ApiError::csrf()`. Token-authenticated requests need no CSRF header.
4. `Authed` calls `touch_user`, and `touch_token` for tokens, at most once per minute per user (skip when `last_active_at` is within the last 60 seconds) so reads do not cause a write each time.
5. Sign-in: normalize the email; if `limiter.is_blocked` answer 429 `too_many_attempts` without checking the password. If the user is unknown, not `active`, or has no password hash, call `verify_dummy`, record a failure and answer 401 `invalid_credentials` with the message `"Email or password is incorrect."`. A wrong password gives the identical status, code and message. An email that fails `normalize_email` is treated the same way, not as a validation error.
6. On success: `record_success`, create a session, set the cookie with `Max-Age=43200`, return the CSRF token in the body.
7. The client address comes from `ConnectInfo<SocketAddr>`. `main.rs` serves with `into_make_service_with_connect_info::<SocketAddr>()`. In tests with `oneshot`, where no connect info exists, the extractor falls back to `127.0.0.1`.
8. `POST /api/setup` succeeds only while `count_users() == 0`, checked inside the same transaction as the insert; afterwards it answers 409 `already_set_up`. It validates email, name (1 to 100 characters after trimming) and password policy, answering 422 with a `fields` entry per failing field.
9. Startup bootstrap in `main.rs` `serve`: if `UF_ADMIN_EMAIL` and `UF_ADMIN_PASSWORD` are both set and no user exists, create that admin (same validation; a failure stops startup with a message that names the variable but never prints the password). If users exist, the variables are ignored silently. If only one of the two is set, startup fails.
10. `accept-invite`: the token must start with `uf-inv-` and match a live invite; the password must pass policy (422 otherwise). In one transaction: mark the invite used, set the password, set status `active`, audit `user.accept_invite`. A bad, used or expired token is 404 `not_found`. A user who is `disabled` cannot accept: 404.
11. `password` change: verify `current_password` (401 `invalid_credentials` on mismatch, and it counts as a sign-in failure for the limiter), check policy on the new one, then in one transaction set the hash, delete the user's other sessions, revoke all their access tokens, and audit `user.change_password`. When called with a session, that session survives.
12. `logout` deletes the session and sets the cookie with `Max-Age=0`. Called with a token it answers 400 `bad_request`.
13. Audit entries for this task: `setup.create_admin`, `auth.login` (on success only), `user.accept_invite`, `user.change_password`. Summaries name the user by email and contain no secret.
14. JSON bodies over 64 KiB on `/api` are rejected with 413 `payload_too_large`; unknown JSON fields are rejected with 400 (`#[serde(deny_unknown_fields)]` on every request type); a body that is not valid JSON is 400 `bad_request`.
15. Any unmatched `/api/*` path answers 404 in the `/api` error shape, not axum's default body.

**Test harness additions** (`tests/common/mod.rs`):

```rust
pub struct Api { pub app: Router, pub store: Store }
pub struct Signed { pub cookie: String, pub csrf: String, pub user_id: i64 }

pub async fn api() -> Api;                       // empty database, insecure cookies
pub async fn seed_user(store: &Store, email: &str, role: Role, password: &str) -> i64;   // active
pub async fn seed_team(store: &Store, name: &str, members: &[(i64, TeamRole)]) -> i64;
pub async fn sign_in(app: &Router, email: &str, password: &str) -> Signed;
pub async fn call(app: &Router, method: &str, path: &str, auth: Option<&Signed>,
                  body: Option<serde_json::Value>) -> (StatusCode, HeaderMap, serde_json::Value);
pub async fn call_with_token(app: &Router, method: &str, path: &str, token: &str,
                             body: Option<serde_json::Value>) -> (StatusCode, HeaderMap, serde_json::Value);
```

`call` sends the cookie and, for non-GET methods, the CSRF header. An empty response body is returned as `Value::Null`. The existing `Harness` keeps working.

**Tests** (`tests/api_auth.rs`), password `"correct horse battery"` throughout:

| Test | Assertion |
|---|---|
| `setup_creates_the_first_admin_once` | `GET /api/setup` says `needs_setup: true`; POST gives 201 with role `admin`, status `active`, no `password_hash` key anywhere in the body; GET then says `false`; a second POST gives 409 `already_set_up` |
| `setup_validates_each_field` | email `"x"`, name `""`, password `"short"`: 422 with `fields` holding all three keys; no user created |
| `login_sets_a_strict_cookie` | 200; `Set-Cookie` contains `uf_session=`, `HttpOnly`, `SameSite=Strict`, `Path=/`, `Max-Age=43200`; no `Secure` with insecure cookies on; with `cookie_secure: true` it contains `Secure` |
| `login_failures_are_indistinguishable` | unknown email, wrong password, disabled user, invited user, malformed email: all 401 `invalid_credentials` with the identical body |
| `login_is_limited_per_email` | 5 wrong passwords then the right one: 429 `too_many_attempts` |
| `login_limit_clears_on_success` | 4 wrong, 1 right (200), 4 wrong, 1 right: 200 |
| `me_returns_user_teams_and_csrf` | user in two teams: both listed with roles; `csrf_token` equals the one from login |
| `requests_without_credentials_are_401` | `GET /api/auth/me` with no cookie; with a random 64-hex cookie; with `Bearer uf-at-` + 64 zeros |
| `csrf_is_required_for_session_writes` | `POST /api/auth/logout` with the cookie and no header: 403 `csrf_failed`; with a wrong header: 403; with the right one: 204 |
| `a_bad_token_is_not_rescued_by_a_cookie` | valid cookie plus `Authorization: Bearer uf-at-bad`: 401 |
| `logout_ends_the_session` | after logout, `me` with the old cookie is 401; `Set-Cookie` has `Max-Age=0` |
| `disabling_a_user_ends_access_at_once` | sign in, set status `disabled` directly in the store, `me` is 401, and the session row is gone |
| `role_change_is_seen_on_the_next_request` | sign in as admin (with a second admin present), lower the role in the store, `me` reports `member` |
| `expired_session_is_401` | set `expires_at` to the past in the store: 401 |
| `accept_invite_activates_the_user` | seed an invited user and an invite: accept gives 204; sign-in works; the same token again is 404 |
| `accept_invite_rejects_bad_input` | unknown token 404; expired invite 404; weak password 422 and the invite stays usable; disabled user 404 |
| `changing_password_ends_other_sessions_and_tokens` | two sessions and a token: change via session A; A still works; B is 401; the token is 401; old password fails sign-in; new one works |
| `changing_password_needs_the_current_one` | wrong current password: 401 and the hash is unchanged |
| `unknown_fields_and_bad_json_are_400` | `{"email":"a@b.co","password":"x","extra":1}` and `{not json` to login |
| `large_bodies_are_413` | a 70 KiB JSON body to login |
| `unknown_api_path_is_404_json` | `GET /api/nope`: 404 with `error.code == "not_found"` |
| `nothing_secret_is_audited` | after setup, login, password change: no `summary` in `audit_log` contains the password, the cookie value, or the CSRF token |
| `v1_still_works` | the plan 1 harness test for a proxied chat completion passes against the combined router |

Unit tests in `identity/limiter.rs`: window expiry using injected `Instant`s (5 failures at t0 block at t0+14min, not at t0+16min); the per-address limit of 20 across different emails; `record_success` clears the email but not the address count; the map does not grow without bound (entries with no failures inside the window are removed on access).

- [ ] Step 1: Extend the harness, write `tests/api_auth.rs` and the limiter tests. Run `cargo test -p ultrafast-gateway --test api_auth`. Expected: compile errors.
- [ ] Step 2: Implement `ApiError`, `Authed`, the limiter, the endpoints, `AppState::new`, the bootstrap.
- [ ] Step 3: Run `cargo test --all`. Expected: all pass.
- [ ] Step 4: Run the binary for real in a temp data directory with `UF_ADMIN_EMAIL` and `UF_ADMIN_PASSWORD` and `--insecure-cookies`, sign in with curl, call `me`; record the real output in the report with the cookie and CSRF values replaced by `<redacted>`.
- [ ] Step 5: fmt, clippy, commit `feat(gateway): admin API foundation and sign-in`.

---

### Task 7: Users and teams endpoints

**Files:**
- Create: `crates/gateway/src/api/users.rs`, `api/teams.rs`, `api/audit.rs`, `tests/api_users.rs`, `tests/api_teams.rs`
- Modify: `crates/gateway/src/api/mod.rs`

**Endpoints:**

| Method and path | Policy action | Request | Success |
|---|---|---|---|
| `GET /api/users` | `ListUsers` | | 200 `{"users":[user]}` filtered by `list_scope` |
| `POST /api/users` | `InviteUser` | `{"email","name","role"}` | 201 `{"user": user, "invite_link": "/accept-invite?token=uf-inv-…"}` |
| `GET /api/users/{id}` | `ViewUser` | | 200 user |
| `PATCH /api/users/{id}` | `UpdateUser` | any of `{"name","role","status"}` | 200 user |
| `DELETE /api/users/{id}` | `DeleteUser` | | 204 |
| `POST /api/users/{id}/invite` | `InviteUser` | | 201 `{"invite_link"}`; replaces earlier invites; only for `invited` users |
| `GET /api/teams` | `ListTeams` | | 200 `{"teams":[{"id","name","member_count","created_at"}]}` filtered |
| `POST /api/teams` | `CreateTeam` | `{"name"}` | 201 team |
| `GET /api/teams/{id}` | `ViewTeam` | | 200 `{"team": team, "members":[{"user_id","email","name","role"}]}` |
| `PATCH /api/teams/{id}` | `RenameTeam` | `{"name"}` | 200 team |
| `DELETE /api/teams/{id}` | `DeleteTeam` | | 204 |
| `PUT /api/teams/{id}/members/{user_id}` | `PutMember` | `{"role"}` | 204 |
| `DELETE /api/teams/{id}/members/{user_id}` | `RemoveMember` | | 204 |
| `GET /api/audit?limit=&before=` | `ViewAudit` | | 200 `{"entries":[audit row]}` |

**Rules:**
1. Every handler: extract `Authed`, load only the facts the `Action` needs, call `require`, then act. For actions on an id, a missing row answers 404 before or after the policy check with the same body, so existence is never revealed. Compute `shares_led_team` as: the target user belongs to at least one team the caller leads.
2. List filtering: `Scope::All` returns everything; `Scope::Teams` returns users who are members of any listed team plus the caller, and teams the caller belongs to in any role; `Scope::Own` returns the caller only, and teams the caller belongs to.
3. `PATCH /api/users/{id}` status may be set to `active` or `disabled` only. Setting `active` on a user with no password hash is 409 `no_password`. `invited` cannot be set by PATCH (422).
4. Last admin: any change that would leave zero users with `role = admin AND status = active` is refused with 409 `last_admin` and the message `"At least one active admin is required."`. This covers role change, status change and delete, by anyone including the admin themselves. The count is read inside the write transaction.
5. When a user is disabled or deleted, or their role changes, all their sessions are deleted in the same transaction. Access tokens are revoked on disable and delete, not on role change.
6. A user cannot delete themselves: 409 `cannot_delete_self`.
7. Team name: 1 to 60 characters after trimming, unique; a duplicate is 409 `team_exists`. A duplicate email on invite is 409 `user_exists`.
8. `PUT` member: the user must exist and not be `disabled` (404 for a missing user, 409 `user_disabled`).
9. Removing or demoting the last lead of a team is allowed; a team may have no lead.
10. Invite links: the token is generated with `generate_secret(INVITE_PREFIX)`, stored hashed, valid 7 days. The link is returned once and never stored or audited.
11. Audit actions: `user.invite`, `user.reinvite`, `user.update`, `user.delete`, `team.create`, `team.rename`, `team.delete`, `team.member_put`, `team.member_remove`. Summaries state what changed in words, for example `Changed role of lena@example.com from member to admin`.
12. Path ids that are not positive integers answer 404.

**Tests.** Fixture for both files: admin `maya`, `arjun` (lead of Platform, member of Research), `lena` (member of Platform), `tomas` (member of Research), `priya` (no team), teams Platform and Research and Growth (empty).

`tests/api_users.rs`:

| Test | Assertion |
|---|---|
| `admin_lists_everyone` | 5 users |
| `lead_lists_their_teams_members_and_self` | arjun sees arjun and lena only (Platform), not tomas |
| `member_lists_only_self` | lena sees 1 |
| `invite_creates_an_invited_user_with_a_link` | 201; status `invited`; link starts with `/accept-invite?token=uf-inv-`; the token works with accept-invite; `audit_log` has `user.invite` and no summary contains the token |
| `only_admins_invite` | arjun and lena get 403 |
| `invite_validates_and_detects_duplicates` | bad email 422; unknown role 422; existing email in another case (`MAYA@example.com`) 409 `user_exists` |
| `reinvite_replaces_the_old_link` | old token is 404 on accept, new one works; reinvite of an active user is 409 |
| `viewing_users_hides_outsiders` | arjun gets lena 200 and tomas 404; lena gets arjun 404 and herself 200; an unknown id is 404 for the admin too, with the same body as the hidden case |
| `users_edit_their_own_name_only` | lena renames herself 200; lena sets her own role 403; lena patches arjun 404 |
| `admin_changes_role_and_status` | 200 each; the target's existing session is 401 afterwards |
| `the_last_admin_is_protected` | with maya the only admin: demote 409 `last_admin`, disable 409, delete 409 (self-delete rule may answer first: accept either `last_admin` or `cannot_delete_self`); after promoting arjun, demoting maya succeeds |
| `two_admins_cannot_both_be_removed` | with two admins, disable one 200, then the other 409 |
| `activating_a_user_without_password_is_refused` | 409 `no_password` |
| `deleting_a_user` | 204; their keys remain with no owner; their session is 401; a second delete is 404 |
| `bad_ids_are_404` | `/api/users/abc`, `/api/users/0`, `/api/users/-1` |

`tests/api_teams.rs`:

| Test | Assertion |
|---|---|
| `admin_lists_all_teams_with_counts` | 3 teams, Platform count 2, Growth 0 |
| `users_list_only_their_teams` | arjun 2, lena 1, priya 0 |
| `only_admins_create_and_delete_teams` | arjun create 403; arjun delete Platform 403; arjun delete Growth 404 |
| `team_names_are_validated_and_unique` | empty 422; 61 characters 422; duplicate 409 `team_exists`; leading and trailing spaces are trimmed |
| `team_detail_lists_members` | lena views Platform 200 with 2 members; lena views Research 404 |
| `leads_rename_their_team` | arjun renames Platform 200; arjun renames Research (member only) 403; lena renames Platform 403; priya renames Platform 404 |
| `leads_add_members_but_not_leads` | arjun adds priya to Platform as member 204; as lead 403; arjun adds to Research 403; lena adds to Platform 403 |
| `leads_remove_members` | arjun removes lena 204; again 404 |
| `adding_a_missing_or_disabled_user` | missing 404; disabled 409 `user_disabled` |
| `admin_appoints_leads` | maya sets lena lead of Platform 204; lena can now rename it |
| `deleting_a_team_keeps_its_keys` | key with that team: after delete, key exists with no team |
| `audit_is_admin_only_and_paged` | arjun 403; maya gets entries newest first; `limit=2` returns 2; `before=<id>` continues |

- [ ] Step 1: Write both test files. Run. Expected: 404s from the unmatched-path handler, so failures on status.
- [ ] Step 2: Implement.
- [ ] Step 3: Run `cargo test --all`. Expected: all pass.
- [ ] Step 4: fmt, clippy, commit `feat(gateway): users, teams and audit endpoints`.

---

### Task 8: Keys, providers and access tokens endpoints

**Files:**
- Create: `crates/gateway/src/api/keys.rs`, `api/providers.rs`, `api/tokens.rs`, `tests/api_keys.rs`, `tests/api_providers.rs`, `tests/api_tokens.rs`
- Modify: `crates/gateway/src/api/mod.rs`, `src/store/keys.rs`, `src/store/providers.rs`, `src/config.rs`

**Store additions:**

```rust
impl Store {
    pub async fn key_by_id(&self, id: i64) -> Result<Option<KeyRow>>;
    pub async fn list_keys(&self) -> Result<Vec<KeyRow>>;             // newest first, includes revoked
    pub async fn provider_by_id(&self, id: i64) -> Result<Option<ProviderRow>>;
    pub async fn list_providers(&self) -> Result<Vec<ProviderRow>>;   // ordered by name
}
impl Tx<'_> {
    pub async fn update_provider(&mut self, id: i64, base_url: Option<&str>,
                                 credential: Option<Option<&[u8]>>) -> Result<bool>;
    pub async fn delete_provider(&mut self, id: i64) -> Result<bool>;
}
// config.rs
pub fn validate_provider_name(name: &str) -> anyhow::Result<()>;
```

**Endpoints:**

| Method and path | Policy action | Request | Success |
|---|---|---|---|
| `GET /api/keys` | `ListKeys` | | 200 `{"keys":[key]}` filtered |
| `POST /api/keys` | `CreateKey` | `{"name","team_id"?, "owner_id"?, "expires_at"?}` | 201 `{"key": key, "secret": "uf-sk-…"}` |
| `GET /api/keys/{id}` | `ViewKey` | | 200 key |
| `DELETE /api/keys/{id}` | `RevokeKey` | | 204 |
| `GET /api/providers` | `ListProviders` | | 200 `{"providers":[provider]}` |
| `POST /api/providers` | `ManageProviders` | `{"name","kind","base_url","api_key"?}` | 201 provider |
| `PATCH /api/providers/{id}` | `ManageProviders` | any of `{"base_url","api_key"}`; `"api_key": null` removes it | 200 provider |
| `DELETE /api/providers/{id}` | `ManageProviders` | | 204 |
| `GET /api/tokens` | `ManageOwnTokens` | | 200 `{"tokens":[token]}` own only |
| `POST /api/tokens` | `ManageOwnTokens` | `{"name","expires_at"?}` | 201 `{"token": token, "secret": "uf-at-…"}` |
| `DELETE /api/tokens/{id}` | `ManageOwnTokens` | | 204; another user's token id is 404 |

Objects:
- key: `{"id","name","display","owner_id","owner_email","team_id","team_name","expires_at","revoked_at","created_at","status"}` where `status` is `active`, `expired` or `revoked`. Never the hash.
- provider: `{"id","name","kind","base_url","has_credential"}`. Never the credential, in any form.
- token: `{"id","name","display","expires_at","revoked_at","last_used_at","created_at"}`.

**Rules:**
1. `POST /api/keys`: `owner_id` defaults to the caller. `name` is 1 to 100 characters after trimming. `expires_at` must pass `check_timestamp` and be in the future (422). If `team_id` is given the team must exist and the owner must be a member of it: 422 with `fields.team_id` = `"owner is not a member of this team"`. The owner must be `active` (422).
2. Key list filtering: `All` everything; `Teams` keys whose `team_id` is a led team plus keys owned by the caller; `Own` keys owned by the caller.
3. Revoking an already revoked key is 204 and writes no second audit entry.
4. `validate_provider_name`: 1 to 40 characters, only `a-z`, `0-9`, `-`, `_`, starting with a letter or digit. This is stricter than plan 1's CLI check; the CLI `provider add` switches to it.
5. `POST /api/providers`: `kind` must parse with `ProviderKind::parse`; `base_url` passes `validate_base_url`; an `api_key` that is an empty string, or only whitespace, is 422 (`fields.api_key` = `"must not be empty"`), not stored as a credential. The same applies to PATCH and to the CLI. A duplicate name is 409 `provider_exists`.
6. The credential is encrypted with `state.cipher` before it reaches the store. Request types holding `api_key` have no `Debug` derive.
7. Access token `name`: 1 to 100 characters. `expires_at` as for keys.
8. Audit actions: `key.create`, `key.revoke`, `provider.create`, `provider.update`, `provider.delete`, `token.create`, `token.revoke`. Provider summaries say `credential set`, `credential replaced` or `credential removed`, never its value or length.
9. The `secret` field appears only in the 201 response of a create call.

**Tests.** Same fixture as Task 7, plus one key each owned by arjun (team Platform), lena (team Platform), tomas (team Research), priya (no team), and one legacy key with no owner.

`tests/api_keys.rs`:

| Test | Assertion |
|---|---|
| `admin_lists_all_keys` | 5, including the legacy key with `owner_id: null` |
| `lead_lists_team_and_own_keys` | arjun sees arjun's and lena's (Platform), not tomas's although arjun is a member of Research |
| `member_lists_own_keys` | lena sees 1 |
| `creating_a_key_returns_the_secret_once` | 201; `secret` starts with `uf-sk-`; `GET /api/keys/{id}` has no `secret` and no `hash`; the secret authenticates a `/v1/chat/completions` call (needs Task 9; mark the `/v1` half `#[ignore = "enabled in task 9"]` here and enable it there) |
| `members_create_keys_for_themselves_in_their_teams` | lena with Platform 201; lena with Research 403; lena with `owner_id` = arjun 403 |
| `leads_create_keys_for_team_members` | arjun for lena in Platform 201; arjun for tomas in Research 403; arjun for lena with no team 403 |
| `owner_must_belong_to_the_team` | maya creates a key for priya in Platform: 422 `fields.team_id` |
| `key_input_is_validated` | empty name 422; `expires_at` `"2999-02-31 00:00:00"` 422; a past `expires_at` 422; unknown team 422; disabled owner 422 |
| `viewing_and_revoking_follow_the_policy` | lena views arjun's key 404; arjun revokes lena's 204; arjun revokes tomas's 404; lena revokes her own 204 |
| `revoking_twice_is_quiet` | second call 204; exactly one `key.revoke` audit entry |
| `status_reflects_expiry_and_revocation` | an expired key reports `expired`; a revoked one `revoked`; revoked wins when both apply |

`tests/api_providers.rs`:

| Test | Assertion |
|---|---|
| `everyone_lists_providers_without_credentials` | lena gets 200; the raw response text contains neither the API key nor the word `credential` except in `has_credential` |
| `only_admins_change_providers` | arjun POST, PATCH, DELETE all 403 |
| `creating_a_provider_encrypts_the_key` | 201 with `has_credential: true`; the stored `credential` bytes do not contain the key and decrypt to it with the harness cipher |
| `provider_input_is_validated` | names `""`, `"Open AI"`, `"a/b"`, `"-x"`, 41 characters: 422; kind `"gemini"` 422; base URL with userinfo 422; `api_key: "  "` 422 |
| `duplicate_provider_name_is_409` | |
| `patch_replaces_and_removes_the_key` | PATCH with a new key: `has_credential` true and it decrypts to the new value; PATCH with `"api_key": null`: `has_credential` false; PATCH with only `base_url`: the credential is untouched |
| `provider_audit_never_shows_the_key` | no `summary` contains the key |
| `deleting_a_provider` | 204 then 404 |

`tests/api_tokens.rs`:

| Test | Assertion |
|---|---|
| `a_token_authenticates_as_its_owner` | lena creates a token; `me` with it reports lena; no CSRF header needed for a POST made with it |
| `tokens_follow_the_owners_current_role` | maya creates a token, is then lowered to member (second admin present): `POST /api/teams` with the token is 403 |
| `tokens_are_private` | arjun lists only his own; arjun deleting lena's token id is 404 |
| `revoked_and_expired_tokens_fail` | both 401 |
| `token_secret_is_shown_once` | list and audit never contain the secret |

- [ ] Step 1: Write the three test files. Run. Expected: failures on status.
- [ ] Step 2: Implement.
- [ ] Step 3: Run `cargo test --all`. Expected: all pass, one ignored.
- [ ] Step 4: fmt, clippy, commit `feat(gateway): keys, providers and access tokens endpoints`.

---

### Task 9: Snapshot

**Files:**
- Create: `crates/gateway/src/snapshot.rs`, `crates/gateway/tests/snapshot.rs`
- Modify: root and gateway `Cargo.toml` (add `arc-swap = "1"`), `src/app.rs`, `src/auth.rs`, `src/proxy.rs`, `src/api/keys.rs`, `src/api/providers.rs`, `src/api/users.rs`, `src/api/teams.rs`, `src/main.rs`, `tests/common/mod.rs`, `tests/api_keys.rs`

**Interfaces (produces):**

```rust
#[derive(Clone)]
pub struct SnapKey { pub id: i64, pub name: String, pub user_id: Option<i64>,
                     pub team_id: Option<i64>, pub expires_at: Option<String> }

#[derive(Clone)]
pub struct SnapProvider { pub id: i64, pub name: String, pub kind: ProviderKind,
                          pub base_url: String, pub api_key: Option<String> }
// manual Debug, api_key redacted

pub struct Snapshot { /* keys: HashMap<String /*hash*/, SnapKey>, providers: HashMap<String, SnapProvider> */ }
impl Snapshot {
    pub async fn load(store: &Store, cipher: &Cipher) -> anyhow::Result<Snapshot>;
    /// The key for this hash, unless it has expired as of `now` (UTC, YYYY-MM-DD HH:MM:SS).
    pub fn key(&self, hash: &str, now: &str) -> Option<&SnapKey>;
    pub fn provider(&self, name: &str) -> Option<&SnapProvider>;
}

// app.rs: AppState gains
pub snapshot: arc_swap::ArcSwap<Snapshot>,
impl AppState {
    pub async fn new(store: Store, cipher: Cipher) -> anyhow::Result<Self>;   // now async and fallible: loads the snapshot
    /// Rebuilds the snapshot from the database and swaps it in.
    pub async fn refresh(&self) -> anyhow::Result<()>;
}
```

**Rules:**
1. `Snapshot::load` reads keys that are not revoked, whose owner (when there is one) is `active`, and all providers. A key whose owner is `disabled` or `invited` is left out. Legacy keys with no owner are included.
2. Provider credentials are decrypted at load. A provider whose credential cannot be decrypted or is not UTF-8, or whose `kind` does not parse, is left out of the snapshot and logged with `tracing::error!` naming the provider only. Loading does not fail because of it.
3. `key()` compares `expires_at` with `now` as strings and returns `None` when `expires_at <= now`. So expiry needs no refresh.
4. `/v1`: `auth::authenticate` takes `&Snapshot` instead of `&Store` and becomes synchronous: `pub fn authenticate(snapshot: &Snapshot, headers: &HeaderMap) -> Result<SnapKey, Response>`. `proxy::chat_completions` loads the snapshot once per request (`state.snapshot.load_full()`) and uses it for the key and the provider. No `state.store` call remains in `auth.rs` or `proxy.rs`.
5. Every `/api` handler that changes keys, providers, user status, user role, or deletes a user or team calls `state.refresh().await` after its transaction commits and before it answers. If the refresh fails the handler logs it and answers 500 `internal_error`; the change is already committed, and the next successful refresh picks it up.
6. `main.rs` `serve` also refreshes every 30 seconds in a background task, so changes made by the CLI against a running gateway appear within that time. The task stops on shutdown.
7. CLI: validate all arguments before loading the master key or opening the database, so a rejected command creates no files. `key create --name` is validated like the API (1 to 100 characters). `provider add` uses `validate_provider_name` and rejects an empty API key.
8. README quickstart: state that the admin API applies changes at once and the CLI's changes reach a running gateway within 30 seconds. Remove the stale v1 line "Rust 1.75+ required" by changing it to 1.88+.

**Tests** (`tests/snapshot.rs`, using a wiremock upstream as in plan 1):

| Test | Assertion |
|---|---|
| `v1_does_not_touch_the_database` | build the harness, then close the store's pool (`store.pool().close().await`); a chat completion with a valid key still returns 200 |
| `a_new_key_works_at_once` | create a key through `POST /api/keys`; the returned secret gets 200 on `/v1/chat/completions` with no restart |
| `a_revoked_key_stops_at_once` | 200, then `DELETE /api/keys/{id}`, then 401 |
| `an_expiring_key_stops_without_refresh` | insert a key expiring in 2 seconds and refresh; 200; wait 3 seconds; 401 with no refresh call in between |
| `a_disabled_owner_stops_their_keys` | PATCH the owner to `disabled`: their key gets 401; set back to `active`: 200 |
| `deleting_the_owner_keeps_the_key_working` | the key becomes ownerless and still returns 200 |
| `a_new_provider_works_at_once` | `POST /api/providers` pointing at the mock; `model: "<name>/m"` returns 200 |
| `a_changed_credential_is_used_at_once` | PATCH `api_key`; the mock expects the new `Authorization` header |
| `a_deleted_provider_is_404` | |
| `an_undecryptable_provider_is_skipped` | write garbage bytes as a credential directly in the store, refresh: `refresh` succeeds; that provider is 404 on `/v1`; another provider still works |
| `snapshot_debug_hides_credentials` | `format!("{:?}", snap_provider)` does not contain the key |
| `cli_changes_appear_after_refresh` | insert a key through the store as the CLI would, call `state.refresh()`: the key works |

Enable the ignored half of `creating_a_key_returns_the_secret_once` in `tests/api_keys.rs`.

Unit tests in `src/main.rs` are not possible for argument order; verify rule 7 for real: run `ultrafast provider add --name "Bad Name" --kind openai --base-url https://x.example` with a fresh temp `UF_DATA_DIR` and record that the command fails and the directory is still empty.

- [ ] Step 1: Write `tests/snapshot.rs`. Run. Expected: compile errors (`Snapshot` not found).
- [ ] Step 2: Implement the snapshot, switch `/v1` to it, add refresh calls, the background task, and the CLI ordering.
- [ ] Step 3: Run `cargo test --all`. Expected: all pass, none ignored.
- [ ] Step 4: Run the real check for rule 7 and a real end-to-end smoke test: start the gateway, sign in, create a provider pointing at `http://127.0.0.1:9` and a key through the API, call `/v1/chat/completions` with the key and get the 502 "Could not reach provider" answer. Record the output with secrets replaced by `<redacted>`.
- [ ] Step 5: fmt, clippy, commit `feat(gateway): serve /v1 from an in-memory snapshot`.

---

### Task 10: Role table test and OpenAPI

**Files:**
- Create: `crates/gateway/src/api/openapi.rs`, `crates/gateway/tests/api_roles.rs`, `openapi/admin.json`
- Modify: root and gateway `Cargo.toml` (add `utoipa = { version = "5", features = ["axum_extras"] }`), every `src/api/*.rs` (annotations), `src/main.rs` (subcommand), `.github/workflows/ci.yml`

**Interfaces (produces):**
- `api::openapi::spec() -> utoipa::openapi::OpenApi`
- CLI subcommand `ultrafast openapi` printing the spec as pretty JSON to stdout. It needs no data directory and creates no files.

**Rules:**
1. Every `/api` endpoint from Tasks 6 to 8 is annotated with `#[utoipa::path(...)]` giving method, path, request body type, each response status with its body type, and a tag (`auth`, `users`, `teams`, `keys`, `providers`, `tokens`, `audit`). Request and response types derive `utoipa::ToSchema`.
2. The spec declares two security schemes, `session` (cookie `uf_session`) and `token` (HTTP bearer), and the `x-csrf-token` header parameter on non-GET operations.
3. The error shape is one shared schema, `ApiErrorBody`, referenced by every error response.
4. Fields that hold secrets in requests (`password`, `api_key`, `token`) are marked `write_only` in the schema; `secret` and `invite_link` in responses carry a description saying they are shown once.
5. `openapi/admin.json` is the output of `ultrafast openapi`, committed. Info: title `Ultrafast Gateway Admin API`, version from `CARGO_PKG_VERSION`.
6. CI gains a step after the tests: `cargo run -q -p ultrafast-gateway -- openapi | diff - openapi/admin.json`, failing with the message `openapi/admin.json is out of date; run: cargo run -p ultrafast-gateway -- openapi > openapi/admin.json`.
7. CI also gains a job that builds and tests with the declared minimum Rust version: `dtolnay/rust-toolchain@1.88` running `cargo test --all --locked`.

**Tests.**

`tests/api_roles.rs` holds one test, `every_endpoint_for_every_role`, driven by a table. Fixture as in Task 8. For each row it makes the call as each of four callers and asserts the status:

| # | Call | admin (maya) | lead (arjun) | member (lena) | no session |
|---|---|---|---|---|---|
| 1 | `GET /api/auth/me` | 200 | 200 | 200 | 401 |
| 2 | `GET /api/users` | 200 | 200 | 200 | 401 |
| 3 | `POST /api/users` (valid body) | 201 | 403 | 403 | 401 |
| 4 | `GET /api/users/{lena}` | 200 | 200 | 200 | 401 |
| 5 | `GET /api/users/{tomas}` | 200 | 404 | 404 | 401 |
| 6 | `PATCH /api/users/{lena}` `{"name"}` | 200 | 404 | 200 | 401 |
| 7 | `PATCH /api/users/{lena}` `{"role":"admin"}` | 200 | 404 | 403 | 401 |
| 8 | `DELETE /api/users/{priya}` | 204 | 404 | 404 | 401 |
| 9 | `GET /api/teams` | 200 | 200 | 200 | 401 |
| 10 | `POST /api/teams` | 201 | 403 | 403 | 401 |
| 11 | `GET /api/teams/{platform}` | 200 | 200 | 200 | 401 |
| 12 | `GET /api/teams/{growth}` | 200 | 404 | 404 | 401 |
| 13 | `PATCH /api/teams/{platform}` | 200 | 200 | 403 | 401 |
| 14 | `DELETE /api/teams/{growth}` | 204 | 404 | 404 | 401 |
| 15 | `PUT /api/teams/{platform}/members/{priya}` member | 204 | 204 | 403 | 401 |
| 16 | `PUT /api/teams/{platform}/members/{priya}` lead | 204 | 403 | 403 | 401 |
| 17 | `DELETE /api/teams/{platform}/members/{lena}` | 204 | 204 | 403 | 401 |
| 18 | `GET /api/keys` | 200 | 200 | 200 | 401 |
| 19 | `POST /api/keys` own, no team | 201 | 201 | 201 | 401 |
| 20 | `GET /api/keys/{lena's}` | 200 | 200 | 200 | 401 |
| 21 | `GET /api/keys/{tomas's}` | 200 | 404 | 404 | 401 |
| 22 | `DELETE /api/keys/{lena's}` | 204 | 204 | 204 | 401 |
| 23 | `GET /api/providers` | 200 | 200 | 200 | 401 |
| 24 | `POST /api/providers` | 201 | 403 | 403 | 401 |
| 25 | `PATCH /api/providers/{id}` | 200 | 403 | 403 | 401 |
| 26 | `DELETE /api/providers/{id}` | 204 | 403 | 403 | 401 |
| 27 | `GET /api/tokens` | 200 | 200 | 200 | 401 |
| 28 | `POST /api/tokens` | 201 | 201 | 201 | 401 |
| 29 | `GET /api/audit` | 200 | 403 | 403 | 401 |
| 30 | `POST /api/auth/logout` | 204 | 204 | 204 | 401 |
| 31 | `POST /api/auth/password` (valid body) | 204 | 204 | 204 | 401 |
| 32 | `POST /api/users/{sam}/invite` (sam is `invited`) | 201 | 403 | 403 | 401 |
| 33 | `DELETE /api/tokens/{caller's own token}` | 204 | 204 | 204 | 401 |

The fixture for this file adds `sam`, an invited user with no team, and one access token per caller.

Each cell runs against a freshly built fixture so earlier calls cannot change later results. A failure prints the row number, the call and the caller.

A second test in the same file, `every_documented_operation_is_in_the_role_table`, walks `api::openapi::spec()` and asserts that each operation that has a security requirement appears in the table above (matched by method and path template), so a new endpoint cannot be added without a row.

In `src/api/openapi.rs`:

| Test | Assertion |
|---|---|
| `spec_lists_every_route` | the set of (method, path) in the spec equals a literal list of all 32 routes from Tasks 6 to 8 |
| `secrets_are_write_only` | `password`, `current_password`, `new_password`, `api_key` and `token` properties are `writeOnly` in every request schema that has them |
| `no_response_schema_has_secret_fields` | no response schema has a property named `password_hash`, `credential`, `key_hash`, `token_hash` or `id_hash` |
| `committed_spec_is_current` | `serde_json::to_value(spec())` equals the parsed content of `../../openapi/admin.json` (path via `env!("CARGO_MANIFEST_DIR")`) |

- [ ] Step 1: Write `tests/api_roles.rs` with the table. Run. Expected: passes for the role cells already implemented; the operation-coverage test fails to compile (`openapi` not found).
- [ ] Step 2: Add the annotations, `spec()`, the subcommand; generate `openapi/admin.json`.
- [ ] Step 3: Run `cargo test --all`. Expected: all pass.
- [ ] Step 4: Run `cargo run -q -p ultrafast-gateway -- openapi | diff - openapi/admin.json` and record that it prints nothing.
- [ ] Step 5: fmt, clippy, commit `feat(gateway): role table test and OpenAPI spec`.

---

## Spec coverage

| Spec section | Covered here | Deferred to |
|---|---|---|
| 6 `/api` with session cookie or access token | Tasks 6 to 8 | |
| 6 In-memory snapshot replaced after each write | Task 9 | models and routes join it in plan 3 |
| 6 First admin by environment or setup screen | Task 6 | the screen itself: console plan |
| 7 Steps 1 and 2 without touching the database | Task 9 | |
| 8 Roles, enforced in one place | Tasks 5 to 8 | |
| 8 Model access | | plan 3 |
| 8 Credentials table: password, token, session, CSRF, sign-in limits | Tasks 2, 4, 6 | |
| 11 Tables: users, teams, team_members, sessions, access_tokens, audit_log | Task 1 | the rest as their plans need them |
| 12 `org_id`, `auth_provider`, `external_id` | Task 1 | |
| 15 One `/api` error shape | Task 6 | |
| 16 Role-by-endpoint table test | Task 10 | |
| Generated OpenAPI spec | Task 10 | admin SDK generation: phase 2 |

## Plan 1 follow-ups folded in

| Follow-up | Task |
|---|---|
| In-memory database can lose its only connection | 1 |
| `revoke_key` does not report whether it revoked; re-revoke overwrites the time | 1 |
| Queries do not filter by organization | 1 |
| Expiry check accepts impossible dates | 1 |
| Empty API key stored as a credential; key name not validated | 8, 9 |
| CLI creates files before validating arguments | 9 |
| Keys and providers read from the database on every request | 9 |
| Unused `pub fn stream_response` | 9 (remove it when editing `proxy.rs`) |
| CI does not test the declared minimum Rust version | 10 |
| README line "Rust 1.75+ required" | 9 |

Not folded in, because they belong to other areas: stream timeout policy and in-stream error retryability (plan 3), thinking blocks and duplicate JSON keys (plan 6), encryption scheme and zeroize (own decision), SSE `finish()` and lossy UTF-8 (plan 6), master key file race, database permission window, Docker `HEALTHCHECK`, stale `deployment/` files.
