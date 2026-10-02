//! Request logs.

use anyhow::Result;
use sqlx::sqlite::SqliteRow;
use sqlx::Row;

use super::{Store, DEFAULT_ORG};

/// A row to be written.
#[derive(Debug, Clone, PartialEq)]
pub struct NewLog {
    pub at: String,
    pub key_id: Option<i64>,
    pub user_id: Option<i64>,
    pub team_id: Option<i64>,
    pub requested: String,
    pub endpoint: String,
    pub stream: bool,
    pub status: i64,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cost_micros: i64,
    pub priced: bool,
    pub cached: bool,
    pub duration_ms: i64,
    /// A JSON array.
    pub attempts: String,
}

/// A stored row.
#[derive(Debug, Clone, PartialEq)]
pub struct LogRow {
    pub id: i64,
    pub at: String,
    pub key_id: Option<i64>,
    pub user_id: Option<i64>,
    pub team_id: Option<i64>,
    pub requested: String,
    pub endpoint: String,
    pub stream: bool,
    pub status: i64,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cost_micros: i64,
    pub priced: bool,
    pub cached: bool,
    pub duration_ms: i64,
    pub attempts: String,
}

fn log_from(r: &SqliteRow) -> LogRow {
    LogRow {
        id: r.get("id"),
        at: r.get("at"),
        key_id: r.get("key_id"),
        user_id: r.get("user_id"),
        team_id: r.get("team_id"),
        requested: r.get("requested"),
        endpoint: r.get("endpoint"),
        stream: r.get::<i64, _>("stream") != 0,
        status: r.get("status"),
        provider: r.get("provider"),
        model: r.get("model"),
        input_tokens: r.get("input_tokens"),
        output_tokens: r.get("output_tokens"),
        cost_micros: r.get("cost_micros"),
        priced: r.get::<i64, _>("priced") != 0,
        cached: r.get::<i64, _>("cached") != 0,
        duration_ms: r.get("duration_ms"),
        attempts: r.get("attempts"),
    }
}

impl Store {
    /// Writes the rows in one transaction.
    pub async fn insert_logs(&self, rows: &[NewLog]) -> Result<()> {
        let mut tx = self.pool().begin().await?;
        for r in rows {
            sqlx::query(
                "INSERT INTO request_logs
                 (org_id, at, key_id, user_id, team_id, requested, endpoint, stream, status,
                  provider, model, input_tokens, output_tokens, cost_micros, priced, cached,
                  duration_ms, attempts)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(DEFAULT_ORG)
            .bind(&r.at)
            .bind(r.key_id)
            .bind(r.user_id)
            .bind(r.team_id)
            .bind(&r.requested)
            .bind(&r.endpoint)
            .bind(r.stream)
            .bind(r.status)
            .bind(&r.provider)
            .bind(&r.model)
            .bind(r.input_tokens)
            .bind(r.output_tokens)
            .bind(r.cost_micros)
            .bind(r.priced)
            .bind(r.cached)
            .bind(r.duration_ms)
            .bind(&r.attempts)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// The newest rows first. `limit` is clamped to 1..=200.
    pub async fn recent_logs(&self, limit: i64) -> Result<Vec<LogRow>> {
        let rows =
            sqlx::query("SELECT * FROM request_logs WHERE org_id = ? ORDER BY id DESC LIMIT ?")
                .bind(DEFAULT_ORG)
                .bind(limit.clamp(1, 200))
                .fetch_all(self.pool())
                .await?;
        Ok(rows.iter().map(log_from).collect())
    }

    /// Deletes up to `limit` rows older than `cutoff` (`at < cutoff`, in the
    /// form of `store::now`) and returns how many went.
    pub async fn delete_logs_before(&self, cutoff: &str, limit: i64) -> Result<u64> {
        let r = sqlx::query(
            "DELETE FROM request_logs WHERE id IN
             (SELECT id FROM request_logs WHERE org_id = ? AND at < ? ORDER BY id LIMIT ?)",
        )
        .bind(DEFAULT_ORG)
        .bind(cutoff)
        .bind(limit)
        .execute(self.pool())
        .await?;
        Ok(r.rows_affected())
    }
}
