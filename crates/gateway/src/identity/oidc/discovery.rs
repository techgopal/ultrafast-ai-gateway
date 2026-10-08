//! The provider's discovery document and signing keys, fetched with the
//! gateway's HTTP client (which follows no redirects), limited in size and
//! time, and cached.

use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use jsonwebtoken::Algorithm;
use reqwest::Url;
use serde_json::Value;
use tokio::sync::Mutex;
use tokio::time::Instant;

use crate::identity::external::ExternalError;

/// The largest document or answer read from the provider.
pub const MAX_BODY_BYTES: usize = 1024 * 1024;
/// How long one request to the provider may take.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a discovery document or a key set is kept.
pub const CACHE_TTL: Duration = Duration::from_secs(3600);
/// How soon after a fetch an unknown `kid` may cause another one.
pub const MIN_REFETCH: Duration = Duration::from_secs(60);
/// How long a failed discovery fetch is remembered.
pub const FAILURE_TTL: Duration = Duration::from_secs(30);
const MAX_KEYS: usize = 100;
const MIN_RSA_BITS: usize = 2048;

/// Reads a JSON answer: status 2xx, no redirect, at most [`MAX_BODY_BYTES`].
/// The errors say what happened and never hold the provider's text.
pub async fn get_json(
    http: &reqwest::Client,
    url: &Url,
    bearer: Option<&str>,
) -> Result<Value, String> {
    let mut request = http
        .get(url.clone())
        .header("accept", "application/json")
        .timeout(REQUEST_TIMEOUT);
    if let Some(token) = bearer {
        request = request.bearer_auth(token);
    }
    let response = request.send().await;
    read_json(response).await
}

