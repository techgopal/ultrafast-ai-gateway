//! SQLite storage. Nothing outside this module writes SQL.

mod audit;
mod keys;
mod logs;
mod models;
mod providers;
mod routes;
mod sessions;
mod settings;
mod teams;
mod users;

use std::path::Path;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{bail, Result};
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteConnection, SqliteJournalMode, SqlitePool, SqlitePoolOptions,
};
use sqlx::Sqlite;
use time::format_description::BorrowedFormatItem;
use time::macros::format_description;
use time::{Duration, OffsetDateTime, PrimitiveDateTime};

pub use audit::{AuditEntry, AuditRow};
pub use keys::{parse_allowed, KeyRow, LiveKey};
pub use logs::{LogDetail, LogFilter, LogRow, LogScope, NewLog, UsageGroup, UsageSums};
pub use models::{grants_of_rows, GrantRow, Grants, ModelRow};
pub use providers::ProviderRow;
pub use routes::{is_missing_reference, RouteRow, RouteSettings, TargetRow, TargetsInput};
pub use sessions::{NewSession, SessionRow, TokenRow, SESSION_SECONDS};
pub use settings::DEFAULT_LOG_RETENTION_DAYS;
pub use teams::{MemberDetail, MemberRow, TeamRow, TeamSummary, UserTeam};
pub use users::{InviteRow, NewUser, UserRow};

/// The only organisation until multi-tenancy arrives. Every query filters by it.
pub const DEFAULT_ORG: i64 = 1;

const TIMESTAMP: &[BorrowedFormatItem<'static>] =
    format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");

/// Timestamps are compared as text against each other and `datetime('now')`,
/// so anything other than exactly `YYYY-MM-DD HH:MM:SS` could compare wrongly.
pub fn check_timestamp(value: &str) -> Result<()> {
    if value.len() == 19 && PrimitiveDateTime::parse(value, TIMESTAMP).is_ok() {
        return Ok(());
    }
    bail!("timestamp must be a real UTC date and time in the form YYYY-MM-DD HH:MM:SS")
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

/// A failure a caller can act on. Every other failure is a plain error.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("already exists")]
    Duplicate,
}

/// Turns a unique-constraint failure into `StoreError::Duplicate`.
pub(crate) fn write_error(e: sqlx::Error) -> anyhow::Error {
    match &e {
        sqlx::Error::Database(db) if db.is_unique_violation() => StoreError::Duplicate.into(),
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
    inner: sqlx::Transaction<'c, Sqlite>,
}

impl Tx<'_> {
    pub async fn commit(self) -> Result<()> {
        self.inner.commit().await?;
        Ok(())
    }

    pub(crate) fn conn(&mut self) -> &mut SqliteConnection {
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
    pub teams: std::collections::HashMap<i64, Vec<UserTeam>>,
}

#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
    /// How many times `teams_of_users` was called, so a test can see that
    /// a list asks once and not once per row.
    teams_of_users_calls: Arc<AtomicU64>,
}

impl Store {
    pub async fn open(path: &Path) -> Result<Self> {
        let opts = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .foreign_keys(true);
        Self::connect(opts, SqlitePoolOptions::new().max_connections(8)).await
    }

    /// One connection only: every in-memory connection is its own database,
    /// so that connection must never be reaped.
    pub async fn open_in_memory() -> Result<Self> {
        let opts = SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .min_connections(1)
            .idle_timeout(None)
            .max_lifetime(None);
        Self::connect(opts, pool).await
    }

    async fn connect(opts: SqliteConnectOptions, pool: SqlitePoolOptions) -> Result<Self> {
        let pool = pool.connect_with(opts).await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        let store = Self {
            pool,
            teams_of_users_calls: Arc::default(),
        };
        // So the planner has statistics for `request_logs` from the first
        // call on (without them the lead's scope OR chose a temporary
        // b-tree sort on a fresh database).
        store.optimize().await?;
        Ok(store)
    }

    /// Lets SQLite refresh the statistics the planner uses. Cheap; it only
    /// analyzes what changed enough to matter. The log list is meant to walk
    /// the primary key downwards and stop at its limit.
    pub async fn optimize(&self) -> Result<()> {
        sqlx::query("PRAGMA optimize").execute(&self.pool).await?;
        Ok(())
    }

    /// Reads every table the snapshot needs inside one read transaction, so
    /// the rows never mix two moments.
    pub async fn snapshot_rows(&self) -> Result<SnapshotRows> {
        let mut tx = self.pool.begin().await?;
        let conn: &mut SqliteConnection = &mut tx;
        let keys = keys::live_keys_in(conn).await?;
        let providers = providers::list_providers_in(conn).await?;
        let models = models::list_models_in(conn).await?;
        let model_grants = models::list_model_grants_in(conn).await?;
        let routes = routes::list_routes_in(conn).await?;
        let route_targets = routes::list_route_targets_in(conn).await?;
        let route_grants = routes::list_route_grants_in(conn).await?;
        let users = users::list_users_in(conn).await?;
        let ids: Vec<i64> = users.iter().map(|u| u.id).collect();
        let teams = teams::teams_of_users_in(conn, &ids).await?;
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
            teams,
        })
    }

    /// How many times `teams_of_users` has been called on this store.
    pub fn teams_of_users_calls(&self) -> u64 {
        self.teams_of_users_calls.load(Ordering::Relaxed)
    }

    pub async fn begin(&self) -> Result<Tx<'_>> {
        Ok(Tx {
            inner: self.pool.begin().await?,
        })
    }

    /// Closes every connection. Every later call fails.
    pub async fn close(&self) {
        self.pool.close().await;
    }

    pub(crate) fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            let mut m = sqlx::migrate!("./migrations");
            m.migrations.to_mut().retain(|x| x.version == 1);
            assert_eq!(m.migrations.len(), 1);
            assert_eq!(
                m.migrations[0].sql.as_ref(),
                include_str!("../../migrations/0001_init.sql")
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
}
