//! The MCP catalog: a curated list of servers the project publishes, signed
//! with Ed25519, fetched at runtime, cached in the database and backed by a
//! copy compiled into the binary.
//!
//! An entry never registers itself: it only pre-fills what an admin adds
//! (`api::mcp::from_catalog`). The signature covers the exact bytes of the
//! file; its public key is compiled in, so rotating the key is a release.
//! With `UF_MCP_CATALOG=off` the gateway makes no request for the catalog at
//! all (air-gapped installs).

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex as StdMutex;
use std::time::Duration;

use anyhow::{bail, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tokio::sync::Mutex;

use crate::store::Store;

/// The copy compiled into the binary, its signature and the key that checks
/// every copy.
pub const BUILTIN_BODY: &str = include_str!("../../../../docs/mcp-catalog/v1.json");
pub const BUILTIN_SIG: &str = include_str!("../../../../docs/mcp-catalog/v1.json.sig");
pub const PUBLIC_KEY: &str = include_str!("../../../../docs/mcp-catalog/public-key.txt");

pub const DEFAULT_URL: &str = "https://mcp.ufaigw.com/mcp-catalog/v1.json";
pub const DEFAULT_REFRESH: Duration = Duration::from_secs(24 * 3600);
/// The largest catalog that is read.
pub const MAX_BYTES: usize = 1024 * 1024;
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_ENTRIES: usize = 500;
/// `max-age` is honoured within these bounds (GitHub Pages says ten minutes,
/// which is far more often than a catalog changes).
const MIN_AGE: i64 = 3600;
const MAX_AGE: i64 = 7 * 24 * 3600;

const K_BODY: &str = "mcp.catalog.body";
const K_SIG: &str = "mcp.catalog.sig";
const K_FETCHED: &str = "mcp.catalog.fetched_at";
const K_EXPIRES: &str = "mcp.catalog.expires_at";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct EnvVar {
    /// The name of the environment variable (stdio) or the one secret that
    /// authenticates to the server (http).
    pub name: String,
    pub secret: bool,
    pub help: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Entry {
    /// `^[a-z0-9][a-z0-9-]{0,62}$`; also the default name of the server.
    pub id: String,
    pub name: String,
    pub description: String,
    pub homepage: String,
    /// `http` or `stdio`.
    pub transport: String,
    #[serde(default)]
    #[schema(required)]
    pub url: Option<String>,
    #[serde(default)]
    #[schema(required)]
    pub command: Option<String>,
    #[serde(default)]
    #[schema(required)]
    pub args: Vec<String>,
    #[serde(default)]
    #[schema(required)]
    pub auth_header: Option<String>,
    #[serde(default)]
    #[schema(required)]
    pub env: Vec<EnvVar>,
    /// `none`, `bearer`, `header` or `oauth`.
    pub auth: String,
    #[serde(default)]
    #[schema(required)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct Doc {
    schema: u32,
    entries: Vec<Entry>,
}

fn name_ok(name: &str) -> bool {
    let b = name.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && (b[0].is_ascii_alphabetic() || b[0] == b'_')
        && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_')
}

/// Parses and checks a catalog. Anything off is an error, not a repair.
pub fn parse(body: &[u8]) -> Result<Vec<Entry>> {
    let doc: Doc = serde_json::from_slice(body)?;
    if doc.schema != 1 {
        bail!("unknown catalog schema");
    }
    if doc.entries.len() > MAX_ENTRIES {
        bail!("too many entries");
    }
    let mut seen = HashSet::new();
    for e in &doc.entries {
        super::validate_server_name(&e.id)?;
        if !seen.insert(e.id.as_str()) {
            bail!("duplicate entry id");
        }
        if e.name.is_empty() || e.name.len() > 100 || e.description.len() > 1000 {
            bail!("bad entry text");
        }
        if !["none", "bearer", "header", "oauth"].contains(&e.auth.as_str()) {
            bail!("bad auth");
        }
        match e.transport.as_str() {
            "http" => {
                let Some(url) = &e.url else {
                    bail!("an http entry needs a url");
                };
                crate::config::validate_base_url(url)?;
                if e.command.is_some() || !e.args.is_empty() {
                    bail!("an http entry has no command");
                }
            }
            "stdio" => {
                if e.command.as_deref().is_none_or(str::is_empty) || e.url.is_some() {
                    bail!("a stdio entry needs a command and no url");
                }
                if e.args.len() > 32 || e.args.iter().any(|a| a.len() > 512) {
                    bail!("bad args");
                }
            }
            _ => bail!("bad transport"),
        }
        if e.env.len() > 16 || e.env.iter().any(|v| !name_ok(&v.name)) {
            bail!("bad env");
        }
        if e.transport == "http"
            && matches!(e.auth.as_str(), "bearer" | "header")
            && e.env.iter().filter(|v| v.secret).count() != 1
        {
            bail!("an http entry with a secret names exactly one secret variable");
        }
        if e.auth == "header" && e.auth_header.is_none() {
            bail!("a header entry names the header");
        }
    }
    Ok(doc.entries)
}

fn decode<const N: usize>(text: &str) -> Option<[u8; N]> {
    STANDARD.decode(text.trim()).ok()?.try_into().ok()
}

/// The compiled-in key.
pub fn public_key() -> VerifyingKey {
    let bytes = decode::<32>(PUBLIC_KEY).expect("docs/mcp-catalog/public-key.txt is a base64 key");
    VerifyingKey::from_bytes(&bytes).expect("docs/mcp-catalog/public-key.txt is a valid key")
}

/// Whether `signature` (base64) is `key`'s signature of exactly `body`.
pub fn verify(key: &VerifyingKey, body: &[u8], signature: &str) -> bool {
    decode::<64>(signature)
        .is_some_and(|sig| key.verify(body, &Signature::from_bytes(&sig)).is_ok())
}

/// Where the entries a view shows came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// Fetched and verified since this process started.
    Remote,
    /// Fetched and verified earlier, kept in the database.
    Cache,
    /// The copy compiled into the binary.
    Builtin,
}

#[derive(Debug, Clone)]
pub struct View {
    pub entries: Vec<Entry>,
    pub source: Source,
    pub fetched_at: Option<String>,
    /// Our words for why the newest attempt did not give a newer copy.
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Config {
    /// `UF_MCP_CATALOG=off`: no request, ever.
    pub off: bool,
    pub url: String,
    /// How often the catalog is fetched when the server names no `max-age`.
    pub refresh: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            off: false,
            url: DEFAULT_URL.to_string(),
            refresh: DEFAULT_REFRESH,
        }
    }
}

