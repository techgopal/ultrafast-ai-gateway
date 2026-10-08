//! Alert channels and events. A channel's URL and secret are kept encrypted
//! with the master key; neither is ever read back except to deliver.

use std::fmt;

use anyhow::Result;
use sqlx::sqlite::SqliteRow;
use sqlx::Row;

use super::{now, write_error, Store, Tx, DEFAULT_ORG};

const CHANNEL_SELECT: &str =
    "SELECT id, name, kind, url_enc, url_host, secret_enc, enabled, created_at FROM alert_channels";
const EVENT_SELECT: &str =
    "SELECT id, rule_id, rule_name, kind, subject, state, summary, details, at, deliveries
     FROM alert_events";

/// A channel as stored.
#[derive(Clone)]
pub struct ChannelRow {
    pub id: i64,
    pub name: String,
    /// `webhook` or `slack`.
    pub kind: String,
    /// The URL, encrypted.
    pub url_enc: Vec<u8>,
    /// Scheme, host and port of the URL: what may be shown.
    pub url_host: String,
    /// The signing secret, encrypted.
    pub secret_enc: Vec<u8>,
    pub enabled: bool,
    pub created_at: String,
}

/// Shows no URL and no secret, not even encrypted.
impl fmt::Debug for ChannelRow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChannelRow")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("kind", &self.kind)
            .field("url_host", &self.url_host)
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

fn channel_from(r: &SqliteRow) -> ChannelRow {
    ChannelRow {
        id: r.get("id"),
        name: r.get("name"),
        kind: r.get("kind"),
        url_enc: r.get("url_enc"),
        url_host: r.get("url_host"),
        secret_enc: r.get("secret_enc"),
        enabled: r.get("enabled"),
        created_at: r.get("created_at"),
    }
}

/// An event to store. `details` is JSON and holds metadata only.
pub struct NewAlertEvent<'a> {
    pub rule_id: Option<i64>,
    pub rule_name: &'a str,
    pub kind: &'a str,
    pub subject: &'a str,
    /// `firing`, `resolved` or `test`.
    pub state: &'a str,
    pub summary: &'a str,
    pub details: &'a str,
    pub at: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertEventRow {
    pub id: i64,
    /// `None` once the rule is deleted, and for a test.
    pub rule_id: Option<i64>,
    pub rule_name: String,
    pub kind: String,
    pub subject: String,
    pub state: String,
    pub summary: String,
    /// JSON.
    pub details: String,
    pub at: String,
    /// JSON: one entry per channel, written when all of them finished.
    pub deliveries: String,
}

fn event_from(r: &SqliteRow) -> AlertEventRow {
    AlertEventRow {
        id: r.get("id"),
        rule_id: r.get("rule_id"),
        rule_name: r.get("rule_name"),
        kind: r.get("kind"),
        subject: r.get("subject"),
        state: r.get("state"),
        summary: r.get("summary"),
        details: r.get("details"),
        at: r.get("at"),
        deliveries: r.get("deliveries"),
    }
}

