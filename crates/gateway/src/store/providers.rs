//! Upstream providers.

use std::fmt;

use anyhow::Result;
use sqlx::Row;

use super::{Store, DEFAULT_ORG};

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

impl Store {
    pub async fn insert_provider(
        &self,
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
        .execute(self.pool())
        .await?;
        Ok(r.last_insert_rowid())
    }

    pub async fn provider_by_name(&self, name: &str) -> Result<Option<ProviderRow>> {
        let row = sqlx::query(
            "SELECT id, name, kind, base_url, credential FROM providers
             WHERE name = ? AND org_id = ?",
        )
        .bind(name)
        .bind(DEFAULT_ORG)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|r| ProviderRow {
            id: r.get("id"),
            name: r.get("name"),
            kind: r.get("kind"),
            base_url: r.get("base_url"),
            credential: r.get("credential"),
        }))
    }
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
