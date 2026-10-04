//! Settings of the gateway: a small key-value table.

use anyhow::Result;
use sqlx::sqlite::SqliteConnection;

use super::{Store, Tx};

const LOG_RETENTION_DAYS: &str = "log_retention_days";
/// Used when the row is missing or unreadable.
pub const DEFAULT_LOG_RETENTION_DAYS: i64 = 30;

const SESSION_HOURS: &str = "session_hours";
/// How long a session lives when the setting is missing or unreadable.
pub const DEFAULT_SESSION_HOURS: i64 = 12;
/// The fewest and the most hours a session may live.
pub const SESSION_HOURS_RANGE: std::ops::RangeInclusive<i64> = 1..=720;

/// How many days request logs are kept, on the connection of a transaction.
pub(super) async fn log_retention_days_in(conn: &mut SqliteConnection) -> Result<i64> {
    let value: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
        .bind(LOG_RETENTION_DAYS)
        .fetch_optional(conn)
        .await?;
    Ok(value
        .and_then(|v| v.parse::<i64>().ok())
        .filter(|days| *days >= 1)
        .unwrap_or(DEFAULT_LOG_RETENTION_DAYS))
}

/// How many hours a new session lives, on the connection of a transaction.
pub(super) async fn session_hours_in(conn: &mut SqliteConnection) -> Result<i64> {
    let value: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
        .bind(SESSION_HOURS)
        .fetch_optional(conn)
        .await?;
    Ok(value
        .and_then(|v| v.parse::<i64>().ok())
        .filter(|hours| SESSION_HOURS_RANGE.contains(hours))
        .unwrap_or(DEFAULT_SESSION_HOURS))
}

impl Store {
    /// How many hours a new session lives.
    pub async fn session_hours(&self) -> Result<i64> {
        let mut conn = self.pool().acquire().await?;
        session_hours_in(&mut conn).await
    }

    /// How many days request logs are kept.
    pub async fn log_retention_days(&self) -> Result<i64> {
        let mut conn = self.pool().acquire().await?;
        log_retention_days_in(&mut conn).await
    }
}

impl Tx<'_> {
    /// How many hours a session made in this transaction lives.
    pub async fn session_hours(&mut self) -> Result<i64> {
        session_hours_in(self.conn()).await
    }

    pub async fn set_session_hours(&mut self, hours: i64) -> Result<()> {
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES (?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(SESSION_HOURS)
        .bind(hours.to_string())
        .execute(self.conn())
        .await?;
        Ok(())
    }

    pub async fn set_log_retention_days(&mut self, days: i64) -> Result<()> {
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES (?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(LOG_RETENTION_DAYS)
        .bind(days.to_string())
        .execute(self.conn())
        .await?;
        Ok(())
    }
}