/// Reads the answer of a request to the provider as capped JSON.
pub async fn read_json(
    response: Result<reqwest::Response, reqwest::Error>,
) -> Result<Value, String> {
    let mut resp = response.map_err(|e| {
        if e.is_timeout() {
            "the provider did not answer in time".to_string()
        } else {
            "the provider could not be reached".to_string()
        }
    })?;
    let status = resp.status();
    if status.is_redirection() {
        return Err("the provider answered with a redirect".to_string());
    }
    if !status.is_success() {
        return Err(format!("the provider answered HTTP {}", status.as_u16()));
    }
    if resp
        .content_length()
        .is_some_and(|n| n > MAX_BODY_BYTES as u64)
    {
        return Err("the provider's answer is too large".to_string());
    }
    let mut body = Vec::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|_| "the provider's answer could not be read".to_string())?
    {
        if body.len() + chunk.len() > MAX_BODY_BYTES {
            return Err("the provider's answer is too large".to_string());
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(|_| "the provider's answer is not JSON".to_string())
}

/// An endpoint address the gateway may call: https, or http on this
/// machine for tests; a host; no credentials, no fragment.
pub fn usable_endpoint(value: &str) -> Option<Url> {
    usable_endpoint_for(value, None)
}

/// Like [`usable_endpoint`], and when `issuer` is given the endpoint must
/// also be of the issuer's kind: the same scheme, so a provider reached over
/// https cannot send the client secret and the code to a plain http
/// address, not even a loopback one. Loopback http is therefore possible
/// only when the issuer itself is loopback http (tests, local providers).
/// Private addresses are not blocked: the provider is chosen by an admin,
/// as the provider URLs are.
fn usable_endpoint_for(value: &str, issuer: Option<&Url>) -> Option<Url> {
    if value.len() > 2048 {
        return None;
    }
    let url = Url::parse(value).ok()?;
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    url.host_str()?;
    let scheme_ok = match url.scheme() {
        "https" => true,
        "http" => loopback,
        _ => false,
    };
    if issuer.is_some_and(|i| i.scheme() != url.scheme()) {
        return None;
    }
    (scheme_ok && url.username().is_empty() && url.password().is_none() && url.fragment().is_none())
        .then_some(url)
}

#[derive(Debug, Clone)]
pub struct Discovery {
    pub issuer: String,
    pub authorization_endpoint: Url,
    pub token_endpoint: Url,
    pub jwks_uri: Url,
    pub userinfo_endpoint: Option<Url>,
    /// Whether the token endpoint takes `client_secret_basic` (the default
    /// when the document does not say).
    pub basic_auth: bool,
}

impl Discovery {
    /// Reads and checks a discovery document for `issuer`.
    pub fn from_document(issuer: &str, doc: &Value) -> Result<Self, String> {
        // Exact: no trailing-slash or case folding. A provider that
        // answers for another issuer could otherwise lend its tokens.
        if doc.get("issuer").and_then(Value::as_str) != Some(issuer) {
            return Err("the document names another issuer".to_string());
        }
        let issuer_url = usable_endpoint(issuer).ok_or("the issuer is not usable")?;
        let endpoint = |name: &str| {
            doc.get(name)
                .and_then(Value::as_str)
                .and_then(|v| usable_endpoint_for(v, Some(&issuer_url)))
                .ok_or_else(|| format!("the document has no usable {name}"))
        };
        let basic_auth = match doc
            .get("token_endpoint_auth_methods_supported")
            .and_then(Value::as_array)
        {
            None => true,
            Some(methods) => methods
                .iter()
                .any(|m| m.as_str() == Some("client_secret_basic")),
        };
        Ok(Self {
            issuer: issuer.to_string(),
            authorization_endpoint: endpoint("authorization_endpoint")?,
            token_endpoint: endpoint("token_endpoint")?,
            jwks_uri: endpoint("jwks_uri")?,
            // An unusable userinfo address is ignored, never called.
            userinfo_endpoint: doc
                .get("userinfo_endpoint")
                .and_then(Value::as_str)
                .and_then(|v| usable_endpoint_for(v, Some(&issuer_url))),
            basic_auth,
        })
    }
}

pub struct DiscoveryCache {
    http: reqwest::Client,
    issuer: String,
    pub(super) ttl: Duration,
    /// How long a failed fetch is remembered.
    pub(super) failure_ttl: Duration,
    slot: Mutex<DiscoverySlot>,
}

#[derive(Default)]
struct DiscoverySlot {
    found: Option<(Instant, Arc<Discovery>)>,
    failed: Option<(Instant, ExternalError)>,
}

impl DiscoveryCache {
    pub fn new(http: reqwest::Client, issuer: &str) -> Self {
        Self {
            http,
            issuer: issuer.to_string(),
            ttl: CACHE_TTL,
            failure_ttl: FAILURE_TTL,
            slot: Mutex::new(DiscoverySlot::default()),
        }
    }

    /// The cached document, or a fresh one. Callers that arrive while a
    /// fetch is under way wait for it and share its result.
    ///
    /// A failure is remembered for [`FAILURE_TTL`] and given again meanwhile:
    /// the sign-in start is open to anyone, and a provider that is down must
    /// not turn each request into an outbound call.
    pub async fn get(&self) -> Result<Arc<Discovery>, ExternalError> {
        let mut slot = self.slot.lock().await;
        if let Some((at, found)) = slot.found.as_ref() {
            if at.elapsed() < self.ttl {
                return Ok(found.clone());
            }
        }
        if let Some((at, error)) = slot.failed.as_ref() {
            if at.elapsed() < self.failure_ttl {
                return Err(error.clone());
            }
        }
        match self.fetch().await {
            Ok(found) => {
                slot.failed = None;
                slot.found = Some((Instant::now(), found.clone()));
                Ok(found)
            }
            Err(error) => {
                slot.failed = Some((Instant::now(), error.clone()));
                Err(error)
            }
        }
    }

    async fn fetch(&self) -> Result<Arc<Discovery>, ExternalError> {
        let url = Url::parse(&format!(
            "{}/.well-known/openid-configuration",
            self.issuer.trim_end_matches('/')
        ))
        .map_err(|_| ExternalError::Discovery("the issuer is not a valid URL".to_string()))?;
        let doc = get_json(&self.http, &url, None)
            .await
            .map_err(ExternalError::Discovery)?;
        Ok(Arc::new(
            Discovery::from_document(&self.issuer, &doc).map_err(ExternalError::Discovery)?,
        ))
    }
}

/// What a signing key is made of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyMaterial {
    Rsa { n: String, e: String },
    Ec { curve: String, x: String, y: String },
}

