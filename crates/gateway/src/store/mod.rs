//! Storage on one connection API (`sqlx::Any`), SQLite today. Nothing outside
//! this module writes SQL, and nothing in it names a database except `dialect`.

mod alerts;
mod audit;
mod backup;
mod budgets;
pub mod dialect;
mod keys;
mod limits;
mod logs;
mod models;
mod portable;
mod providers;
mod routes;
#[cfg(feature = "test-support")]
mod scratch;
mod sessions;
mod settings;
mod teams;
mod users;

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{bail, Result};
use sqlx::any::{install_default_drivers, AnyPoolOptions};
use sqlx::Any;
use sqlx::{AnyConnection, AnyPool};
use time::format_description::BorrowedFormatItem;
use time::macros::format_description;
use time::{Duration, OffsetDateTime, PrimitiveDateTime};

pub use alerts::{AlertEventRow, ChannelRow, NewAlertEvent, RuleRow, StateRow};
pub use audit::{AuditEntry, AuditRow};
pub use backup::POSTGRES_BACKUP_TEXT;
pub use budgets::{BudgetRow, UsageDelta, UsageRow, UsageTotal};
pub use dialect::Dialect;
pub(crate) use dialect::Dialected;
pub use keys::{parse_allowed, KeyRow, LiveKey};
pub use limits::LimitRow;
pub use logs::{LogDetail, LogFilter, LogRow, LogScope, NewLog, UsageGroup, UsageSums};
pub use models::{grants_of_rows, GrantRow, Grants, ModelRow};
pub use portable::ConfigState;
pub use providers::ProviderRow;
pub use routes::{is_missing_reference, RouteRow, RouteSettings, TargetRow, TargetsInput};
pub use sessions::{NewSession, SessionRow, TokenRow, SESSION_SECONDS};
pub use settings::{
    OidcSettings, DEFAULT_LOG_RETENTION_DAYS, DEFAULT_OIDC_GROUPS_CLAIM, DEFAULT_OIDC_LABEL,
    DEFAULT_SESSION_HOURS, SESSION_HOURS_RANGE,
};
pub use teams::{MemberDetail, MemberRow, TeamRow, TeamSummary, UserTeam};
pub use users::{InviteRow, NewUser, UserRow};

/// The only organisation until multi-tenancy arrives. Every query filters by it.
pub const DEFAULT_ORG: i64 = 1;

