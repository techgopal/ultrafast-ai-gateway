//! Settings of the gateway: a small key-value table.

use anyhow::Result;

use super::{Store, Tx};

const LOG_RETENTION_DAYS: &str = "log_retention_days";
/// Used when the row is missing or unreadable.
pub const DEFAULT_LOG_RETENTION_DAYS: i64 = 30;

impl Store {
    /// How many days request logs are kept.
    pub async fn log_retention_days(&self) -> Result<i64> {
        let value: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
            .bind(LOG_RETENTION_DAYS)
            .fetch_optional(self.pool())
            .await?;
        Ok(value
            .and_then(|v| v.parse::<i64>().ok())
            .filter(|days| *days >= 1)
            .unwrap_or(DEFAULT_LOG_RETENTION_DAYS))
    }
}

impl Tx<'_> {
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