impl Tx<'_> {
    /// A taken name is `StoreError::Duplicate`.
    pub async fn insert_alert_channel(
        &mut self,
        name: &str,
        kind: &str,
        url_enc: &[u8],
        url_host: &str,
        secret_enc: &[u8],
        enabled: bool,
    ) -> Result<i64> {
        let r = sqlx::query(
            "INSERT INTO alert_channels
                 (org_id, name, kind, url_enc, url_host, secret_enc, enabled, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(DEFAULT_ORG)
        .bind(name)
        .bind(kind)
        .bind(url_enc)
        .bind(url_host)
        .bind(secret_enc)
        .bind(enabled)
        .bind(now())
        .execute(self.conn())
        .await
        .map_err(write_error)?;
        Ok(r.last_insert_rowid())
    }

    /// For a read inside a transaction; see `Store::alert_channel_by_id`.
    pub async fn alert_channel_by_id(&mut self, id: i64) -> Result<Option<ChannelRow>> {
        let sql = format!("{CHANNEL_SELECT} WHERE id = ? AND org_id = ?");
        let row = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(id)
            .bind(DEFAULT_ORG)
            .fetch_optional(self.conn())
            .await?;
        Ok(row.as_ref().map(channel_from))
    }

    /// Changes what is given. `url` is the encrypted URL and its shown form.
    /// A taken name is `StoreError::Duplicate`; `false`: no such channel.
    pub async fn update_alert_channel(
        &mut self,
        id: i64,
        name: Option<&str>,
        url: Option<(&[u8], &str)>,
        enabled: Option<bool>,
    ) -> Result<bool> {
        let r = sqlx::query(
            "UPDATE alert_channels
             SET name = COALESCE(?, name),
                 url_enc = COALESCE(?, url_enc),
                 url_host = COALESCE(?, url_host),
                 enabled = COALESCE(?, enabled)
             WHERE id = ? AND org_id = ?",
        )
        .bind(name)
        .bind(url.map(|(enc, _)| enc))
        .bind(url.map(|(_, host)| host))
        .bind(enabled)
        .bind(id)
        .bind(DEFAULT_ORG)
        .execute(self.conn())
        .await
        .map_err(write_error)?;
        Ok(r.rows_affected() == 1)
    }

    pub async fn set_alert_channel_secret(&mut self, id: i64, secret_enc: &[u8]) -> Result<bool> {
        let r = sqlx::query("UPDATE alert_channels SET secret_enc = ? WHERE id = ? AND org_id = ?")
            .bind(secret_enc)
            .bind(id)
            .bind(DEFAULT_ORG)
            .execute(self.conn())
            .await?;
        Ok(r.rows_affected() == 1)
    }

    /// Its links to rules go with it. Returns `false` if there is no such channel.
    pub async fn delete_alert_channel(&mut self, id: i64) -> Result<bool> {
        let r = sqlx::query("DELETE FROM alert_channels WHERE id = ? AND org_id = ?")
            .bind(id)
            .bind(DEFAULT_ORG)
            .execute(self.conn())
            .await?;
        Ok(r.rows_affected() == 1)
    }

    pub async fn insert_alert_event(&mut self, e: NewAlertEvent<'_>) -> Result<i64> {
        let r = sqlx::query(
            "INSERT INTO alert_events
                 (org_id, rule_id, rule_name, kind, subject, state, summary, details, at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(DEFAULT_ORG)
        .bind(e.rule_id)
        .bind(e.rule_name)
        .bind(e.kind)
        .bind(e.subject)
        .bind(e.state)
        .bind(e.summary)
        .bind(e.details)
        .bind(e.at)
        .execute(self.conn())
        .await?;
        Ok(r.last_insert_rowid())
    }
}

impl Store {
    pub async fn alert_channel_by_id(&self, id: i64) -> Result<Option<ChannelRow>> {
        let sql = format!("{CHANNEL_SELECT} WHERE id = ? AND org_id = ?");
        let row = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(id)
            .bind(DEFAULT_ORG)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.as_ref().map(channel_from))
    }

    /// Ordered by name.
    pub async fn list_alert_channels(&self) -> Result<Vec<ChannelRow>> {
        let sql = format!("{CHANNEL_SELECT} WHERE org_id = ? ORDER BY name");
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(DEFAULT_ORG)
            .fetch_all(self.pool())
            .await?;
        Ok(rows.iter().map(channel_from).collect())
    }

    /// `(channel id, rule id, rule name)` of every link, by rule name.
    pub async fn alert_channel_rules(&self) -> Result<Vec<(i64, i64, String)>> {
        let rows = sqlx::query(
            "SELECT l.channel_id, r.id, r.name
             FROM alert_rule_channels l
             JOIN alert_rules r ON r.id = l.rule_id
             WHERE r.org_id = ?
             ORDER BY r.name",
        )
        .bind(DEFAULT_ORG)
        .fetch_all(self.pool())
        .await?;
        Ok(rows
            .iter()
            .map(|r| (r.get(0), r.get(1), r.get(2)))
            .collect())
    }

    pub async fn alert_event(&self, id: i64) -> Result<Option<AlertEventRow>> {
        let sql = format!("{EVENT_SELECT} WHERE id = ? AND org_id = ?");
        let row = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(id)
            .bind(DEFAULT_ORG)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.as_ref().map(event_from))
    }

    /// Newest first.
    pub async fn alert_events(&self, limit: i64) -> Result<Vec<AlertEventRow>> {
        let sql = format!("{EVENT_SELECT} WHERE org_id = ? ORDER BY id DESC LIMIT ?");
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(DEFAULT_ORG)
            .bind(limit)
            .fetch_all(self.pool())
            .await?;
        Ok(rows.iter().map(event_from).collect())
    }

    /// Writes the outcome of every channel in one statement. Returns
    /// `false` if there is no such event.
    pub async fn set_alert_event_deliveries(&self, id: i64, deliveries: &str) -> Result<bool> {
        let r = sqlx::query("UPDATE alert_events SET deliveries = ? WHERE id = ? AND org_id = ?")
            .bind(deliveries)
            .bind(id)
            .bind(DEFAULT_ORG)
            .execute(self.pool())
            .await?;
        Ok(r.rows_affected() == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::Cipher;
    use crate::store::StoreError;

    fn cipher() -> Cipher {
        Cipher::from_hex(&Cipher::generate_master_hex()).unwrap()
    }

    async fn add(s: &Store, c: &Cipher, name: &str) -> i64 {
        let mut tx = s.begin().await.unwrap();
        let id = tx
            .insert_alert_channel(
                name,
                "webhook",
                &c.encrypt(b"https://hooks.example.com/T0/secret-path?token=abc"),
                "https://hooks.example.com",
                &c.encrypt(b"whsec_abc"),
                true,
            )
            .await
            .unwrap();
        tx.commit().await.unwrap();
        id
    }

    #[tokio::test]
    async fn migration_creates_every_alert_table() {
        let s = Store::open_in_memory().await.unwrap();
        for table in [
            "alert_channels",
            "alert_rules",
            "alert_rule_channels",
            "alert_state",
            "alert_events",
        ] {
            let n: i64 =
                sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) FROM {table}")))
                    .fetch_one(s.pool())
                    .await
                    .unwrap();
            assert_eq!(n, 0, "{table}");
        }
    }

    #[tokio::test]
    async fn url_and_secret_are_stored_encrypted_and_round_trip() {
        let s = Store::open_in_memory().await.unwrap();
        let c = cipher();
        let id = add(&s, &c, "ops").await;
        let row = s.alert_channel_by_id(id).await.unwrap().unwrap();
        assert_eq!(row.name, "ops");
        assert_eq!(row.url_host, "https://hooks.example.com");
        assert!(row.enabled);
        assert_eq!(
            c.decrypt(&row.url_enc).unwrap(),
            b"https://hooks.example.com/T0/secret-path?token=abc"
        );
        assert_eq!(c.decrypt(&row.secret_enc).unwrap(), b"whsec_abc");
        // Nothing readable is in the stored bytes.
        let raw: Vec<u8> = sqlx::query_scalar("SELECT url_enc FROM alert_channels WHERE id = ?")
            .bind(id)
            .fetch_one(s.pool())
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&raw).to_string();
        assert!(!text.contains("secret-path") && !text.contains("hooks.example.com"));
        // Debug shows the host (what may be shown) and no URL path, secret or bytes.
        let shown = format!("{row:?}");
        assert!(
            !shown.contains("secret-path") && !shown.contains("whsec") && !shown.contains("token")
        );
    }

    #[tokio::test]
    async fn a_taken_name_is_a_duplicate_and_update_and_delete_work() {
        let s = Store::open_in_memory().await.unwrap();
        let c = cipher();
        let a = add(&s, &c, "a").await;
        let b = add(&s, &c, "b").await;
        let mut tx = s.begin().await.unwrap();
        let err = tx
            .insert_alert_channel("a", "slack", b"x", "h", b"y", true)
            .await
            .unwrap_err();
        assert!(matches!(
            err.downcast_ref::<StoreError>(),
            Some(StoreError::Duplicate)
        ));
        drop(tx);
        let mut tx = s.begin().await.unwrap();
        let err = tx
            .update_alert_channel(b, Some("a"), None, None)
            .await
            .unwrap_err();
        assert!(matches!(
            err.downcast_ref::<StoreError>(),
            Some(StoreError::Duplicate)
        ));
        drop(tx);
        let mut tx = s.begin().await.unwrap();
        assert!(tx
            .update_alert_channel(a, Some("c"), Some((b"new", "http://h:8080")), Some(false))
            .await
            .unwrap());
        assert!(tx.set_alert_channel_secret(a, b"sec").await.unwrap());
        assert!(!tx
            .update_alert_channel(999, None, None, None)
            .await
            .unwrap());
        tx.commit().await.unwrap();
        let row = s.alert_channel_by_id(a).await.unwrap().unwrap();
        assert_eq!(
            (row.name.as_str(), row.url_host.as_str(), row.enabled),
            ("c", "http://h:8080", false)
        );
        assert_eq!(row.url_enc, b"new");
        assert_eq!(row.secret_enc, b"sec");
        let mut tx = s.begin().await.unwrap();
        assert!(tx.delete_alert_channel(a).await.unwrap());
        assert!(!tx.delete_alert_channel(a).await.unwrap());
        tx.commit().await.unwrap();
        assert_eq!(s.list_alert_channels().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn events_round_trip_and_deliveries_are_replaced() {
        let s = Store::open_in_memory().await.unwrap();
        let mut tx = s.begin().await.unwrap();
        let id = tx
            .insert_alert_event(NewAlertEvent {
                rule_id: None,
                rule_name: "r",
                kind: "test",
                subject: "channel:1",
                state: "test",
                summary: "hello",
                details: "{}",
                at: "2999-01-01 00:00:00",
            })
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let e = s.alert_event(id).await.unwrap().unwrap();
        assert_eq!((e.state.as_str(), e.deliveries.as_str()), ("test", "[]"));
        assert!(s.set_alert_event_deliveries(id, "[1]").await.unwrap());
        assert_eq!(s.alert_events(10).await.unwrap()[0].deliveries, "[1]");
        let mut tx = s.begin().await.unwrap();
        let bad = tx
            .insert_alert_event(NewAlertEvent {
                rule_id: None,
                rule_name: "r",
                kind: "k",
                subject: "s",
                state: "bogus",
                summary: "",
                details: "{}",
                at: "2999-01-01 00:00:00",
            })
            .await;
        assert!(bad.is_err(), "the state is checked");
    }

    /// A rule id is given out again after a delete (rowid reuse). What the
    /// old rule left behind must not pass to the new one.
    #[tokio::test]
    async fn a_reused_rule_id_inherits_nothing() {
        let s = Store::open_in_memory().await.unwrap();
        let c = cipher();
        let channel = add(&s, &c, "ops").await;
        let old: i64 = sqlx::query_scalar(
            "INSERT INTO alert_rules (name, kind, params, created_at)
             VALUES ('old', 'circuit_open', '{}', '2999-01-01 00:00:00') RETURNING id",
        )
        .fetch_one(s.pool())
        .await
        .unwrap();
        sqlx::query("INSERT INTO alert_rule_channels (rule_id, channel_id) VALUES (?, ?)")
            .bind(old)
            .bind(channel)
            .execute(s.pool())
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO alert_state (rule_id, subject, firing, since)
             VALUES (?, 'target:p/m', 1, '2999-01-01 00:00:00')",
        )
        .bind(old)
        .execute(s.pool())
        .await
        .unwrap();
        let mut tx = s.begin().await.unwrap();
        let event = tx
            .insert_alert_event(NewAlertEvent {
                rule_id: Some(old),
                rule_name: "old",
                kind: "circuit_open",
                subject: "target:p/m",
                state: "firing",
                summary: "x",
                details: "{}",
                at: "2999-01-01 00:00:00",
            })
            .await
            .unwrap();
        tx.commit().await.unwrap();
        sqlx::query("DELETE FROM alert_rules WHERE id = ?")
            .bind(old)
            .execute(s.pool())
            .await
            .unwrap();
        let new: i64 = sqlx::query_scalar(
            "INSERT INTO alert_rules (name, kind, params, created_at)
             VALUES ('new', 'circuit_open', '{}', '2999-01-01 00:00:00') RETURNING id",
        )
        .fetch_one(s.pool())
        .await
        .unwrap();
        assert_eq!(new, old, "the id is given out again");
        for table in ["alert_state", "alert_rule_channels"] {
            let n: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                "SELECT COUNT(*) FROM {table} WHERE rule_id = ?"
            )))
            .bind(new)
            .fetch_one(s.pool())
            .await
            .unwrap();
            assert_eq!(n, 0, "{table}");
        }
        // The old rule's event stays, tied to no rule, with its name.
        let e = s.alert_event(event).await.unwrap().unwrap();
        assert_eq!((e.rule_id, e.rule_name.as_str()), (None, "old"));
    }
}