const TIMESTAMP: &[BorrowedFormatItem<'static>] =
    format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");

/// Timestamps are compared as text against each other and [`now`],
/// so anything other than exactly `YYYY-MM-DD HH:MM:SS` could compare wrongly.
pub fn check_timestamp(value: &str) -> Result<()> {
    if value.len() == 19 && PrimitiveDateTime::parse(value, TIMESTAMP).is_ok() {
        return Ok(());
    }
    bail!("timestamp must be a real UTC date and time in the form YYYY-MM-DD HH:MM:SS")
}

/// A timestamp written by [`now`], read back as a UTC time.
pub fn parse_timestamp(value: &str) -> Option<OffsetDateTime> {
    PrimitiveDateTime::parse(value, TIMESTAMP)
        .ok()
        .map(PrimitiveDateTime::assume_utc)
}

/// `at` as `YYYY-MM-DD HH:MM:SS` (UTC), the form [`now`] writes.
pub fn format_timestamp(at: OffsetDateTime) -> String {
    at.to_offset(time::UtcOffset::UTC)
        .format(TIMESTAMP)
        .expect("a UTC time formats with a fixed numeric layout")
}

/// The current UTC time as `YYYY-MM-DD HH:MM:SS`.
pub fn now() -> String {
    after(0)
}

/// The current UTC time plus `seconds`, as `YYYY-MM-DD HH:MM:SS`.
pub fn after(seconds: i64) -> String {
    (OffsetDateTime::now_utc() + Duration::seconds(seconds))
        .format(TIMESTAMP)
        .expect("a UTC time formats with a fixed numeric layout")
}

/// The advisory lock that serializes the transactions that start with
/// [`Store::begin_immediate`] on PostgreSQL.
const WRITE_LOCK: i64 = 0x5546_4741_5445_0001;

/// A file path as the path part of a `sqlite:` URL.
fn encode_path(path: &Path) -> String {
    let mut out = String::new();
    for b in path.to_string_lossy().bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(char::from(b));
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// A flag as it is stored: an integer, on every database.
pub(crate) fn flag(on: bool) -> i64 {
    i64::from(on)
}

/// The day after `day` (`YYYY-MM-DD`), or `None` when it is not a date.
pub(crate) fn next_day(day: &str) -> Option<String> {
    let date = time::Date::parse(day, format_description!("[year]-[month]-[day]")).ok()?;
    date.next_day()?
        .format(format_description!("[year]-[month]-[day]"))
        .ok()
}

/// A failure a caller can act on. Every other failure is a plain error.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("already exists")]
    Duplicate,
}

/// Turns a unique-constraint failure into `StoreError::Duplicate`.
pub(crate) fn write_error(e: sqlx::Error) -> anyhow::Error {
    match &e {
        sqlx::Error::Database(db) if Dialect::is_unique_violation(db.as_ref()) => {
            StoreError::Duplicate.into()
        }
        _ => e.into(),
    }
}

/// A database transaction. Every write goes through one, together with the
/// audit entry that records it. Dropping it without `commit` rolls back.
///
/// It holds a pool connection until it is committed or dropped. Do not call
/// a `Store` method while one is open: on a pool with a single connection
/// (the in-memory database) that call waits forever. Read what you need
/// first, then `begin`, write, and `commit`.
pub struct Tx<'c> {
    inner: sqlx::Transaction<'c, Any>,
    dialect: Dialect,
}

impl Dialected for Tx<'_> {
    fn dialect(&self) -> Dialect {
        self.dialect
    }
}

impl Tx<'_> {
    pub async fn commit(self) -> Result<()> {
        self.inner.commit().await?;
        Ok(())
    }

    pub(crate) fn conn(&mut self) -> &mut AnyConnection {
        &mut self.inner
    }
}

/// Everything the snapshot is built from, read at one moment.
pub struct SnapshotRows {
    pub keys: Vec<LiveKey>,
    pub providers: Vec<ProviderRow>,
    pub models: Vec<ModelRow>,
    pub model_grants: Vec<GrantRow>,
    pub routes: Vec<RouteRow>,
    pub route_targets: Vec<TargetRow>,
    pub route_grants: Vec<(i64, i64)>,
    pub users: Vec<UserRow>,
    pub limits: Vec<LimitRow>,
    pub budgets: Vec<BudgetRow>,
    pub teams: std::collections::HashMap<i64, Vec<UserTeam>>,
    /// `(id, created_at)` of every team.
    pub team_stamps: Vec<(i64, String)>,
}

#[derive(Clone)]
pub struct Store {
    pool: AnyPool,
    dialect: Dialect,
    /// The directory of the database file; `None` for an in-memory one.
    dir: Option<std::path::PathBuf>,
    /// How many times `teams_of_users` was called, so a test can see that
    /// a list asks once and not once per row.
    teams_of_users_calls: Arc<AtomicU64>,
    /// Drops a test's private schema when the last clone goes away.
    #[cfg(feature = "test-support")]
    scratch: Option<Arc<scratch::Schema>>,
}

impl Dialected for Store {
    fn dialect(&self) -> Dialect {
        self.dialect
    }
}

