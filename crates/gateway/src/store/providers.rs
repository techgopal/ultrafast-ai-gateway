//! Upstream providers.

use std::fmt;

use anyhow::Result;
use sqlx::sqlite::SqliteRow;
use sqlx::{AssertSqlSafe, Row};

use super::{write_error, Store, Tx, DEFAULT_ORG};

const PROVIDER_SELECT: &str = "SELECT id, name, kind, base_url, credential FROM providers";

fn provider_from(r: &SqliteRow) -> ProviderRow {
    ProviderRow {
        id: r.get("id"),
        name: r.get("name"),
        kind: r.get("kind"),
        base_url: r.get("base_url"),
        credential: r.get("credential"),
    }
}

#[derive(Clone)]
pub struct ProviderRow {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub base_url: String,
    /// Encrypted with the master key.
    pub credential: Option<Vec<u8>>,
}

/// Shows only whether a credential is present, never its bytes.
impl fmt::Debug for ProviderRow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let credential = if self.credential.is_some() {
            "<present>"
        } else {
            "<none>"
        };
        f.debug_struct("ProviderRow")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("kind", &self.kind)
            .field("base_url", &self.base_url)
            .field("credential", &credential)
            .finish()
    }
}

impl Tx<'_> {
    /// For use inside a transaction; see `Store::provider_by_id`.
    pub async fn provider_by_id(&mut self, id: i64) -> Result<Option<ProviderRow>> {
        let sql = format!("{PROVIDER_SELECT} WHERE id = ? AND org_id = ?");
        let row = sqlx::query(AssertSqlSafe(sql))
            .bind(id)
            .bind(DEFAULT_ORG)
            .fetch_optional(self.conn())
            .await?;
        Ok(row.as_ref().map(provider_from))
    }

    /// Changes what is given and leaves the rest. For `credential`, `None`
    /// leaves it, `Some(None)` removes it and `Some(Some(_))` replaces it.
    /// Returns `false` if there is no such provider.
    pub async fn update_provider(
        &mut self,
        id: i64,
        base_url: Option<&str>,
        credential: Option<Option<&[u8]>>,
    ) -> Result<bool> {
        let r = sqlx::query(
            "UPDATE providers
             SET base_url = COALESCE(?, base_url),
                 credential = CASE WHEN ? THEN ? ELSE credential END
             WHERE id = ? AND org_id = ?",
        )
        .bind(base_url)
        .bind(credential.is_some())
        .bind(credential.flatten())
        .bind(id)
        .bind(DEFAULT_ORG)
        .execute(self.conn())
        .await?;
        Ok(r.rows_affected() == 1)
    }

    /// Returns `false` if there is no such provider.
    pub async fn delete_provider(&mut self, id: i64) -> Result<bool> {
        let r = sqlx::query("DELETE FROM providers WHERE id = ? AND org_id = ?")
            .bind(id)
            .bind(DEFAULT_ORG)
            .execute(self.conn())
            .await?;
        Ok(r.rows_affected() == 1)
    }

    /// A taken name is `StoreError::Duplicate`.
    pub async fn insert_provider(
        &mut self,
        name: &str,
        kind: &str,
        base_url: &str,
        credential: Option<&[u8]>,
    ) -> Result<i64> {
        let r = sqlx::query(
            "INSERT INTO providers (org_id, name, kind, base_url, credential)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(DEFAULT_ORG)
        .bind(name)
        .bind(kind)
        .bind(base_url)
        .bind(credential)
        .execute(self.conn())
        .await
        .map_err(write_error)?;
        Ok(r.last_insert_rowid())
    }
}

impl Store {
    pub async fn insert_provider(
        &self,
        name: &str,
        kind: &str,
        base_url: &str,
        credential: Option<&[u8]>,
    ) -> Result<i64> {
        let mut tx = self.begin().await?;
        let id = tx.insert_provider(name, kind, base_url, credential).await?;
        tx.commit().await?;
        Ok(id)
    }

    pub async fn provider_by_name(&self, name: &str) -> Result<Option<ProviderRow>> {
        let sql = format!("{PROVIDER_SELECT} WHERE name = ? AND org_id = ?");
        let row = sqlx::query(AssertSqlSafe(sql))
            .bind(name)
            .bind(DEFAULT_ORG)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.as_ref().map(provider_from))
    }

    pub async fn provider_by_id(&self, id: i64) -> Result<Option<ProviderRow>> {
        let sql = format!("{PROVIDER_SELECT} WHERE id = ? AND org_id = ?");
        let row = sqlx::query(AssertSqlSafe(sql))
            .bind(id)
            .bind(DEFAULT_ORG)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.as_ref().map(provider_from))
    }

    /// Ordered by name.
    pub async fn list_providers(&self) -> Result<Vec<ProviderRow>> {
        let mut conn = self.pool().acquire().await?;
        list_providers_in(&mut conn).await
    }
}