#[derive(Debug, Clone)]
pub struct SigKey {
    pub kid: Option<String>,
    /// The `alg` the key is restricted to, if it says.
    pub alg: Option<String>,
    /// The `use` of the key, if it says.
    pub usage: Option<String>,
    pub material: KeyMaterial,
}

impl SigKey {
    /// Whether this key may verify a token signed with `alg`.
    pub fn fits(&self, alg: Algorithm) -> bool {
        if self.usage.as_deref().is_some_and(|u| u != "sig") {
            return false;
        }
        if self.alg.as_deref().is_some_and(|a| a != alg_name(alg)) {
            return false;
        }
        match (&self.material, alg) {
            (
                KeyMaterial::Rsa { .. },
                Algorithm::RS256
                | Algorithm::RS384
                | Algorithm::RS512
                | Algorithm::PS256
                | Algorithm::PS384
                | Algorithm::PS512,
            ) => true,
            (KeyMaterial::Ec { curve, .. }, Algorithm::ES256) => curve == "P-256",
            (KeyMaterial::Ec { curve, .. }, Algorithm::ES384) => curve == "P-384",
            _ => false,
        }
    }

    pub fn decoding_key(&self) -> Option<jsonwebtoken::DecodingKey> {
        match &self.material {
            KeyMaterial::Rsa { n, e } => jsonwebtoken::DecodingKey::from_rsa_components(n, e).ok(),
            KeyMaterial::Ec { x, y, .. } => {
                jsonwebtoken::DecodingKey::from_ec_components(x, y).ok()
            }
        }
    }

    /// Reads one key of a key set. Keys the gateway cannot verify with (other
    /// types, a modulus under 2048 bits) are `None`.
    fn from_json(value: &Value) -> Option<Self> {
        let text = |name: &str| value.get(name).and_then(Value::as_str).map(str::to_string);
        let material = match value.get("kty").and_then(Value::as_str)? {
            "RSA" => {
                let (n, e) = (text("n")?, text("e")?);
                let modulus = URL_SAFE_NO_PAD.decode(n.trim_end_matches('=')).ok()?;
                let bits = modulus.iter().skip_while(|b| **b == 0).count() * 8;
                if bits < MIN_RSA_BITS {
                    return None;
                }
                KeyMaterial::Rsa { n, e }
            }
            "EC" => KeyMaterial::Ec {
                curve: text("crv")?,
                x: text("x")?,
                y: text("y")?,
            },
            _ => return None,
        };
        let key = Self {
            kid: text("kid"),
            alg: text("alg"),
            usage: text("use"),
            material,
        };
        // A key that cannot be loaded is not kept.
        key.decoding_key().map(|_| key)
    }
}

/// The signing keys of the provider.
pub struct Jwks {
    http: reqwest::Client,
    uri: Url,
    pub(super) ttl: Duration,
    pub(super) min_refetch: Duration,
    state: Mutex<JwksState>,
}

#[derive(Default)]
struct JwksState {
    keys: Vec<SigKey>,
    /// The last successful fetch.
    fetched: Option<Instant>,
    /// The last attempt, successful or not, and whether it failed. The
    /// throttle counts attempts: a provider that is failing is not asked
    /// again within the minute either.
    attempted: Option<Instant>,
    failed: bool,
}

impl Jwks {
    pub fn new(http: reqwest::Client, uri: Url) -> Self {
        Self {
            http,
            uri,
            ttl: CACHE_TTL,
            min_refetch: MIN_REFETCH,
            state: Mutex::new(JwksState::default()),
        }
    }

    pub fn uri(&self) -> &Url {
        &self.uri
    }

