//! The audit log. Entries are written in the transaction of the change they record.

use anyhow::Result;
use sqlx::Row;

use super::dialect::Dialected;
use super::{Store, Tx, DEFAULT_ORG};

/// Most rows one call to `list_audit` returns.
const MAX_PAGE: i64 = 200;

/// The caller writes `summary`; it must not contain a secret.
pub struct AuditEntry<'a> {
    pub actor_user_id: Option<i64>,
    pub actor_email: &'a str,
    pub action: &'a str,
    pub target_type: &'a str,
    pub target_id: Option<i64>,
    pub summary: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, utoipa::ToSchema)]
pub struct AuditRow {
    pub id: i64,
    pub at: String,
    pub actor_email: String,
    pub action: String,
    pub target_type: String,
    #[schema(required)]
    pub target_id: Option<i64>,
    pub summary: String,
}

impl Store {
    /// Newest first. `limit` is clamped to 1..=200; `before_id` continues
    /// from the last id of the previous page.
    pub async fn list_audit(&self, limit: i64, before_id: Option<i64>) -> Result<Vec<AuditRow>> {
        let rows = self
            .q(
                "SELECT id, at, actor_email, action, target_type, target_id, summary
             FROM audit_log
             WHERE org_id = ? AND (? IS NULL OR id < ?)
             ORDER BY id DESC
             LIMIT ?",
            )
            .bind(DEFAULT_ORG)
            .bind(before_id)
            .bind(before_id)
            .bind(limit.clamp(1, MAX_PAGE))
            .fetch_all(self.pool())
            .await?;
        Ok(rows
            .iter()
            .map(|r| AuditRow {
                id: r.get("id"),
                at: r.get("at"),
                actor_email: r.get("actor_email"),
                action: r.get("action"),
                target_type: r.get("target_type"),
                target_id: r.get("target_id"),
                summary: r.get("summary"),
            })
            .collect())
    }
}

impl Tx<'_> {
    pub async fn audit(&mut self, e: AuditEntry<'_>) -> Result<()> {
        self.q("INSERT INTO audit_log
                 (org_id, actor_user_id, actor_email, action, target_type, target_id, summary)
             VALUES (?, ?, ?, ?, ?, ?, ?)")
            .bind(DEFAULT_ORG)
            .bind(e.actor_user_id)
            .bind(e.actor_email)
            .bind(e.action)
            .bind(e.target_type)
            .bind(e.target_id)
            .bind(e.summary)
            .execute(self.conn())
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::check_timestamp;

    fn entry(summary: &str) -> AuditEntry<'_> {
        AuditEntry {
            actor_user_id: Some(7),
            actor_email: "maya@example.com",
            action: "team.create",
            target_type: "team",
            target_id: Some(3),
            summary,
        }
    }

    async fn ids(s: &Store, limit: i64, before: Option<i64>) -> Vec<i64> {
        s.list_audit(limit, before)
            .await
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect()
    }

    #[tokio::test]
    async fn audit_entry_round_trip() {
        let s = Store::open_in_memory().await.unwrap();
        let mut tx = s.begin().await.unwrap();
        tx.audit(entry("created team platform")).await.unwrap();
        tx.audit(AuditEntry {
            actor_user_id: None,
            actor_email: "system",
            action: "setup",
            target_type: "org",
            target_id: None,
            summary: "first run",
        })
        .await
        .unwrap();
        tx.commit().await.unwrap();

        let rows = s.list_audit(10, None).await.unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].actor_email, "system");
        assert_eq!(rows[0].target_id, None);
        let first = &rows[1];
        assert!(check_timestamp(&first.at).is_ok());
        assert_eq!(first.actor_email, "maya@example.com");
        assert_eq!(first.action, "team.create");
        assert_eq!(first.target_type, "team");
        assert_eq!(first.target_id, Some(3));
        assert_eq!(first.summary, "created team platform");
    }

    #[tokio::test]
    async fn audit_is_newest_first_and_paged() {
        let s = Store::open_in_memory().await.unwrap();
        let mut tx = s.begin().await.unwrap();
        for i in 0..5 {
            tx.audit(entry(&format!("entry {i}"))).await.unwrap();
        }
        tx.commit().await.unwrap();

        assert_eq!(ids(&s, 2, None).await, [5, 4]);
        assert_eq!(ids(&s, 2, Some(4)).await, [3, 2]);
        assert_eq!(ids(&s, 0, None).await, [5]);
        assert_eq!(ids(&s, -3, None).await, [5]);
        assert_eq!(ids(&s, 10, Some(1)).await, [] as [i64; 0]);

        let mut tx = s.begin().await.unwrap();
        for i in 5..205 {
            tx.audit(entry(&format!("entry {i}"))).await.unwrap();
        }
        tx.commit().await.unwrap();
        let page = ids(&s, 1000, None).await;
        assert_eq!(page.len(), 200);
        assert_eq!(page[0], 205);
        assert_eq!(page[199], 6);
    }

    #[tokio::test]
    async fn audit_is_scoped_to_the_org() {
        let s = Store::open_in_memory().await.unwrap();
        sqlx::query(
            "INSERT INTO audit_log (org_id, actor_email, action, target_type, summary)
             VALUES (2, 'a@example.com', 'x', 'y', 'z')",
        )
        .execute(s.pool())
        .await
        .unwrap();
        assert!(s.list_audit(10, None).await.unwrap().is_empty());
    }
}