pub(crate) async fn list_providers_in(
    conn: &mut sqlx::SqliteConnection,
) -> Result<Vec<ProviderRow>> {
    let sql = format!("{PROVIDER_SELECT} WHERE org_id = ? ORDER BY name");
    let rows = sqlx::query(AssertSqlSafe(sql))
        .bind(DEFAULT_ORG)
        .fetch_all(&mut *conn)
        .await?;
    Ok(rows.iter().map(provider_from).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn provider_round_trip_and_unique_name() {
        let s = Store::open_in_memory().await.unwrap();
        s.insert_provider(
            "openai",
            "openai",
            "https://api.openai.com/v1",
            Some(b"enc"),
        )
        .await
        .unwrap();
        let p = s.provider_by_name("openai").await.unwrap().unwrap();
        assert_eq!(p.kind, "openai");
        assert_eq!(p.base_url, "https://api.openai.com/v1");
        assert_eq!(p.credential.as_deref(), Some(&b"enc"[..]));
        assert!(s.provider_by_name("missing").await.unwrap().is_none());
        assert!(s
            .insert_provider("openai", "openai", "x", None)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn provider_lookup_is_scoped_to_the_org() {
        let s = Store::open_in_memory().await.unwrap();
        sqlx::query("INSERT INTO providers (org_id, name, kind, base_url) VALUES (2, 'other', 'openai', 'x')")
            .execute(s.pool())
            .await
            .unwrap();
        assert!(s.provider_by_name("other").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn providers_are_listed_updated_and_deleted() {
        use crate::store::StoreError;

        let s = Store::open_in_memory().await.unwrap();
        let b = s
            .insert_provider("b", "openai", "http://b", Some(b"one"))
            .await
            .unwrap();
        let a = s
            .insert_provider("a", "anthropic", "http://a", None)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO providers (org_id, name, kind, base_url) VALUES (2, 'z', 'openai', 'x')",
        )
        .execute(s.pool())
        .await
        .unwrap();
        let names: Vec<String> = s
            .list_providers()
            .await
            .unwrap()
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert_eq!(names, ["a", "b"]);

        let err = s
            .insert_provider("a", "openai", "x", None)
            .await
            .unwrap_err();
        assert!(matches!(
            err.downcast_ref::<StoreError>(),
            Some(StoreError::Duplicate)
        ));

        let mut tx = s.begin().await.unwrap();
        assert!(tx
            .update_provider(b, Some("http://b2"), None)
            .await
            .unwrap());
        let p = tx.provider_by_id(b).await.unwrap().unwrap();
        assert_eq!(p.base_url, "http://b2");
        assert_eq!(p.credential.as_deref(), Some(&b"one"[..]));
        assert!(tx
            .update_provider(b, None, Some(Some(b"two")))
            .await
            .unwrap());
        let p = tx.provider_by_id(b).await.unwrap().unwrap();
        assert_eq!(p.base_url, "http://b2");
        assert_eq!(p.credential.as_deref(), Some(&b"two"[..]));
        assert!(tx.update_provider(b, None, Some(None)).await.unwrap());
        assert_eq!(
            tx.provider_by_id(b).await.unwrap().unwrap().credential,
            None
        );
        assert!(!tx.update_provider(b + 100, Some("x"), None).await.unwrap());
        assert!(tx.delete_provider(a).await.unwrap());
        assert!(!tx.delete_provider(a).await.unwrap());
        tx.commit().await.unwrap();

        assert!(s.provider_by_id(a).await.unwrap().is_none());
        assert!(s.provider_by_id(b).await.unwrap().is_some());
    }

    #[test]
    fn provider_debug_does_not_print_the_credential() {
        let mut p = ProviderRow {
            id: 1,
            name: "openai".into(),
            kind: "openai".into(),
            base_url: "https://api.openai.com/v1".into(),
            credential: Some(vec![222, 173, 190, 239]),
        };
        let shown = format!("{p:?}");
        assert!(shown.contains("openai"));
        assert!(shown.contains("<present>"));
        assert!(!shown.contains("222"));
        assert!(!shown.contains("173"));
        p.credential = None;
        assert!(format!("{p:?}").contains("<none>"));
    }
}
