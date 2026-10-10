//! Guardrails and where they are attached. An external guardrail's URL and
//! signing secret are kept encrypted with the master key; neither is read
//! back except to call it. The rules are JSON, validated before they are
//! written.

use std::fmt;

use anyhow::Result;
use sqlx::any::AnyRow;
use sqlx::AnyConnection;
use sqlx::Row;

use super::dialect::Dialected;
use super::{flag, now, write_error, Store, Tx, DEFAULT_ORG};

const SELECT: &str = "SELECT id, name, description, kind, rules, external_url_enc,
        external_url_host, external_secret_enc, timeout_ms, fail_mode, directions,
        enabled, is_default, created_at
     FROM guardrails";

/// A guardrail as stored.
#[derive(Clone)]
pub struct GuardrailRow {
    pub id: i64,
    pub name: String,
    pub description: String,
    /// `rules` or `external`.
    pub kind: String,
    /// A JSON array of rules (empty for an external guardrail).
    pub rules: String,
    /// The URL of an external guardrail, encrypted. An empty text, encrypted,
    /// is a guardrail an import made that has no URL yet.
    pub url_enc: Option<Vec<u8>>,
    /// Scheme, host and port of the URL: what may be shown. Empty until a
    /// URL is set.
    pub url_host: Option<String>,
    /// The signing secret of an external guardrail, encrypted.
    pub secret_enc: Option<Vec<u8>>,
    pub timeout_ms: i64,
    /// `open` or `closed`.
    pub fail_mode: String,
    /// `input`, `output` or `both`: what an external guardrail is asked about.
    pub directions: String,
    pub enabled: bool,
    /// Applies to every call of the gateway.
    pub is_default: bool,
    pub created_at: String,
}

/// Shows no URL and no secret, not even encrypted.
impl fmt::Debug for GuardrailRow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GuardrailRow")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("kind", &self.kind)
            .field("url_host", &self.url_host)
            .field("enabled", &self.enabled)
            .field("is_default", &self.is_default)
            .finish_non_exhaustive()
    }
}

fn from_row(r: &AnyRow) -> GuardrailRow {
    GuardrailRow {
        id: r.get("id"),
        name: r.get("name"),
        description: r.get("description"),
        kind: r.get("kind"),
        rules: r.get("rules"),
        url_enc: r.get("external_url_enc"),
        url_host: r.get("external_url_host"),
        secret_enc: r.get("external_secret_enc"),
        timeout_ms: r.get("timeout_ms"),
        fail_mode: r.get("fail_mode"),
        directions: r.get("directions"),
        enabled: r.get::<i64, _>("enabled") != 0,
        is_default: r.get::<i64, _>("is_default") != 0,
        created_at: r.get("created_at"),
    }
}

/// What `insert_guardrail` stores.
pub struct NewGuardrail<'a> {
    pub name: &'a str,
    pub description: &'a str,
    /// `rules` or `external`.
    pub kind: &'a str,
    /// A JSON array; `[]` for an external guardrail.
    pub rules: &'a str,
    /// The encrypted URL and its shown form (external only).
    pub url: Option<(&'a [u8], &'a str)>,
    /// The encrypted signing secret (external only).
    pub secret_enc: Option<&'a [u8]>,
    pub timeout_ms: i64,
    pub fail_mode: &'a str,
    pub directions: &'a str,
    pub enabled: bool,
    pub is_default: bool,
}

/// What `update_guardrail` changes; `None` leaves a field as it is.
#[derive(Default)]
pub struct GuardrailPatch<'a> {
    pub name: Option<&'a str>,
    pub description: Option<&'a str>,
    pub rules: Option<&'a str>,
    pub url: Option<(&'a [u8], &'a str)>,
    pub timeout_ms: Option<i64>,
    pub fail_mode: Option<&'a str>,
    pub directions: Option<&'a str>,
    pub enabled: Option<bool>,
    pub is_default: Option<bool>,
}

/// Every guardrail by name, on any connection.
pub(super) async fn list_guardrails_in(conn: &mut AnyConnection) -> Result<Vec<GuardrailRow>> {
    let sql = format!("{SELECT} WHERE org_id = ? ORDER BY name");
    let rows = conn
        .q_dyn(sql)
        .bind(DEFAULT_ORG)
        .fetch_all(&mut *conn)
        .await?;
    Ok(rows.iter().map(from_row).collect())
}

