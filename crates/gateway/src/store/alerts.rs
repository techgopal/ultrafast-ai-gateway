//! Alert channels and events. A channel's URL and secret are kept encrypted
//! with the master key; neither is ever read back except to deliver.

use std::fmt;

use anyhow::Result;
use sqlx::any::AnyRow;
use sqlx::AnyConnection;
use sqlx::Row;

use super::dialect::Dialected;
use super::{flag, now, write_error, Store, Tx, DEFAULT_ORG};

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

fn channel_from(r: &AnyRow) -> ChannelRow {
    ChannelRow {
        id: r.get("id"),
        name: r.get("name"),
        kind: r.get("kind"),
        url_enc: r.get("url_enc"),
        url_host: r.get("url_host"),
        secret_enc: r.get("secret_enc"),
        enabled: r.get::<i64, _>("enabled") != 0,
        created_at: r.get("created_at"),
    }
}

const RULE_SELECT: &str = "SELECT id, name, kind, params, enabled, created_at FROM alert_rules";

/// A rule as stored. `params` is JSON, validated when it was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleRow {
    pub id: i64,
    pub name: String,
    /// `budget`, `error_rate` or `circuit_open`.
    pub kind: String,
    pub params: String,
    pub enabled: bool,
    pub created_at: String,
    /// The channels it sends to, by id.
    pub channel_ids: Vec<i64>,
}

/// A subject a rule is firing for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateRow {
    pub rule_id: i64,
    pub subject: String,
    /// UTC, `YYYY-MM-DD HH:MM:SS`.
    pub since: String,
}

fn rule_from(r: &AnyRow) -> RuleRow {
    RuleRow {
        id: r.get("id"),
        name: r.get("name"),
        kind: r.get("kind"),
        params: r.get("params"),
        enabled: r.get::<i64, _>("enabled") != 0,
        created_at: r.get("created_at"),
        channel_ids: Vec::new(),
    }
}

/// Every rule by name, with its channels, on any connection.
pub(super) async fn list_alert_rules_in(conn: &mut AnyConnection) -> Result<Vec<RuleRow>> {
    let sql = format!("{RULE_SELECT} WHERE org_id = ? ORDER BY name");
    let mut rules: Vec<RuleRow> = conn
        .q_dyn(sql)
        .bind(DEFAULT_ORG)
        .fetch_all(&mut *conn)
        .await?
        .iter()
        .map(rule_from)
        .collect();
    let links = conn
        .q("SELECT l.rule_id, l.channel_id FROM alert_rule_channels l
         JOIN alert_rules r ON r.id = l.rule_id
         WHERE r.org_id = ? ORDER BY l.channel_id")
        .bind(DEFAULT_ORG)
        .fetch_all(&mut *conn)
        .await?;
    for link in &links {
        let (rule_id, channel_id): (i64, i64) = (link.get(0), link.get(1));
        if let Some(rule) = rules.iter_mut().find(|r| r.id == rule_id) {
            rule.channel_ids.push(channel_id);
        }
    }
    Ok(rules)
}

/// `(id, name, kind)` of every channel: no URL and no secret, not even encrypted.
pub(super) async fn list_channel_names_in(
    conn: &mut AnyConnection,
) -> Result<Vec<(i64, String, String)>> {
    let rows = conn
        .q("SELECT id, name, kind FROM alert_channels WHERE org_id = ? ORDER BY name")
        .bind(DEFAULT_ORG)
        .fetch_all(conn)
        .await?;
    Ok(rows
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2)))
        .collect())
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

