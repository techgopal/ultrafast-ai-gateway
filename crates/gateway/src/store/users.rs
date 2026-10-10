//! Users and their invites.

use sqlx::AnyConnection;
use std::fmt;

use anyhow::{anyhow, Context, Result};
use sqlx::any::AnyRow;
use sqlx::Row;

use super::dialect::Dialected;
use super::{check_timestamp, now, write_error, Store, Tx, DEFAULT_ORG};
use crate::identity::{Role, UserStatus};

const USER_COLUMNS: &str =
    "id, email, name, role, status, password_hash, auth_provider, external_id, created_at, last_active_at";

const COUNT_ACTIVE_ADMINS: &str =
    "SELECT COUNT(*) FROM users WHERE org_id = ? AND role = 'admin' AND status = 'active'";

#[derive(Clone, PartialEq, Eq)]
pub struct UserRow {
    pub id: i64,
    pub email: String,
    pub name: String,
    pub role: Role,
    pub status: UserStatus,
    pub password_hash: Option<String>,
    /// `password`, or the id of the sign-in provider the user is linked to.
    pub auth_provider: String,
    /// The user's identity at that provider; `None` for password users.
    pub external_id: Option<String>,
    pub created_at: String,
    pub last_active_at: Option<String>,
}

/// Shows only whether a password hash is present, never the hash.
impl fmt::Debug for UserRow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let password_hash = if self.password_hash.is_some() {
            "<present>"
        } else {
            "<none>"
        };
        f.debug_struct("UserRow")
            .field("id", &self.id)
            .field("email", &self.email)
            .field("name", &self.name)
            .field("role", &self.role)
            .field("status", &self.status)
            .field("password_hash", &password_hash)
            .field("auth_provider", &self.auth_provider)
            .field("external_id", &self.external_id)
            .field("created_at", &self.created_at)
            .field("last_active_at", &self.last_active_at)
            .finish()
    }
}

/// `email` must already be normalized with `identity::normalize_email`.
/// No `Debug`: it carries a password hash.
pub struct NewUser<'a> {
    pub email: &'a str,
    pub name: &'a str,
    pub role: Role,
    pub status: UserStatus,
    pub password_hash: Option<&'a str>,
}

/// No `Debug`: rows are looked up by the hash of a secret token.
pub struct InviteRow {
    pub id: i64,
    pub user_id: i64,
    pub expires_at: String,
    /// `invite` (a new user chooses a password) or `set_password` (a user
    /// of the identity provider gets one).
    pub kind: String,
}

fn user_from(r: &AnyRow) -> Result<UserRow> {
    let role: String = r.get("role");
    let status: String = r.get("status");
    Ok(UserRow {
        id: r.get("id"),
        email: r.get("email"),
        name: r.get("name"),
        role: Role::parse(&role).ok_or_else(|| anyhow!("stored user role is not known"))?,
        status: UserStatus::parse(&status)
            .ok_or_else(|| anyhow!("stored user status is not known"))?,
        password_hash: r.get("password_hash"),
        auth_provider: r.get("auth_provider"),
        external_id: r.get("external_id"),
        created_at: r.get("created_at"),
        last_active_at: r.get("last_active_at"),
    })
}

impl Store {
    pub async fn count_users(&self) -> Result<i64> {
        let n = self
            .scalar("SELECT COUNT(*) FROM users WHERE org_id = ?")
            .bind(DEFAULT_ORG)
            .fetch_one(self.pool())
            .await?;
        Ok(n)
    }

    pub async fn count_active_admins(&self) -> Result<i64> {
        let n = self
            .scalar(COUNT_ACTIVE_ADMINS)
            .bind(DEFAULT_ORG)
            .fetch_one(self.pool())
            .await?;
        Ok(n)
    }

    pub async fn user_by_id(&self, id: i64) -> Result<Option<UserRow>> {
        let sql = format!("SELECT {USER_COLUMNS} FROM users WHERE id = ? AND org_id = ?");
        let row = self
            .q_dyn(sql)
            .bind(id)
            .bind(DEFAULT_ORG)
            .fetch_optional(self.pool())
            .await?;
        row.as_ref().map(user_from).transpose()
    }

