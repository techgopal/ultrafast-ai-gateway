//! Virtual keys.

use anyhow::{Context, Result};
use sqlx::any::AnyRow;
use sqlx::AnyConnection;
use sqlx::Row;

use super::dialect::Dialected;
use super::{check_timestamp, flag, now, write_error, Store, Tx, DEFAULT_ORG};
use crate::tags::{self, Tags};

/// A virtual key as stored. It never holds the key or its hash.
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
    /// The email of the owner, if the key has one.
    pub owner_email: Option<String>,
    /// The name of the team, if the key belongs to one.
    pub team_name: Option<String>,
    /// Whether the key has an owner who is not active. Such a key does not
    /// work on `/v1`.
    pub owner_inactive: bool,
    /// The names the key may call; `None` is no allowlist.
    pub allowed: Option<Vec<String>>,
    /// The tags every call of the key is recorded with.
    pub tags: Tags,
    /// A non-admin made it for another user: it acts for its team only.
    pub team_only: bool,
}

/// Reads the stored allowlist. A value that cannot be read is an empty
/// allowlist, so a damaged row can only take access away.
pub fn parse_allowed(raw: Option<&str>) -> Option<Vec<String>> {
    raw.map(|text| serde_json::from_str(text).unwrap_or_default())
}

/// Every key query reads through this, so the hash is never selected.
const KEY_SELECT: &str = "SELECT k.id, k.name, k.display, k.user_id, k.team_id,
            k.expires_at, k.revoked_at, k.created_at, k.allowed, k.tags, k.team_only,
            u.email AS owner_email, t.name AS team_name,
            CASE WHEN u.id IS NOT NULL AND u.status <> 'active' THEN 1 ELSE 0 END AS owner_inactive
     FROM virtual_keys k
     LEFT JOIN users u ON u.id = k.user_id AND u.org_id = k.org_id
     LEFT JOIN teams t ON t.id = k.team_id AND t.org_id = k.org_id";

const KEY_ORDER: &str = "ORDER BY k.created_at DESC, k.id DESC";

fn key_from(r: &AnyRow) -> KeyRow {
    KeyRow {
        id: r.get("id"),
        name: r.get("name"),
        display: r.get("display"),
        user_id: r.get("user_id"),
        team_id: r.get("team_id"),
        expires_at: r.get("expires_at"),
        revoked_at: r.get("revoked_at"),
        created_at: r.get("created_at"),
        owner_email: r.get("owner_email"),
        team_name: r.get("team_name"),
        owner_inactive: r.get::<i64, _>("owner_inactive") != 0,
        allowed: parse_allowed(r.get::<Option<String>, _>("allowed").as_deref()),
        tags: tags::parse_stored(r.get::<Option<String>, _>("tags").as_deref()),
        team_only: r.get::<i64, _>("team_only") != 0,
    }
}

/// A key that can work on `/v1`, with its hash. It has no `Debug`.
pub struct LiveKey {
    pub hash: String,
    pub id: i64,
    pub name: String,
    pub user_id: Option<i64>,
    pub team_id: Option<i64>,
    pub expires_at: Option<String>,
    pub allowed: Option<Vec<String>>,
    pub tags: Tags,
    /// It acts for its team only (see `access`).
    pub team_only: bool,
}

