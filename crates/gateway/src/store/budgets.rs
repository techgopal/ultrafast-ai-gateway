//! Budgets, their cached usage, and the spend counted in the request logs.

use anyhow::{anyhow, Result};
use sqlx::sqlite::{SqliteConnection, SqliteRow};
use sqlx::{AssertSqlSafe, Row};

use super::{AuditEntry, Store, Tx, DEFAULT_ORG};
use crate::budgets::{BudgetAction, Period};
use crate::limits::LimitScope;

/// A stored budget with the name of what it is set on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetRow {
    pub id: i64,
    pub scope: LimitScope,
    /// `None` for the gateway.
    pub scope_id: Option<i64>,
    pub amount_micros: u64,
    pub period: Period,
    pub action: BudgetAction,
    /// The key's or team's name, or the user's email. `None` for the
    /// gateway, and for a subject that is gone.
    pub name: Option<String>,
    /// For a key, the user that owns it.
    pub key_owner: Option<i64>,
    /// For a key, the team it belongs to.
    pub key_team: Option<i64>,
}

impl BudgetRow {
    /// How a refusal or a list names the subject.
    pub fn label(&self) -> String {
        self.scope.label(self.name.as_deref().unwrap_or(""))
    }

    /// Whether the subject exists (the gateway always does).
    pub fn has_subject(&self) -> bool {
        self.scope == LimitScope::Gateway || self.name.is_some()
    }
}

/// What a budget has spent in one period, as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageRow {
    pub budget_id: i64,
    /// The UTC date the period began on, `YYYY-MM-DD`.
    pub period_start: String,
    pub spent_micros: u64,
}

const SELECT: &str = "SELECT b.id, b.scope, b.scope_id, b.amount_micros, b.period, b.action,
            CASE b.scope WHEN 'key' THEN k.name WHEN 'user' THEN u.email WHEN 'team' THEN t.name END AS name,
            k.user_id AS key_owner,
            k.team_id AS key_team
     FROM budgets b
     LEFT JOIN virtual_keys k ON b.scope = 'key' AND k.id = b.scope_id AND k.org_id = b.org_id
     LEFT JOIN users u ON b.scope = 'user' AND u.id = b.scope_id AND u.org_id = b.org_id
     LEFT JOIN teams t ON b.scope = 'team' AND t.id = b.scope_id AND t.org_id = b.org_id";

fn budget_from(r: &SqliteRow) -> Result<BudgetRow> {
    let scope: String = r.get("scope");
    let period: String = r.get("period");
    let action: String = r.get("action");
    let amount: i64 = r.get("amount_micros");
    Ok(BudgetRow {
        id: r.get("id"),
        scope: LimitScope::parse(&scope)
            .ok_or_else(|| anyhow!("stored budget scope is not known"))?,
        scope_id: r.get("scope_id"),
        amount_micros: u64::try_from(amount).map_err(|_| anyhow!("stored budget is negative"))?,
        period: Period::parse(&period)
            .ok_or_else(|| anyhow!("stored budget period is not known"))?,
        action: BudgetAction::parse(&action)
            .ok_or_else(|| anyhow!("stored budget action is not known"))?,
        name: r.get("name"),
        key_owner: r.get("key_owner"),
        key_team: r.get("key_team"),
    })
}

/// Every budget, oldest first, on the connection of a transaction.
pub(super) async fn list_budgets_in(conn: &mut SqliteConnection) -> Result<Vec<BudgetRow>> {
    let sql = format!("{SELECT} WHERE b.org_id = ? ORDER BY b.id");
    let rows = sqlx::query(AssertSqlSafe(sql))
        .bind(DEFAULT_ORG)
        .fetch_all(conn)
        .await?;
    rows.iter().map(budget_from).collect()
}

fn to_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

impl Store {
    /// Every budget, oldest first.
    pub async fn list_budgets(&self) -> Result<Vec<BudgetRow>> {
        let mut conn = self.pool().acquire().await?;
        list_budgets_in(&mut conn).await
    }