fn event_from(r: &AnyRow) -> AlertEventRow {
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
        let id: i64 = self
            .scalar(
                "INSERT INTO alert_channels
                 (org_id, name, kind, url_enc, url_host, secret_enc, enabled, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
            )
            .bind(DEFAULT_ORG)
            .bind(name)
            .bind(kind)
            .bind(url_enc)
            .bind(url_host)
            .bind(secret_enc)
            .bind(flag(enabled))
            .bind(now())
            .fetch_one(self.conn())
            .await
            .map_err(write_error)?;
        Ok(id)
    }

    /// For a read inside a transaction; see `Store::alert_channel_by_id`.
    pub async fn alert_channel_by_id(&mut self, id: i64) -> Result<Option<ChannelRow>> {
        let sql = format!("{CHANNEL_SELECT} WHERE id = ? AND org_id = ?");
        let row = self
            .q_dyn(sql)
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
        let r = self
            .q("UPDATE alert_channels
             SET name = COALESCE(?, name),
                 url_enc = COALESCE(?, url_enc),
                 url_host = COALESCE(?, url_host),
                 enabled = COALESCE(?, enabled)
             WHERE id = ? AND org_id = ?")
            .bind(name)
            .bind(url.map(|(enc, _)| enc))
            .bind(url.map(|(_, host)| host))
            .bind(enabled.map(flag))
            .bind(id)
            .bind(DEFAULT_ORG)
            .execute(self.conn())
            .await
            .map_err(write_error)?;
        Ok(r.rows_affected() == 1)
    }

    pub async fn set_alert_channel_secret(&mut self, id: i64, secret_enc: &[u8]) -> Result<bool> {
        let r = self
            .q("UPDATE alert_channels SET secret_enc = ? WHERE id = ? AND org_id = ?")
            .bind(secret_enc)
            .bind(id)
            .bind(DEFAULT_ORG)
            .execute(self.conn())
            .await?;
        Ok(r.rows_affected() == 1)
    }

    /// Its links to rules go with it. Returns `false` if there is no such channel.
    pub async fn delete_alert_channel(&mut self, id: i64) -> Result<bool> {
        let r = self
            .q("DELETE FROM alert_channels WHERE id = ? AND org_id = ?")
            .bind(id)
            .bind(DEFAULT_ORG)
            .execute(self.conn())
            .await?;
        Ok(r.rows_affected() == 1)
    }

    /// A taken name is `StoreError::Duplicate`.
    pub async fn insert_alert_rule(
        &mut self,
        name: &str,
        kind: &str,
        params: &str,
        enabled: bool,
    ) -> Result<i64> {
        let id: i64 = self
            .scalar(
                "INSERT INTO alert_rules (org_id, name, kind, params, enabled, created_at)
             VALUES (?, ?, ?, ?, ?, ?) RETURNING id",
            )
            .bind(DEFAULT_ORG)
            .bind(name)
            .bind(kind)
            .bind(params)
            .bind(flag(enabled))
            .bind(now())
            .fetch_one(self.conn())
            .await
            .map_err(write_error)?;
        Ok(id)
    }

    /// Changes what is given. A taken name is `StoreError::Duplicate`;
    /// `false`: no such rule.
    pub async fn update_alert_rule(
        &mut self,
        id: i64,
        name: Option<&str>,
        params: Option<&str>,
        enabled: Option<bool>,
    ) -> Result<bool> {
        let r = self
            .q("UPDATE alert_rules
             SET name = COALESCE(?, name),
                 params = COALESCE(?, params),
                 enabled = COALESCE(?, enabled)
             WHERE id = ? AND org_id = ?")
            .bind(name)
            .bind(params)
            .bind(enabled.map(flag))
            .bind(id)
            .bind(DEFAULT_ORG)
            .execute(self.conn())
            .await
            .map_err(write_error)?;
        Ok(r.rows_affected() == 1)
    }

    /// Replaces the channels a rule sends to.
    pub async fn set_alert_rule_channels(
        &mut self,
        rule_id: i64,
        channel_ids: &[i64],
    ) -> Result<()> {
        self.q("DELETE FROM alert_rule_channels WHERE rule_id = ?")
            .bind(rule_id)
            .execute(self.conn())
            .await?;
        for channel_id in channel_ids {
            self.q(
                "INSERT INTO alert_rule_channels (rule_id, channel_id) VALUES (?, ?)
                 ON CONFLICT DO NOTHING",
            )
            .bind(rule_id)
            .bind(channel_id)
            .execute(self.conn())
            .await?;
        }
        Ok(())
    }

    /// Its links, states and channels' links go with it; its events stay.
    pub async fn delete_alert_rule(&mut self, id: i64) -> Result<bool> {
        let r = self
            .q("DELETE FROM alert_rules WHERE id = ? AND org_id = ?")
            .bind(id)
            .bind(DEFAULT_ORG)
            .execute(self.conn())
            .await?;
        Ok(r.rows_affected() == 1)
    }

    /// Forgets every subject a rule is firing for.
    pub async fn clear_alert_states(&mut self, rule_id: i64) -> Result<()> {
        self.q("DELETE FROM alert_state WHERE rule_id = ?")
            .bind(rule_id)
            .execute(self.conn())
            .await?;
        Ok(())
    }

    /// Marks `(rule, subject)` as firing, only while the rule exists and is
    /// enabled: a rule the API disabled a moment ago (and cleared) must not
    /// get a state back. Only the first writer wins: with several gateway
    /// processes on one database, the one that finds the state already
    /// there records nothing, so an episode is announced once. `false`:
    /// nothing was written.
    pub async fn upsert_alert_state(
        &mut self,
        rule_id: i64,
        subject: &str,
        since: &str,
    ) -> Result<bool> {
        let r = self
            .q("INSERT INTO alert_state (rule_id, subject, firing, since)
             SELECT ?, ?, 1, ? WHERE EXISTS (SELECT 1 FROM alert_rules WHERE id = ? AND enabled = 1)
             ON CONFLICT (rule_id, subject) DO UPDATE SET firing = 1, since = excluded.since
             WHERE alert_state.firing = 0")
            .bind(rule_id)
            .bind(subject)
            .bind(since)
            .bind(rule_id)
            .execute(self.conn())
            .await?;
        Ok(r.rows_affected() > 0)
    }

    /// Forgets one firing state. `false`: there was none (another process
    /// resolved it first, or the rule was cleared), so nothing is to be said.
    pub async fn delete_alert_state(&mut self, rule_id: i64, subject: &str) -> Result<bool> {
        let r = self
            .q("DELETE FROM alert_state WHERE rule_id = ? AND subject = ?")
            .bind(rule_id)
            .bind(subject)
            .execute(self.conn())
            .await?;
        Ok(r.rows_affected() > 0)
    }

    /// Forgets the subjects of a rule that start with `prefix`, except `keep`:
    /// the periods of a budget that are over.
    pub async fn delete_alert_states_except(
        &mut self,
        rule_id: i64,
        prefix: &str,
        keep: &str,
    ) -> Result<()> {
        self.q("DELETE FROM alert_state
             WHERE rule_id = ? AND substr(subject, 1, length(?)) = ? AND subject <> ?")
            .bind(rule_id)
            .bind(prefix)
            .bind(prefix)
            .bind(keep)
            .execute(self.conn())
            .await?;
        Ok(())
    }

    pub async fn insert_alert_event(&mut self, e: NewAlertEvent<'_>) -> Result<i64> {
        let id: i64 = self
            .scalar(
                "INSERT INTO alert_events
                 (org_id, rule_id, rule_name, kind, subject, state, summary, details, at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
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
            .fetch_one(self.conn())
            .await?;
        Ok(id)
    }
}

impl Store {
    pub async fn alert_channel_by_id(&self, id: i64) -> Result<Option<ChannelRow>> {
        let sql = format!("{CHANNEL_SELECT} WHERE id = ? AND org_id = ?");
        let row = self
            .q_dyn(sql)
            .bind(id)
            .bind(DEFAULT_ORG)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.as_ref().map(channel_from))
    }

    /// Ordered by name.
    pub async fn list_alert_channels(&self) -> Result<Vec<ChannelRow>> {
        let sql = format!("{CHANNEL_SELECT} WHERE org_id = ? ORDER BY name");
        let rows = self
            .q_dyn(sql)
            .bind(DEFAULT_ORG)
            .fetch_all(self.pool())
            .await?;
        Ok(rows.iter().map(channel_from).collect())
    }

    /// `(channel id, rule id, rule name)` of every link, by rule name.
    pub async fn alert_channel_rules(&self) -> Result<Vec<(i64, i64, String)>> {
        let rows = self
            .q("SELECT l.channel_id, r.id, r.name
             FROM alert_rule_channels l
             JOIN alert_rules r ON r.id = l.rule_id
             WHERE r.org_id = ?
             ORDER BY r.name")
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
        let row = self
            .q_dyn(sql)
            .bind(id)
            .bind(DEFAULT_ORG)
            .fetch_optional(self.pool())
            .await?;
        Ok(row.as_ref().map(event_from))
    }

    /// Every rule by name, with the channels it sends to.
    pub async fn list_alert_rules(&self) -> Result<Vec<RuleRow>> {
        let mut conn = self.pool().acquire().await?;
        list_alert_rules_in(&mut conn).await
    }

    pub async fn alert_rule_by_id(&self, id: i64) -> Result<Option<RuleRow>> {
        Ok(self
            .list_alert_rules()
            .await?
            .into_iter()
            .find(|r| r.id == id))
    }

    /// What rules are firing for, by rule then subject.
    pub async fn alert_states(&self) -> Result<Vec<StateRow>> {
        let rows = self
            .q("SELECT s.rule_id, s.subject, s.since FROM alert_state s
             JOIN alert_rules r ON r.id = s.rule_id
             WHERE r.org_id = ? AND s.firing = 1 ORDER BY s.rule_id, s.subject")
            .bind(DEFAULT_ORG)
            .fetch_all(self.pool())
            .await?;
        Ok(rows
            .iter()
            .map(|r| StateRow {
                rule_id: r.get(0),
                subject: r.get(1),
                since: r.get(2),
            })
            .collect())
    }

    /// The enabled channels a rule sends to.
    pub async fn enabled_channel_ids_of_rule(&self, rule_id: i64) -> Result<Vec<i64>> {
        let rows = self
            .q("SELECT c.id FROM alert_rule_channels l
             JOIN alert_channels c ON c.id = l.channel_id
             WHERE l.rule_id = ? AND c.enabled = 1 ORDER BY c.id")
            .bind(rule_id)
            .fetch_all(self.pool())
            .await?;
        Ok(rows.iter().map(|r| r.get(0)).collect())
    }

    /// Newest first, at most `limit`, those before `before_id` when given.
    pub async fn alert_events_page(
        &self,
        rule_id: Option<i64>,
        state: Option<&str>,
        before_id: Option<i64>,
        limit: i64,
    ) -> Result<Vec<AlertEventRow>> {
        let sql = format!(
            "{EVENT_SELECT} WHERE org_id = ?
               AND (? IS NULL OR rule_id = ?)
               AND (? IS NULL OR state = ?)
               AND (? IS NULL OR id < ?)
             ORDER BY id DESC LIMIT ?"
        );
        let rows = self
            .q_dyn(sql)
            .bind(DEFAULT_ORG)
            .bind(rule_id)
            .bind(rule_id)
            .bind(state)
            .bind(state)
            .bind(before_id)
            .bind(before_id)
            .bind(limit)
            .fetch_all(self.pool())
            .await?;
        Ok(rows.iter().map(event_from).collect())
    }

    /// Newest first.
    pub async fn alert_events(&self, limit: i64) -> Result<Vec<AlertEventRow>> {
        let sql = format!("{EVENT_SELECT} WHERE org_id = ? ORDER BY id DESC LIMIT ?");
        let rows = self
            .q_dyn(sql)
            .bind(DEFAULT_ORG)
            .bind(limit)
            .fetch_all(self.pool())
            .await?;
        Ok(rows.iter().map(event_from).collect())
    }

    /// Writes the outcome of every channel in one statement. Returns
    /// `false` if there is no such event.
    pub async fn set_alert_event_deliveries(&self, id: i64, deliveries: &str) -> Result<bool> {
        let r = self
            .q("UPDATE alert_events SET deliveries = ? WHERE id = ? AND org_id = ?")
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
    use crate::store::Dialect;
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
        let raw: Vec<u8> = s
            .scalar("SELECT url_enc FROM alert_channels WHERE id = ?")
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
        s.q("INSERT INTO alert_rule_channels (rule_id, channel_id) VALUES (?, ?)")
            .bind(old)
            .bind(channel)
            .execute(s.pool())
            .await
            .unwrap();
        s.q("INSERT INTO alert_state (rule_id, subject, firing, since)
             VALUES (?, 'target:p/m', 1, '2999-01-01 00:00:00')")
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
        s.q("DELETE FROM alert_rules WHERE id = ?")
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
        match s.dialect() {
            // SQLite gives the id out again; that is what this test is about.
            Dialect::Sqlite => assert_eq!(new, old, "the id is given out again"),
            // PostgreSQL never does, so nothing could be inherited anyway.
            Dialect::Postgres => assert_ne!(new, old),
        }
        for table in ["alert_state", "alert_rule_channels"] {
            let n: i64 = s
                .scalar_dyn(format!("SELECT COUNT(*) FROM {table} WHERE rule_id = ?"))
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

    #[tokio::test]
    async fn rules_keep_their_channels_states_and_names() {
        let s = Store::open_in_memory().await.unwrap();
        let c = cipher();
        let a = add(&s, &c, "a").await;
        let b = add(&s, &c, "b").await;
        let mut tx = s.begin().await.unwrap();
        let rule = tx
            .insert_alert_rule("r", "circuit_open", "{}", true)
            .await
            .unwrap();
        tx.set_alert_rule_channels(rule, &[b, a, a]).await.unwrap();
        let err = tx
            .insert_alert_rule("r", "circuit_open", "{}", true)
            .await
            .unwrap_err();
        assert!(matches!(
            err.downcast_ref::<StoreError>(),
            Some(StoreError::Duplicate)
        ));
        drop(tx);
        let mut tx = s.begin().await.unwrap();
        let rule = tx
            .insert_alert_rule("r", "circuit_open", "{}", true)
            .await
            .unwrap();
        tx.set_alert_rule_channels(rule, &[b, a, a]).await.unwrap();
        tx.upsert_alert_state(rule, "budget:1:2999-01-01", "2999-01-01 00:00:00")
            .await
            .unwrap();
        tx.upsert_alert_state(rule, "budget:1:2999-02-01", "2999-02-01 00:00:00")
            .await
            .unwrap();
        tx.upsert_alert_state(rule, "budget:2:2999-01-01", "2999-01-01 00:00:00")
            .await
            .unwrap();
        tx.delete_alert_states_except(rule, "budget:1:", "budget:1:2999-02-01")
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let row = s.alert_rule_by_id(rule).await.unwrap().unwrap();
        assert_eq!(row.channel_ids, [a, b], "a link once, by channel id");
        let subjects: Vec<_> = s
            .alert_states()
            .await
            .unwrap()
            .into_iter()
            .map(|r| r.subject)
            .collect();
        assert_eq!(subjects, ["budget:1:2999-02-01", "budget:2:2999-01-01"]);
        assert_eq!(s.enabled_channel_ids_of_rule(rule).await.unwrap(), [a, b]);
        let mut tx = s.begin().await.unwrap();
        assert!(tx
            .update_alert_channel(a, None, None, Some(false))
            .await
            .unwrap());
        tx.commit().await.unwrap();
        assert_eq!(s.enabled_channel_ids_of_rule(rule).await.unwrap(), [b]);
        // Deleting a channel unlinks it; deleting the rule drops its states.
        let mut tx = s.begin().await.unwrap();
        tx.delete_alert_channel(b).await.unwrap();
        assert!(tx.delete_alert_rule(rule).await.unwrap());
        tx.commit().await.unwrap();
        assert!(s.alert_rule_by_id(rule).await.unwrap().is_none());
        assert!(s.alert_states().await.unwrap().is_empty());
    }
}
