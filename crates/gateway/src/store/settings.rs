//! Settings of the gateway: a small key-value table.

use std::collections::HashMap;
use std::fmt;

use anyhow::Result;
use sqlx::AnyConnection;
use sqlx::Row;

use super::dialect::Dialected;
use super::{Store, Tx};

const LOG_RETENTION_DAYS: &str = "log_retention_days";
/// Used when the row is missing or unreadable.
pub const DEFAULT_LOG_RETENTION_DAYS: i64 = 30;

const SESSION_HOURS: &str = "session_hours";
/// How long a session lives when the setting is missing or unreadable.
pub const DEFAULT_SESSION_HOURS: i64 = 12;
/// The fewest and the most hours a session may live.
pub const SESSION_HOURS_RANGE: std::ops::RangeInclusive<i64> = 1..=720;

/// The label of the sign-in button when none was set.
pub const DEFAULT_OIDC_LABEL: &str = "SSO";
/// The claim that lists a user's groups when none was set.
pub const DEFAULT_OIDC_GROUPS_CLAIM: &str = "groups";

/// Settings of single sign-on with OpenID Connect. All keys start with
/// `oidc.`. A missing row means the default.
#[derive(Clone, PartialEq, Eq)]
pub struct OidcSettings {
    pub enabled: bool,
    pub label: String,
    pub issuer: String,
    pub client_id: String,
    /// The client secret encrypted with the master key, as hex. `None`:
    /// no secret was ever saved.
    pub client_secret_enc: Option<String>,
    /// Scopes sent besides `openid email profile`, space separated.
    pub scopes: String,
    pub groups_claim: String,
    pub admin_group: String,
    pub link_by_email: bool,
    pub auto_create: bool,
    /// Lower case.
    pub allowed_domains: Vec<String>,
}

impl Default for OidcSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            label: DEFAULT_OIDC_LABEL.to_string(),
            issuer: String::new(),
            client_id: String::new(),
            client_secret_enc: None,
            scopes: String::new(),
            groups_claim: DEFAULT_OIDC_GROUPS_CLAIM.to_string(),
            admin_group: String::new(),
            link_by_email: true,
            auto_create: false,
            allowed_domains: Vec::new(),
        }
    }
}

/// Shows only whether a secret is present.
impl fmt::Debug for OidcSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OidcSettings")
            .field("enabled", &self.enabled)
            .field("label", &self.label)
            .field("issuer", &self.issuer)
            .field("client_id", &self.client_id)
            .field(
                "client_secret_enc",
                &if self.client_secret_enc.is_some() {
                    "<present>"
                } else {
                    "<none>"
                },
            )
            .field("scopes", &self.scopes)
            .field("groups_claim", &self.groups_claim)
            .field("admin_group", &self.admin_group)
            .field("link_by_email", &self.link_by_email)
            .field("auto_create", &self.auto_create)
            .field("allowed_domains", &self.allowed_domains)
            .finish()
    }
}

async fn oidc_settings_in(conn: &mut AnyConnection) -> Result<OidcSettings> {
    let rows = conn
        .q("SELECT key, value FROM settings WHERE substr(key, 1, 5) = 'oidc.'")
        .fetch_all(conn)
        .await?;
    let mut values: HashMap<String, String> = rows
        .iter()
        .map(|r| (r.get("key"), r.get("value")))
        .collect();
    let mut s = OidcSettings::default();
    let flag = |v: Option<String>, default: bool| match v.as_deref() {
        Some("1") => true,
        Some("0") => false,
        _ => default,
    };
    s.enabled = flag(values.remove("oidc.enabled"), s.enabled);
    s.link_by_email = flag(values.remove("oidc.link_by_email"), s.link_by_email);
    s.auto_create = flag(values.remove("oidc.auto_create"), s.auto_create);
    if let Some(v) = values.remove("oidc.label").filter(|v| !v.is_empty()) {
        s.label = v;
    }
    if let Some(v) = values.remove("oidc.groups_claim").filter(|v| !v.is_empty()) {
        s.groups_claim = v;
    }
    s.issuer = values.remove("oidc.issuer").unwrap_or_default();
    s.client_id = values.remove("oidc.client_id").unwrap_or_default();
    s.client_secret_enc = values
        .remove("oidc.client_secret_enc")
        .filter(|v| !v.is_empty());
    s.scopes = values.remove("oidc.scopes").unwrap_or_default();
    s.admin_group = values.remove("oidc.admin_group").unwrap_or_default();
    s.allowed_domains = values
        .remove("oidc.allowed_domains")
        .unwrap_or_default()
        .split(',')
        .filter(|d| !d.is_empty())
        .map(str::to_string)
        .collect();
    Ok(s)
}

async fn put_setting(conn: &mut AnyConnection, key: &str, value: &str) -> Result<()> {
    conn.q("INSERT INTO settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value")
        .bind(key)
        .bind(value)
        .execute(conn)
        .await?;
    Ok(())
}