    /// The key named `kid` (or the only key, when the token names none).
    ///
    /// The set is fetched when it is missing or older than the TTL. A `kid`
    /// it does not hold causes one more fetch, but not within
    /// [`MIN_REFETCH`] of the last one, so tokens with made-up key ids
    /// cannot make the gateway hammer the provider. The lock is held over
    /// the fetch: lookups that arrive meanwhile wait and share its result.
    pub async fn find(&self, kid: Option<&str>) -> Result<SigKey, ExternalError> {
        let mut state = self.state.lock().await;
        if state.fetched.is_none_or(|at| at.elapsed() >= self.ttl) {
            if state.failed
                && state
                    .attempted
                    .is_some_and(|at| at.elapsed() < self.min_refetch)
            {
                return Err(ExternalError::Discovery(
                    "key set: the last attempt failed".to_string(),
                ));
            }
            self.refresh(&mut state).await?;
        }
        if let Some(key) = lookup(&state.keys, kid) {
            return Ok(key);
        }
        if state
            .attempted
            .is_some_and(|at| at.elapsed() >= self.min_refetch)
        {
            self.refresh(&mut state).await?;
            if let Some(key) = lookup(&state.keys, kid) {
                return Ok(key);
            }
        }
        Err(ExternalError::Token("unknown signing key".to_string()))
    }

    async fn refresh(&self, state: &mut JwksState) -> Result<(), ExternalError> {
        state.attempted = Some(Instant::now());
        state.failed = true;
        let doc = get_json(&self.http, &self.uri, None)
            .await
            .map_err(|m| ExternalError::Discovery(format!("key set: {m}")))?;
        let keys = doc
            .get("keys")
            .and_then(Value::as_array)
            .ok_or_else(|| ExternalError::Discovery("key set: no keys".to_string()))?;
        state.keys = keys
            .iter()
            .take(MAX_KEYS)
            .filter_map(SigKey::from_json)
            .collect();
        state.fetched = Some(Instant::now());
        state.failed = false;
        Ok(())
    }
}

fn lookup(keys: &[SigKey], kid: Option<&str>) -> Option<SigKey> {
    match kid {
        Some(kid) => keys.iter().find(|k| k.kid.as_deref() == Some(kid)).cloned(),
        None if keys.len() == 1 => keys.first().cloned(),
        None => None,
    }
}