/// `(route id, guardrail id, guardrail name)` of every link, in the order
/// each route lists them.
pub(super) async fn route_guardrail_refs_in(
    conn: &mut AnyConnection,
) -> Result<Vec<(i64, i64, String)>> {
    refs_in(
        conn,
        "SELECT l.route_id, g.id, g.name FROM route_guardrails l
         JOIN guardrails g ON g.id = l.guardrail_id
         WHERE g.org_id = ? ORDER BY l.route_id, l.position",
    )
    .await
}

/// `(key id, guardrail id, guardrail name)`, as for routes.
pub(super) async fn key_guardrail_refs_in(
    conn: &mut AnyConnection,
) -> Result<Vec<(i64, i64, String)>> {
    refs_in(
        conn,
        "SELECT l.key_id, g.id, g.name FROM key_guardrails l
         JOIN guardrails g ON g.id = l.guardrail_id
         WHERE g.org_id = ? ORDER BY l.key_id, l.position",
    )
    .await
}

/// `(team id, guardrail id, guardrail name)`, as for routes.
pub(super) async fn team_guardrail_refs_in(
    conn: &mut AnyConnection,
) -> Result<Vec<(i64, i64, String)>> {
    refs_in(
        conn,
        "SELECT l.team_id, g.id, g.name FROM team_guardrails l
         JOIN guardrails g ON g.id = l.guardrail_id
         WHERE g.org_id = ? ORDER BY l.team_id, l.position",
    )
    .await
}

/// `(user id, guardrail id, guardrail name)`, as for routes.
pub(super) async fn user_guardrail_refs_in(
    conn: &mut AnyConnection,
) -> Result<Vec<(i64, i64, String)>> {
    refs_in(
        conn,
        "SELECT l.user_id, g.id, g.name FROM user_guardrails l
         JOIN guardrails g ON g.id = l.guardrail_id
         WHERE g.org_id = ? ORDER BY l.user_id, l.position",
    )
    .await
}

async fn refs_in(conn: &mut AnyConnection, sql: &'static str) -> Result<Vec<(i64, i64, String)>> {
    let rows = conn.q(sql).bind(DEFAULT_ORG).fetch_all(conn).await?;
    Ok(rows
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2)))
        .collect())
}