impl Config {
    /// `UF_MCP_CATALOG` (`off`), `UF_MCP_CATALOG_URL` and
    /// `UF_MCP_CATALOG_REFRESH` (seconds). An unusable value reads as the default.
    pub fn from_env() -> Self {
        // A test build never reaches for the network on its own; the catalog
        // tests give their service an explicit `Config`.
        let off = cfg!(feature = "test-support")
            || std::env::var("UF_MCP_CATALOG").is_ok_and(|v| v.trim().eq_ignore_ascii_case("off"));
        let url = std::env::var("UF_MCP_CATALOG_URL")
            .ok()
            .map(|u| u.trim().to_string())
            .filter(|u| crate::config::validate_base_url(u).is_ok())
            .unwrap_or_else(|| DEFAULT_URL.to_string());
        let refresh = std::env::var("UF_MCP_CATALOG_REFRESH")
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .filter(|n| *n >= 60)
            .map_or(DEFAULT_REFRESH, Duration::from_secs);
        Self { off, url, refresh }
    }
}

/// The catalog service of a gateway process.
pub struct Catalog {
    store: Store,
    http: reqwest::Client,
    config: Config,
    key: VerifyingKey,
    /// Set by a successful refresh in this process, cleared by a failed one.
    fresh: AtomicBool,
    error: StdMutex<Option<String>>,
    /// The earliest a refresher pass looks at the database again.
    next_check: StdMutex<Option<OffsetDateTime>>,
    /// One refresh at a time.
    flight: Mutex<()>,
}

fn rfc3339(t: OffsetDateTime) -> String {
    t.format(&Rfc3339).unwrap_or_default()
}

/// `max-age` of a `Cache-Control` value.
fn max_age(value: &str) -> Option<i64> {
    value.split(',').find_map(|part| {
        part.trim()
            .strip_prefix("max-age=")
            .and_then(|n| n.trim().parse::<i64>().ok())
    })
}