    /// `email` must already be normalized.
    pub async fn user_by_email(&self, email: &str) -> Result<Option<UserRow>> {
        let sql = format!("SELECT {USER_COLUMNS} FROM users WHERE email = ? AND org_id = ?");
        let row = self
            .q_dyn(sql)
            .bind(email)
            .bind(DEFAULT_ORG)
            .fetch_optional(self.pool())
            .await?;
        row.as_ref().map(user_from).transpose()
    }

    /// The user linked to this identity at a sign-in provider, whatever
    /// the user's status.
    pub async fn user_by_external(
        &self,
        provider: &str,
        external_id: &str,
    ) -> Result<Option<UserRow>> {
        let sql = format!(
            "SELECT {USER_COLUMNS} FROM users
             WHERE org_id = ? AND auth_provider = ? AND external_id = ?"
        );
        let row = self
            .q_dyn(sql)
            .bind(DEFAULT_ORG)
            .bind(provider)
            .bind(external_id)
            .fetch_optional(self.pool())
            .await?;
        row.as_ref().map(user_from).transpose()
    }

    /// Ordered by email.
    pub async fn list_users(&self) -> Result<Vec<UserRow>> {
        let mut conn = self.pool().acquire().await?;
        list_users_in(&mut conn).await
    }

    /// The users who belong to any of the teams, and the user `own_id`.
    /// Ordered by email.
    pub async fn list_users_in_teams(&self, team_ids: &[i64], own_id: i64) -> Result<Vec<UserRow>> {
        let marks = vec!["?"; team_ids.len()].join(", ");
        // With no team the list is `IN (NULL)`, which matches nothing.
        let marks = if marks.is_empty() { "NULL" } else { &marks };
        let sql = format!(
            "SELECT {USER_COLUMNS} FROM users
             WHERE org_id = ?
               AND (id = ? OR id IN (
                   SELECT user_id FROM team_members
                   WHERE org_id = ? AND team_id IN ({marks})))
             ORDER BY email"
        );
        let mut query = self
            .q_dyn(sql)
            .bind(DEFAULT_ORG)
            .bind(own_id)
            .bind(DEFAULT_ORG);
        for team_id in team_ids {
            query = query.bind(team_id);
        }
        let rows = query.fetch_all(self.pool()).await?;
        rows.iter().map(user_from).collect()
    }

    /// Whether `user_id` belongs to at least one team that `lead_id` leads.
    pub async fn shares_led_team(&self, lead_id: i64, user_id: i64) -> Result<bool> {
        let found: i64 = self
            .scalar(
                "SELECT CASE WHEN EXISTS (
                 SELECT 1 FROM team_members led
                 JOIN team_members theirs ON theirs.team_id = led.team_id
                 WHERE led.user_id = ? AND led.role = 'lead' AND led.org_id = ?
                   AND theirs.user_id = ? AND theirs.org_id = ?) THEN 1 ELSE 0 END",
            )
            .bind(lead_id)
            .bind(DEFAULT_ORG)
            .bind(user_id)
            .bind(DEFAULT_ORG)
            .fetch_one(self.pool())
            .await?;
        Ok(found != 0)
    }

    /// Records that the user was active just now.
    pub async fn touch_user(&self, id: i64) -> Result<()> {
        self.q("UPDATE users SET last_active_at = ? WHERE id = ? AND org_id = ?")
            .bind(now())
            .bind(id)
            .bind(DEFAULT_ORG)
            .execute(self.pool())
            .await?;
        Ok(())
    }

    /// Finds an invite that is unused and has not expired.
    pub async fn invite_by_hash(&self, hash: &str) -> Result<Option<InviteRow>> {
        let row = self
            .q("SELECT id, user_id, expires_at, kind FROM invites
             WHERE token_hash = ?
               AND org_id = ?
               AND used_at IS NULL
               AND expires_at > ?")
            .bind(hash)
            .bind(DEFAULT_ORG)
            .bind(now())
            .fetch_optional(self.pool())
            .await?;
        Ok(row.map(|r| InviteRow {
            id: r.get("id"),
            user_id: r.get("user_id"),
            expires_at: r.get("expires_at"),
            kind: r.get("kind"),
        }))
    }
}

