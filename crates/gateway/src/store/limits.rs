//! Rate limits.

use anyhow::{anyhow, Result};
use sqlx::sqlite::{SqliteConnection, SqliteRow};
use sqlx::{AssertSqlSafe, Row};

use super::{Store, Tx, DEFAULT_ORG};
use crate::limits::{LimitScope, RateLimit};

/// A stored limit with the name of what it is set on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LimitRow {
    pub id: i64,
    pub scope: LimitScope,
    /// `None` for the gateway.
    pub scope_id: Option<i64>,
    pub limit: RateLimit,
    /// The key's or team's name, or the user's email. `None` for the
    /// gateway, and for a subject that is gone.
    pub name: Option<String>,
    /// For a key, the user that owns it.
    pub key_owner: Option<i64>,
}

impl LimitRow {
    /// How a refusal or a list names the subject.
    pub fn label(&self) -> String {
        self.scope.label(self.name.as_deref().unwrap_or(""))
    }

    /// Whether the subject exists (the gateway always does).
    pub fn has_subject(&self) -> bool {
        self.scope == LimitScope::Gateway || self.name.is_some()
    }
}

const SELECT: &str = "SELECT l.id, l.scope, l.scope_id,
            l.requests_per_minute, l.tokens_per_minute, l.concurrent,
            CASE l.scope WHEN 'key' THEN k.name WHEN 'user' THEN u.email WHEN 'team' THEN t.name END AS name,
            k.user_id AS key_owner
     FROM rate_limits l
     LEFT JOIN virtual_keys k ON l.scope = 'key' AND k.id = l.scope_id AND k.org_id = l.org_id
     LEFT JOIN users u ON l.scope = 'user' AND u.id = l.scope_id AND u.org_id = l.org_id
     LEFT JOIN teams t ON l.scope = 'team' AND t.id = l.scope_id AND t.org_id = l.org_id";

fn count(r: &SqliteRow, column: &str) -> Result<Option<u64>> {
    let value: Option<i64> = r.get(column);
    value
        .map(|v| u64::try_from(v).map_err(|_| anyhow!("stored limit is negative")))
        .transpose()
}

fn limit_from(r: &SqliteRow) -> Result<LimitRow> {
    let scope: String = r.get("scope");
    Ok(LimitRow {
        id: r.get("id"),
        scope: LimitScope::parse(&scope)
            .ok_or_else(|| anyhow!("stored limit scope is not known"))?,
        scope_id: r.get("scope_id"),
        limit: RateLimit {
            requests_per_minute: count(r, "requests_per_minute")?,
            tokens_per_minute: count(r, "tokens_per_minute")?,
            concurrent: count(r, "concurrent")?,
        },
        name: r.get("name"),
        key_owner: r.get("key_owner"),
    })
}

/// Every limit, oldest first, on the connection of a transaction.
pub(super) async fn list_limits_in(conn: &mut SqliteConnection) -> Result<Vec<LimitRow>> {
    let sql = format!("{SELECT} WHERE l.org_id = ? ORDER BY l.id");
    let rows = sqlx::query(AssertSqlSafe(sql))
        .bind(DEFAULT_ORG)
        .fetch_all(conn)
        .await?;
    rows.iter().map(limit_from).collect()
}

async fn limit_in(conn: &mut SqliteConnection, id: i64) -> Result<Option<LimitRow>> {
    let sql = format!("{SELECT} WHERE l.id = ? AND l.org_id = ?");
    let row = sqlx::query(AssertSqlSafe(sql))
        .bind(id)
        .bind(DEFAULT_ORG)
        .fetch_optional(conn)
        .await?;
    row.as_ref().map(limit_from).transpose()
}

fn as_i64(value: Option<u64>) -> Result<Option<i64>> {
    value
        .map(|v| i64::try_from(v).map_err(|_| anyhow!("limit is too large")))
        .transpose()
}

impl Store {
    /// Every limit, oldest first.
    pub async fn list_limits(&self) -> Result<Vec<LimitRow>> {
        let mut conn = self.pool().acquire().await?;
        list_limits_in(&mut conn).await
    }
}

impl Tx<'_> {
    pub async fn limit_by_id(&mut self, id: i64) -> Result<Option<LimitRow>> {
        limit_in(self.conn(), id).await
    }

    /// Sets the limits of a subject, replacing the ones it had. Returns the
    /// id of the row, which stays the same for a subject.
    pub async fn upsert_limit(
        &mut self,
        scope: LimitScope,
        scope_id: Option<i64>,
        limit: &RateLimit,
    ) -> Result<i64> {
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO rate_limits (org_id, scope, scope_id, requests_per_minute, tokens_per_minute, concurrent)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT (org_id, scope, COALESCE(scope_id, 0)) DO UPDATE SET
                 requests_per_minute = excluded.requests_per_minute,
                 tokens_per_minute = excluded.tokens_per_minute,
                 concurrent = excluded.concurrent
             RETURNING id",
        )
        .bind(DEFAULT_ORG)
        .bind(scope.as_str())
        .bind(scope_id)
        .bind(as_i64(limit.requests_per_minute)?)
        .bind(as_i64(limit.tokens_per_minute)?)
        .bind(as_i64(limit.concurrent)?)
        .fetch_one(self.conn())
        .await?;
        Ok(id)
    }

    pub async fn delete_limit(&mut self, id: i64) -> Result<bool> {
        let r = sqlx::query("DELETE FROM rate_limits WHERE id = ? AND org_id = ?")
            .bind(id)
            .bind(DEFAULT_ORG)
            .execute(self.conn())
            .await?;
        Ok(r.rows_affected() == 1)
    }
}