impl Catalog {
    pub fn new(store: Store, http: reqwest::Client, config: Config, key: VerifyingKey) -> Self {
        Self {
            store,
            http,
            config,
            key,
            fresh: AtomicBool::new(false),
            error: StdMutex::new(None),
            next_check: StdMutex::new(None),
            flight: Mutex::new(()),
        }
    }

    pub fn from_env(store: Store, http: reqwest::Client) -> Self {
        Self::new(store, http, Config::from_env(), public_key())
    }

    /// The store, for a test that starts a second service on the same database.
    #[cfg(feature = "test-support")]
    pub fn store_for_tests(&self) -> Store {
        self.store.clone()
    }

    pub fn is_off(&self) -> bool {
        self.config.off
    }

    fn set_error(&self, e: Option<String>) {
        *self
            .error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = e;
    }

    /// The verified cached copy, if the database holds one that checks out.
    async fn cached(&self) -> Option<(Vec<Entry>, String)> {
        let body = self.store.setting(K_BODY).await.ok().flatten()?;
        let sig = self.store.setting(K_SIG).await.ok().flatten()?;
        if !verify(&self.key, body.as_bytes(), &sig) {
            return None;
        }
        let fetched = self.store.setting(K_FETCHED).await.ok().flatten()?;
        Some((parse(body.as_bytes()).ok()?, fetched))
    }

    /// What the console shows now.
    pub async fn view(&self) -> View {
        let error = self
            .error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let Some((entries, fetched)) = self.cached().await {
            let source = if self.fresh.load(Ordering::Relaxed) {
                Source::Remote
            } else {
                Source::Cache
            };
            return View {
                entries,
                source,
                fetched_at: Some(fetched),
                error,
            };
        }
        let entries = parse(BUILTIN_BODY.as_bytes()).unwrap_or_default();
        View {
            entries,
            source: Source::Builtin,
            fetched_at: None,
            error,
        }
    }

    /// The entry with this id in the copy that is shown.
    pub async fn entry(&self, id: &str) -> Option<Entry> {
        self.view().await.entries.into_iter().find(|e| e.id == id)
    }

    /// Fetches the catalog now (a manual refresh). A no-op when off. The
    /// outcome is in `view().error`.
    pub async fn refresh(&self, now: OffsetDateTime) {
        if self.config.off {
            return;
        }
        let _flight = self.flight.lock().await;
        self.refresh_locked(now).await;
    }

    /// Fetches the catalog when the cached copy is older than its `max-age`
    /// (or the configured interval). Cheap when nothing is due: the
    /// database is looked at once per due time, not once per call.
    pub async fn refresh_if_due(&self, now: OffsetDateTime) {
        if self.config.off {
            return;
        }
        let waiting = *self
            .next_check
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if waiting.is_some_and(|t| now < t) {
            return;
        }
        let _flight = self.flight.lock().await;
        // The database says when another process, or an earlier pass, last
        // got a copy.
        let expires = self
            .store
            .setting(K_EXPIRES)
            .await
            .ok()
            .flatten()
            .and_then(|v| v.parse::<i64>().ok());
        if let Some(at) = expires.and_then(|s| OffsetDateTime::from_unix_timestamp(s).ok()) {
            if now < at {
                *self
                    .next_check
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(at);
                return;
            }
        }
        self.refresh_locked(now).await;
    }

    async fn refresh_locked(&self, now: OffsetDateTime) {
        // Whatever happens, the next automatic look is one interval away.
        let retry = now + self.config.refresh.min(Duration::from_secs(3600));
        match self.fetch().await {
            Ok((body, sig, age)) => {
                let age = age.unwrap_or(self.config.refresh.as_secs() as i64);
                let expires = now + time::Duration::seconds(age.clamp(MIN_AGE, MAX_AGE));
                let stored = async {
                    let mut tx = self.store.begin_immediate().await?;
                    tx.put_setting(K_BODY, &body).await?;
                    tx.put_setting(K_SIG, &sig).await?;
                    tx.put_setting(K_FETCHED, &rfc3339(now)).await?;
                    tx.put_setting(K_EXPIRES, &expires.unix_timestamp().to_string())
                        .await?;
                    tx.commit().await
                }
                .await;
                if stored.is_err() {
                    self.fresh.store(false, Ordering::Relaxed);
                    self.set_error(Some("could not store the catalog".to_string()));
                    *self
                        .next_check
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(retry);
                    return;
                }
                self.fresh.store(true, Ordering::Relaxed);
                self.set_error(None);
                *self
                    .next_check
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(expires);
            }
            Err(why) => {
                self.fresh.store(false, Ordering::Relaxed);
                let shown = match self.cached().await {
                    Some((_, fetched)) => {
                        format!(
                            "showing the copy from {}",
                            fetched.get(..10).unwrap_or(&fetched)
                        )
                    }
                    None => "showing the built-in copy".to_string(),
                };
                self.set_error(Some(format!("{why}, {shown}")));
                *self
                    .next_check
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(retry);
            }
        }
    }