impl Tx<'_> {
    /// A taken name is `StoreError::Duplicate`.
    pub async fn insert_guardrail(&mut self, g: NewGuardrail<'_>) -> Result<i64> {
        let id: i64 = self
            .scalar(
                "INSERT INTO guardrails
                 (org_id, name, description, kind, rules, external_url_enc, external_url_host,
                  external_secret_enc, timeout_ms, fail_mode, directions, enabled, is_default,
                  created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
            )
            .bind(DEFAULT_ORG)
            .bind(g.name)
            .bind(g.description)
            .bind(g.kind)
            .bind(g.rules)
            .bind(g.url.map(|(enc, _)| enc))
            .bind(g.url.map(|(_, host)| host))
            .bind(g.secret_enc)
            .bind(g.timeout_ms)
            .bind(g.fail_mode)
            .bind(g.directions)
            .bind(flag(g.enabled))
            .bind(flag(g.is_default))
            .bind(now())
            .fetch_one(self.conn())
            .await
            .map_err(write_error)?;
        Ok(id)
    }

    /// For a read inside a transaction; see `Store::guardrail_by_id`.
    pub async fn guardrail_by_id(&mut self, id: i64) -> Result<Option<GuardrailRow>> {
        let sql = format!("{SELECT} WHERE id = ? AND org_id = ?");
        let row = self
            .q_dyn(sql)
            .bind(id)
            .bind(DEFAULT_ORG)
            .fetch_optional(self.conn())
            .await?;
        Ok(row.as_ref().map(from_row))
    }

    /// Changes what is given. A taken name is `StoreError::Duplicate`;
    /// `false`: no such guardrail.
    pub async fn update_guardrail(&mut self, id: i64, p: GuardrailPatch<'_>) -> Result<bool> {
        let r = self
            .q("UPDATE guardrails
             SET name = COALESCE(?, name),
                 description = COALESCE(?, description),
                 rules = COALESCE(?, rules),
                 external_url_enc = COALESCE(?, external_url_enc),
                 external_url_host = COALESCE(?, external_url_host),
                 timeout_ms = COALESCE(?, timeout_ms),
                 fail_mode = COALESCE(?, fail_mode),
                 directions = COALESCE(?, directions),
                 enabled = COALESCE(?, enabled),
                 is_default = COALESCE(?, is_default)
             WHERE id = ? AND org_id = ?")
            .bind(p.name)
            .bind(p.description)
            .bind(p.rules)
            .bind(p.url.map(|(enc, _)| enc))
            .bind(p.url.map(|(_, host)| host))
            .bind(p.timeout_ms)
            .bind(p.fail_mode)
            .bind(p.directions)
            .bind(p.enabled.map(flag))
            .bind(p.is_default.map(flag))
            .bind(id)
            .bind(DEFAULT_ORG)
            .execute(self.conn())
            .await
            .map_err(write_error)?;
        Ok(r.rows_affected() == 1)
    }

    pub async fn set_guardrail_secret(&mut self, id: i64, secret_enc: &[u8]) -> Result<bool> {
        let r = self
            .q("UPDATE guardrails SET external_secret_enc = ? WHERE id = ? AND org_id = ?")
            .bind(secret_enc)
            .bind(id)
            .bind(DEFAULT_ORG)
            .execute(self.conn())
            .await?;
        Ok(r.rows_affected() == 1)
    }

    /// Its attachments to routes and keys go with it. Returns `false` if
    /// there is no such guardrail.
    pub async fn delete_guardrail(&mut self, id: i64) -> Result<bool> {
        let r = self
            .q("DELETE FROM guardrails WHERE id = ? AND org_id = ?")
            .bind(id)
            .bind(DEFAULT_ORG)
            .execute(self.conn())
            .await?;
        Ok(r.rows_affected() == 1)
    }

    /// Replaces what is attached to a route, in this order. A guardrail that
    /// does not exist is a foreign key failure (`is_missing_reference`):
    /// check first, as a failed statement ends a PostgreSQL transaction.
    pub async fn replace_route_guardrails(
        &mut self,
        route_id: i64,
        guardrail_ids: &[i64],
    ) -> Result<()> {
        self.q("DELETE FROM route_guardrails WHERE route_id = ?")
            .bind(route_id)
            .execute(self.conn())
            .await?;
        for (position, guardrail_id) in (0_i64..).zip(guardrail_ids) {
            self.q(
                "INSERT INTO route_guardrails (route_id, guardrail_id, position) VALUES (?, ?, ?)",
            )
            .bind(route_id)
            .bind(guardrail_id)
            .bind(position)
            .execute(self.conn())
            .await?;
        }
        Ok(())
    }

    /// The guardrails attached to a route, in order.
    pub async fn route_guardrail_ids(&mut self, route_id: i64) -> Result<Vec<i64>> {
        let rows = self
            .q("SELECT guardrail_id FROM route_guardrails WHERE route_id = ? ORDER BY position")
            .bind(route_id)
            .fetch_all(self.conn())
            .await?;
        Ok(rows.iter().map(|r| r.get(0)).collect())
    }

    /// The guardrails attached to a key, in order.
    pub async fn key_guardrail_ids(&mut self, key_id: i64) -> Result<Vec<i64>> {
        let rows = self
            .q("SELECT guardrail_id FROM key_guardrails WHERE key_id = ? ORDER BY position")
            .bind(key_id)
            .fetch_all(self.conn())
            .await?;
        Ok(rows.iter().map(|r| r.get(0)).collect())
    }

    /// As [`Tx::replace_route_guardrails`], for a key.
    pub async fn replace_key_guardrails(
        &mut self,
        key_id: i64,
        guardrail_ids: &[i64],
    ) -> Result<()> {
        self.q("DELETE FROM key_guardrails WHERE key_id = ?")
            .bind(key_id)
            .execute(self.conn())
            .await?;
        for (position, guardrail_id) in (0_i64..).zip(guardrail_ids) {
            self.q("INSERT INTO key_guardrails (key_id, guardrail_id, position) VALUES (?, ?, ?)")
                .bind(key_id)
                .bind(guardrail_id)
                .bind(position)
                .execute(self.conn())
                .await?;
        }
        Ok(())
    }

    /// The guardrails attached to a team, in order.
    pub async fn team_guardrail_ids(&mut self, team_id: i64) -> Result<Vec<i64>> {
        let rows = self
            .q("SELECT guardrail_id FROM team_guardrails WHERE team_id = ? ORDER BY position")
            .bind(team_id)
            .fetch_all(self.conn())
            .await?;
        Ok(rows.iter().map(|r| r.get(0)).collect())
    }

    /// The guardrails attached to a user, in order.
    pub async fn user_guardrail_ids(&mut self, user_id: i64) -> Result<Vec<i64>> {
        let rows = self
            .q("SELECT guardrail_id FROM user_guardrails WHERE user_id = ? ORDER BY position")
            .bind(user_id)
            .fetch_all(self.conn())
            .await?;
        Ok(rows.iter().map(|r| r.get(0)).collect())
    }

    /// As [`Tx::replace_route_guardrails`], for a team.
    pub async fn replace_team_guardrails(
        &mut self,
        team_id: i64,
        guardrail_ids: &[i64],
    ) -> Result<()> {
        self.q("DELETE FROM team_guardrails WHERE team_id = ?")
            .bind(team_id)
            .execute(self.conn())
            .await?;
        for (position, guardrail_id) in (0_i64..).zip(guardrail_ids) {
            self.q(
                "INSERT INTO team_guardrails (team_id, guardrail_id, position) VALUES (?, ?, ?)",
            )
            .bind(team_id)
            .bind(guardrail_id)
            .bind(position)
            .execute(self.conn())
            .await?;
        }
        Ok(())
    }

    /// As [`Tx::replace_route_guardrails`], for a user.
    pub async fn replace_user_guardrails(
        &mut self,
        user_id: i64,
        guardrail_ids: &[i64],
    ) -> Result<()> {
        self.q("DELETE FROM user_guardrails WHERE user_id = ?")
            .bind(user_id)
            .execute(self.conn())
            .await?;
        for (position, guardrail_id) in (0_i64..).zip(guardrail_ids) {
            self.q(
                "INSERT INTO user_guardrails (user_id, guardrail_id, position) VALUES (?, ?, ?)",
            )
            .bind(user_id)
            .bind(guardrail_id)
            .bind(position)
            .execute(self.conn())
            .await?;
        }
        Ok(())
    }
}

impl Store {
    /// Every guardrail, by name.
    pub async fn list_guardrails(&self) -> Result<Vec<GuardrailRow>> {
        let sql = format!("{SELECT} WHERE org_id = ? ORDER BY name");
        let rows = self
            .q_dyn(sql)
            .bind(DEFAULT_ORG)
            .fetch_all(self.pool())
            .await?;
        Ok(rows.iter().map(from_row).collect())
    }

    pub async fn guardrail_by_id(&self, id: i64) -> Result<Option<GuardrailRow>> {
        let sql = format!("{SELECT} WHERE id = ? AND org_id = ?");
        let row = self
            .q_dyn(sql)
            .bind(id)
            .bind(DEFAULT_ORG)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.as_ref().map(from_row))
    }

    /// `(guardrail id, route id, route name)` of every attachment, by route
    /// name, then guardrail id.
    pub async fn guardrail_routes(&self) -> Result<Vec<(i64, i64, String)>> {
        let rows = self
            .q("SELECT l.guardrail_id, r.id, r.name FROM route_guardrails l
             JOIN routes r ON r.id = l.route_id
             WHERE r.org_id = ? ORDER BY r.name, l.guardrail_id")
            .bind(DEFAULT_ORG)
            .fetch_all(self.pool())
            .await?;
        Ok(rows
            .iter()
            .map(|r| (r.get(0), r.get(1), r.get(2)))
            .collect())
    }

    /// `(guardrail id, number of keys)` for every guardrail with a key.
    pub async fn guardrail_key_counts(&self) -> Result<Vec<(i64, i64)>> {
        let rows = self
            .q("SELECT l.guardrail_id, COUNT(*) FROM key_guardrails l
             JOIN guardrails g ON g.id = l.guardrail_id
             WHERE g.org_id = ? GROUP BY l.guardrail_id")
            .bind(DEFAULT_ORG)
            .fetch_all(self.pool())
            .await?;
        Ok(rows.iter().map(|r| (r.get(0), r.get(1))).collect())
    }

    /// `(route id, guardrail id, guardrail name)` of every attachment, in
    /// each route's order.
    pub async fn route_guardrail_refs(&self) -> Result<Vec<(i64, i64, String)>> {
        let mut conn = self.pool().acquire().await?;
        route_guardrail_refs_in(&mut conn).await
    }

    /// `(team id, guardrail id, guardrail name)`, as for routes.
    pub async fn team_guardrail_refs(&self) -> Result<Vec<(i64, i64, String)>> {
        let mut conn = self.pool().acquire().await?;
        team_guardrail_refs_in(&mut conn).await
    }

    /// `(user id, guardrail id, guardrail name)`, as for routes.
    pub async fn user_guardrail_refs(&self) -> Result<Vec<(i64, i64, String)>> {
        let mut conn = self.pool().acquire().await?;
        user_guardrail_refs_in(&mut conn).await
    }

    /// `(key id, guardrail id, guardrail name)`, as for routes.
    pub async fn key_guardrail_refs(&self) -> Result<Vec<(i64, i64, String)>> {
        let mut conn = self.pool().acquire().await?;
        key_guardrail_refs_in(&mut conn).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{RouteSettings, StoreError};

    const DEFAULTS: RouteSettings = RouteSettings {
        retries: 2,
        first_token_timeout_ms: 30_000,
        total_timeout_ms: 300_000,
        breaker_failures: 5,
        breaker_window_s: 60,
        breaker_open_s: 30,
    };

    fn rules<'a>(name: &'a str) -> NewGuardrail<'a> {
        NewGuardrail {
            name,
            description: "",
            kind: "rules",
            rules: "[]",
            url: None,
            secret_enc: None,
            timeout_ms: 3000,
            fail_mode: "open",
            directions: "both",
            enabled: true,
            is_default: false,
        }
    }

    #[tokio::test]
    async fn guardrails_round_trip_and_names_are_unique() {
        let s = Store::open_in_memory().await.unwrap();
        let mut tx = s.begin().await.unwrap();
        let a = tx.insert_guardrail(rules("pii")).await.unwrap();
        let ext = tx
            .insert_guardrail(NewGuardrail {
                kind: "external",
                url: Some((b"enc", "https://hook.example.com")),
                secret_enc: Some(b"sec"),
                timeout_ms: 5000,
                fail_mode: "closed",
                directions: "output",
                is_default: true,
                ..rules("ext")
            })
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let mut tx = s.begin().await.unwrap();
        let dup = tx.insert_guardrail(rules("pii")).await.unwrap_err();
        assert!(matches!(
            dup.downcast_ref::<StoreError>(),
            Some(StoreError::Duplicate)
        ));
        drop(tx);

        let all = s.list_guardrails().await.unwrap();
        assert_eq!(
            all.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(),
            ["ext", "pii"]
        );
        let e = s.guardrail_by_id(ext).await.unwrap().unwrap();
        assert_eq!(
            (e.kind.as_str(), e.fail_mode.as_str(), e.directions.as_str()),
            ("external", "closed", "output")
        );
        assert_eq!((e.timeout_ms, e.is_default), (5000, true));
        assert_eq!(e.url_host.as_deref(), Some("https://hook.example.com"));
        assert_eq!(e.url_enc.as_deref(), Some(&b"enc"[..]));
        let r = s.guardrail_by_id(a).await.unwrap().unwrap();
        assert_eq!((r.url_enc, r.url_host, r.secret_enc), (None, None, None));
        // The debug form shows no URL and no secret.
        assert!(!format!("{e:?}").contains("enc"));
    }

    #[tokio::test]
    async fn update_changes_what_is_given_only() {
        let s = Store::open_in_memory().await.unwrap();
        let mut tx = s.begin().await.unwrap();
        let id = tx.insert_guardrail(rules("pii")).await.unwrap();
        assert!(tx
            .update_guardrail(
                id,
                GuardrailPatch {
                    description: Some("d"),
                    enabled: Some(false),
                    is_default: Some(true),
                    rules: Some("[1]"),
                    ..Default::default()
                },
            )
            .await
            .unwrap());
        assert!(!tx
            .update_guardrail(999, GuardrailPatch::default())
            .await
            .unwrap());
        tx.commit().await.unwrap();
        let g = s.guardrail_by_id(id).await.unwrap().unwrap();
        assert_eq!(
            (g.name.as_str(), g.description.as_str(), g.rules.as_str()),
            ("pii", "d", "[1]")
        );
        assert_eq!((g.enabled, g.is_default, g.timeout_ms), (false, true, 3000));
    }

    #[tokio::test]
    async fn attachments_keep_their_order_and_go_with_either_side() {
        let s = Store::open_in_memory().await.unwrap();
        let mut tx = s.begin().await.unwrap();
        let a = tx.insert_guardrail(rules("a")).await.unwrap();
        let b = tx.insert_guardrail(rules("b")).await.unwrap();
        let route = tx.insert_route("r", &DEFAULTS, true).await.unwrap();
        tx.replace_route_guardrails(route, &[b, a]).await.unwrap();
        tx.commit().await.unwrap();
        let refs = s.route_guardrail_refs().await.unwrap();
        assert_eq!(refs, vec![(route, b, "b".into()), (route, a, "a".into())]);
        assert_eq!(
            s.guardrail_routes().await.unwrap(),
            vec![(a, route, "r".into()), (b, route, "r".into())]
        );

        let mut tx = s.begin().await.unwrap();
        tx.replace_route_guardrails(route, &[a]).await.unwrap();
        assert!(tx.delete_guardrail(a).await.unwrap());
        tx.commit().await.unwrap();
        assert!(s.route_guardrail_refs().await.unwrap().is_empty());
        let mut tx = s.begin().await.unwrap();
        tx.replace_route_guardrails(route, &[b]).await.unwrap();
        assert!(tx.delete_route(route).await.unwrap());
        tx.commit().await.unwrap();
        assert!(s.route_guardrail_refs().await.unwrap().is_empty());
        assert!(s.guardrail_by_id(b).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn a_guardrail_that_is_not_there_is_a_foreign_key_failure() {
        let s = Store::open_in_memory().await.unwrap();
        let mut tx = s.begin().await.unwrap();
        let route = tx.insert_route("r", &DEFAULTS, true).await.unwrap();
        tx.commit().await.unwrap();
        let mut tx = s.begin().await.unwrap();
        let e = tx
            .replace_route_guardrails(route, &[999])
            .await
            .unwrap_err();
        assert!(crate::store::is_missing_reference(&e), "{e:#}");
    }

    /// A route that is deleted leaves nothing for the route that is given
    /// its id again (SQLite hands ids out again).
    #[tokio::test]
    async fn a_route_given_an_old_id_inherits_no_guardrails() {
        let s = Store::open_in_memory().await.unwrap();
        let mut tx = s.begin().await.unwrap();
        let g = tx.insert_guardrail(rules("g")).await.unwrap();
        let old = tx.insert_route("old", &DEFAULTS, true).await.unwrap();
        tx.replace_route_guardrails(old, &[g]).await.unwrap();
        assert!(tx.delete_route(old).await.unwrap());
        let new = tx.insert_route("new", &DEFAULTS, true).await.unwrap();
        tx.commit().await.unwrap();
        if s.dialect() == crate::store::Dialect::Sqlite {
            assert_eq!(old, new, "the id was given out again");
        }
        assert!(s.route_guardrail_refs().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_request_log_has_a_column_for_the_outcome() {
        let s = Store::open_in_memory().await.unwrap();
        let rows = s
            .q("SELECT guardrails FROM request_logs")
            .fetch_all(s.pool())
            .await
            .unwrap();
        assert!(rows.is_empty());
    }

    /// A guardrail that is deleted leaves nothing for the guardrail that is
    /// given its id again (SQLite hands ids out again).
    #[tokio::test]
    async fn a_guardrail_given_an_old_id_inherits_no_attachments() {
        let s = Store::open_in_memory().await.unwrap();
        let mut tx = s.begin().await.unwrap();
        let old = tx.insert_guardrail(rules("old")).await.unwrap();
        let route = tx.insert_route("r", &DEFAULTS, true).await.unwrap();
        tx.replace_route_guardrails(route, &[old]).await.unwrap();
        tx.commit().await.unwrap();
        s.insert_key("k", "h", "d", None).await.unwrap();
        let key = s.active_key_by_hash("h").await.unwrap().unwrap().id;
        let mut tx = s.begin().await.unwrap();
        tx.replace_key_guardrails(key, &[old]).await.unwrap();
        assert!(tx.delete_guardrail(old).await.unwrap());
        let new = tx.insert_guardrail(rules("new")).await.unwrap();
        tx.commit().await.unwrap();
        if s.dialect() == crate::store::Dialect::Sqlite {
            assert_eq!(old, new, "the id was given out again");
        }
        assert!(s.route_guardrail_refs().await.unwrap().is_empty());
        assert!(s.key_guardrail_refs().await.unwrap().is_empty());
        assert!(s.guardrail_key_counts().await.unwrap().is_empty());
    }
}
