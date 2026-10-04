//! Sign-in sessions and access tokens.

use std::fmt;

use anyhow::{Context, Result};
use sqlx::sqlite::SqliteRow;
use sqlx::{AssertSqlSafe, Row};

use super::{after, check_timestamp, write_error, Store, Tx, DEFAULT_ORG};
use crate::secrets::{hash_key, TOKEN_PREFIX};

/// How long a session lasts after sign-in.
pub const SESSION_SECONDS: i64 = super::DEFAULT_SESSION_HOURS * 60 * 60;

/// Length of a session cookie value: 32 bytes as hex.
const SESSION_VALUE_LEN: usize = 64;

const TOKEN_COLUMNS: &str =
    "id, user_id, name, display, expires_at, revoked_at, last_used_at, created_at";

const TOKEN_IS_LIVE: &str =
    "revoked_at IS NULL AND (expires_at IS NULL OR expires_at > datetime('now'))";

/// A session that was just created. Both fields are secrets, so this type
/// has no `Debug`.
pub struct NewSession {
    /// The cookie value. Shown to the browser once. Only its hash is stored.
    pub id: String,
    pub csrf_token: String,
    /// How long the session lives, in seconds: the cookie's `Max-Age`.
    pub max_age_seconds: i64,
}

#[derive(Clone, PartialEq, Eq)]
pub struct SessionRow {
    pub id: i64,
    pub user_id: i64,
    pub csrf_token: String,
    pub expires_at: String,
}

/// Redacts the CSRF token, so logging a `SessionRow` cannot leak it.
impl fmt::Debug for SessionRow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionRow")
            .field("id", &self.id)
            .field("user_id", &self.user_id)
            .field("csrf_token", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// An access token as stored. It never holds the token or its hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenRow {
    pub id: i64,
    pub user_id: i64,
    pub name: String,
    pub display: String,
    pub expires_at: Option<String>,
    pub revoked_at: Option<String>,
    pub last_used_at: Option<String>,
    pub created_at: String,
}

fn token_from(r: &SqliteRow) -> TokenRow {
    TokenRow {
        id: r.get("id"),
        user_id: r.get("user_id"),
        name: r.get("name"),
        display: r.get("display"),
        expires_at: r.get("expires_at"),
        revoked_at: r.get("revoked_at"),
        last_used_at: r.get("last_used_at"),
        created_at: r.get("created_at"),
    }
}

/// 32 random bytes as lowercase hex.
fn random_hex() -> String {
    let mut bytes = [0u8; 32];
    crate::secrets::fill_random(&mut bytes);
    hex::encode(bytes)
}

