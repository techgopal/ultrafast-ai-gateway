//! Request logs.

use anyhow::Result;
use sqlx::sqlite::SqliteRow;
use sqlx::{AssertSqlSafe, Row};

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

/// A stored row with the names of its key, user and team, which are `None`
/// when the object is gone or the row never had one.
#[derive(Debug, Clone, PartialEq)]
pub struct LogDetail {
    pub row: LogRow,
    pub key_name: Option<String>,
    pub user_email: Option<String>,
    pub team_name: Option<String>,
}

/// Which rows a reader may see, before any filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogScope {
    All,
    /// Rows of these teams, of their members, and of `own_user_id`.
    Teams {
        team_ids: Vec<i64>,
        own_user_id: i64,
    },
    Own {
        user_id: i64,
    },
}

/// Filters of a list. They only narrow the scope. `from` and `to` are in
/// the form of `store::now` and both inclusive.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogFilter {
    pub before: Option<i64>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub key_id: Option<i64>,
    pub user_id: Option<i64>,
    pub team_id: Option<i64>,
    /// Matches the model that answered or the name that was asked for.
    pub model: Option<String>,
    pub status: Option<i64>,
}

const DETAIL_SELECT: &str = "SELECT l.*, k.name AS key_name, u.email AS user_email,
            t.name AS team_name
     FROM request_logs l
     LEFT JOIN virtual_keys k ON k.id = l.key_id AND k.org_id = l.org_id
     LEFT JOIN users u ON u.id = l.user_id AND u.org_id = l.org_id
     LEFT JOIN teams t ON t.id = l.team_id AND t.org_id = l.org_id";

fn detail_from(r: &SqliteRow) -> LogDetail {
    LogDetail {
        row: log_from(r),
        key_name: r.get("key_name"),
        user_email: r.get("user_email"),
        team_name: r.get("team_name"),
    }
}

/// The scope as SQL over `l`, with the values to bind in order.
fn scope_sql(scope: &LogScope) -> (String, Vec<i64>) {
    match scope {
        LogScope::All => ("1 = 1".into(), vec![]),
        LogScope::Own { user_id } => ("l.user_id = ?".into(), vec![*user_id]),
        LogScope::Teams {
            team_ids,
            own_user_id,
        } => {
            let marks = vec!["?"; team_ids.len()].join(", ");
            let marks = if marks.is_empty() { "NULL" } else { &marks };
            // Members come from one subquery, not one lookup per row.
            let sql = format!(
                "(l.user_id = ? OR l.team_id IN ({marks}) OR l.user_id IN
                  (SELECT m.user_id FROM team_members m
                   WHERE m.org_id = l.org_id AND m.team_id IN ({marks})))"
            );
            let mut binds = vec![*own_user_id];
            binds.extend(team_ids);
            binds.extend(team_ids);
            (sql, binds)
        }
    }
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

    /// The rows in `scope` that pass `filter`, newest first by id. `limit` is
    /// clamped to 1..=200.
    pub async fn list_logs(
        &self,
        scope: &LogScope,
        filter: &LogFilter,
        limit: i64,
    ) -> Result<Vec<LogDetail>> {
        let (scope_clause, scope_ints) = scope_sql(scope);
        let mut clauses = vec!["l.org_id = ?".to_string(), scope_clause];
        let mut ints = vec![DEFAULT_ORG];
        ints.extend(scope_ints);
        // Placeholders and binds must agree in order: every integer
        // clause is added before any text clause.
        for (clause, value) in [
            ("l.id < ?", filter.before),
            ("l.key_id = ?", filter.key_id),
            ("l.user_id = ?", filter.user_id),
            ("l.team_id = ?", filter.team_id),
            ("l.status = ?", filter.status),
        ] {
            if let Some(v) = value {
                clauses.push(clause.to_string());
                ints.push(v);
            }
        }
        let mut text_values: Vec<&str> = Vec::new();
        if let Some(v) = &filter.from {
            clauses.push("l.at >= ?".into());
            text_values.push(v);
        }
        if let Some(v) = &filter.to {
            clauses.push("l.at <= ?".into());
            text_values.push(v);
        }
        if let Some(v) = &filter.model {
            clauses.push("(l.model = ? OR l.requested = ?)".into());
            text_values.push(v);
            text_values.push(v);
        }
        let sql = format!(
            "{DETAIL_SELECT} WHERE {} ORDER BY l.id DESC LIMIT ?",
            clauses.join(" AND ")
        );
        let mut query = sqlx::query(AssertSqlSafe(sql));
        for v in &ints {
            query = query.bind(*v);
        }
        for v in text_values {
            query = query.bind(v);
        }
        let rows = query
            .bind(limit.clamp(1, 200))
            .fetch_all(self.pool())
            .await?;
        Ok(rows.iter().map(detail_from).collect())
    }

    /// One row with its names, whatever its scope. The caller decides
    /// whether the reader may see it.
    pub async fn log_by_id(&self, id: i64) -> Result<Option<LogDetail>> {
        let sql = format!("{DETAIL_SELECT} WHERE l.org_id = ? AND l.id = ?");
        let row = sqlx::query(AssertSqlSafe(sql))
            .bind(DEFAULT_ORG)
            .bind(id)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.as_ref().map(detail_from))
    }

    /// Whether `user_id` is a member of any of the teams.
    pub async fn is_member_of_any(&self, user_id: i64, team_ids: &[i64]) -> Result<bool> {
        if team_ids.is_empty() {
            return Ok(false);
        }
        let marks = vec!["?"; team_ids.len()].join(", ");
        let sql = format!(
            "SELECT EXISTS (SELECT 1 FROM team_members
             WHERE org_id = ? AND user_id = ? AND team_id IN ({marks}))"
        );
        let mut query = sqlx::query_scalar(AssertSqlSafe(sql))
            .bind(DEFAULT_ORG)
            .bind(user_id);
        for t in team_ids {
            query = query.bind(*t);
        }
        Ok(query.fetch_one(self.pool()).await?)
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