    /// What the cache holds for a budget in a period: `(spent, alerted)`.
    pub async fn budget_usage(
        &self,
        budget_id: i64,
        period_start: &str,
    ) -> Result<Option<(u64, bool)>> {
        let row: Option<(i64, i64)> = sqlx::query_as(
            "SELECT spent_micros, alerted FROM budget_usage WHERE budget_id = ? AND period_start = ?",
        )
        .bind(budget_id)
        .bind(period_start)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|(spent, alerted)| (u64::try_from(spent).unwrap_or(0), alerted != 0)))
    }

    /// The cost of the logged calls since `since` (`YYYY-MM-DD HH:MM:SS`,
    /// UTC) that count for a subject. A team counts the calls of its keys
    /// and of its members, as the live counters do.
    pub async fn spend_since(
        &self,
        scope: LimitScope,
        scope_id: Option<i64>,
        since: &str,
    ) -> Result<u64> {
        let (filter, binds): (&str, usize) = match scope {
            LimitScope::Gateway => ("1 = 1", 0),
            LimitScope::Key => ("key_id = ?", 1),
            LimitScope::User => ("user_id = ?", 1),
            LimitScope::Team => (
                "(team_id = ? OR user_id IN (SELECT user_id FROM team_members WHERE team_id = ?))",
                2,
            ),
        };
        let sql = format!(
            "SELECT COALESCE(SUM(cost_micros), 0) FROM request_logs
             WHERE org_id = ? AND at >= ? AND {filter}"
        );
        let mut q = sqlx::query_scalar(AssertSqlSafe(sql))
            .bind(DEFAULT_ORG)
            .bind(since);
        for _ in 0..binds {
            q = q.bind(scope_id);
        }
        let sum: i64 = q.fetch_one(self.pool()).await?;
        Ok(u64::try_from(sum).unwrap_or(0))
    }

    /// Writes the counters of budgets that still exist; one transaction.
    pub async fn write_budget_usage(&self, rows: &[UsageRow]) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let mut tx = self.begin().await?;
        for r in rows {
            tx.write_usage(&r.period_start, r.budget_id, r.spent_micros)
                .await?;
        }
        tx.commit().await
    }

    /// Writes the alert of a budget for a period, once: the first call for
    /// the period writes the audit row (by `system`) and returns true; any
    /// later call, also after a restart, writes nothing. Nothing is written
    /// for a budget that is gone.
    pub async fn record_budget_alert(
        &self,
        budget_id: i64,
        period_start: &str,
        spent_micros: u64,
        summary: &str,
    ) -> Result<bool> {
        let mut tx = self.begin().await?;
        tx.write_usage(period_start, budget_id, spent_micros)
            .await?;
        let marked = sqlx::query(
            "UPDATE budget_usage SET alerted = 1
             WHERE budget_id = ? AND period_start = ? AND alerted = 0",
        )
        .bind(budget_id)
        .bind(period_start)
        .execute(tx.conn())
        .await?
        .rows_affected();
        if marked == 1 {
            tx.audit(AuditEntry {
                actor_user_id: None,
                actor_email: "system",
                action: "budget.alert",
                target_type: "budget",
                target_id: Some(budget_id),
                summary,
            })
            .await?;
        }
        tx.commit().await?;
        Ok(marked == 1)
    }
}

impl Tx<'_> {
    async fn write_usage(&mut self, period_start: &str, budget_id: i64, spent: u64) -> Result<()> {
        // Within a period spend only grows, so a late or older write never
        // lowers what the row holds. The WHERE clause keeps a budget that was deleted meanwhile from
        // failing the foreign key.
        sqlx::query(
            "INSERT INTO budget_usage (budget_id, period_start, spent_micros)
             SELECT ?, ?, ? WHERE EXISTS (SELECT 1 FROM budgets WHERE id = ?)
             ON CONFLICT (budget_id, period_start) DO UPDATE SET spent_micros = MAX(spent_micros, excluded.spent_micros)",
        )
        .bind(budget_id)
        .bind(period_start)
        .bind(to_i64(spent))
        .bind(budget_id)
        .execute(self.conn())
        .await?;
        Ok(())
    }

    pub async fn budget_by_id(&mut self, id: i64) -> Result<Option<BudgetRow>> {
        let sql = format!("{SELECT} WHERE b.id = ? AND b.org_id = ?");
        let row = sqlx::query(AssertSqlSafe(sql))
            .bind(id)
            .bind(DEFAULT_ORG)
            .fetch_optional(self.conn())
            .await?;
        row.as_ref().map(budget_from).transpose()
    }

    /// Sets the amount and action of the budget of a subject for a period,
    /// creating it when it is new. Returns the id of the row, which stays
    /// the same for a subject and period.
    pub async fn upsert_budget(
        &mut self,
        scope: LimitScope,
        scope_id: Option<i64>,
        amount_micros: u64,
        period: Period,
        action: BudgetAction,
    ) -> Result<i64> {
        let amount = i64::try_from(amount_micros).map_err(|_| anyhow!("budget is too large"))?;
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO budgets (org_id, scope, scope_id, amount_micros, period, action)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT (org_id, scope, COALESCE(scope_id, 0), period) DO UPDATE SET
                 amount_micros = excluded.amount_micros,
                 action = excluded.action
             RETURNING id",
        )
        .bind(DEFAULT_ORG)
        .bind(scope.as_str())
        .bind(scope_id)
        .bind(amount)
        .bind(period.as_str())
        .bind(action.as_str())
        .fetch_one(self.conn())
        .await?;
        Ok(id)
    }

    pub async fn delete_budget(&mut self, id: i64) -> Result<bool> {
        let r = sqlx::query("DELETE FROM budgets WHERE id = ? AND org_id = ?")
            .bind(id)
            .bind(DEFAULT_ORG)
            .execute(self.conn())
            .await?;
        Ok(r.rows_affected() == 1)
    }
}
