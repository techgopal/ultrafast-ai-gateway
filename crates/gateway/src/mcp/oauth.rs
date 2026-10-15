//! OAuth toward an MCP server, as the MCP authorization spec has it: the
//! gateway is the OAuth client and gets a token for the server itself.
//!
//! There is no passthrough. The virtual key of an agent is never sent to a
//! server, and a token the gateway holds is never sent to an agent; the token
//! is made for the server's URL (the RFC 8707 `resource`), by the client
//! credentials grant or by RFC 8693 token exchange, with credentials the admin
//! stored. One client acts for the server, not the person behind the agent.
//!
//! A token is kept in memory and, encrypted, in the database (so gateways that
//! share it, and a gateway that restarts, do not each ask the token endpoint
//! for one), until a minute before it ends. Asking is single-flight per
//! server. A server that answers 401 gets one new token and one more try; if
//! that is refused too, the call fails and the cached token is gone.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::StreamExt;
use serde_json::Value;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use tokio::sync::Mutex as AsyncMutex;

use super::upstream::UpstreamError;
use super::{decrypt_secret, encrypt_value};
use crate::secrets::Cipher;
use crate::store::{format_timestamp, parse_timestamp, McpServerRow, Store};

/// A token is used until this long before it ends.
pub const MARGIN: Duration = Duration::from_secs(60);
/// What `expires_in` is taken to be when the endpoint does not say.
const DEFAULT_LIFETIME: u64 = 300;
const MAX_BODY: usize = 64 * 1024;
const MAX_TOKEN: usize = 8 * 1024;
const TIMEOUT: Duration = Duration::from_secs(10);
const TOKEN_EXCHANGE: &str = "urn:ietf:params:oauth:grant-type:token-exchange";
const ACCESS_TOKEN: &str = "urn:ietf:params:oauth:token-type:access_token";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grant {
    ClientCredentials,
    TokenExchange,
}

impl Grant {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "client_credentials" => Some(Self::ClientCredentials),
            "token_exchange" => Some(Self::TokenExchange),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClientCredentials => "client_credentials",
            Self::TokenExchange => "token_exchange",
        }
    }
}

/// What it takes to get a token for a server. The secret is in the clear:
/// the client secret, or, for token exchange, the token to exchange.
#[derive(Clone)]
pub struct Config {
    pub server_id: i64,
    pub token_url: String,
    pub client_id: String,
    pub secret: String,
    pub scopes: Option<String>,
    pub grant: Grant,
    /// The server's URL: the resource the token is for.
    pub resource: String,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("server_id", &self.server_id)
            .field("grant", &self.grant)
            .finish_non_exhaustive()
    }
}

impl Config {
    /// `None` for a server that does not sign in with OAuth, or whose secret
    /// cannot be read.
    pub fn of_row(row: &McpServerRow, cipher: &Cipher) -> Option<Self> {
        Some(Self {
            server_id: row.id,
            token_url: row.oauth_token_url.clone()?,
            client_id: row.oauth_client_id.clone()?,
            secret: decrypt_secret(cipher, row.oauth_client_secret_enc.as_deref()?)?,
            scopes: row.oauth_scopes.clone().filter(|s| !s.is_empty()),
            grant: Grant::parse(row.oauth_grant.as_deref()?)?,
            resource: row.url.clone(),
        })
    }

    /// Changes whenever anything a token depends on changes.
    fn fingerprint(&self) -> String {
        let mut h = Sha256::new();
        for part in [
            &self.token_url,
            &self.client_id,
            &self.secret,
            self.scopes.as_deref().unwrap_or(""),
            self.grant.as_str(),
            &self.resource,
        ] {
            h.update(part.as_bytes());
            h.update([0]);
        }
        hex::encode(&h.finalize()[..8])
    }
}

struct Cached {
    token: String,
    fingerprint: String,
    ends: OffsetDateTime,
}

/// Gets and keeps the tokens of the servers.
pub struct TokenService {
    http: reqwest::Client,
    store: Store,
    cipher: Cipher,
    memory: Mutex<HashMap<i64, Cached>>,
    gates: Mutex<HashMap<i64, Arc<AsyncMutex<()>>>>,
}

impl TokenService {
    /// `http` must not follow redirects: the secret goes to the token URL only.
    pub fn new(http: reqwest::Client, store: Store, cipher: Cipher) -> Self {
        Self {
            http,
            store,
            cipher,
            memory: Mutex::default(),
            gates: Mutex::default(),
        }
    }

    fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
        m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn usable(ends: OffsetDateTime) -> bool {
        OffsetDateTime::now_utc() + MARGIN < ends
    }

    fn remembered(&self, cfg: &Config, fingerprint: &str) -> Option<String> {
        let memory = Self::lock(&self.memory);
        let c = memory.get(&cfg.server_id)?;
        (c.fingerprint == fingerprint && Self::usable(c.ends)).then(|| c.token.clone())
    }