impl Tx<'_> {
    /// The user as the transaction sees them.
    pub async fn user_by_id(&mut self, id: i64) -> Result<Option<UserRow>> {
        let sql = format!("SELECT {USER_COLUMNS} FROM users WHERE id = ? AND org_id = ?");
        let row = self
            .q_dyn(sql)
            .bind(id)
            .bind(DEFAULT_ORG)
            .fetch_optional(self.conn())
            .await?;
        row.as_ref().map(user_from).transpose()
    }

    /// The user with this email as the transaction sees them. `email` must
    /// already be normalized.
    pub async fn user_by_email(&mut self, email: &str) -> Result<Option<UserRow>> {
        let sql = format!("SELECT {USER_COLUMNS} FROM users WHERE email = ? AND org_id = ?");
        let row = self
            .q_dyn(sql)
            .bind(email)
            .bind(DEFAULT_ORG)
            .fetch_optional(self.conn())
            .await?;
        row.as_ref().map(user_from).transpose()
    }

    /// The user linked to this identity at a sign-in provider, as the
    /// transaction sees them, whatever the user's status.
    pub async fn user_by_external(
        &mut self,
        provider: &str,
        external_id: &str,
    ) -> Result<Option<UserRow>> {
        let sql = format!(
            "SELECT {USER_COLUMNS} FROM users
             WHERE org_id = ? AND auth_provider = ? AND external_id = ?"
        );
        let row = self
            .q_dyn(sql)
            .bind(DEFAULT_ORG)
            .bind(provider)
            .bind(external_id)
            .fetch_optional(self.conn())
            .await?;
        row.as_ref().map(user_from).transpose()
    }

    /// Counts inside the transaction, so it sees the transaction's own changes.
    pub async fn count_users(&mut self) -> Result<i64> {
        let n = self
            .scalar("SELECT COUNT(*) FROM users WHERE org_id = ?")
            .bind(DEFAULT_ORG)
            .fetch_one(self.conn())
            .await?;
        Ok(n)
    }

    /// Fails with `StoreError::Duplicate` when the email is taken.
    pub async fn insert_user(&mut self, u: NewUser<'_>) -> Result<i64> {
        let id: i64 = self
            .scalar(
                "INSERT INTO users (org_id, email, name, role, status, password_hash)
             VALUES (?, ?, ?, ?, ?, ?) RETURNING id",
            )
            .bind(DEFAULT_ORG)
            .bind(u.email)
            .bind(u.name)
            .bind(u.role.as_str())
            .bind(u.status.as_str())
            .bind(u.password_hash)
            .fetch_one(self.conn())
            .await
            .map_err(write_error)?;
        Ok(id)
    }

    /// Links the user to an identity at a sign-in provider. Fails with
    /// `StoreError::Duplicate` when another user has that identity; false
    /// when the user does not exist.
    pub async fn link_external(
        &mut self,
        user_id: i64,
        provider: &str,
        external_id: &str,
    ) -> Result<bool> {
        let r = self
            .q("UPDATE users SET auth_provider = ?, external_id = ? WHERE id = ? AND org_id = ?")
            .bind(provider)
            .bind(external_id)
            .bind(user_id)
            .bind(DEFAULT_ORG)
            .execute(self.conn())
            .await
            .map_err(write_error)?;
        Ok(r.rows_affected() == 1)
    }

    pub async fn set_user_name(&mut self, id: i64, name: &str) -> Result<bool> {
        self.set_user_column(
            "UPDATE users SET name = ? WHERE id = ? AND org_id = ?",
            name,
            id,
        )
        .await
    }

    pub async fn set_user_role(&mut self, id: i64, role: Role) -> Result<bool> {
        self.set_user_column(
            "UPDATE users SET role = ? WHERE id = ? AND org_id = ?",
            role.as_str(),
            id,
        )
        .await
    }

    pub async fn set_user_status(&mut self, id: i64, status: UserStatus) -> Result<bool> {
        self.set_user_column(
            "UPDATE users SET status = ? WHERE id = ? AND org_id = ?",
            status.as_str(),
            id,
        )
        .await
    }

    pub async fn set_user_password(&mut self, id: i64, hash: &str) -> Result<bool> {
        self.set_user_column(
            "UPDATE users SET password_hash = ? WHERE id = ? AND org_id = ?",
            hash,
            id,
        )
        .await
    }

    async fn set_user_column(&mut self, sql: &'static str, value: &str, id: i64) -> Result<bool> {
        let r = self
            .q(sql)
            .bind(value)
            .bind(id)
            .bind(DEFAULT_ORG)
            .execute(self.conn())
            .await?;
        Ok(r.rows_affected() == 1)
    }

    /// Also removes the user's invites, memberships, sessions and access
    /// tokens, and detaches their keys.
    pub async fn delete_user(&mut self, id: i64) -> Result<bool> {
        let r = self
            .q("DELETE FROM users WHERE id = ? AND org_id = ?")
            .bind(id)
            .bind(DEFAULT_ORG)
            .execute(self.conn())
            .await?;
        Ok(r.rows_affected() == 1)
    }

    /// Counts inside the transaction, so it sees the transaction's own changes.
    pub async fn count_active_admins(&mut self) -> Result<i64> {
        let n = self
            .scalar(COUNT_ACTIVE_ADMINS)
            .bind(DEFAULT_ORG)
            .fetch_one(self.conn())
            .await?;
        Ok(n)
    }

    /// `expires_at` must be UTC in the form `YYYY-MM-DD HH:MM:SS`.
    pub async fn insert_invite(
        &mut self,
        user_id: i64,
        token_hash: &str,
        expires_at: &str,
    ) -> Result<i64> {
        self.insert_invite_of_kind(user_id, token_hash, expires_at, "invite")
            .await
    }

    /// `kind` is `invite` or `set_password`; the database refuses any other.
    pub async fn insert_invite_of_kind(
        &mut self,
        user_id: i64,
        token_hash: &str,
        expires_at: &str,
        kind: &str,
    ) -> Result<i64> {
        check_timestamp(expires_at).context("expires_at is not valid")?;
        let id: i64 = self.scalar(
            "INSERT INTO invites (org_id, user_id, token_hash, expires_at, kind) VALUES (?, ?, ?, ?, ?) RETURNING id",
        )
        .bind(DEFAULT_ORG)
        .bind(user_id)
        .bind(token_hash)
        .bind(expires_at)
        .bind(kind)
        .fetch_one(self.conn())
        .await?;
        Ok(id)
    }

    /// Returns `false` if the invite was already used.
    pub async fn use_invite(&mut self, invite_id: i64) -> Result<bool> {
        let r = self
            .q("UPDATE invites SET used_at = ?
             WHERE id = ? AND org_id = ? AND used_at IS NULL")
            .bind(now())
            .bind(invite_id)
            .bind(DEFAULT_ORG)
            .execute(self.conn())
            .await?;
        Ok(r.rows_affected() == 1)
    }

    pub async fn delete_invites_of(&mut self, user_id: i64) -> Result<()> {
        self.q("DELETE FROM invites WHERE user_id = ? AND org_id = ?")
            .bind(user_id)
            .bind(DEFAULT_ORG)
            .execute(self.conn())
            .await?;
        Ok(())
    }
}