/// Whether `value` has the shape of a session cookie value: exactly 64
/// lowercase hex characters.
fn is_session_value(value: &str) -> bool {
    value.len() == SESSION_VALUE_LEN
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl Store {
    /// Creates a session; see `Tx::create_session`.
    pub async fn create_session(&self, user_id: i64) -> Result<NewSession> {
        let mut tx = self.begin().await?;
        let session = tx.create_session(user_id).await?;
        tx.commit().await?;
        Ok(session)
    }

    /// Finds the session for a cookie value, unless it has expired. It does
    /// not check the user's status.
    pub async fn live_session(&self, cookie_value: &str) -> Result<Option<SessionRow>> {
        if !is_session_value(cookie_value) {
            return Ok(None);
        }
        let row = sqlx::query(
            "SELECT id, user_id, csrf_token, expires_at FROM sessions
             WHERE id_hash = ? AND org_id = ? AND expires_at > datetime('now')",
        )
        .bind(hash_key(cookie_value))
        .bind(DEFAULT_ORG)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|r| SessionRow {
            id: r.get("id"),
            user_id: r.get("user_id"),
            csrf_token: r.get("csrf_token"),
            expires_at: r.get("expires_at"),
        }))
    }

    /// Returns `false` if there was no such session.
    pub async fn delete_session(&self, cookie_value: &str) -> Result<bool> {
        if !is_session_value(cookie_value) {
            return Ok(false);
        }
        let r = sqlx::query("DELETE FROM sessions WHERE id_hash = ? AND org_id = ?")
            .bind(hash_key(cookie_value))
            .bind(DEFAULT_ORG)
            .execute(self.pool())
            .await?;
        Ok(r.rows_affected() == 1)
    }

    /// Returns how many sessions were deleted.
    pub async fn delete_sessions_of(&self, user_id: i64) -> Result<u64> {
        let r = sqlx::query("DELETE FROM sessions WHERE user_id = ? AND org_id = ?")
            .bind(user_id)
            .bind(DEFAULT_ORG)
            .execute(self.pool())
            .await?;
        Ok(r.rows_affected())
    }

    /// Returns how many sessions were deleted.
    pub async fn delete_expired_sessions(&self) -> Result<u64> {
        let r =
            sqlx::query("DELETE FROM sessions WHERE org_id = ? AND expires_at <= datetime('now')")
                .bind(DEFAULT_ORG)
                .execute(self.pool())
                .await?;
        Ok(r.rows_affected())
    }

    /// Finds the access token with this full value, unless it is revoked or
    /// has expired. It does not check the user's status.
    pub async fn live_token(&self, token: &str) -> Result<Option<TokenRow>> {
        if !token.starts_with(TOKEN_PREFIX) {
            return Ok(None);
        }
        let sql = format!(
            "SELECT {TOKEN_COLUMNS} FROM access_tokens
             WHERE token_hash = ? AND org_id = ? AND {TOKEN_IS_LIVE}"
        );
        let row = sqlx::query(AssertSqlSafe(sql))
            .bind(hash_key(token))
            .bind(DEFAULT_ORG)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.as_ref().map(token_from))
    }

    /// Records that the token was used just now.
    pub async fn touch_token(&self, id: i64) -> Result<()> {
        sqlx::query(
            "UPDATE access_tokens SET last_used_at = datetime('now') WHERE id = ? AND org_id = ?",
        )
        .bind(id)
        .bind(DEFAULT_ORG)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    /// Finds a token whether or not it is live.
    pub async fn token_by_id(&self, id: i64) -> Result<Option<TokenRow>> {
        let sql = format!("SELECT {TOKEN_COLUMNS} FROM access_tokens WHERE id = ? AND org_id = ?");
        let row = sqlx::query(AssertSqlSafe(sql))
            .bind(id)
            .bind(DEFAULT_ORG)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.as_ref().map(token_from))
    }

    /// Newest first, including revoked and expired tokens.
    pub async fn list_tokens_of(&self, user_id: i64) -> Result<Vec<TokenRow>> {
        let sql = format!(
            "SELECT {TOKEN_COLUMNS} FROM access_tokens
             WHERE user_id = ? AND org_id = ?
             ORDER BY created_at DESC, id DESC"
        );
        let rows = sqlx::query(AssertSqlSafe(sql))
            .bind(user_id)
            .bind(DEFAULT_ORG)
            .fetch_all(self.pool())
            .await?;
        Ok(rows.iter().map(token_from).collect())
    }
}

