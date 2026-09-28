//! Virtual keys.

use anyhow::{Context, Result};
use sqlx::Row;

use super::{check_timestamp, Store, Tx, DEFAULT_ORG};

#[derive(Debug, Clone)]
pub struct KeyRow {
    pub id: i64,
    pub name: String,
    pub display: String,
    pub user_id: Option<i64>,
    pub team_id: Option<i64>,
    pub expires_at: Option<String>,
    pub revoked_at: Option<String>,
    pub created_at: String,
}

impl Store {
    /// `expires_at` must be UTC in the form `YYYY-MM-DD HH:MM:SS`.
    pub async fn insert_key(
        &self,
        name: &str,
        hash: &str,
        display: &str,
        expires_at: Option<&str>,
    ) -> Result<i64> {
        let mut tx = self.begin().await?;
        let id = tx
            .insert_key(name, hash, display, expires_at, None, None)
            .await?;
        tx.commit().await?;
        Ok(id)
    }

    pub async fn active_key_by_hash(&self, hash: &str) -> Result<Option<KeyRow>> {
        let row = sqlx::query(
            "SELECT id, name, display, user_id, team_id, expires_at, revoked_at, created_at
             FROM virtual_keys
             WHERE key_hash = ?
               AND org_id = ?
               AND revoked_at IS NULL
               AND (expires_at IS NULL OR expires_at > datetime('now'))",
        )
        .bind(hash)
        .bind(DEFAULT_ORG)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|r| KeyRow {
            id: r.get("id"),
            name: r.get("name"),
            display: r.get("display"),
            user_id: r.get("user_id"),
            team_id: r.get("team_id"),
            expires_at: r.get("expires_at"),
            revoked_at: r.get("revoked_at"),
            created_at: r.get("created_at"),
        }))
    }

    /// Returns whether a live key was revoked. Revoking again changes nothing.
    pub async fn revoke_key(&self, id: i64) -> Result<bool> {
        let mut tx = self.begin().await?;
        let revoked = tx.revoke_key(id).await?;
        tx.commit().await?;
        Ok(revoked)
    }
}

impl Tx<'_> {
    /// `expires_at` must be UTC in the form `YYYY-MM-DD HH:MM:SS`.
    pub async fn insert_key(
        &mut self,
        name: &str,
        hash: &str,
        display: &str,
        expires_at: Option<&str>,
        user_id: Option<i64>,
        team_id: Option<i64>,
    ) -> Result<i64> {
        if let Some(value) = expires_at {
            check_timestamp(value).context("expires_at is not valid")?;
        }
        let r = sqlx::query(
            "INSERT INTO virtual_keys
                 (org_id, name, key_hash, display, expires_at, user_id, team_id)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(DEFAULT_ORG)
        .bind(name)
        .bind(hash)
        .bind(display)
        .bind(expires_at)
        .bind(user_id)
        .bind(team_id)
        .execute(self.conn())
        .await?;
        Ok(r.last_insert_rowid())
    }

    /// Returns whether a live key was revoked. Revoking again changes nothing.
    pub async fn revoke_key(&mut self, id: i64) -> Result<bool> {
        let r = sqlx::query(
            "UPDATE virtual_keys SET revoked_at = datetime('now')
             WHERE id = ? AND org_id = ? AND revoked_at IS NULL",
        )
        .bind(id)
        .bind(DEFAULT_ORG)
        .execute(self.conn())
        .await?;
        Ok(r.rows_affected() == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn key_lookup_honours_revocation_and_expiry() {
        let s = Store::open_in_memory().await.unwrap();
        let live = s
            .insert_key("live", "h1", "uf-sk-…aaaa", None)
            .await
            .unwrap();
        s.insert_key("future", "h2", "uf-sk-…bbbb", Some("2999-01-01 00:00:00"))
            .await
            .unwrap();
        s.insert_key("past", "h3", "uf-sk-…cccc", Some("2000-01-01 00:00:00"))
            .await
            .unwrap();

        assert_eq!(
            s.active_key_by_hash("h1").await.unwrap().unwrap().name,
            "live"
        );
        assert!(s.active_key_by_hash("h2").await.unwrap().is_some());
        assert!(s.active_key_by_hash("h3").await.unwrap().is_none());
        assert!(s.active_key_by_hash("nope").await.unwrap().is_none());

        s.revoke_key(live).await.unwrap();
        assert!(s.active_key_by_hash("h1").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn insert_key_rejects_malformed_expiry() {
        let s = Store::open_in_memory().await.unwrap();
        for (i, bad) in [
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
        ]
        .into_iter()
        .enumerate()
        {
            let hash = format!("h{i}");
            assert!(
                s.insert_key("k", &hash, "d", Some(bad)).await.is_err(),
                "accepted {bad:?}"
            );
            assert!(
                s.active_key_by_hash(&hash).await.unwrap().is_none(),
                "stored a key for {bad:?}"
            );
        }
    }

    async fn revoked_at(s: &Store, id: i64) -> Option<String> {
        sqlx::query("SELECT revoked_at FROM virtual_keys WHERE id = ?")
            .bind(id)
            .fetch_one(s.pool())
            .await
            .unwrap()
            .get("revoked_at")
    }

    #[tokio::test]
    async fn revoke_reports_whether_it_revoked() {
        let s = Store::open_in_memory().await.unwrap();
        let id = s.insert_key("k", "h", "d", None).await.unwrap();

        assert!(s.revoke_key(id).await.unwrap());
        // Backdate the stamp so a second revoke that rewrote it would show.
        sqlx::query("UPDATE virtual_keys SET revoked_at = '2000-01-01 00:00:00' WHERE id = ?")
            .bind(id)
            .execute(s.pool())
            .await
            .unwrap();

        assert!(!s.revoke_key(id).await.unwrap());
        assert_eq!(
            revoked_at(&s, id).await.as_deref(),
            Some("2000-01-01 00:00:00")
        );
        assert!(!s.revoke_key(id + 1000).await.unwrap());
    }

    #[tokio::test]
    async fn key_lookup_is_scoped_to_the_org() {
        let s = Store::open_in_memory().await.unwrap();
        sqlx::query(
            "INSERT INTO virtual_keys (org_id, name, key_hash, display) VALUES (2, 'k', 'h', 'd')",
        )
        .execute(s.pool())
        .await
        .unwrap();
        assert!(s.active_key_by_hash("h").await.unwrap().is_none());
        assert!(!s.revoke_key(1).await.unwrap());
    }

    #[tokio::test]
    async fn key_row_carries_new_fields() {
        let s = Store::open_in_memory().await.unwrap();
        let id = s
            .insert_key("k", "h", "uf-sk-…aaaa", Some("2999-01-01 00:00:00"))
            .await
            .unwrap();
        let k = s.active_key_by_hash("h").await.unwrap().unwrap();
        assert_eq!(k.id, id);
        assert_eq!(k.expires_at.as_deref(), Some("2999-01-01 00:00:00"));
        assert_eq!(k.revoked_at, None);
        assert_eq!(k.user_id, None);
        assert_eq!(k.team_id, None);
        assert!(crate::store::check_timestamp(&k.created_at).is_ok());
    }
}