impl Store {
    /// The keys that are not revoked and whose owner, if there is one, is
    /// active. Expired keys are included: expiry is checked at use.
    pub async fn live_keys(&self) -> Result<Vec<LiveKey>> {
        let mut conn = self.pool().acquire().await?;
        live_keys_in(&mut conn).await
    }

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
        let sql = format!(
            "{KEY_SELECT}
             WHERE k.key_hash = ?
               AND k.org_id = ?
               AND k.revoked_at IS NULL
               AND (k.expires_at IS NULL OR k.expires_at > ?)"
        );
        let row = self
            .q_dyn(sql)
            .bind(hash)
            .bind(DEFAULT_ORG)
            .bind(now())
            .fetch_optional(self.pool())
            .await?;
        Ok(row.as_ref().map(key_from))
    }

    /// Finds a key whether or not it is live.
    pub async fn key_by_id(&self, id: i64) -> Result<Option<KeyRow>> {
        let sql = format!("{KEY_SELECT} WHERE k.id = ? AND k.org_id = ?");
        let row = self
            .q_dyn(sql)
            .bind(id)
            .bind(DEFAULT_ORG)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.as_ref().map(key_from))
    }

    /// Newest first, including revoked and expired keys.
    pub async fn list_keys(&self) -> Result<Vec<KeyRow>> {
        let sql = format!("{KEY_SELECT} WHERE k.org_id = ? {KEY_ORDER}");
        let rows = self
            .q_dyn(sql)
            .bind(DEFAULT_ORG)
            .fetch_all(self.pool())
            .await?;
        Ok(rows.iter().map(key_from).collect())
    }

    /// The keys that belong to any of the teams, and the keys owned by
    /// `own_id`. Newest first, including revoked and expired keys.
    pub async fn list_keys_in_teams(&self, team_ids: &[i64], own_id: i64) -> Result<Vec<KeyRow>> {
        let marks = vec!["?"; team_ids.len()].join(", ");
        // With no team the list is `IN (NULL)`, which matches nothing.
        let marks = if marks.is_empty() { "NULL" } else { &marks };
        let sql = format!(
            "{KEY_SELECT}
             WHERE k.org_id = ? AND (k.user_id = ? OR k.team_id IN ({marks}))
             {KEY_ORDER}"
        );
        let mut query = self.q_dyn(sql).bind(DEFAULT_ORG).bind(own_id);
        for team_id in team_ids {
            query = query.bind(team_id);
        }
        let rows = query.fetch_all(self.pool()).await?;
        Ok(rows.iter().map(key_from).collect())
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
        let id: i64 = self
            .scalar(
                "INSERT INTO virtual_keys
                 (org_id, name, key_hash, display, expires_at, user_id, team_id)
             VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING id",
            )
            .bind(DEFAULT_ORG)
            .bind(name)
            .bind(hash)
            .bind(display)
            .bind(expires_at)
            .bind(user_id)
            .bind(team_id)
            .fetch_one(self.conn())
            .await
            .map_err(write_error)?;
        Ok(id)
    }

    /// Records who made the key, and whether it acts for its team only.
    pub async fn set_key_origin(&mut self, id: i64, user_id: i64, team_only: bool) -> Result<()> {
        self.q("UPDATE virtual_keys SET created_by = ?, team_only = ? WHERE id = ? AND org_id = ?")
            .bind(user_id)
            .bind(flag(team_only))
            .bind(id)
            .bind(DEFAULT_ORG)
            .execute(self.conn())
            .await?;
        Ok(())
    }

    /// Sets the names the key may call. `None` removes the allowlist.
    pub async fn set_key_allowed(&mut self, id: i64, allowed: Option<&[String]>) -> Result<()> {
        let json = allowed.map(serde_json::to_string).transpose()?;
        self.q("UPDATE virtual_keys SET allowed = ? WHERE id = ? AND org_id = ?")
            .bind(json)
            .bind(id)
            .bind(DEFAULT_ORG)
            .execute(self.conn())
            .await?;
        Ok(())
    }

    /// Sets the tags of the key; an empty set removes them.
    pub async fn set_key_tags(&mut self, id: i64, tags: &Tags) -> Result<bool> {
        let r = self
            .q("UPDATE virtual_keys SET tags = ? WHERE id = ? AND org_id = ?")
            .bind(tags::to_stored(tags))
            .bind(id)
            .bind(DEFAULT_ORG)
            .execute(self.conn())
            .await?;
        Ok(r.rows_affected() == 1)
    }

    /// Revokes every key of the user that is not revoked yet. Returns how many.
    pub async fn revoke_keys_of(&mut self, user_id: i64) -> Result<u64> {
        let r = self
            .q("UPDATE virtual_keys SET revoked_at = ?
             WHERE user_id = ? AND org_id = ? AND revoked_at IS NULL")
            .bind(now())
            .bind(user_id)
            .bind(DEFAULT_ORG)
            .execute(self.conn())
            .await?;
        Ok(r.rows_affected())
    }

    /// Revokes the team keys the user owns: such a key needs its owner.
    pub async fn revoke_team_keys_of(&mut self, user_id: i64) -> Result<u64> {
        let r = self
            .q("UPDATE virtual_keys SET revoked_at = ?
             WHERE user_id = ? AND org_id = ? AND team_only = 1 AND revoked_at IS NULL")
            .bind(now())
            .bind(user_id)
            .bind(DEFAULT_ORG)
            .execute(self.conn())
            .await?;
        Ok(r.rows_affected())
    }

    /// How many keys of the user work: not revoked and not expired.
    pub async fn count_live_keys_of(&mut self, user_id: i64) -> Result<i64> {
        let count = self
            .scalar(
                "SELECT COUNT(*) FROM virtual_keys
             WHERE user_id = ? AND org_id = ? AND revoked_at IS NULL
               AND (expires_at IS NULL OR expires_at > ?)",
            )
            .bind(user_id)
            .bind(DEFAULT_ORG)
            .bind(now())
            .fetch_one(self.conn())
            .await?;
        Ok(count)
    }

    /// Returns whether a live key was revoked. Revoking again changes nothing.
    pub async fn revoke_key(&mut self, id: i64) -> Result<bool> {
        let r = self
            .q("UPDATE virtual_keys SET revoked_at = ?
             WHERE id = ? AND org_id = ? AND revoked_at IS NULL")
            .bind(now())
            .bind(id)
            .bind(DEFAULT_ORG)
            .execute(self.conn())
            .await?;
        Ok(r.rows_affected() == 1)
    }
}

pub(crate) async fn live_keys_in(conn: &mut AnyConnection) -> Result<Vec<LiveKey>> {
    let rows = conn
        .q(
            "SELECT k.key_hash, k.id, k.name, k.user_id, k.team_id, k.expires_at, k.allowed,
                k.tags, k.team_only
         FROM virtual_keys k
         LEFT JOIN users u ON u.id = k.user_id AND u.org_id = k.org_id
         WHERE k.org_id = ?
           AND k.revoked_at IS NULL
           AND (k.user_id IS NULL OR u.status = 'active')",
        )
        .bind(DEFAULT_ORG)
        .fetch_all(&mut *conn)
        .await?;
    Ok(rows
        .iter()
        .map(|r| LiveKey {
            hash: r.get("key_hash"),
            id: r.get("id"),
            name: r.get("name"),
            user_id: r.get("user_id"),
            team_id: r.get("team_id"),
            expires_at: r.get("expires_at"),
            allowed: parse_allowed(r.get::<Option<String>, _>("allowed").as_deref()),
            tags: tags::parse_stored(r.get::<Option<String>, _>("tags").as_deref()),
            team_only: r.get::<i64, _>("team_only") != 0,
        })
        .collect())
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
    async fn revoking_the_keys_of_a_user_leaves_the_rest() {
        use crate::identity::{Role, UserStatus};
        use crate::store::NewUser;

        let s = Store::open_in_memory().await.unwrap();
        let mut tx = s.begin().await.unwrap();
        let mut users = Vec::new();
        for email in ["lena@example.com", "tomas@example.com"] {
            let user = NewUser {
                email,
                name: "User",
                role: Role::Member,
                status: UserStatus::Active,
                password_hash: None,
            };
            users.push(tx.insert_user(user).await.unwrap());
        }
        let (lena, tomas) = (users[0], users[1]);
        let one = tx
            .insert_key("one", "h1", "d1", None, Some(lena), None)
            .await
            .unwrap();
        let gone = tx
            .insert_key("gone", "h2", "d2", None, Some(lena), None)
            .await
            .unwrap();
        tx.insert_key("other", "h3", "d3", None, Some(tomas), None)
            .await
            .unwrap();
        tx.insert_key("legacy", "h4", "d4", None, None, None)
            .await
            .unwrap();
        tx.revoke_key(gone).await.unwrap();
        tx.commit().await.unwrap();
        sqlx::query("UPDATE virtual_keys SET revoked_at = '2000-01-01 00:00:00' WHERE id = ?")
            .bind(gone)
            .execute(s.pool())
            .await
            .unwrap();

        let mut tx = s.begin().await.unwrap();
        // Lena's key that has expired does not count either.
        tx.insert_key(
            "old",
            "h5",
            "d5",
            Some("2000-01-01 00:00:00"),
            Some(lena),
            None,
        )
        .await
        .unwrap();
        assert_eq!(tx.count_live_keys_of(lena).await.unwrap(), 1);
        assert_eq!(tx.count_live_keys_of(tomas).await.unwrap(), 1);
        assert_eq!(tx.count_live_keys_of(tomas + 100).await.unwrap(), 0);
        tx.commit().await.unwrap();

        let mut tx = s.begin().await.unwrap();
        assert_eq!(tx.revoke_keys_of(lena).await.unwrap(), 2);
        assert_eq!(tx.revoke_keys_of(lena).await.unwrap(), 0);
        assert_eq!(tx.revoke_keys_of(tomas + 100).await.unwrap(), 0);
        tx.commit().await.unwrap();

        assert!(revoked_at(&s, one).await.is_some());
        // The earlier revocation keeps its time.
        assert_eq!(
            revoked_at(&s, gone).await.as_deref(),
            Some("2000-01-01 00:00:00")
        );
        assert!(s.active_key_by_hash("h3").await.unwrap().is_some());
        assert!(s.active_key_by_hash("h4").await.unwrap().is_some());
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
    async fn keys_are_listed_with_owner_and_team() {
        use crate::identity::{Role, TeamRole, UserStatus};
        use crate::store::NewUser;

        let s = Store::open_in_memory().await.unwrap();
        let mut tx = s.begin().await.unwrap();
        let lena = tx
            .insert_user(NewUser {
                email: "lena@example.com",
                name: "Lena",
                role: Role::Member,
                status: UserStatus::Active,
                password_hash: None,
            })
            .await
            .unwrap();
        let team = tx.insert_team("Platform").await.unwrap();
        tx.put_member(team, lena, TeamRole::Member).await.unwrap();
        let legacy = tx
            .insert_key("old", "h1", "d1", None, None, None)
            .await
            .unwrap();
        let own = tx
            .insert_key("own", "h2", "d2", None, Some(lena), None)
            .await
            .unwrap();
        let shared = tx
            .insert_key("shared", "h3", "d3", None, None, Some(team))
            .await
            .unwrap();
        tx.revoke_key(shared).await.unwrap();
        tx.commit().await.unwrap();

        let ids = |rows: Vec<KeyRow>| rows.into_iter().map(|k| k.id).collect::<Vec<_>>();
        assert_eq!(ids(s.list_keys().await.unwrap()), [shared, own, legacy]);
        assert_eq!(
            ids(s.list_keys_in_teams(&[team], lena).await.unwrap()),
            [shared, own]
        );
        assert_eq!(ids(s.list_keys_in_teams(&[], lena).await.unwrap()), [own]);
        assert!(s
            .list_keys_in_teams(&[], lena + 100)
            .await
            .unwrap()
            .is_empty());

        let k = s.key_by_id(own).await.unwrap().unwrap();
        assert_eq!(k.owner_email.as_deref(), Some("lena@example.com"));
        assert_eq!(k.team_name, None);
        let k = s.key_by_id(shared).await.unwrap().unwrap();
        assert_eq!(k.owner_email, None);
        assert_eq!(k.team_name.as_deref(), Some("Platform"));
        assert!(k.revoked_at.is_some());
        assert!(s.key_by_id(shared + 100).await.unwrap().is_none());
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