impl Tx<'_> {
    /// Creates a session that lives as many hours as the settings say
    /// (`SESSION_SECONDS` unless they say otherwise).
    pub async fn create_session(&mut self, user_id: i64) -> Result<NewSession> {
        let max_age_seconds = self.session_hours().await? * 3600;
        let session = NewSession {
            id: random_hex(),
            csrf_token: random_hex(),
            max_age_seconds,
        };
        sqlx::query(
            "INSERT INTO sessions (org_id, user_id, id_hash, csrf_token, expires_at)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(DEFAULT_ORG)
        .bind(user_id)
        .bind(hash_key(&session.id))
        .bind(&session.csrf_token)
        .bind(after(max_age_seconds))
        .execute(self.conn())
        .await?;
        Ok(session)
    }

    /// Deletes the session with this row id. Returns `false` if there was
    /// no such session.
    pub async fn delete_session_by_id(&mut self, id: i64) -> Result<bool> {
        let r = sqlx::query("DELETE FROM sessions WHERE id = ? AND org_id = ?")
            .bind(id)
            .bind(DEFAULT_ORG)
            .execute(self.conn())
            .await?;
        Ok(r.rows_affected() == 1)
    }

    /// Deletes the user's sessions except the one with row id `keep`, and
    /// returns how many that was.
    pub async fn delete_other_sessions_of(&mut self, user_id: i64, keep: i64) -> Result<u64> {
        let r = sqlx::query("DELETE FROM sessions WHERE user_id = ? AND org_id = ? AND id != ?")
            .bind(user_id)
            .bind(DEFAULT_ORG)
            .bind(keep)
            .execute(self.conn())
            .await?;
        Ok(r.rows_affected())
    }

    /// `hash` is the SHA-256 hex of the full token. `expires_at`, when
    /// given, must be UTC in the form `YYYY-MM-DD HH:MM:SS`.
    pub async fn insert_token(
        &mut self,
        user_id: i64,
        name: &str,
        hash: &str,
        display: &str,
        expires_at: Option<&str>,
    ) -> Result<i64> {
        if let Some(at) = expires_at {
            check_timestamp(at).context("expires_at is not valid")?;
        }
        let r = sqlx::query(
            "INSERT INTO access_tokens (org_id, user_id, name, token_hash, display, expires_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(DEFAULT_ORG)
        .bind(user_id)
        .bind(name)
        .bind(hash)
        .bind(display)
        .bind(expires_at)
        .execute(self.conn())
        .await
        .map_err(write_error)?;
        Ok(r.last_insert_rowid())
    }

    /// Returns `false` if there is no such token or it was already revoked.
    pub async fn revoke_token(&mut self, id: i64) -> Result<bool> {
        let r = sqlx::query(
            "UPDATE access_tokens SET revoked_at = datetime('now')
             WHERE id = ? AND org_id = ? AND revoked_at IS NULL",
        )
        .bind(id)
        .bind(DEFAULT_ORG)
        .execute(self.conn())
        .await?;
        Ok(r.rows_affected() == 1)
    }

    /// Returns how many sessions were deleted.
    pub async fn delete_sessions_of(&mut self, user_id: i64) -> Result<u64> {
        let r = sqlx::query("DELETE FROM sessions WHERE user_id = ? AND org_id = ?")
            .bind(user_id)
            .bind(DEFAULT_ORG)
            .execute(self.conn())
            .await?;
        Ok(r.rows_affected())
    }

    /// Revokes every token of the user that is not yet revoked, and returns
    /// how many that was.
    pub async fn revoke_tokens_of(&mut self, user_id: i64) -> Result<u64> {
        let r = sqlx::query(
            "UPDATE access_tokens SET revoked_at = datetime('now')
             WHERE user_id = ? AND org_id = ? AND revoked_at IS NULL",
        )
        .bind(user_id)
        .bind(DEFAULT_ORG)
        .execute(self.conn())
        .await?;
        Ok(r.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{Role, UserStatus};
    use crate::secrets::{generate_secret, NewKey, KEY_PREFIX};
    use crate::store::{check_timestamp, NewUser, StoreError};

    async fn add_user(s: &Store, email: &str) -> i64 {
        let mut tx = s.begin().await.unwrap();
        let id = tx
            .insert_user(NewUser {
                email,
                name: "Someone",
                role: Role::Member,
                status: UserStatus::Active,
                password_hash: None,
            })
            .await
            .unwrap();
        tx.commit().await.unwrap();
        id
    }

    async fn add_token(
        s: &Store,
        user_id: i64,
        name: &str,
        expires_at: Option<&str>,
    ) -> (i64, NewKey) {
        let t = generate_secret(TOKEN_PREFIX);
        let mut tx = s.begin().await.unwrap();
        let id = tx
            .insert_token(user_id, name, &t.hash, &t.display, expires_at)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        (id, t)
    }

    async fn count(s: &Store, table: &str) -> i64 {
        sqlx::query_scalar(AssertSqlSafe(format!("SELECT COUNT(*) FROM {table}")))
            .fetch_one(s.pool())
            .await
            .unwrap()
    }

    #[test]
    fn session_lasts_twelve_hours() {
        assert_eq!(SESSION_SECONDS, 43_200);
    }

    #[tokio::test]
    async fn session_round_trip() {
        let s = Store::open_in_memory().await.unwrap();
        let user = add_user(&s, "maya@example.com").await;
        let new = s.create_session(user).await.unwrap();
        assert!(is_session_value(&new.id));
        assert!(is_session_value(&new.csrf_token));
        assert_ne!(new.id, new.csrf_token);

        let row = s.live_session(&new.id).await.unwrap().unwrap();
        assert_eq!(row.user_id, user);
        assert_eq!(row.csrf_token, new.csrf_token);
        assert!(check_timestamp(&row.expires_at).is_ok());
        assert!(row.expires_at >= after(SESSION_SECONDS - 5));
        assert!(row.expires_at <= after(SESSION_SECONDS + 5));

        let other = s.create_session(user).await.unwrap();
        assert_ne!(other.id, new.id);
        assert_ne!(other.csrf_token, new.csrf_token);
    }

    #[tokio::test]
    async fn session_value_is_not_stored() {
        let s = Store::open_in_memory().await.unwrap();
        let user = add_user(&s, "maya@example.com").await;
        let new = s.create_session(user).await.unwrap();
        let hash: String = sqlx::query_scalar("SELECT id_hash FROM sessions")
            .fetch_one(s.pool())
            .await
            .unwrap();
        assert_eq!(hash, hash_key(&new.id));
        assert_ne!(hash, new.id);

        let row = sqlx::query(
            "SELECT CAST(id AS TEXT), CAST(org_id AS TEXT), CAST(user_id AS TEXT),
                    id_hash, csrf_token, expires_at, created_at
             FROM sessions",
        )
        .fetch_one(s.pool())
        .await
        .unwrap();
        let columns: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pragma_table_info('sessions')")
            .fetch_one(s.pool())
            .await
            .unwrap();
        assert_eq!(columns, 7, "a new column must be added to this test");
        for i in 0..7 {
            let value: String = row.get(i);
            assert!(
                !value.contains(&new.id),
                "column {i} holds the cookie value"
            );
        }
        // The hash itself is not a usable cookie value.
        assert!(s.live_session(&hash).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn malformed_session_values_find_nothing() {
        let s = Store::open_in_memory().await.unwrap();
        let user = add_user(&s, "maya@example.com").await;
        let new = s.create_session(user).await.unwrap();
        let upper = new.id.to_uppercase();
        let with_g = format!("{}g", &new.id[..63]);
        let too_long = format!("{}a", new.id);
        // Closing the pool proves that none of these reaches the database.
        let closed = Store::open_in_memory().await.unwrap();
        closed.pool().close().await;
        assert!(closed.live_session(&new.id).await.is_err());
        for bad in ["", "abc", &upper, &with_g, &too_long] {
            assert!(s.live_session(bad).await.unwrap().is_none());
            assert!(closed.live_session(bad).await.unwrap().is_none());
            assert!(!closed.delete_session(bad).await.unwrap());
        }
        assert_eq!(upper.len(), 64);
        assert_eq!(with_g.len(), 64);
        assert_eq!(too_long.len(), 65);
        assert!(s.live_session(&new.id).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn expired_session_is_not_live() {
        let s = Store::open_in_memory().await.unwrap();
        let user = add_user(&s, "maya@example.com").await;
        let old = s.create_session(user).await.unwrap();
        let fresh = s.create_session(user).await.unwrap();
        assert_eq!(s.delete_expired_sessions().await.unwrap(), 0);
        sqlx::query("UPDATE sessions SET expires_at = ? WHERE id_hash = ?")
            .bind(after(-1))
            .bind(hash_key(&old.id))
            .execute(s.pool())
            .await
            .unwrap();
        assert!(s.live_session(&old.id).await.unwrap().is_none());
        assert_eq!(s.delete_expired_sessions().await.unwrap(), 1);
        assert_eq!(s.delete_expired_sessions().await.unwrap(), 0);
        assert!(s.live_session(&fresh.id).await.unwrap().is_some());
        assert_eq!(count(&s, "sessions").await, 1);
    }

    #[tokio::test]
    async fn delete_session() {
        let s = Store::open_in_memory().await.unwrap();
        let user = add_user(&s, "maya@example.com").await;
        let new = s.create_session(user).await.unwrap();
        let other = s.create_session(user).await.unwrap();
        assert!(s.delete_session(&new.id).await.unwrap());
        assert!(!s.delete_session(&new.id).await.unwrap());
        assert!(s.live_session(&new.id).await.unwrap().is_none());
        assert!(s.live_session(&other.id).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn delete_sessions_of_user() {
        let s = Store::open_in_memory().await.unwrap();
        let maya = add_user(&s, "maya@example.com").await;
        let omar = add_user(&s, "omar@example.com").await;
        let a = s.create_session(maya).await.unwrap();
        let b = s.create_session(maya).await.unwrap();
        let c = s.create_session(omar).await.unwrap();
        assert_eq!(s.delete_sessions_of(maya).await.unwrap(), 2);
        assert_eq!(s.delete_sessions_of(maya).await.unwrap(), 0);
        assert!(s.live_session(&a.id).await.unwrap().is_none());
        assert!(s.live_session(&b.id).await.unwrap().is_none());
        assert!(s.live_session(&c.id).await.unwrap().is_some());
    }

    async fn s_row_id(tx: &mut Tx<'_>, cookie_value: &str) -> i64 {
        sqlx::query_scalar("SELECT id FROM sessions WHERE id_hash = ?")
            .bind(hash_key(cookie_value))
            .fetch_one(tx.conn())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn sessions_are_deleted_by_row_id() {
        let s = Store::open_in_memory().await.unwrap();
        let user = add_user(&s, "maya@example.com").await;
        let a = s.create_session(user).await.unwrap();
        let b = s.create_session(user).await.unwrap();
        let id = s.live_session(&a.id).await.unwrap().unwrap().id;
        let mut tx = s.begin().await.unwrap();
        assert!(tx.delete_session_by_id(id).await.unwrap());
        assert!(!tx.delete_session_by_id(id).await.unwrap());
        assert!(!tx.delete_session_by_id(id + 1000).await.unwrap());
        tx.commit().await.unwrap();
        assert!(s.live_session(&a.id).await.unwrap().is_none());
        assert!(s.live_session(&b.id).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn other_sessions_of_a_user_can_be_deleted() {
        let s = Store::open_in_memory().await.unwrap();
        let maya = add_user(&s, "maya@example.com").await;
        let omar = add_user(&s, "omar@example.com").await;
        let a = s.create_session(maya).await.unwrap();
        let b = s.create_session(maya).await.unwrap();
        let c = s.create_session(omar).await.unwrap();
        let keep = s.live_session(&a.id).await.unwrap().unwrap().id;

        // A session created in a transaction that is dropped does not exist.
        let dropped = {
            let mut tx = s.begin().await.unwrap();
            tx.create_session(maya).await.unwrap()
        };
        assert!(s.live_session(&dropped.id).await.unwrap().is_none());

        let mut tx = s.begin().await.unwrap();
        let gone = s_row_id(&mut tx, &c.id).await;
        assert!(tx.delete_session_by_id(gone).await.unwrap());
        assert!(!tx.delete_session_by_id(gone).await.unwrap());
        drop(tx);
        assert!(s.live_session(&c.id).await.unwrap().is_some());

        let mut tx = s.begin().await.unwrap();
        assert_eq!(tx.delete_other_sessions_of(maya, keep).await.unwrap(), 1);
        assert_eq!(tx.delete_other_sessions_of(maya, keep).await.unwrap(), 0);
        tx.commit().await.unwrap();
        assert!(s.live_session(&a.id).await.unwrap().is_some());
        assert!(s.live_session(&b.id).await.unwrap().is_none());
        assert!(s.live_session(&c.id).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn transaction_ends_sessions_and_tokens_of_user() {
        let s = Store::open_in_memory().await.unwrap();
        let maya = add_user(&s, "maya@example.com").await;
        let omar = add_user(&s, "omar@example.com").await;
        let a = s.create_session(maya).await.unwrap();
        let b = s.create_session(maya).await.unwrap();
        let c = s.create_session(omar).await.unwrap();
        let (first, t1) = add_token(&s, maya, "ci", None).await;
        let (_, t2) = add_token(&s, maya, "laptop", None).await;
        let (_, t3) = add_token(&s, omar, "ci", None).await;

        // A rolled back transaction changes nothing.
        {
            let mut tx = s.begin().await.unwrap();
            assert_eq!(tx.delete_sessions_of(maya).await.unwrap(), 2);
            assert_eq!(tx.revoke_tokens_of(maya).await.unwrap(), 2);
        }
        assert!(s.live_session(&a.id).await.unwrap().is_some());
        assert!(s.live_token(&t1.full).await.unwrap().is_some());

        let mut tx = s.begin().await.unwrap();
        assert!(tx.revoke_token(first).await.unwrap());
        assert_eq!(tx.delete_sessions_of(maya).await.unwrap(), 2);
        assert_eq!(tx.revoke_tokens_of(maya).await.unwrap(), 1);
        assert_eq!(tx.revoke_tokens_of(maya).await.unwrap(), 0);
        tx.commit().await.unwrap();

        assert!(s.live_session(&a.id).await.unwrap().is_none());
        assert!(s.live_session(&b.id).await.unwrap().is_none());
        assert!(s.live_session(&c.id).await.unwrap().is_some());
        assert!(s.live_token(&t1.full).await.unwrap().is_none());
        assert!(s.live_token(&t2.full).await.unwrap().is_none());
        assert!(s.live_token(&t3.full).await.unwrap().is_some());
        assert_eq!(s.list_tokens_of(maya).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn session_debug_hides_csrf() {
        let s = Store::open_in_memory().await.unwrap();
        let user = add_user(&s, "maya@example.com").await;
        let new = s.create_session(user).await.unwrap();
        let row = s.live_session(&new.id).await.unwrap().unwrap();
        for shown in [format!("{row:?}"), format!("{row:#?}")] {
            assert!(!shown.contains(&row.csrf_token));
            assert!(!shown.contains(&new.id));
            assert!(!shown.contains(&hash_key(&new.id)));
            assert!(shown.contains("<redacted>"));
            assert!(shown.contains(&row.expires_at));
        }
    }

    #[tokio::test]
    async fn token_round_trip() {
        let s = Store::open_in_memory().await.unwrap();
        let user = add_user(&s, "maya@example.com").await;
        let (id, t) = add_token(&s, user, "ci", Some("2999-01-01 00:00:00")).await;

        let row = s.live_token(&t.full).await.unwrap().unwrap();
        assert_eq!(row.id, id);
        assert_eq!(row.user_id, user);
        assert_eq!(row.name, "ci");
        assert_eq!(row.display, t.display);
        assert_eq!(row.expires_at.as_deref(), Some("2999-01-01 00:00:00"));
        assert_eq!(row.revoked_at, None);
        assert_eq!(row.last_used_at, None);
        assert!(check_timestamp(&row.created_at).is_ok());
        assert_eq!(s.token_by_id(id).await.unwrap(), Some(row.clone()));
        assert_eq!(s.token_by_id(id + 1).await.unwrap(), None);

        assert!(s.live_token(&t.hash).await.unwrap().is_none());
        let other = generate_secret(TOKEN_PREFIX);
        assert!(s.live_token(&other.full).await.unwrap().is_none());
        let bare = &t.full[TOKEN_PREFIX.len()..];
        assert!(s.live_token(bare).await.unwrap().is_none());
        assert!(s.live_token("").await.unwrap().is_none());
        assert!(s.live_token(TOKEN_PREFIX).await.unwrap().is_none());

        let shown = format!("{row:?}");
        assert!(!shown.contains(&t.full));
        assert!(!shown.contains(bare));
        assert!(!shown.contains(&t.hash));
    }

    #[tokio::test]
    async fn values_without_the_token_prefix_are_not_looked_up() {
        let s = Store::open_in_memory().await.unwrap();
        let user = add_user(&s, "maya@example.com").await;
        // Stored under the hash of a value with the wrong prefix: a lookup
        // by hash alone would find it.
        let key = generate_secret(KEY_PREFIX);
        let mut tx = s.begin().await.unwrap();
        tx.insert_token(user, "odd", &key.hash, &key.display, None)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        assert!(s.live_token(&key.full).await.unwrap().is_none());

        let closed = Store::open_in_memory().await.unwrap();
        closed.pool().close().await;
        assert!(closed.live_token(&key.full).await.unwrap().is_none());
        assert!(closed.live_token("UF-AT-abc").await.unwrap().is_none());
        assert!(closed.live_token(" uf-at-abc").await.unwrap().is_none());
        assert!(closed.live_token("uf-at-abc").await.is_err());
    }

    #[tokio::test]
    async fn token_only_the_hash_is_stored() {
        let s = Store::open_in_memory().await.unwrap();
        let user = add_user(&s, "maya@example.com").await;
        let (_, t) = add_token(&s, user, "ci", None).await;
        let row = sqlx::query("SELECT name, token_hash, display FROM access_tokens")
            .fetch_one(s.pool())
            .await
            .unwrap();
        assert_eq!(row.get::<String, _>("token_hash"), t.hash);
        let secret = &t.full[TOKEN_PREFIX.len()..];
        for column in ["name", "token_hash", "display"] {
            assert!(!row.get::<String, _>(column).contains(secret));
        }
    }

    #[tokio::test]
    async fn revoked_and_expired_tokens_are_not_live() {
        let s = Store::open_in_memory().await.unwrap();
        let user = add_user(&s, "maya@example.com").await;
        let (revoked, t1) = add_token(&s, user, "revoked", None).await;
        let (expired, t2) = add_token(&s, user, "expired", Some("2000-01-01 00:00:00")).await;
        let (_, t3) = add_token(&s, user, "live", Some(&after(60))).await;
        assert!(s.live_token(&t1.full).await.unwrap().is_some());
        assert!(s.live_token(&t2.full).await.unwrap().is_none());

        let mut tx = s.begin().await.unwrap();
        assert!(tx.revoke_token(revoked).await.unwrap());
        assert!(!tx.revoke_token(revoked).await.unwrap());
        assert!(!tx.revoke_token(9999).await.unwrap());
        tx.commit().await.unwrap();

        assert!(s.live_token(&t1.full).await.unwrap().is_none());
        assert!(s.live_token(&t3.full).await.unwrap().is_some());
        let row = s.token_by_id(revoked).await.unwrap().unwrap();
        assert!(check_timestamp(row.revoked_at.as_deref().unwrap()).is_ok());
        let row = s.token_by_id(expired).await.unwrap().unwrap();
        assert_eq!(row.revoked_at, None);
    }

    #[tokio::test]
    async fn token_expiry_is_validated() {
        let s = Store::open_in_memory().await.unwrap();
        let user = add_user(&s, "maya@example.com").await;
        let t = generate_secret(TOKEN_PREFIX);
        let mut tx = s.begin().await.unwrap();
        let err = tx
            .insert_token(user, "ci", &t.hash, &t.display, Some("tomorrow"))
            .await
            .expect_err("must be rejected");
        assert!(!format!("{err:?}").contains(&t.hash));
        tx.commit().await.unwrap();
        assert_eq!(count(&s, "access_tokens").await, 0);
        assert!(s.list_tokens_of(user).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn duplicate_token_hash_is_reported() {
        let s = Store::open_in_memory().await.unwrap();
        let user = add_user(&s, "maya@example.com").await;
        let (_, t) = add_token(&s, user, "ci", None).await;
        let mut tx = s.begin().await.unwrap();
        let err = tx
            .insert_token(user, "again", &t.hash, &t.display, None)
            .await
            .expect_err("must be rejected");
        assert!(matches!(
            err.downcast_ref::<StoreError>(),
            Some(StoreError::Duplicate)
        ));
        assert!(!format!("{err:?}").contains(&t.hash));
    }

    #[tokio::test]
    async fn tokens_are_listed_newest_first_and_touched() {
        let s = Store::open_in_memory().await.unwrap();
        let maya = add_user(&s, "maya@example.com").await;
        let omar = add_user(&s, "omar@example.com").await;
        let (one, _) = add_token(&s, maya, "one", None).await;
        let (two, _) = add_token(&s, maya, "two", None).await;
        let (three, _) = add_token(&s, maya, "three", None).await;
        add_token(&s, omar, "other", None).await;
        sqlx::query("UPDATE access_tokens SET created_at = '2030-01-01 00:00:00' WHERE id = ?")
            .bind(one)
            .execute(s.pool())
            .await
            .unwrap();

        let ids: Vec<i64> = s
            .list_tokens_of(maya)
            .await
            .unwrap()
            .iter()
            .map(|t| t.id)
            .collect();
        assert_eq!(ids, [one, three, two]);

        s.touch_token(two).await.unwrap();
        let used = s.token_by_id(two).await.unwrap().unwrap().last_used_at;
        assert!(check_timestamp(used.as_deref().unwrap()).is_ok());
        assert_eq!(
            s.token_by_id(three).await.unwrap().unwrap().last_used_at,
            None
        );
    }

    #[tokio::test]
    async fn deleting_a_user_removes_sessions_and_tokens() {
        let s = Store::open_in_memory().await.unwrap();
        let maya = add_user(&s, "maya@example.com").await;
        let omar = add_user(&s, "omar@example.com").await;
        let session = s.create_session(maya).await.unwrap();
        let kept = s.create_session(omar).await.unwrap();
        let (id, t) = add_token(&s, maya, "ci", None).await;

        let mut tx = s.begin().await.unwrap();
        assert!(tx.delete_user(maya).await.unwrap());
        tx.commit().await.unwrap();

        assert!(s.live_session(&session.id).await.unwrap().is_none());
        assert!(s.live_token(&t.full).await.unwrap().is_none());
        assert!(s.token_by_id(id).await.unwrap().is_none());
        assert!(s.list_tokens_of(maya).await.unwrap().is_empty());
        assert_eq!(count(&s, "sessions").await, 1);
        assert_eq!(count(&s, "access_tokens").await, 0);
        assert!(s.live_session(&kept.id).await.unwrap().is_some());
    }
}