pub(crate) async fn list_users_in(conn: &mut AnyConnection) -> Result<Vec<UserRow>> {
    let sql = format!("SELECT {USER_COLUMNS} FROM users WHERE org_id = ? ORDER BY email");
    let rows = conn
        .q_dyn(sql)
        .bind(DEFAULT_ORG)
        .fetch_all(&mut *conn)
        .await?;
    rows.iter().map(user_from).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn external_identities_are_unique_and_found() {
        let s = Store::open_in_memory().await.unwrap();
        let mut tx = s.begin().await.unwrap();
        let mut ids = Vec::new();
        for email in ["a@example.com", "b@example.com", "c@example.com"] {
            ids.push(
                tx.insert_user(new_user(email, Role::Member, UserStatus::Active))
                    .await
                    .unwrap(),
            );
        }
        // Password users all have no external id and never collide.
        assert!(tx
            .user_by_id(ids[0])
            .await
            .unwrap()
            .unwrap()
            .external_id
            .is_none());
        assert!(tx
            .link_external(ids[0], "oidc", "https://idp|sub-1")
            .await
            .unwrap());
        tx.commit().await.unwrap();
        // Another user cannot take the same identity. (A failed statement
        // ends a PostgreSQL transaction: it gets its own.)
        let mut tx = s.begin().await.unwrap();
        let err = tx
            .link_external(ids[1], "oidc", "https://idp|sub-1")
            .await
            .unwrap_err();
        assert!(matches!(
            err.downcast_ref::<StoreError>(),
            Some(StoreError::Duplicate)
        ));
        drop(tx);
        let mut tx = s.begin().await.unwrap();
        // The same id under another provider is another identity.
        assert!(tx
            .link_external(ids[1], "other", "https://idp|sub-1")
            .await
            .unwrap());
        // Linking again to the same value is fine; a missing user is false.
        assert!(tx
            .link_external(ids[0], "oidc", "https://idp|sub-1")
            .await
            .unwrap());
        assert!(!tx.link_external(9999, "oidc", "x|y").await.unwrap());
        tx.commit().await.unwrap();

        let found = s
            .user_by_external("oidc", "https://idp|sub-1")
            .await
            .unwrap()
            .expect("linked");
        assert_eq!(found.id, ids[0]);
        assert_eq!(found.auth_provider, "oidc");
        assert_eq!(found.external_id.as_deref(), Some("https://idp|sub-1"));
        assert!(s
            .user_by_external("oidc", "https://idp|sub-2")
            .await
            .unwrap()
            .is_none());
        assert!(s
            .user_by_external("password", "https://idp|sub-1")
            .await
            .unwrap()
            .is_none());
        assert_eq!(
            s.user_by_id(ids[2]).await.unwrap().unwrap().auth_provider,
            "password"
        );
    }

    #[tokio::test]
    async fn users_of_teams_and_shared_led_teams() {
        let s = Store::open_in_memory().await.unwrap();
        let mut tx = s.begin().await.unwrap();
        let mut ids = Vec::new();
        for email in [
            "a@example.com",
            "b@example.com",
            "c@example.com",
            "d@example.com",
        ] {
            ids.push(
                tx.insert_user(new_user(email, Role::Member, UserStatus::Active))
                    .await
                    .unwrap(),
            );
        }
        let (a, b, c, d) = (ids[0], ids[1], ids[2], ids[3]);
        let one = tx.insert_team("one").await.unwrap();
        let two = tx.insert_team("two").await.unwrap();
        tx.put_member(one, a, TeamRole::Lead).await.unwrap();
        tx.put_member(one, b, TeamRole::Member).await.unwrap();
        tx.put_member(two, a, TeamRole::Member).await.unwrap();
        tx.put_member(two, c, TeamRole::Lead).await.unwrap();
        assert_eq!(
            tx.user_by_id(a).await.unwrap().unwrap().email,
            "a@example.com"
        );
        assert!(tx.user_by_id(d + 1).await.unwrap().is_none());
        tx.commit().await.unwrap();

        let emails = |rows: Vec<UserRow>| rows.into_iter().map(|u| u.id).collect::<Vec<_>>();
        assert_eq!(
            emails(s.list_users_in_teams(&[one], a).await.unwrap()),
            [a, b]
        );
        assert_eq!(
            emails(s.list_users_in_teams(&[one], d).await.unwrap()),
            [a, b, d]
        );
        assert_eq!(
            emails(s.list_users_in_teams(&[one, two], a).await.unwrap()),
            [a, b, c]
        );
        assert_eq!(emails(s.list_users_in_teams(&[], d).await.unwrap()), [d]);

        assert!(s.shares_led_team(a, b).await.unwrap());
        assert!(s.shares_led_team(a, a).await.unwrap());
        // `a` is only a member of team two.
        assert!(!s.shares_led_team(a, c).await.unwrap());
        assert!(s.shares_led_team(c, a).await.unwrap());
        assert!(!s.shares_led_team(b, a).await.unwrap());
        assert!(!s.shares_led_team(a, d).await.unwrap());
    }
    use crate::identity::TeamRole;
    use crate::store::{after, check_timestamp, StoreError};

    fn new_user(email: &str, role: Role, status: UserStatus) -> NewUser<'_> {
        NewUser {
            email,
            name: "Someone",
            role,
            status,
            password_hash: None,
        }
    }

    async fn add_user(s: &Store, u: NewUser<'_>) -> i64 {
        let mut tx = s.begin().await.unwrap();
        let id = tx.insert_user(u).await.unwrap();
        tx.commit().await.unwrap();
        id
    }

    #[tokio::test]
    async fn user_round_trip() {
        let s = Store::open_in_memory().await.unwrap();
        assert_eq!(s.count_users().await.unwrap(), 0);
        let id = add_user(
            &s,
            NewUser {
                email: "maya@example.com",
                name: "Maya",
                role: Role::Admin,
                status: UserStatus::Active,
                password_hash: Some("$argon2id$stored"),
            },
        )
        .await;

        let by_id = s.user_by_id(id).await.unwrap().unwrap();
        assert_eq!(by_id.id, id);
        assert_eq!(by_id.email, "maya@example.com");
        assert_eq!(by_id.name, "Maya");
        assert_eq!(by_id.role, Role::Admin);
        assert_eq!(by_id.status, UserStatus::Active);
        assert_eq!(by_id.password_hash.as_deref(), Some("$argon2id$stored"));
        assert!(check_timestamp(&by_id.created_at).is_ok());
        assert_eq!(by_id.last_active_at, None);

        let by_email = s.user_by_email("maya@example.com").await.unwrap().unwrap();
        assert_eq!(by_email, by_id);
        assert_eq!(s.count_users().await.unwrap(), 1);

        assert!(s.user_by_id(id + 1).await.unwrap().is_none());
        assert!(s.user_by_email("x@example.com").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn users_are_listed_by_email() {
        let s = Store::open_in_memory().await.unwrap();
        for email in ["c@example.com", "a@example.com", "b@example.com"] {
            add_user(&s, new_user(email, Role::Member, UserStatus::Active)).await;
        }
        let emails: Vec<String> = s
            .list_users()
            .await
            .unwrap()
            .into_iter()
            .map(|u| u.email)
            .collect();
        assert_eq!(emails, ["a@example.com", "b@example.com", "c@example.com"]);
    }

    #[tokio::test]
    async fn duplicate_email_is_reported() {
        let s = Store::open_in_memory().await.unwrap();
        add_user(
            &s,
            new_user("maya@example.com", Role::Member, UserStatus::Active),
        )
        .await;
        let mut tx = s.begin().await.unwrap();
        let err = tx
            .insert_user(new_user(
                "maya@example.com",
                Role::Admin,
                UserStatus::Invited,
            ))
            .await
            .expect_err("a second user with the same email");
        drop(tx);
        assert!(matches!(
            err.downcast_ref::<StoreError>(),
            Some(StoreError::Duplicate)
        ));
        assert_eq!(s.count_users().await.unwrap(), 1);
    }

    #[test]
    fn user_debug_hides_password_hash() {
        let mut row = UserRow {
            id: 1,
            email: "maya@example.com".into(),
            name: "Maya".into(),
            role: Role::Admin,
            status: UserStatus::Active,
            password_hash: Some("$argon2id$very-secret-hash".into()),
            auth_provider: "password".into(),
            external_id: None,
            created_at: "2026-01-01 00:00:00".into(),
            last_active_at: None,
        };
        let shown = format!("{row:?}");
        assert!(shown.contains("maya@example.com"));
        assert!(shown.contains("<present>"));
        assert!(!shown.contains("argon2id"));
        assert!(!shown.contains("very-secret-hash"));
        row.password_hash = None;
        assert!(format!("{row:?}").contains("<none>"));
    }

    #[tokio::test]
    async fn user_updates_report_changes() {
        let s = Store::open_in_memory().await.unwrap();
        let id = add_user(
            &s,
            new_user("maya@example.com", Role::Member, UserStatus::Invited),
        )
        .await;
        let unknown = id + 1000;

        let mut tx = s.begin().await.unwrap();
        assert!(tx.set_user_name(id, "Maya R").await.unwrap());
        assert!(tx.set_user_role(id, Role::Admin).await.unwrap());
        assert!(tx.set_user_status(id, UserStatus::Active).await.unwrap());
        assert!(tx.set_user_password(id, "$argon2id$new").await.unwrap());
        assert!(!tx.set_user_name(unknown, "x").await.unwrap());
        assert!(!tx.set_user_role(unknown, Role::Admin).await.unwrap());
        assert!(!tx
            .set_user_status(unknown, UserStatus::Active)
            .await
            .unwrap());
        assert!(!tx.set_user_password(unknown, "h").await.unwrap());
        assert!(!tx.delete_user(unknown).await.unwrap());
        tx.commit().await.unwrap();

        let u = s.user_by_id(id).await.unwrap().unwrap();
        assert_eq!(u.name, "Maya R");
        assert_eq!(u.role, Role::Admin);
        assert_eq!(u.status, UserStatus::Active);
        assert_eq!(u.password_hash.as_deref(), Some("$argon2id$new"));
        assert_eq!(u.email, "maya@example.com");
    }

    #[tokio::test]
    async fn touch_sets_last_active() {
        let s = Store::open_in_memory().await.unwrap();
        let id = add_user(
            &s,
            new_user("maya@example.com", Role::Member, UserStatus::Active),
        )
        .await;
        s.touch_user(id).await.unwrap();
        let at = s
            .user_by_id(id)
            .await
            .unwrap()
            .unwrap()
            .last_active_at
            .unwrap();
        assert!(check_timestamp(&at).is_ok());
    }

    #[tokio::test]
    async fn active_admin_count() {
        let s = Store::open_in_memory().await.unwrap();
        let mut tx = s.begin().await.unwrap();
        for (email, role, status) in [
            ("a@example.com", Role::Admin, UserStatus::Active),
            ("b@example.com", Role::Admin, UserStatus::Disabled),
            ("c@example.com", Role::Admin, UserStatus::Invited),
            ("d@example.com", Role::Member, UserStatus::Active),
        ] {
            tx.insert_user(new_user(email, role, status)).await.unwrap();
        }
        assert_eq!(tx.count_active_admins().await.unwrap(), 1);
        assert_eq!(tx.count_users().await.unwrap(), 4);
        tx.commit().await.unwrap();
        assert_eq!(s.count_active_admins().await.unwrap(), 1);
        assert_eq!(s.count_users().await.unwrap(), 4);
    }

    #[tokio::test]
    async fn invite_lifecycle() {
        let s = Store::open_in_memory().await.unwrap();
        let mut tx = s.begin().await.unwrap();
        let user = tx
            .insert_user(new_user(
                "maya@example.com",
                Role::Member,
                UserStatus::Invited,
            ))
            .await
            .unwrap();
        let expires = after(3600);
        let live = tx.insert_invite(user, "live", &expires).await.unwrap();
        tx.insert_invite(user, "stale", "2000-01-01 00:00:00")
            .await
            .unwrap();
        assert!(tx.insert_invite(user, "bad", "tomorrow").await.is_err());
        tx.commit().await.unwrap();

        let found = s.invite_by_hash("live").await.unwrap().unwrap();
        assert_eq!(found.id, live);
        assert_eq!(found.user_id, user);
        assert_eq!(found.expires_at, expires);
        assert!(s.invite_by_hash("stale").await.unwrap().is_none());
        assert!(s.invite_by_hash("bad").await.unwrap().is_none());
        assert!(s.invite_by_hash("unknown").await.unwrap().is_none());

        let mut tx = s.begin().await.unwrap();
        assert!(tx.use_invite(live).await.unwrap());
        assert!(!tx.use_invite(live).await.unwrap());
        tx.commit().await.unwrap();
        assert!(s.invite_by_hash("live").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn invites_of_a_user_can_be_deleted() {
        let s = Store::open_in_memory().await.unwrap();
        let mut tx = s.begin().await.unwrap();
        let a = tx
            .insert_user(new_user("a@example.com", Role::Member, UserStatus::Invited))
            .await
            .unwrap();
        let b = tx
            .insert_user(new_user("b@example.com", Role::Member, UserStatus::Invited))
            .await
            .unwrap();
        tx.insert_invite(a, "ha", &after(3600)).await.unwrap();
        tx.insert_invite(b, "hb", &after(3600)).await.unwrap();
        tx.delete_invites_of(a).await.unwrap();
        tx.commit().await.unwrap();
        assert!(s.invite_by_hash("ha").await.unwrap().is_none());
        assert!(s.invite_by_hash("hb").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn deleting_a_user_removes_dependents() {
        let s = Store::open_in_memory().await.unwrap();
        let mut tx = s.begin().await.unwrap();
        let user = tx
            .insert_user(new_user(
                "maya@example.com",
                Role::Member,
                UserStatus::Active,
            ))
            .await
            .unwrap();
        tx.insert_invite(user, "inv", &after(3600)).await.unwrap();
        let team = tx.insert_team("platform").await.unwrap();
        tx.put_member(team, user, TeamRole::Lead).await.unwrap();
        tx.insert_key("k", "h", "uf-sk-…aaaa", None, Some(user), None)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        assert_eq!(
            s.active_key_by_hash("h").await.unwrap().unwrap().user_id,
            Some(user)
        );

        let mut tx = s.begin().await.unwrap();
        assert!(tx.delete_user(user).await.unwrap());
        assert!(!tx.delete_user(user).await.unwrap());
        tx.commit().await.unwrap();

        assert!(s.user_by_id(user).await.unwrap().is_none());
        assert!(s.invite_by_hash("inv").await.unwrap().is_none());
        let invites: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM invites")
            .fetch_one(s.pool())
            .await
            .unwrap();
        assert_eq!(invites, 0);
        assert!(s.members_of(team).await.unwrap().is_empty());
        assert!(s.memberships_of(user).await.unwrap().is_empty());
        let key = s.active_key_by_hash("h").await.unwrap().unwrap();
        assert_eq!(key.user_id, None);
        assert!(s.team_by_id(team).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn users_are_scoped_to_the_org() {
        let s = Store::open_in_memory().await.unwrap();
        sqlx::query(
            "INSERT INTO users (org_id, email, name, role, status)
             VALUES (2, 'other@example.com', 'Other', 'admin', 'active')",
        )
        .execute(s.pool())
        .await
        .unwrap();
        assert_eq!(s.count_users().await.unwrap(), 0);
        assert_eq!(s.count_active_admins().await.unwrap(), 0);
        assert!(s.user_by_id(1).await.unwrap().is_none());
        assert!(s
            .user_by_email("other@example.com")
            .await
            .unwrap()
            .is_none());
        assert!(s.list_users().await.unwrap().is_empty());
        let mut tx = s.begin().await.unwrap();
        assert!(!tx.set_user_name(1, "x").await.unwrap());
        assert!(!tx.delete_user(1).await.unwrap());
    }
}
