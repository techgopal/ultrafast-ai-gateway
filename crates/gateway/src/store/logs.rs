//! Request logs.

use anyhow::Result;
use sqlx::any::AnyRow;
use sqlx::Row;

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use super::dialect::{Dialect, Dialected};
use super::{flag, next_day, Store, DEFAULT_ORG};

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
    /// The tokens and cost are an estimate (a stream that ended without the
    /// provider's usage report).
    pub estimated: bool,
    pub duration_ms: i64,
    /// A JSON array.
    pub attempts: String,
    /// A JSON object of strings; `None` for no tags.
    pub tags: Option<String>,
    /// What the guardrails found (a JSON object: guardrail ids and names,
    /// actions, counts; never matched text); `None` when they found nothing.
    pub guardrails: Option<String>,
    /// The prompt template the call used, as `name@version`; text, so it
    /// outlives the template.
    pub prompt: Option<String>,
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
    /// The tokens and cost are an estimate (a stream that ended without the
    /// provider's usage report).
    pub estimated: bool,
    pub duration_ms: i64,
    pub attempts: String,
    /// A JSON object of strings; `None` for no tags.
    pub tags: Option<String>,
    /// See [`NewLog::guardrails`].
    pub guardrails: Option<String>,
    /// See [`NewLog::prompt`].
    pub prompt: Option<String>,
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
    /// Only calls on this endpoint (`chat`, `images`, ...), as logged.
    pub endpoint: Option<String>,
    /// Only calls answered with status 400 or more.
    pub errors: bool,
    /// Only calls that carry every one of these tags (name, value). The
    /// names are checked by the caller.
    pub tags: Vec<(String, String)>,
    /// Only calls whose worst guardrail action was this one (a block is worse
    /// than a redaction, a redaction worse than a flag), in either direction.
    pub guardrail: Option<crate::guardrails::log::LoggedAction>,
}

/// What `usage` groups by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsageGroup {
    Day,
    Model,
    Key,
    User,
    Team,
    /// The value of the tag with this name; calls without it are `(none)`.
    Tag(String),
}

/// Sums over the rows of one group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageSums {
    /// The day, the model name, or the id of the key, user or team;
    /// empty for rows that have none.
    pub group: String,
    /// What to show for the group: the name, `(none)` for rows without an
    /// owner, `(deleted)` when the object is gone.
    pub label: String,
    pub requests: i64,
    /// Calls answered with 400 or more, except 499 (the caller went away).
    pub errors: i64,
    /// Calls the caller abandoned (status 499).
    pub cancelled: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cost_micros: i64,
    pub unpriced_requests: i64,
}

const DETAIL_SELECT: &str = "SELECT l.*, k.name AS key_name, u.email AS user_email,
            t.name AS team_name
     FROM request_logs l
     LEFT JOIN virtual_keys k ON k.id = l.key_id AND k.org_id = l.org_id
     LEFT JOIN users u ON u.id = l.user_id AND u.org_id = l.org_id
     LEFT JOIN teams t ON t.id = l.team_id AND t.org_id = l.org_id";

fn detail_from(r: &AnyRow) -> LogDetail {
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
            // Members come from one subquery that does not depend on the
            // row (the org is a constant), so SQLite runs it once, not per row.
            let sql = format!(
                "(l.user_id = ? OR l.team_id IN ({marks}) OR l.user_id IN
                  (SELECT m.user_id FROM team_members m
                   WHERE m.org_id = {DEFAULT_ORG} AND m.team_id IN ({marks})))"
            );
            let mut binds = vec![*own_user_id];
            binds.extend(team_ids);
            binds.extend(team_ids);
            (sql, binds)
        }
    }
}

fn log_from(r: &AnyRow) -> LogRow {
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
        estimated: r.get::<i64, _>("estimated") != 0,
        duration_ms: r.get("duration_ms"),
        attempts: r.get("attempts"),
        tags: r.get("tags"),
        guardrails: r.get("guardrails"),
        prompt: r.get("prompt"),
    }
}

/// Rows per `INSERT`: 22 binds each, so a chunk stays far under the 32766
/// (SQLite) and 65535 (PostgreSQL) parameter limits.
const LOG_INSERT_CHUNK: usize = 1000;