pub fn alg_name(alg: Algorithm) -> &'static str {
    match alg {
        Algorithm::RS256 => "RS256",
        Algorithm::RS384 => "RS384",
        Algorithm::RS512 => "RS512",
        Algorithm::PS256 => "PS256",
        Algorithm::PS384 => "PS384",
        Algorithm::PS512 => "PS512",
        Algorithm::ES256 => "ES256",
        Algorithm::ES384 => "ES384",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::oidc::testkit::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn http() -> reqwest::Client {
        crate::app::http_client()
    }

    async fn discovery_server(issuer_override: Option<&str>) -> MockServer {
        let server = MockServer::start().await;
        let issuer = issuer_override.map_or_else(|| server.uri(), str::to_string);
        Mock::given(method("GET"))
            .and(path("/.well-known/openid-configuration"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "issuer": issuer,
                "authorization_endpoint": format!("{}/authorize", server.uri()),
                "token_endpoint": format!("{}/token", server.uri()),
                "jwks_uri": format!("{}/jwks", server.uri()),
                "userinfo_endpoint": format!("{}/userinfo", server.uri()),
            })))
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn discovery_is_read_checked_and_cached() {
        let server = discovery_server(None).await;
        let cache = DiscoveryCache::new(http(), &server.uri());
        let a = cache.get().await.unwrap();
        assert_eq!(a.issuer, server.uri());
        assert_eq!(a.token_endpoint.path(), "/token");
        assert!(a.basic_auth);
        assert!(a.userinfo_endpoint.is_some());
        let b = cache.get().await.unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn discovery_is_refetched_after_the_ttl() {
        let server = discovery_server(None).await;
        let mut cache = DiscoveryCache::new(http(), &server.uri());
        cache.ttl = Duration::ZERO;
        cache.get().await.unwrap();
        cache.get().await.unwrap();
        assert_eq!(server.received_requests().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn a_document_naming_another_issuer_is_refused() {
        let server = discovery_server(Some("https://evil.example.com")).await;
        let cache = DiscoveryCache::new(http(), &server.uri());
        assert!(matches!(
            cache.get().await,
            Err(ExternalError::Discovery(m)) if m.contains("issuer")
        ));
    }

    #[tokio::test]
    async fn the_issuer_match_is_exact_not_trailing_slash_insensitive() {
        // The document says "<uri>", the configuration says "<uri>/".
        let server = discovery_server(None).await;
        let cache = DiscoveryCache::new(http(), &format!("{}/", server.uri()));
        assert!(matches!(
            cache.get().await,
            Err(ExternalError::Discovery(_))
        ));
    }

    #[test]
    fn discovery_documents_are_checked_field_by_field() {
        let good = serde_json::json!({
            "issuer": "https://idp.example.com",
            "authorization_endpoint": "https://idp.example.com/a",
            "token_endpoint": "https://idp.example.com/t",
            "jwks_uri": "https://idp.example.com/k",
        });
        assert!(Discovery::from_document("https://idp.example.com", &good).is_ok());
        for (field, value) in [
            (
                "authorization_endpoint",
                serde_json::json!("http://idp.example.com/a"),
            ),
            (
                "token_endpoint",
                serde_json::json!("ftp://idp.example.com/t"),
            ),
            (
                "jwks_uri",
                serde_json::json!("https://user:pw@idp.example.com/k"),
            ),
            ("jwks_uri", serde_json::json!(5)),
            ("token_endpoint", serde_json::Value::Null),
        ] {
            let mut doc = good.clone();
            doc[field] = value;
            assert!(
                Discovery::from_document("https://idp.example.com", &doc).is_err(),
                "{field}"
            );
        }
        let mut no_issuer = good.clone();
        no_issuer.as_object_mut().unwrap().remove("issuer");
        assert!(Discovery::from_document("https://idp.example.com", &no_issuer).is_err());
        // A trailing slash on one side only is a different issuer.
        assert!(Discovery::from_document("https://idp.example.com/", &good).is_err());
        // Loopback http is allowed (tests, local IdPs).
        let local = serde_json::json!({
            "issuer": "http://127.0.0.1:9000",
            "authorization_endpoint": "http://127.0.0.1:9000/a",
            "token_endpoint": "http://localhost:9000/t",
            "jwks_uri": "http://127.0.0.1:9000/k",
        });
        assert!(Discovery::from_document("http://127.0.0.1:9000", &local).is_ok());
    }

    #[test]
    fn the_token_endpoint_auth_method_follows_the_document() {
        let mut doc = serde_json::json!({
            "issuer": "https://i.example.com",
            "authorization_endpoint": "https://i.example.com/a",
            "token_endpoint": "https://i.example.com/t",
            "jwks_uri": "https://i.example.com/k",
            "token_endpoint_auth_methods_supported": ["client_secret_post"],
        });
        assert!(
            !Discovery::from_document("https://i.example.com", &doc)
                .unwrap()
                .basic_auth
        );
        doc["token_endpoint_auth_methods_supported"] =
            serde_json::json!(["client_secret_post", "client_secret_basic"]);
        assert!(
            Discovery::from_document("https://i.example.com", &doc)
                .unwrap()
                .basic_auth
        );
    }

    #[tokio::test]
    async fn a_failed_discovery_is_remembered_for_a_while() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        let mut cache = DiscoveryCache::new(http(), &server.uri());
        for _ in 0..5 {
            assert!(matches!(
                cache.get().await,
                Err(ExternalError::Discovery(_))
            ));
        }
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
        cache.failure_ttl = Duration::ZERO;
        assert!(cache.get().await.is_err());
        assert_eq!(server.received_requests().await.unwrap().len(), 2);
    }

    #[test]
    fn endpoints_must_be_of_the_issuers_kind() {
        let doc = |issuer: &str, token: &str| {
            serde_json::json!({
                "issuer": issuer,
                "authorization_endpoint": format!("{issuer}/a"),
                "token_endpoint": token,
                "jwks_uri": format!("{issuer}/k"),
            })
        };
        // An https issuer may not send the code and secret to http, even on loopback.
        for token in ["http://127.0.0.1/t", "http://localhost:9/t"] {
            assert!(
                Discovery::from_document(
                    "https://idp.example.com",
                    &doc("https://idp.example.com", token)
                )
                .is_err(),
                "{token}"
            );
        }
        // A loopback http issuer may not point at https endpoints.
        assert!(Discovery::from_document(
            "http://127.0.0.1:9",
            &doc("http://127.0.0.1:9", "https://idp.example.com/t")
        )
        .is_err());
        assert!(Discovery::from_document(
            "http://127.0.0.1:9",
            &doc("http://127.0.0.1:9", "http://127.0.0.1:9/t")
        )
        .is_ok());
        // An unusable userinfo endpoint of the wrong kind is ignored.
        let mut d = doc("https://idp.example.com", "https://idp.example.com/t");
        d["userinfo_endpoint"] = "http://127.0.0.1/u".into();
        assert!(Discovery::from_document("https://idp.example.com", &d)
            .unwrap()
            .userinfo_endpoint
            .is_none());
    }

    #[tokio::test]
    async fn a_failing_key_set_is_not_asked_again_within_the_minute() {
        // Cold, failing: one request for any number of lookups.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let jwks = jwks_for(&server);
        for _ in 0..5 {
            assert!(jwks.find(Some("k1")).await.is_err());
        }
        assert_eq!(server.received_requests().await.unwrap().len(), 1);

        // Warm, then failing on the refetch for an unknown kid: one more request only.
        let server = MockServer::start().await;
        let good = Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"keys": [test_keys().rsa_jwk("k1")]})),
            )
            .up_to_n_times(1)
            .mount_as_scoped(&server)
            .await;
        let mut jwks = jwks_for(&server);
        jwks.find(Some("k1")).await.unwrap();
        drop(good);
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        jwks.min_refetch = Duration::ZERO;
        assert!(jwks.find(Some("nope")).await.is_err());
        jwks.min_refetch = MIN_REFETCH;
        for _ in 0..5 {
            assert!(jwks.find(Some("nope")).await.is_err());
        }
        assert_eq!(server.received_requests().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn a_redirect_a_big_body_and_an_error_are_refused() {
        let server = MockServer::start().await;
        Mock::given(path("/redirect"))
            .respond_with(
                ResponseTemplate::new(302).insert_header("location", "http://127.0.0.1:1/x"),
            )
            .mount(&server)
            .await;
        Mock::given(path("/big"))
            .respond_with(
                ResponseTemplate::new(200).set_body_bytes(vec![b' '; MAX_BODY_BYTES + 10]),
            )
            .mount(&server)
            .await;
        Mock::given(path("/err"))
            .respond_with(ResponseTemplate::new(500).set_body_string("secret internal text"))
            .mount(&server)
            .await;
        Mock::given(path("/text"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>"))
            .mount(&server)
            .await;
        for p in ["redirect", "big", "err", "text"] {
            let url = Url::parse(&format!("{}/{p}", server.uri())).unwrap();
            let err = get_json(&http(), &url, None).await.unwrap_err();
            assert!(!err.contains("secret internal"), "{p}: {err}");
        }
    }

    async fn jwks_server(keys: Vec<serde_json::Value>) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/jwks"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "keys": keys })),
            )
            .mount(&server)
            .await;
        server
    }

    fn jwks_for(server: &MockServer) -> Jwks {
        Jwks::new(
            http(),
            Url::parse(&format!("{}/jwks", server.uri())).unwrap(),
        )
    }

    #[tokio::test]
    async fn keys_are_found_by_kid_and_cached() {
        let keys = test_keys();
        let server = jwks_server(vec![keys.rsa_jwk("k1"), keys.ec_jwk("k2")]).await;
        let jwks = jwks_for(&server);
        assert!(jwks.find(Some("k1")).await.unwrap().fits(Algorithm::RS256));
        assert!(jwks.find(Some("k2")).await.unwrap().fits(Algorithm::ES256));
        assert!(!jwks.find(Some("k2")).await.unwrap().fits(Algorithm::ES384));
        assert!(!jwks.find(Some("k1")).await.unwrap().fits(Algorithm::ES256));
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
        // Two keys and no kid: ambiguous.
        assert!(jwks.find(None).await.is_err());
    }

    #[tokio::test]
    async fn the_only_key_serves_a_token_without_kid() {
        let server = jwks_server(vec![test_keys().rsa_jwk("k1")]).await;
        assert!(jwks_for(&server).find(None).await.is_ok());
    }

    #[tokio::test]
    async fn an_unknown_kid_refetches_once_per_minute() {
        let server = jwks_server(vec![test_keys().rsa_jwk("k1")]).await;
        let jwks = jwks_for(&server);
        jwks.find(Some("k1")).await.unwrap();
        for _ in 0..5 {
            assert!(matches!(
                jwks.find(Some("nope")).await,
                Err(ExternalError::Token(_))
            ));
        }
        // The first fetch was moments ago: no refetch for an unknown kid.
        assert_eq!(server.received_requests().await.unwrap().len(), 1);

        let mut jwks = jwks_for(&server);
        jwks.min_refetch = Duration::ZERO;
        jwks.find(Some("k1")).await.unwrap();
        assert!(jwks.find(Some("nope")).await.is_err());
        // With the minute over: the cold fetch, then one refetch for the
        // unknown kid (1 from the first jwks above, 2 from this one).
        assert_eq!(server.received_requests().await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn a_rotated_key_is_picked_up_by_a_refetch() {
        let keys = test_keys();
        let server = MockServer::start().await;
        let old = Mock::given(method("GET"))
            .and(path("/jwks"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"keys": [keys.rsa_jwk("old")]})),
            )
            .up_to_n_times(1)
            .mount_as_scoped(&server)
            .await;
        let mut jwks = jwks_for(&server);
        jwks.min_refetch = Duration::ZERO;
        jwks.find(Some("old")).await.unwrap();
        drop(old);
        Mock::given(method("GET"))
            .and(path("/jwks"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"keys": [keys.rsa_jwk("new")]})),
            )
            .mount(&server)
            .await;
        assert!(jwks.find(Some("new")).await.is_ok());
    }

    #[tokio::test]
    async fn concurrent_cold_lookups_fetch_once() {
        let server = jwks_server(vec![test_keys().rsa_jwk("k1")]).await;
        let jwks = Arc::new(jwks_for(&server));
        let tasks: Vec<_> = (0..8)
            .map(|_| {
                let j = jwks.clone();
                tokio::spawn(async move { j.find(Some("k1")).await.is_ok() })
            })
            .collect();
        for t in tasks {
            assert!(t.await.unwrap());
        }
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn unusable_keys_are_skipped_not_fatal() {
        let keys = test_keys();
        let server = jwks_server(vec![
            serde_json::json!({"kty": "oct", "kid": "h", "k": "c2VjcmV0"}),
            serde_json::json!({"kty": "OKP", "crv": "Ed25519", "kid": "ed", "x": "AAAA"}),
            serde_json::json!({"kty": "RSA", "kid": "tiny", "n": "AQAB", "e": "AQAB"}),
            serde_json::json!("junk"),
            keys.rsa_jwk("ok"),
        ])
        .await;
        let jwks = jwks_for(&server);
        assert!(jwks.find(Some("ok")).await.is_ok());
        for k in ["h", "ed", "tiny"] {
            assert!(jwks.find(Some(k)).await.is_err(), "{k}");
        }
    }

    #[tokio::test]
    async fn a_key_restricted_by_use_or_alg_does_not_fit_other_uses() {
        let keys = test_keys();
        let mut enc = keys.rsa_jwk("enc");
        enc["use"] = "enc".into();
        let mut other = keys.rsa_jwk("pinned");
        other["alg"] = "RS512".into();
        let server = jwks_server(vec![enc, other]).await;
        let jwks = jwks_for(&server);
        assert!(!jwks.find(Some("enc")).await.unwrap().fits(Algorithm::RS256));
        let pinned = jwks.find(Some("pinned")).await.unwrap();
        assert!(!pinned.fits(Algorithm::RS256));
        assert!(pinned.fits(Algorithm::RS512));
    }
}