    /// A token for the server: kept, or new. Asking is single-flight per server.
    pub async fn token(&self, cfg: &Config) -> Result<String, UpstreamError> {
        let fingerprint = cfg.fingerprint();
        if let Some(t) = self.remembered(cfg, &fingerprint) {
            return Ok(t);
        }
        let gate = Self::lock(&self.gates)
            .entry(cfg.server_id)
            .or_default()
            .clone();
        let _one_at_a_time = gate.lock().await;
        if let Some(t) = self.remembered(cfg, &fingerprint) {
            return Ok(t);
        }
        // Another gateway process, or an earlier run, may have one.
        if let Ok(Some((enc, ends))) = self.store.mcp_token(cfg.server_id).await {
            if let Some((token, ends)) = self.open(&enc, &ends, &fingerprint) {
                Self::lock(&self.memory).insert(
                    cfg.server_id,
                    Cached {
                        token: token.clone(),
                        fingerprint,
                        ends,
                    },
                );
                return Ok(token);
            }
        }
        let (token, ends) = self.fetch(cfg).await?;
        Self::lock(&self.memory).insert(
            cfg.server_id,
            Cached {
                token: token.clone(),
                fingerprint: fingerprint.clone(),
                ends,
            },
        );
        let sealed = serde_json::json!({"t": token, "f": fingerprint}).to_string();
        if let Err(e) = self
            .store
            .put_mcp_token(
                cfg.server_id,
                &encrypt_value(&self.cipher, &sealed),
                &format_timestamp(ends),
            )
            .await
        {
            tracing::warn!(error = %e, "could not keep an MCP token");
        }
        Ok(token)
    }

    /// The token and its end, from the stored form, if it is still good and
    /// was made for this configuration.
    fn open(&self, enc: &str, ends: &str, fingerprint: &str) -> Option<(String, OffsetDateTime)> {
        let ends = parse_timestamp(ends).filter(|e| Self::usable(*e))?;
        let sealed: Value = serde_json::from_str(&decrypt_secret(&self.cipher, enc)?).ok()?;
        (sealed["f"] == fingerprint).then_some(())?;
        Some((sealed["t"].as_str()?.to_string(), ends))
    }

    /// Forgets the token of a server, here and in the database.
    pub async fn forget(&self, server_id: i64) {
        Self::lock(&self.memory).remove(&server_id);
        if let Err(e) = self.store.delete_mcp_token(server_id).await {
            tracing::warn!(error = %e, "could not forget an MCP token");
        }
    }

    /// Runs `call` with a bearer token. A refusal (401) gets one new token and
    /// one more try, then it is the answer.
    pub async fn with_bearer<T, F, Fut>(
        &self,
        cfg: &Config,
        mut call: F,
    ) -> Result<T, UpstreamError>
    where
        F: FnMut(String) -> Fut,
        Fut: Future<Output = Result<T, UpstreamError>>,
    {
        let mut refreshed = false;
        loop {
            let token = self.token(cfg).await?;
            match call(token).await {
                Err(UpstreamError::Auth) if !refreshed => {
                    refreshed = true;
                    self.forget(cfg.server_id).await;
                }
                other => {
                    if matches!(other, Err(UpstreamError::Auth)) {
                        // Refused again: a token that does not work is not kept.
                        self.forget(cfg.server_id).await;
                    }
                    return other;
                }
            }
        }
    }

    async fn fetch(&self, cfg: &Config) -> Result<(String, OffsetDateTime), UpstreamError> {
        let mut form: Vec<(&str, &str)> = Vec::new();
        match cfg.grant {
            Grant::ClientCredentials => {
                form.push(("grant_type", "client_credentials"));
                form.push(("client_id", &cfg.client_id));
                form.push(("client_secret", &cfg.secret));
            }
            Grant::TokenExchange => {
                form.push(("grant_type", TOKEN_EXCHANGE));
                form.push(("client_id", &cfg.client_id));
                form.push(("subject_token", &cfg.secret));
                form.push(("subject_token_type", ACCESS_TOKEN));
                form.push(("requested_token_type", ACCESS_TOKEN));
            }
        }
        form.push(("resource", &cfg.resource));
        if let Some(scopes) = &cfg.scopes {
            form.push(("scope", scopes));
        }
        let resp = self
            .http
            .post(&cfg.token_url)
            .timeout(TIMEOUT)
            .header("accept", "application/json")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(encode_form(&form))
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    UpstreamError::Timeout
                } else {
                    UpstreamError::Connect
                }
            })?;
        if !resp.status().is_success() {
            return Err(UpstreamError::Auth);
        }
        let mut body = Vec::new();
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| UpstreamError::Connect)?;
            if body.len() + chunk.len() > MAX_BODY {
                return Err(UpstreamError::TooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        let reply: Value = serde_json::from_slice(&body).map_err(|_| UpstreamError::Protocol)?;
        let token = reply["access_token"]
            .as_str()
            .filter(|t| !t.is_empty() && t.len() <= MAX_TOKEN && !t.contains(char::is_control))
            .ok_or(UpstreamError::Protocol)?
            .to_string();
        if reply["token_type"]
            .as_str()
            .is_some_and(|t| !t.eq_ignore_ascii_case("bearer"))
        {
            return Err(UpstreamError::Protocol);
        }
        let lifetime = reply["expires_in"].as_u64().unwrap_or(DEFAULT_LIFETIME);
        let ends = OffsetDateTime::now_utc()
            + time::Duration::seconds(i64::try_from(lifetime).unwrap_or(i64::MAX / 2).min(86_400));
        Ok((token, ends))
    }
}