/// How long to wait for the PostgreSQL server: at start-up, and for a
/// connection while it is away.
pub const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Tries to connect until the server answers or `wait` is over. A refused
/// connection is retried (the server may be starting); any other error ends
/// it at once. Only the connection is timed, never the migrations.
async fn wait_for_server(url: &str, wait: std::time::Duration) -> Result<()> {
    use sqlx::Connection;
    let deadline = tokio::time::Instant::now() + wait;
    let mut last_refused = false;
    let mut pause = std::time::Duration::from_millis(50);
    loop {
        match tokio::time::timeout_at(deadline, AnyConnection::connect(url)).await {
            Ok(Ok(conn)) => {
                let _ = conn.close().await;
                return Ok(());
            }
            Ok(Err(sqlx::Error::Io(e))) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
                last_refused = true;
            }
            Ok(Err(e)) => return Err(e.into()),
            Err(_) => {}
        }
        if tokio::time::Instant::now() + pause >= deadline {
            bail!(
                "no answer within {} s{}",
                wait.as_secs().max(1),
                if last_refused {
                    " (the connection was refused)"
                } else {
                    ""
                }
            );
        }
        tokio::time::sleep(pause).await;
        pause = (pause * 2).min(std::time::Duration::from_secs(1));
    }
}

impl Store {
    /// Opens (creating it if needed) the SQLite database at `path`.
    pub async fn open(path: &Path) -> Result<Self> {
        // sqlx's SQLite defaults give foreign keys on and a busy timeout of
        // 5 s; WAL is set on each connection (a no-op once the file is in
        // it). `mode=rwc` creates the file.
        let url = format!("sqlite:{}?mode=rwc", encode_path(path));
        let dir = match path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent.to_path_buf(),
            _ => std::path::PathBuf::from("."),
        };
        let pool = AnyPoolOptions::new()
            .max_connections(8)
            .after_connect(|conn, _| {
                Box::pin(async move {
                    sqlx::query("PRAGMA journal_mode = WAL")
                        .execute(conn)
                        .await?;
                    Ok(())
                })
            });
        Self::connect(&url, pool, Some(dir)).await
    }

    /// One connection only: every in-memory connection is its own database,
    /// so that connection must never be reaped.
    ///
    /// With the `test-support` feature (the tests turn it on) and when
    /// `UF_TEST_DATABASE_URL` names a PostgreSQL database, this is a
    /// fresh, private schema in it instead (dropped when the store goes
    /// away), so the whole test suite can run on either database.
    pub async fn open_in_memory() -> Result<Self> {
        #[cfg(feature = "test-support")]
        if let Some(url) = scratch::test_database_url() {
            return scratch::open(&url).await;
        }
        let pool = AnyPoolOptions::new()
            .max_connections(1)
            .min_connections(1)
            .idle_timeout(None)
            .max_lifetime(None);
        Self::connect("sqlite::memory:", pool, None).await
    }

    /// Connects to the PostgreSQL database at `url` (`postgres://` or
    /// `postgresql://`) with up to `max` connections, and brings its schema
    /// up to date. The tables live in the schema the connection's
    /// `search_path` starts with (`public` unless the server says otherwise).
    ///
    /// The wait for the server is capped at [`CONNECT_TIMEOUT`]; running the
    /// migrations is not (a large one may take longer).
    pub async fn connect_url(url: &str, max: u32) -> Result<Self> {
        Self::connect_url_within(url, max, CONNECT_TIMEOUT).await
    }

    /// [`Store::connect_url`] with the cap on the wait for the server given.
    /// Nothing the server or the driver said is quoted except the last
    /// connection error of a server that refused: the URL is never in it.
    pub async fn connect_url_within(
        url: &str,
        max: u32,
        wait: std::time::Duration,
    ) -> Result<Self> {
        install_default_drivers();
        if Dialect::of_url(url) != Some(Dialect::Postgres) {
            bail!("the database URL must start with postgres:// or postgresql://");
        }
        wait_for_server(url, wait).await?;
        // A call that needs a connection while the database is away answers
        // after this, not after sqlx's 30 s default.
        let pool = AnyPoolOptions::new()
            .max_connections(max.max(1))
            .acquire_timeout(CONNECT_TIMEOUT);
        Self::connect(url, pool, None).await
    }

    async fn connect(
        url: &str,
        pool: AnyPoolOptions,
        dir: Option<std::path::PathBuf>,
    ) -> Result<Self> {
        install_default_drivers();
        let dialect = Dialect::of_url(url).ok_or_else(|| {
            anyhow::anyhow!("the database URL must start with sqlite: or postgres://")
        })?;
        let pool = pool.connect(url).await?;
        match dialect {
            Dialect::Sqlite => sqlx::migrate!("./migrations/sqlite").run(&pool).await?,
            Dialect::Postgres => sqlx::migrate!("./migrations/postgres").run(&pool).await?,
        }
        let store = Self {
            pool,
            dialect,
            dir,
            teams_of_users_calls: Arc::default(),
            #[cfg(feature = "test-support")]
            scratch: None,
        };
        // So the planner has statistics for `request_logs` from the first
        // call on (without them the lead's scope OR chose a temporary
        // b-tree sort on a fresh database).
        if let Err(e) = store.optimize().await {
            tracing::warn!(error = %e, "could not optimize the database");
        }
        Ok(store)
    }

    pub fn dialect(&self) -> Dialect {
        self.dialect
    }

    /// Lets SQLite refresh the statistics the planner uses. Cheap; it only
    /// analyzes what changed enough to matter. The log list is meant to walk
    /// the primary key downwards and stop at its limit. Other databases keep
    /// their own statistics.
    pub async fn optimize(&self) -> Result<()> {
        if self.dialect == Dialect::Sqlite {
            self.q("PRAGMA optimize").execute(&self.pool).await?;
        }
        Ok(())
    }

    /// Reads every table the snapshot needs inside one read transaction, so
    /// the rows never mix two moments.
    pub async fn snapshot_rows(&self) -> Result<SnapshotRows> {
        let mut tx = self.begin_read().await?;
        let conn: &mut AnyConnection = &mut tx;
        let keys = keys::live_keys_in(conn).await?;
        let providers = providers::list_providers_in(conn).await?;
        let models = models::list_models_in(conn).await?;
        let model_grants = models::list_model_grants_in(conn).await?;
        let routes = routes::list_routes_in(conn).await?;
        let route_targets = routes::list_route_targets_in(conn).await?;
        let route_grants = routes::list_route_grants_in(conn).await?;
        let users = users::list_users_in(conn).await?;
        let teams = teams::teams_of_all_users_in(conn).await?;
        let team_stamps = teams::team_stamps_in(conn).await?;
        let limits = limits::list_limits_in(conn).await?;
        let budgets = budgets::list_budgets_in(conn).await?;
        tx.commit().await?;
        Ok(SnapshotRows {
            keys,
            providers,
            models,
            model_grants,
            routes,
            route_targets,
            route_grants,
            users,
            limits,
            budgets,
            teams,
            team_stamps,
        })
    }

    /// How many times `teams_of_users` has been called on this store.
    pub fn teams_of_users_calls(&self) -> u64 {
        self.teams_of_users_calls.load(Ordering::Relaxed)
    }

    /// A transaction for reads that must see one moment: PostgreSQL would
    /// otherwise give each statement its own (READ COMMITTED), so a
    /// concurrent delete could show up in one table and not in another. Here
    /// it is `REPEATABLE READ, READ ONLY`. SQLite's WAL mode already holds a
    /// read transaction to the moment of its first read.
    pub(crate) async fn begin_read(&self) -> Result<sqlx::Transaction<'static, Any>> {
        let mut tx = self.pool.begin().await?;
        if self.dialect == Dialect::Postgres {
            self.q("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
                .execute(&mut *tx)
                .await?;
        }
        Ok(tx)
    }

    pub async fn begin(&self) -> Result<Tx<'_>> {
        Ok(Tx {
            inner: self.pool.begin().await?,
            dialect: self.dialect,
        })
    }

    /// Like [`Store::begin`], but takes the write lock at once (`BEGIN
    /// IMMEDIATE`). For a transaction that reads, plans and then writes: a
    /// deferred one fails with SQLITE_BUSY_SNAPSHOT, which the busy timeout
    /// does not retry, when anything else commits in between.
    pub async fn begin_immediate(&self) -> Result<Tx<'_>> {
        match self.dialect {
            Dialect::Sqlite => Ok(Tx {
                inner: self.pool.begin_with("BEGIN IMMEDIATE").await?,
                dialect: self.dialect,
            }),
            Dialect::Postgres => {
                let mut inner = self.pool.begin().await?;
                // One writer at a time, as BEGIN IMMEDIATE gives on SQLite;
                // the lock goes with the transaction. The function answers
                // void, a type the driver cannot decode, so it is not selected.
                self.q("SELECT 1 WHERE pg_advisory_xact_lock(?) IS NOT NULL")
                    .bind(WRITE_LOCK)
                    .fetch_optional(&mut *inner)
                    .await?;
                Ok(Tx {
                    inner,
                    dialect: self.dialect,
                })
            }
        }
    }

    /// Closes every connection. Every later call fails.
    pub async fn close(&self) {
        self.pool.close().await;
    }

    pub(crate) fn pool(&self) -> &AnyPool {
        &self.pool
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};

    const MALFORMED: [&str; 16] = [
        "",
        "never",
        "2999-01-01",
        "2999-01-01T00:00:00",
        "2999-01-01 00:00:00Z",
        "2999-01-01 00:00:00.000",
        " 2999-01-01 00:00:00",
        "2999-1-1 00:00:00",
        "2999-13-01 00:00:00",
        "2999-01-32 00:00:00",
        "2999-01-00 00:00:00",
        "2999-01-01 24:00:00",
        "2999-01-01 00:60:00",
        "2999-01-01 00:00:60",
        "zzzz-01-01 00:00:00",
        "٢٩٩٩-01-01 00:00:00",
    ];

    #[test]
    fn timestamp_rejects_malformed_values() {
        for bad in MALFORMED {
            assert!(check_timestamp(bad).is_err(), "accepted {bad:?}");
        }
    }

    #[test]
    fn timestamp_rejects_impossible_dates() {
        for bad in [
            "2999-02-31 00:00:00",
            "2023-02-29 00:00:00",
            "2999-04-31 00:00:00",
        ] {
            assert!(check_timestamp(bad).is_err(), "accepted {bad:?}");
        }
    }

    #[test]
    fn timestamp_accepts_boundaries() {
        for good in [
            "2999-12-31 23:59:59",
            "2024-02-29 00:00:00",
            "0001-01-01 00:00:00",
        ] {
            assert!(check_timestamp(good).is_ok(), "rejected {good:?}");
        }
    }

    #[test]
    fn now_and_after_are_well_formed() {
        assert!(check_timestamp(&now()).is_ok());
        assert!(check_timestamp(&after(3600)).is_ok());
        assert!(after(3600) > now());
        assert!(after(-1) < now());
    }

    #[tokio::test]
    async fn in_memory_database_survives_idle() {
        let s = Store::open_in_memory().await.unwrap();
        s.insert_key("k", "h", "d", None).await.unwrap();
        for _ in 0..50 {
            let conn = s.pool().acquire().await.unwrap();
            drop(conn);
            tokio::task::yield_now().await;
        }
        assert!(s.active_key_by_hash("h").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn dropped_transaction_rolls_back() {
        let s = Store::open_in_memory().await.unwrap();
        {
            let mut tx = s.begin().await.unwrap();
            tx.insert_user(NewUser {
                email: "maya@example.com",
                name: "Maya",
                role: crate::identity::Role::Admin,
                status: crate::identity::UserStatus::Active,
                password_hash: None,
            })
            .await
            .unwrap();
            tx.audit(AuditEntry {
                actor_user_id: None,
                actor_email: "maya@example.com",
                action: "user.create",
                target_type: "user",
                target_id: Some(1),
                summary: "created maya@example.com",
            })
            .await
            .unwrap();
        }
        assert_eq!(s.count_users().await.unwrap(), 0);
        assert!(s.list_audit(10, None).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn committed_transaction_keeps_change_and_audit() {
        let s = Store::open_in_memory().await.unwrap();
        let mut tx = s.begin().await.unwrap();
        let id = tx.insert_team("platform").await.unwrap();
        tx.audit(AuditEntry {
            actor_user_id: None,
            actor_email: "maya@example.com",
            action: "team.create",
            target_type: "team",
            target_id: Some(id),
            summary: "created team platform",
        })
        .await
        .unwrap();
        tx.commit().await.unwrap();
        assert!(s.team_by_id(id).await.unwrap().is_some());
        assert_eq!(s.list_audit(10, None).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn data_survives_reopen_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gateway.db");
        {
            let s = Store::open(&path).await.unwrap();
            s.insert_key("k", "h", "d", None).await.unwrap();
        }
        let s = Store::open(&path).await.unwrap();
        assert!(s.active_key_by_hash("h").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn file_database_uses_wal_and_enforces_foreign_keys() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(&dir.path().join("gateway.db")).await.unwrap();
        let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
            .fetch_one(s.pool())
            .await
            .unwrap();
        assert_eq!(mode, "wal");
        let foreign_keys: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(s.pool())
            .await
            .unwrap();
        assert_eq!(foreign_keys, 1);
        // A key whose owner does not exist is refused.
        let refused = sqlx::query(
            "INSERT INTO virtual_keys (name, key_hash, display, user_id) VALUES ('k', 'h', 'd', 999)",
        )
        .execute(s.pool())
        .await;
        assert!(refused.is_err());
    }

    #[tokio::test]
    async fn plan1_database_migrates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gateway.db");
        {
            let opts = SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true)
                .journal_mode(SqliteJournalMode::Wal)
                .foreign_keys(true);
            let pool = SqlitePoolOptions::new()
                .max_connections(1)
                .connect_with(opts)
                .await
                .unwrap();
            let mut m = sqlx::migrate!("./migrations/sqlite");
            m.migrations.to_mut().retain(|x| x.version == 1);
            assert_eq!(m.migrations.len(), 1);
            assert_eq!(
                m.migrations[0].sql.as_ref(),
                include_str!("../../migrations/sqlite/0001_init.sql")
            );
            m.run(&pool).await.unwrap();
            sqlx::query(
                "INSERT INTO providers (name, kind, base_url, credential) VALUES (?, ?, ?, ?)",
            )
            .bind("openai")
            .bind("openai")
            .bind("https://api.openai.com/v1")
            .bind(Some(&b"enc"[..]))
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query("INSERT INTO virtual_keys (name, key_hash, display) VALUES (?, ?, ?)")
                .bind("old")
                .bind("h")
                .bind("uf-sk-…aaaa")
                .execute(&pool)
                .await
                .unwrap();
            pool.close().await;
        }

        let s = Store::open(&path).await.unwrap();
        let p = s.provider_by_name("openai").await.unwrap().unwrap();
        assert_eq!(p.base_url, "https://api.openai.com/v1");
        assert_eq!(p.credential.as_deref(), Some(&b"enc"[..]));
        let k = s.active_key_by_hash("h").await.unwrap().unwrap();
        assert_eq!(k.name, "old");
        assert_eq!(k.user_id, None);
        assert_eq!(k.team_id, None);
    }

    /// `(table, column)` of every column and the name of every named index,
    /// from the catalog of the database behind `s`.
    async fn shape_of(
        s: &Store,
    ) -> (
        std::collections::BTreeSet<String>,
        std::collections::BTreeSet<String>,
    ) {
        let (columns, indexes) = match s.dialect() {
            Dialect::Sqlite => (
                "SELECT m.name || '.' || p.name FROM sqlite_master m, pragma_table_info(m.name) p
                 WHERE m.type = 'table' AND m.name NOT LIKE 'sqlite_%' AND m.name <> '_sqlx_migrations'",
                "SELECT name FROM sqlite_master WHERE type = 'index' AND name NOT LIKE 'sqlite_%'",
            ),
            Dialect::Postgres => (
                "SELECT table_name || '.' || column_name FROM information_schema.columns
                 WHERE table_schema = current_schema() AND table_name <> '_sqlx_migrations'",
                "SELECT CAST(c.relname AS TEXT) FROM pg_index i
                 JOIN pg_class c ON c.oid = i.indexrelid
                 JOIN pg_namespace n ON n.oid = c.relnamespace
                 WHERE n.nspname = current_schema()
                   AND NOT EXISTS (SELECT 1 FROM pg_constraint k WHERE k.conindid = i.indexrelid)"
            ),
        };
        let columns: Vec<String> = s.scalar(columns).fetch_all(s.pool()).await.unwrap();
        let indexes: Vec<String> = s.scalar(indexes).fetch_all(s.pool()).await.unwrap();
        (columns.into_iter().collect(), indexes.into_iter().collect())
    }

    /// The PostgreSQL baseline plus its later migrations are the SQLite migrations: the same
    /// tables, columns and named indexes (PostgreSQL adds `route_grants.seq`,
    /// what SQLite's rowid is). A later migration must go into both.
    #[tokio::test]
    async fn the_postgres_baseline_has_the_tables_of_the_sqlite_migrations() {
        if scratch::test_database_url().is_none() {
            eprintln!("SKIPPED without UF_TEST_DATABASE_URL: nothing to compare with");
            return;
        }
        let pg = Store::open_in_memory().await.unwrap();
        assert_eq!(pg.dialect(), Dialect::Postgres);
        let dir = tempfile::tempdir().unwrap();
        let lite = Store::open(&dir.path().join("gateway.db")).await.unwrap();
        let (mut pg_columns, pg_indexes) = shape_of(&pg).await;
        let (lite_columns, lite_indexes) = shape_of(&lite).await;
        assert!(pg_columns.remove("route_grants.seq"));
        assert_eq!(pg_columns, lite_columns);
        assert_eq!(pg_indexes, lite_indexes);
        assert!(
            lite_columns.contains("users.external_id") && lite_indexes.contains("users_external")
        );
    }

    #[tokio::test]
    async fn connect_url_takes_only_postgres_urls_and_lands_in_the_url_schema() {
        let e = Store::connect_url("sqlite::memory:", 2)
            .await
            .err()
            .unwrap();
        assert!(e.to_string().contains("postgres"), "{e}");
        let e = Store::connect_url("mysql://x/y", 2).await.err().unwrap();
        assert!(e.to_string().contains("postgres"), "{e}");
        let Some(base) = scratch::test_database_url() else {
            eprintln!("SKIPPED without UF_TEST_DATABASE_URL: no PostgreSQL to connect to");
            return;
        };
        // An own schema, chosen the way an operator can: by the URL.
        let admin = Store::open_in_memory().await.unwrap();
        let schema = format!("t_url_{}", std::process::id());
        admin
            .q_dyn(format!("DROP SCHEMA IF EXISTS {schema} CASCADE"))
            .execute(admin.pool())
            .await
            .unwrap();
        admin
            .q_dyn(format!("CREATE SCHEMA {schema}"))
            .execute(admin.pool())
            .await
            .unwrap();
        let sep = if base.contains('?') { '&' } else { '?' };
        let url = format!("{base}{sep}options=-c%20search_path%3D{schema}");
        let store = Store::connect_url(&url, 3).await.unwrap();
        assert_eq!(
            store.pool().options().get_acquire_timeout(),
            CONNECT_TIMEOUT
        );
        store.insert_key("k", "h", "d", None).await.unwrap();
        assert!(store.active_key_by_hash("h").await.unwrap().is_some());
        let tables: i64 = admin
            .scalar_dyn(format!(
                "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema = '{schema}' AND table_name = 'virtual_keys'"
            ))
            .fetch_one(admin.pool())
            .await
            .unwrap();
        assert_eq!(tables, 1, "the tables are in the schema of the URL");
        // Opening it again changes nothing (the migrations are recorded).
        let again = Store::connect_url(&url, 3).await.unwrap();
        assert!(again.active_key_by_hash("h").await.unwrap().is_some());
        admin
            .q_dyn(format!("DROP SCHEMA {schema} CASCADE"))
            .execute(admin.pool())
            .await
            .unwrap();
    }

    /// The wait for the server is capped; the migrations are not: a run held
    /// up longer than the cap by a lock on its own table still finishes.
    #[tokio::test]
    async fn a_slow_migration_is_not_cut_off_by_the_connect_cap() {
        use sqlx::Connection;
        let Some(base) = scratch::test_database_url() else {
            eprintln!("SKIPPED without UF_TEST_DATABASE_URL: no PostgreSQL to connect to");
            return;
        };
        let admin = Store::open_in_memory().await.unwrap();
        let schema = format!("t_slow_{}", std::process::id());
        for sql in [
            format!("DROP SCHEMA IF EXISTS {schema} CASCADE"),
            format!("CREATE SCHEMA {schema}"),
        ] {
            admin.q_dyn(sql).execute(admin.pool()).await.unwrap();
        }
        let sep = if base.contains('?') { '&' } else { '?' };
        let url = format!("{base}{sep}options=-c%20search_path%3D{schema}");
        drop(Store::connect_url(&url, 2).await.unwrap());
        // Another session holds the migration table for 3 s.
        let mut holder = sqlx::postgres::PgConnection::connect(&url).await.unwrap();
        sqlx::query("BEGIN").execute(&mut holder).await.unwrap();
        sqlx::query("LOCK TABLE _sqlx_migrations IN ACCESS EXCLUSIVE MODE")
            .execute(&mut holder)
            .await
            .unwrap();
        let release = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            sqlx::query("COMMIT").execute(&mut holder).await.unwrap();
        });
        let started = std::time::Instant::now();
        let store = Store::connect_url_within(&url, 2, std::time::Duration::from_secs(1))
            .await
            .expect("the cap is for the connection, not the migrations");
        assert!(started.elapsed() >= std::time::Duration::from_secs(2));
        drop(store);
        release.await.unwrap();
        admin
            .q_dyn(format!("DROP SCHEMA {schema} CASCADE"))
            .execute(admin.pool())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_refused_port_is_reported_with_the_wait() {
        let e = Store::connect_url_within(
            "postgres://u:hunter2-secret@127.0.0.1:1/db",
            2,
            std::time::Duration::from_secs(1),
        )
        .await
        .err()
        .unwrap();
        let text = format!("{e:#}");
        assert!(text.contains("within 1 s"), "{text}");
        assert!(text.contains("refused"), "{text}");
        assert!(!text.contains("hunter2"), "{text}");
    }

    /// A read transaction sees one moment: what another session commits
    /// meanwhile is not in its later reads (on PostgreSQL this is
    /// REPEATABLE READ, READ ONLY; on SQLite the WAL snapshot).
    #[tokio::test]
    async fn a_read_transaction_sees_one_moment() {
        let dir = tempfile::tempdir().unwrap();
        let store = if scratch::test_database_url().is_some() {
            Store::open_in_memory().await.unwrap()
        } else {
            // An in-memory SQLite store has one connection only.
            Store::open(&dir.path().join("gateway.db")).await.unwrap()
        };
        async fn count(store: &Store, tx: &mut sqlx::Transaction<'static, Any>) -> i64 {
            store
                .scalar("SELECT COUNT(*) FROM teams")
                .fetch_one(&mut **tx)
                .await
                .unwrap()
        }
        let mut tx = store.begin_read().await.unwrap();
        let before: i64 = count(&store, &mut tx).await;
        let mut other = store.begin().await.unwrap();
        other.insert_team("Late").await.unwrap();
        other.commit().await.unwrap();
        assert_eq!(
            count(&store, &mut tx).await,
            before,
            "the later commit is not seen"
        );
        if store.dialect() == Dialect::Postgres {
            let iso: String = store
                .scalar("SHOW transaction_isolation")
                .fetch_one(&mut *tx)
                .await
                .unwrap();
            assert_eq!(iso, "repeatable read");
            let write = store
                .q("INSERT INTO teams (org_id, name) VALUES (1, 'No')")
                .execute(&mut *tx)
                .await;
            assert!(write.is_err(), "the transaction is read only");
        }
        drop(tx);
        // And it is the transaction the snapshot and the export read in.
        assert_eq!(
            store.snapshot_rows().await.unwrap().team_stamps.len() as i64,
            before + 1
        );
        let fresh = store.begin_read().await.unwrap();
        drop(fresh);
    }
}