/// The `INSERT` for `rows` rows, as the driver takes it. Built once per
/// dialect and size: a flush is mostly full chunks of one size.
fn log_insert_sql(dialect: Dialect, rows: usize) -> String {
    static CACHE: OnceLock<Mutex<HashMap<(Dialect, usize), String>>> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    cache
        .entry((dialect, rows))
        .or_insert_with(|| {
            let mut sql = String::from(
                "INSERT INTO request_logs
                 (org_id, at, key_id, user_id, team_id, requested, endpoint, stream, status,
                  provider, model, input_tokens, output_tokens, cost_micros, priced, cached,
                  estimated, duration_ms, attempts, tags, guardrails, prompt) VALUES ",
            );
            for i in 0..rows {
                if i > 0 {
                    sql.push_str(", ");
                }
                sql.push_str("(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)");
            }
            dialect.sql(&sql).into_owned()
        })
        .clone()
}

impl Store {
    /// Writes the rows in one transaction, a multi-row `INSERT` per chunk of
    /// at most [`LOG_INSERT_CHUNK`] rows (one round trip each, which matters
    /// when the database is a network away).
    pub async fn insert_logs(&self, rows: &[NewLog]) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool().begin().await?;
        for chunk in rows.chunks(LOG_INSERT_CHUNK) {
            let sql = log_insert_sql(self.dialect(), chunk.len());
            let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
            for r in chunk {
                query = query
                    .bind(DEFAULT_ORG)
                    .bind(&r.at)
                    .bind(r.key_id)
                    .bind(r.user_id)
                    .bind(r.team_id)
                    .bind(&r.requested)
                    .bind(&r.endpoint)
                    .bind(flag(r.stream))
                    .bind(r.status)
                    .bind(&r.provider)
                    .bind(&r.model)
                    .bind(r.input_tokens)
                    .bind(r.output_tokens)
                    .bind(r.cost_micros)
                    .bind(flag(r.priced))
                    .bind(flag(r.cached))
                    .bind(flag(r.estimated))
                    .bind(r.duration_ms)
                    .bind(&r.attempts)
                    .bind(&r.tags)
                    .bind(&r.guardrails)
                    .bind(&r.prompt);
            }
            query.execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// The newest rows first. `limit` is clamped to 1..=200.
    pub async fn recent_logs(&self, limit: i64) -> Result<Vec<LogRow>> {
        let rows = self
            .q("SELECT * FROM request_logs WHERE org_id = ? ORDER BY id DESC LIMIT ?")
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
        if filter.errors {
            clauses.push("l.status >= 400".into());
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
        if let Some(v) = &filter.endpoint {
            clauses.push("l.endpoint = ?".into());
            text_values.push(v);
        }
        // The path is bound, never written into the statement.
        let tag_paths: Vec<String> = filter
            .tags
            .iter()
            .map(|(n, _)| self.dialect().tag_key(n))
            .collect();
        for ((_, value), path) in filter.tags.iter().zip(&tag_paths) {
            clauses.push(format!("{} = ?", self.dialect().json_text("l.tags")));
            text_values.push(path);
            text_values.push(value);
        }
        // The worst action is the `action` member of the stored object, one
        // of three words this code wrote. The path is bound, as for tags.
        let action_path = self.dialect().tag_key("action");
        if let Some(action) = filter.guardrail {
            clauses.push(format!("{} = ?", self.dialect().json_text("l.guardrails")));
            text_values.push(&action_path);
            text_values.push(action.as_str());
        }
        let sql = format!(
            "{DETAIL_SELECT} WHERE {} ORDER BY l.id DESC LIMIT ?",
            clauses.join(" AND ")
        );
        let mut query = self.q_dyn(sql);
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

    /// Sums of the rows in `scope` between the days `from` and `to` (both
    /// `YYYY-MM-DD`, inclusive, UTC), one row per group, in one statement.
    /// `day` is ordered by day, the others by requests, most first.
    pub async fn usage(
        &self,
        scope: &LogScope,
        from: &str,
        to: &str,
        group: UsageGroup,
    ) -> Result<Vec<UsageSums>> {
        let (scope_clause, scope_ints) = scope_sql(scope);
        // (group expression, joined table with its name column)
        let json_tag = self.dialect().json_text("l.tags");
        let (expr, names) = match group {
            UsageGroup::Day => ("substr(l.at, 1, 10)", None),
            UsageGroup::Model => ("coalesce(l.provider || '/' || l.model, l.requested)", None),
            UsageGroup::Key => ("l.key_id", Some(("virtual_keys", "name"))),
            UsageGroup::User => ("l.user_id", Some(("users", "email"))),
            UsageGroup::Team => ("l.team_id", Some(("teams", "name"))),
            UsageGroup::Tag(_) => (json_tag.as_str(), None),
        };
        let (label, join) = match names {
            None if matches!(group, UsageGroup::Tag(_)) => {
                ("coalesce(a.gid, '(none)')".to_string(), String::new())
            }
            None => ("a.gid".to_string(), String::new()),
            Some((table, column)) => (
                format!("CASE WHEN a.gid IS NULL THEN '(none)' ELSE coalesce(n.{column}, '(deleted)') END"),
                format!("LEFT JOIN {table} n ON n.id = a.gid AND n.org_id = {DEFAULT_ORG}"),
            ),
        };
        let order = if group == UsageGroup::Day {
            "a.gid"
        } else {
            // The rows without a value first, as SQLite sorts a NULL (a
            // NULL sorts last in PostgreSQL).
            "a.requests DESC, (a.gid IS NOT NULL), a.gid"
        };
        // Both bounds are text over the `at` index; the day after `to`
        // is excluded, so a whole last day counts.
        let sql = format!(
            "SELECT coalesce(CAST(a.gid AS TEXT), '') AS grp, {label} AS label,
                    a.requests, a.errors, a.cancelled, a.input_tokens, a.output_tokens,
                    a.cost_micros, a.unpriced
             FROM (SELECT {expr} AS gid,
                          COUNT(*) AS requests,
                          CAST(coalesce(SUM(CASE WHEN l.status >= 400 AND l.status <> 499 THEN 1 ELSE 0 END), 0) AS BIGINT) AS errors,
                          CAST(coalesce(SUM(CASE WHEN l.status = 499 THEN 1 ELSE 0 END), 0) AS BIGINT) AS cancelled,
                          CAST(coalesce(SUM(l.input_tokens), 0) AS BIGINT) AS input_tokens,
                          CAST(coalesce(SUM(l.output_tokens), 0) AS BIGINT) AS output_tokens,
                          CAST(coalesce(SUM(l.cost_micros), 0) AS BIGINT) AS cost_micros,
                          CAST(coalesce(SUM(CASE WHEN l.priced = 0 AND
                              (l.input_tokens IS NOT NULL OR l.output_tokens IS NOT NULL)
                              THEN 1 ELSE 0 END), 0) AS BIGINT) AS unpriced
                   FROM request_logs l
                   WHERE l.org_id = ? AND {scope_clause} AND l.at >= ? AND l.at < ?
                   GROUP BY gid) a
             {join}
             ORDER BY {order}"
        );
        let mut query = self.q_dyn(sql);
        // The group expression comes first in the statement.
        if let UsageGroup::Tag(name) = &group {
            query = query.bind(self.dialect().tag_key(name));
        }
        let mut query = query.bind(DEFAULT_ORG);
        for v in &scope_ints {
            query = query.bind(*v);
        }
        let rows = query
            .bind(from)
            .bind(next_day(to))
            .fetch_all(self.pool())
            .await?;
        Ok(rows
            .iter()
            .map(|r| UsageSums {
                group: r.get("grp"),
                label: r.get("label"),
                requests: r.get("requests"),
                errors: r.get("errors"),
                cancelled: r.get("cancelled"),
                input_tokens: r.get("input_tokens"),
                output_tokens: r.get("output_tokens"),
                cost_micros: r.get("cost_micros"),
                unpriced_requests: r.get("unpriced"),
            })
            .collect())
    }

    /// One row with its names, whatever its scope. The caller decides
    /// whether the reader may see it.
    pub async fn log_by_id(&self, id: i64) -> Result<Option<LogDetail>> {
        let sql = format!("{DETAIL_SELECT} WHERE l.org_id = ? AND l.id = ?");
        let row = self
            .q_dyn(sql)
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
            "SELECT CASE WHEN EXISTS (SELECT 1 FROM team_members
             WHERE org_id = ? AND user_id = ? AND team_id IN ({marks})) THEN 1 ELSE 0 END"
        );
        let mut query = self.scalar_dyn(sql).bind(DEFAULT_ORG).bind(user_id);
        for t in team_ids {
            query = query.bind(*t);
        }
        let found: i64 = query.fetch_one(self.pool()).await?;
        Ok(found != 0)
    }

    /// Deletes up to `limit` rows older than `cutoff` (`at < cutoff`, in the
    /// form of `store::now`) and returns how many went.
    pub async fn delete_logs_before(&self, cutoff: &str, limit: i64) -> Result<u64> {
        let r = self
            .q("DELETE FROM request_logs WHERE id IN
             (SELECT id FROM request_logs WHERE org_id = ? AND at < ? ORDER BY id LIMIT ?)")
            .bind(DEFAULT_ORG)
            .bind(cutoff)
            .bind(limit)
            .execute(self.pool())
            .await?;
        Ok(r.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::AssertSqlSafe;

    fn row(i: i64) -> NewLog {
        NewLog {
            at: "2026-01-01 10:00:00".into(),
            key_id: Some(i % 3),
            user_id: Some(i % 7),
            team_id: None,
            requested: "m".into(),
            endpoint: "chat".into(),
            stream: false,
            status: 200,
            provider: None,
            model: None,
            input_tokens: None,
            output_tokens: None,
            cost_micros: 0,
            priced: false,
            cached: false,
            estimated: false,
            duration_ms: 1,
            attempts: "[]".into(),
            tags: None,
            guardrails: None,
            prompt: None,
        }
    }

    /// A batch bigger than one chunk lands complete and in order (ids follow
    /// the order given), including the NULL columns, on both databases.
    #[tokio::test]
    async fn a_batch_of_2500_rows_lands_complete_and_in_order() {
        let store = Store::open_in_memory().await.unwrap();
        let rows: Vec<NewLog> = (0..2500)
            .map(|i| {
                let mut r = row(i);
                r.requested = format!("m{i}");
                if i % 2 == 0 {
                    r.provider = Some("p".into());
                    r.tags = Some("{\"a\":\"b\"}".into());
                }
                r
            })
            .collect();
        store.insert_logs(&rows).await.unwrap();
        store.insert_logs(&[]).await.unwrap();
        let got: Vec<(i64, String, Option<String>)> =
            sqlx::query_as("SELECT id, requested, provider FROM request_logs ORDER BY id")
                .fetch_all(store.pool())
                .await
                .unwrap();
        assert_eq!(got.len(), 2500);
        for (i, (_, requested, provider)) in got.iter().enumerate() {
            assert_eq!(requested, &format!("m{i}"));
            assert_eq!(provider.is_some(), i % 2 == 0, "row {i}");
        }
        assert!(got.windows(2).all(|w| w[0].0 < w[1].0));
    }

    #[test]
    fn the_insert_statement_is_built_once_per_size() {
        let a = log_insert_sql(Dialect::Postgres, 2);
        assert!(a.contains("$44") && !a.contains("$45"));
        assert_eq!(a, log_insert_sql(Dialect::Postgres, 2));
        assert!(!log_insert_sql(Dialect::Sqlite, 2).contains('$'));
    }

    #[tokio::test]
    async fn the_lead_scope_has_no_correlated_subquery() {
        let store = Store::open_in_memory().await.unwrap();
        if store.dialect() != Dialect::Sqlite {
            eprintln!("SKIPPED on PostgreSQL: EXPLAIN QUERY PLAN is SQLite's planner output");
            return;
        }
        let scope = LogScope::Teams {
            team_ids: vec![1, 2],
            own_user_id: 3,
        };
        let (clause, binds) = scope_sql(&scope);
        let sql = format!("EXPLAIN QUERY PLAN SELECT 1 FROM request_logs l WHERE {clause}");
        let mut q = sqlx::query(AssertSqlSafe(sql));
        for b in binds {
            q = q.bind(b);
        }
        let plan: Vec<String> = q
            .fetch_all(store.pool())
            .await
            .unwrap()
            .iter()
            .map(|r| r.get::<String, _>("detail"))
            .collect();
        assert!(
            plan.iter().any(|d| d.contains("SUBQUERY")),
            "unexpected plan: {plan:?}"
        );
        assert!(
            plan.iter().all(|d| !d.contains("CORRELATED")),
            "correlated subquery: {plan:?}"
        );
    }

    #[tokio::test]
    async fn optimize_gives_the_planner_statistics_for_the_logs() {
        let store = Store::open_in_memory().await.unwrap();
        if store.dialect() != Dialect::Sqlite {
            eprintln!("SKIPPED on PostgreSQL: sqlite_stat1 and PRAGMA optimize are SQLite's");
            return;
        }
        let rows: Vec<NewLog> = (0..500).map(row).collect();
        store.insert_logs(&rows).await.unwrap();
        // A read through the indexes, as the API does, then the pragma.
        store
            .usage(
                &LogScope::Own { user_id: 1 },
                "2026-01-01",
                "2026-01-02",
                UsageGroup::Day,
            )
            .await
            .unwrap();
        store.optimize().await.unwrap();
        let stats: i64 =
            sqlx::query_scalar("SELECT count(*) FROM sqlite_stat1 WHERE tbl = 'request_logs'")
                .fetch_one(store.pool())
                .await
                .unwrap();
        assert!(stats > 0, "no statistics for request_logs");
    }
}
