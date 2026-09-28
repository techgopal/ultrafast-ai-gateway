//! SQLite storage. Nothing outside this module writes SQL.

mod audit;
mod keys;
mod providers;
mod teams;
mod users;

use std::path::Path;
use std::str::FromStr;

use anyhow::{bail, Result};
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteConnection, SqliteJournalMode, SqlitePool, SqlitePoolOptions,
};
use sqlx::Sqlite;
use time::format_description::BorrowedFormatItem;
use time::macros::format_description;
use time::{Duration, OffsetDateTime, PrimitiveDateTime};

pub use audit::{AuditEntry, AuditRow};
pub use keys::KeyRow;
pub use providers::ProviderRow;
pub use teams::{MemberRow, TeamRow};
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

#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
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
        Ok(Self { pool })
    }

    pub async fn begin(&self) -> Result<Tx<'_>> {
        Ok(Tx {
            inner: self.pool.begin().await?,
        })
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