/// How many days request logs are kept, on the connection of a transaction.
pub(super) async fn log_retention_days_in(conn: &mut AnyConnection) -> Result<i64> {
    let value: Option<String> = conn
        .scalar("SELECT value FROM settings WHERE key = ?")
        .bind(LOG_RETENTION_DAYS)
        .fetch_optional(conn)
        .await?;
    Ok(value
        .and_then(|v| v.parse::<i64>().ok())
        .filter(|days| *days >= 1)
        .unwrap_or(DEFAULT_LOG_RETENTION_DAYS))
}

/// How many hours a new session lives, on the connection of a transaction.
pub(super) async fn session_hours_in(conn: &mut AnyConnection) -> Result<i64> {
    let value: Option<String> = conn
        .scalar("SELECT value FROM settings WHERE key = ?")
        .bind(SESSION_HOURS)
        .fetch_optional(conn)
        .await?;
    Ok(value
        .and_then(|v| v.parse::<i64>().ok())
        .filter(|hours| SESSION_HOURS_RANGE.contains(hours))
        .unwrap_or(DEFAULT_SESSION_HOURS))
}

impl Store {
    /// The single sign-on settings.
    pub async fn oidc_settings(&self) -> Result<OidcSettings> {
        let mut conn = self.pool().acquire().await?;
        oidc_settings_in(&mut conn).await
    }

    /// How many hours a new session lives.
    pub async fn session_hours(&self) -> Result<i64> {
        let mut conn = self.pool().acquire().await?;
        session_hours_in(&mut conn).await
    }

    /// How many days request logs are kept.
    pub async fn log_retention_days(&self) -> Result<i64> {
        let mut conn = self.pool().acquire().await?;
        log_retention_days_in(&mut conn).await
    }
}

impl Tx<'_> {
    /// Writes every single sign-on setting. The stored client secret is
    /// replaced only when `settings.client_secret_enc` holds one.
    pub async fn set_oidc_settings(&mut self, settings: &OidcSettings) -> Result<()> {
        let flag = |on: bool| if on { "1" } else { "0" };
        let conn = self.conn();
        put_setting(conn, "oidc.enabled", flag(settings.enabled)).await?;
        put_setting(conn, "oidc.label", &settings.label).await?;
        put_setting(conn, "oidc.issuer", &settings.issuer).await?;
        put_setting(conn, "oidc.client_id", &settings.client_id).await?;
        if let Some(secret) = &settings.client_secret_enc {
            put_setting(conn, "oidc.client_secret_enc", secret).await?;
        }
        put_setting(conn, "oidc.scopes", &settings.scopes).await?;
        put_setting(conn, "oidc.groups_claim", &settings.groups_claim).await?;
        put_setting(conn, "oidc.admin_group", &settings.admin_group).await?;
        put_setting(conn, "oidc.link_by_email", flag(settings.link_by_email)).await?;
        put_setting(conn, "oidc.auto_create", flag(settings.auto_create)).await?;
        put_setting(
            conn,
            "oidc.allowed_domains",
            &settings.allowed_domains.join(","),
        )
        .await?;
        Ok(())
    }

    /// How many hours a session made in this transaction lives.
    pub async fn session_hours(&mut self) -> Result<i64> {
        session_hours_in(self.conn()).await
    }

    pub async fn set_session_hours(&mut self, hours: i64) -> Result<()> {
        self.q("INSERT INTO settings (key, value) VALUES (?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value")
            .bind(SESSION_HOURS)
            .bind(hours.to_string())
            .execute(self.conn())
            .await?;
        Ok(())
    }

    pub async fn set_log_retention_days(&mut self, days: i64) -> Result<()> {
        self.q("INSERT INTO settings (key, value) VALUES (?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value")
            .bind(LOG_RETENTION_DAYS)
            .bind(days.to_string())
            .execute(self.conn())
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn oidc_settings_default_round_trip_and_keep_the_secret() {
        let store = Store::open_in_memory().await.unwrap();
        assert_eq!(
            store.oidc_settings().await.unwrap(),
            OidcSettings::default()
        );

        let saved = OidcSettings {
            enabled: true,
            label: "Corp".into(),
            issuer: "https://idp.example.com".into(),
            client_id: "gw".into(),
            client_secret_enc: Some("abcdef".into()),
            scopes: "offline_access".into(),
            groups_claim: "roles".into(),
            admin_group: "admins".into(),
            link_by_email: false,
            auto_create: true,
            allowed_domains: vec!["a.example".into(), "b.example".into()],
        };
        let mut tx = store.begin().await.unwrap();
        tx.set_oidc_settings(&saved).await.unwrap();
        tx.commit().await.unwrap();
        assert_eq!(store.oidc_settings().await.unwrap(), saved);

        // No new secret: the stored one stays.
        let mut next = saved.clone();
        next.client_secret_enc = None;
        next.label = "Other".into();
        let mut tx = store.begin().await.unwrap();
        tx.set_oidc_settings(&next).await.unwrap();
        tx.commit().await.unwrap();
        let read = store.oidc_settings().await.unwrap();
        assert_eq!(read.label, "Other");
        assert_eq!(read.client_secret_enc.as_deref(), Some("abcdef"));
        assert!(!format!("{read:?}").contains("abcdef"));
    }
}