/// `name=value` pairs joined by `&`, every byte outside the unreserved set of
/// RFC 3986 percent-encoded.
fn encode_form(pairs: &[(&str, &str)]) -> String {
    fn escape(text: &str, out: &mut String) {
        for b in text.bytes() {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
                out.push(char::from(b));
            } else {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    let mut out = String::new();
    for (i, (name, value)) in pairs.iter().enumerate() {
        if i > 0 {
            out.push('&');
        }
        escape(name, &mut out);
        out.push('=');
        escape(value, &mut out);
    }
    out
}

/// Checks a token URL against what the server says of itself
/// (`/.well-known/oauth-protected-resource`, RFC 9728): when it names
/// authorization servers, the token URL must be on one of them. A server that
/// says nothing is not held to anything. Nothing here is sent a secret.
pub async fn check_token_url(
    http: &reqwest::Client,
    server_url: &str,
    token_url: &str,
) -> Result<(), String> {
    let (Ok(server), Ok(token)) = (
        reqwest::Url::parse(server_url),
        reqwest::Url::parse(token_url),
    ) else {
        return Err("the address cannot be read".to_string());
    };
    let origin = |u: &reqwest::Url| {
        Some((
            u.scheme().to_string(),
            u.host_str()?.to_ascii_lowercase(),
            u.port_or_known_default(),
        ))
    };
    let mut well_known = server.clone();
    let path = server.path().trim_end_matches('/').to_string();
    well_known.set_path(&format!("/.well-known/oauth-protected-resource{path}"));
    well_known.set_query(None);
    let Ok(resp) = http
        .get(well_known)
        .timeout(Duration::from_secs(5))
        .send()
        .await
    else {
        return Ok(());
    };
    if !resp.status().is_success() {
        return Ok(());
    }
    let mut body = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(Ok(chunk)) = stream.next().await {
        if body.len() + chunk.len() > MAX_BODY {
            return Ok(());
        }
        body.extend_from_slice(&chunk);
    }
    let Ok(doc) = serde_json::from_slice::<Value>(&body) else {
        return Ok(());
    };
    let Some(servers) = doc["authorization_servers"]
        .as_array()
        .filter(|a| !a.is_empty())
    else {
        return Ok(());
    };
    let wanted = origin(&token);
    let named = servers
        .iter()
        .filter_map(Value::as_str)
        .filter_map(|s| reqwest::Url::parse(s).ok())
        .any(|s| origin(&s) == wanted);
    if named {
        Ok(())
    } else {
        Err("The token URL is not on an authorization server this MCP server names.".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_form_is_percent_encoded() {
        assert_eq!(
            encode_form(&[("a", "b c"), ("r", "https://x/y?z=1&w"), ("u", "é~-._")]),
            "a=b%20c&r=https%3A%2F%2Fx%2Fy%3Fz%3D1%26w&u=%C3%A9~-._"
        );
    }

    #[test]
    fn a_configuration_has_a_fingerprint_that_follows_what_a_token_depends_on() {
        let base = Config {
            server_id: 1,
            token_url: "https://idp.example/token".into(),
            client_id: "gw".into(),
            secret: "s".into(),
            scopes: None,
            grant: Grant::ClientCredentials,
            resource: "https://mcp.example/mcp".into(),
        };
        let same = base.clone();
        assert_eq!(base.fingerprint(), same.fingerprint());
        for changed in [
            Config {
                token_url: "https://other.example/token".into(),
                ..base.clone()
            },
            Config {
                client_id: "x".into(),
                ..base.clone()
            },
            Config {
                secret: "t".into(),
                ..base.clone()
            },
            Config {
                scopes: Some("a".into()),
                ..base.clone()
            },
            Config {
                grant: Grant::TokenExchange,
                ..base.clone()
            },
            Config {
                resource: "https://mcp.example/other".into(),
                ..base.clone()
            },
        ] {
            assert_ne!(base.fingerprint(), changed.fingerprint());
        }
        assert!(!format!("{base:?}").contains("idp.example"));
        assert_eq!(Grant::parse("token_exchange"), Some(Grant::TokenExchange));
        assert_eq!(Grant::parse("password"), None);
    }
}