    /// One download of the catalog and its signature, checked. The error is
    /// our words.
    async fn fetch(&self) -> Result<(String, String, Option<i64>), &'static str> {
        const FAILED: &str = "could not fetch the catalog";
        let body = self.get(&self.config.url, true).await.ok_or(FAILED)?;
        let sig = self
            .get(&format!("{}.sig", self.config.url), false)
            .await
            .ok_or(FAILED)?;
        let (body, age) = body;
        let (sig, _) = sig;
        if !verify(&self.key, &body, &sig_text(&sig)) {
            return Err("catalog signature invalid");
        }
        if parse(&body).is_err() {
            return Err("catalog not valid");
        }
        let body = String::from_utf8(body).map_err(|_| "catalog not valid")?;
        Ok((body, sig_text(&sig), age))
    }

    async fn get(&self, url: &str, want_age: bool) -> Option<(Vec<u8>, Option<i64>)> {
        let resp = self
            .http
            .get(url)
            .timeout(FETCH_TIMEOUT)
            .send()
            .await
            .ok()?;
        if !resp.status().is_success() {
            return None;
        }
        if resp.content_length().is_some_and(|n| n > MAX_BYTES as u64) {
            return None;
        }
        let age = want_age
            .then(|| {
                resp.headers()
                    .get("cache-control")
                    .and_then(|v| v.to_str().ok())
                    .and_then(max_age)
            })
            .flatten();
        let mut buf = Vec::new();
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.ok()?;
            if buf.len() + chunk.len() > MAX_BYTES {
                return None;
            }
            buf.extend_from_slice(&chunk);
        }
        Some((buf, age))
    }
}

fn sig_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_catalog_is_signed_by_the_embedded_key() {
        assert!(
            verify(&public_key(), BUILTIN_BODY.as_bytes(), BUILTIN_SIG),
            "docs/mcp-catalog/v1.json was edited without signing it again"
        );
        let entries = parse(BUILTIN_BODY.as_bytes()).unwrap();
        assert!(entries.len() >= 9);
        assert!(entries.iter().any(|e| e.transport == "stdio"));
        assert!(entries.iter().any(|e| e.transport == "http"));
    }

    #[test]
    fn one_changed_byte_breaks_the_signature() {
        let mut body = BUILTIN_BODY.as_bytes().to_vec();
        body[40] ^= 1;
        assert!(!verify(&public_key(), &body, BUILTIN_SIG));
        assert!(!verify(
            &public_key(),
            BUILTIN_BODY.as_bytes(),
            "not base64!"
        ));
    }

    #[test]
    fn max_age_is_read_from_cache_control() {
        assert_eq!(max_age("public, max-age=3600"), Some(3600));
        assert_eq!(max_age("no-store"), None);
    }

    #[test]
    fn entries_are_checked() {
        let bad = |entry: &str| {
            parse(format!(r#"{{"schema":1,"entries":[{entry}]}}"#).as_bytes()).is_err()
        };
        let base = |extra: &str| {
            format!(
                r#"{{"id":"x","name":"X","description":"d","homepage":"https://x.example","auth":"none","tags":[]{extra}}}"#
            )
        };
        assert!(!bad(&base(
            r#","transport":"http","url":"https://x.example/mcp""#
        )));
        assert!(bad(&base(r#","transport":"http""#)));
        assert!(bad(&base(r#","transport":"stdio""#)));
        assert!(bad(&base(r#","transport":"ftp""#)));
        assert!(!bad(&base(r#","transport":"stdio","command":"npx""#)));
        assert!(bad(&base(
            r#","transport":"stdio","command":"npx","env":[{"name":"A B","secret":true,"help":""}]"#
        )));
        assert!(parse(br#"{"schema":2,"entries":[]}"#).is_err());
    }
}
