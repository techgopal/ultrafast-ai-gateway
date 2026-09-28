//! SQLite storage. Nothing outside this module writes SQL.

use std::fmt;
use std::path::Path;
use std::str::FromStr;

use anyhow::{bail, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions};
use sqlx::Row;

#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
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

#[derive(Debug, Clone)]
pub struct KeyRow {
    pub id: i64,
    pub name: String,
    pub display: String,
}

/// Expiry is compared as text against `datetime('now')`, so anything other
/// than exactly `YYYY-MM-DD HH:MM:SS` could compare as never expiring.
fn check_expires_at(value: &str) -> Result<()> {
    let b = value.as_bytes();
    let shape_ok = b.len() == 19
        && b.iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            10 => *c == b' ',
            13 | 16 => *c == b':',
            _ => c.is_ascii_digit(),
        });
    if shape_ok {
        let num = |at: usize| (b[at] - b'0') * 10 + (b[at + 1] - b'0');
        let (month, day) = (num(5), num(8));
        if (1..=12).contains(&month)
            && (1..=31).contains(&day)
            && num(11) < 24
            && num(14) < 60
            && num(17) < 60
        {
            return Ok(());
        }
    }
    bail!("expires_at must be a UTC timestamp in the form YYYY-MM-DD HH:MM:SS")
}

impl Store {
    pub async fn open(path: &Path) -> Result<Self> {
        let opts = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .foreign_keys(true);
        Self::connect(opts, 8).await
    }

    /// One connection only: every in-memory connection is its own database.
    pub async fn open_in_memory() -> Result<Self> {
        let opts = SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true);
        Self::connect(opts, 1).await
    }

    async fn connect(opts: SqliteConnectOptions, max: u32) -> Result<Self> {
        let pool = SqlitePoolOptions::new()
            .max_connections(max)
            .connect_with(opts)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    pub async fn insert_provider(
        &self,
        name: &str,
        kind: &str,
        base_url: &str,
        credential: Option<&[u8]>,
    ) -> Result<i64> {
        let r = sqlx::query(
            "INSERT INTO providers (name, kind, base_url, credential) VALUES (?, ?, ?, ?)",
        )
        .bind(name)
        .bind(kind)
        .bind(base_url)
        .bind(credential)
        .execute(&self.pool)
        .await?;
        Ok(r.last_insert_rowid())
    }

    pub async fn provider_by_name(&self, name: &str) -> Result<Option<ProviderRow>> {
        let row = sqlx::query(
            "SELECT id, name, kind, base_url, credential FROM providers WHERE name = ?",
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| ProviderRow {
            id: r.get("id"),
            name: r.get("name"),
            kind: r.get("kind"),
            base_url: r.get("base_url"),
            credential: r.get("credential"),
        }))
    }

    /// `expires_at` must be UTC in the form `YYYY-MM-DD HH:MM:SS`.
    pub async fn insert_key(
        &self,
        name: &str,
        hash: &str,
        display: &str,
        expires_at: Option<&str>,
    ) -> Result<i64> {
        if let Some(value) = expires_at {
            check_expires_at(value)?;
        }
        let r = sqlx::query(
            "INSERT INTO virtual_keys (name, key_hash, display, expires_at) VALUES (?, ?, ?, ?)",
        )
        .bind(name)
        .bind(hash)
        .bind(display)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;
        Ok(r.last_insert_rowid())
    }

    pub async fn active_key_by_hash(&self, hash: &str) -> Result<Option<KeyRow>> {
        let row = sqlx::query(
            "SELECT id, name, display FROM virtual_keys
             WHERE key_hash = ?
               AND revoked_at IS NULL
               AND (expires_at IS NULL OR expires_at > datetime('now'))",
        )
        .bind(hash)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|r| KeyRow {
            id: r.get("id"),
            name: r.get("name"),
            display: r.get("display"),
        }))
    }

    pub async fn revoke_key(&self, id: i64) -> Result<()> {
        sqlx::query("UPDATE virtual_keys SET revoked_at = datetime('now') WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
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
    async fn data_survives_reopen_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gateway.db");
        {
            let s = Store::open(&path).await.unwrap();
            s.insert_key("k", "h", "d", None).await.unwrap();
        }
        let s = Store::open(&path).await.unwrap();
        assert!(s.active_key_by_hash("h").await.unwrap().is_some());
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
