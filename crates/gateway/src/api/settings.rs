//! Settings of the gateway. Only an admin reads or changes them.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use super::{require, ApiError, ApiJson, Authed};
use crate::app::AppState;
use crate::identity::limiter;
use crate::identity::policy::Action;
use crate::store::{
    AuditEntry, OidcSettings, DEFAULT_OIDC_GROUPS_CLAIM, DEFAULT_OIDC_LABEL, SESSION_HOURS_RANGE,
};

/// The fewest and the most days request logs may be kept.
const RETENTION_DAYS: std::ops::RangeInclusive<i64> = 1..=3650;

/// The limits of failed sign-ins, as they are built in.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct LoginLimits {
    /// Failures older than this no longer count.
    pub window_minutes: u64,
    /// Failures one email may have inside the window.
    pub max_per_email: u64,
    /// Failures one client address may have inside the window.
    pub max_per_address: u64,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SettingsView {
    /// How many days request logs are kept before they are deleted.
    pub log_retention_days: i64,
    /// How many hours a session lives, from sign-in. Sessions that exist
    /// keep the lifetime they were made with.
    pub session_hours: i64,
    /// The networks (CIDR) whose forwarding headers are believed, as the
    /// gateway was started. Read only: it is a flag of `ultrafast serve`.
    pub trusted_proxies: Vec<String>,
    /// Read only: built in.
    pub login_limits: LoginLimits,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateSettingsRequest {
    /// 1 to 3650.
    #[serde(default)]
    log_retention_days: Option<i64>,
    /// 1 to 720. Applies to sessions made from now on.
    #[serde(default)]
    session_hours: Option<i64>,
}

async fn view_of(state: &AppState) -> Result<SettingsView, ApiError> {
    Ok(SettingsView {
        log_retention_days: state.store.log_retention_days().await?,
        session_hours: state.store.session_hours().await?,
        trusted_proxies: state
            .trusted_proxies
            .iter()
            .map(ToString::to_string)
            .collect(),
        login_limits: LoginLimits {
            window_minutes: limiter::WINDOW.as_secs() / 60,
            max_per_email: limiter::MAX_PER_EMAIL as u64,
            max_per_address: limiter::MAX_PER_ADDRESS as u64,
        },
    })
}

#[utoipa::path(
    get,
    path = "/settings",
    tag = "settings",
    operation_id = "settings_view",
    responses(
        (status = 200, description = "The settings.", body = SettingsView),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn view(
    State(state): State<Arc<AppState>>,
    authed: Authed,
) -> Result<Response, ApiError> {
    require(&authed.principal, &Action::ManageSettings)?;
    Ok(Json(view_of(&state).await?).into_response())
}

#[utoipa::path(
    patch,
    path = "/settings",
    tag = "settings",
    operation_id = "settings_update",
    request_body = UpdateSettingsRequest,
    responses(
        (status = 200, description = "The settings after the change.", body = SettingsView),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 413, description = "The request body is too large.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "A value is out of range; `fields` names each of them.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn update(
    State(state): State<Arc<AppState>>,
    authed: Authed,
    ApiJson(req): ApiJson<UpdateSettingsRequest>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    require(me, &Action::ManageSettings)?;
    if req.log_retention_days.is_none() && req.session_hours.is_none() {
        return Err(ApiError::bad_request(
            "Send at least one of log_retention_days and session_hours.",
        ));
    }
    // Every field is checked before any is written.
    let mut fields = BTreeMap::new();
    if req
        .log_retention_days
        .is_some_and(|days| !RETENTION_DAYS.contains(&days))
    {
        fields.insert(
            "log_retention_days".to_string(),
            "must be from 1 to 3650".to_string(),
        );
    }
    if req
        .session_hours
        .is_some_and(|hours| !SESSION_HOURS_RANGE.contains(&hours))
    {
        fields.insert(
            "session_hours".to_string(),
            "must be from 1 to 720".to_string(),
        );
    }
    if !fields.is_empty() {
        return Err(ApiError::validation(fields));
    }
    let mut tx = state.store.begin().await?;
    if let Some(days) = req.log_retention_days {
        tx.set_log_retention_days(days).await?;
        tx.audit(AuditEntry {
            actor_user_id: Some(me.user_id),
            actor_email: &me.email,
            action: "settings.update",
            target_type: "settings",
            target_id: None,
            summary: &format!("Set log retention to {days} days"),
        })
        .await?;
    }
    if let Some(hours) = req.session_hours {
        tx.set_session_hours(hours).await?;
        tx.audit(AuditEntry {
            actor_user_id: Some(me.user_id),
            actor_email: &me.email,
            action: "settings.update",
            target_type: "settings",
            target_id: None,
            summary: &format!("Set session lifetime to {hours} hours"),
        })
        .await?;
    }
    tx.commit().await?;
    Ok(Json(view_of(&state).await?).into_response())
}

// ---- Single sign-on (OpenID Connect) ----

const MAX_LABEL_CHARS: usize = 40;
const MAX_URL_BYTES: usize = 2048;
const MAX_ID_BYTES: usize = 512;
const MAX_SECRET_BYTES: usize = 4096;
const MAX_SCOPES_BYTES: usize = 1000;
const MAX_CLAIM_BYTES: usize = 100;
const MAX_GROUP_BYTES: usize = 200;
const MAX_DOMAINS: usize = 100;
/// The most bytes read from the issuer for one document.
const MAX_DOCUMENT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct OidcView {
    /// Whether the sign-in button is offered.
    pub enabled: bool,
    /// The name on the button: "Sign in with <label>".
    pub label: String,
    /// The issuer URL of the identity provider; empty until set.
    pub issuer: String,
    pub client_id: String,
    /// Whether a client secret is stored. The secret itself is never
    /// returned.
    pub client_secret_set: bool,
    /// True when a secret is stored but cannot be decrypted (the master key
    /// changed). Single sign-on stays off until a new secret is saved.
    pub client_secret_unreadable: bool,
    /// Scopes asked for besides `openid email profile`, space separated.
    pub scopes: String,
    /// The ID token claim that lists the user's groups.
    pub groups_claim: String,
    /// Members of this group become admins on sign-in; empty: roles are
    /// never changed by sign-in.
    pub admin_group: String,
    /// Whether a person who signs in is matched to an existing user by a
    /// verified email address.
    pub link_by_email: bool,
    /// Whether a person from an allowed domain gets a Member account on
    /// first sign-in.
    pub auto_create: bool,
    /// Lower case domains, like `example.com`.
    pub allowed_domains: Vec<String>,
    /// The address to register at the identity provider; null without a
    /// public URL.
    pub redirect_uri: Option<String>,
    /// Whether the gateway was started with `UF_PUBLIC_URL`. Single
    /// sign-on cannot be turned on without it.
    pub public_url_set: bool,
}

fn oidc_view_of(state: &AppState, s: &OidcSettings) -> OidcView {
    OidcView {
        enabled: s.enabled,
        label: s.label.clone(),
        issuer: s.issuer.clone(),
        client_id: s.client_id.clone(),
        client_secret_set: s.client_secret_enc.is_some(),
        client_secret_unreadable: s.client_secret_enc.is_some()
            && state.oidc_client_secret(s).is_none(),
        scopes: s.scopes.clone(),
        groups_claim: s.groups_claim.clone(),
        admin_group: s.admin_group.clone(),
        link_by_email: s.link_by_email,
        auto_create: s.auto_create,
        allowed_domains: s.allowed_domains.clone(),
        redirect_uri: state.oidc_redirect_uri(),
        public_url_set: state.public_url.is_some(),
    }
}

fn default_label() -> String {
    DEFAULT_OIDC_LABEL.to_string()
}

fn default_groups_claim() -> String {
    DEFAULT_OIDC_GROUPS_CLAIM.to_string()
}

fn default_true() -> bool {
    true
}

/// Every setting. A field left out takes its default, except
/// `client_secret`: left out, the stored secret is kept.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct OidcUpdateRequest {
    /// Needs `UF_PUBLIC_URL`, an issuer, a client id and a client secret.
    #[serde(default)]
    enabled: bool,
    /// 1 to 40 characters. Default "SSO".
    #[serde(default = "default_label")]
    label: String,
    /// An `https` URL (`http` only for localhost, 127.0.0.1 and [::1]),
    /// without credentials, query or fragment. May be empty while single
    /// sign-on is off.
    #[serde(default)]
    issuer: String,
    /// At most 512 bytes. May be empty while single sign-on is off.
    #[serde(default)]
    client_id: String,
    /// Write only: replaces the stored secret. Left out: the stored secret
    /// stays. At most 4096 bytes.
    #[serde(default)]
    #[schema(nullable = false)]
    client_secret: Option<String>,
    /// Extra scopes, space separated.
    #[serde(default)]
    scopes: String,
    /// Default "groups".
    #[serde(default = "default_groups_claim")]
    groups_claim: String,
    #[serde(default)]
    admin_group: String,
    /// Default true.
    #[serde(default = "default_true")]
    link_by_email: bool,
    /// Needs at least one allowed domain. Default false.
    #[serde(default)]
    auto_create: bool,
    /// At most 100 domains; stored in lower case.
    #[serde(default)]
    allowed_domains: Vec<String>,
}

/// Whether `url` is the issuer or an endpoint of one this gateway may call:
/// `https`, or `http` for the machine's own loopback addresses only (a test
/// provider). No credentials; endpoints may have a query, issuers not.
fn check_provider_url(value: &str, issuer: bool) -> Result<reqwest::Url, &'static str> {
    if value.len() > MAX_URL_BYTES {
        return Err("must be at most 2048 bytes");
    }
    let url = reqwest::Url::parse(value).map_err(|_| "must be a valid URL")?;
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if url.host_str().is_none() {
        return Err("must be an https:// URL with a host");
    }
    match url.scheme() {
        "https" => {}
        "http" if loopback => {}
        "http" => return Err("must be https (http is allowed only for localhost)"),
        _ => return Err("must be an https:// URL"),
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("must not hold a user name or password");
    }
    if url.fragment().is_some() || (issuer && url.query().is_some()) {
        return Err("must not hold a query or a fragment");
    }
    Ok(url)
}

/// A domain like `example.com`, lower case.
fn clean_domain(value: &str) -> Option<String> {
    let d = value.trim().to_ascii_lowercase();
    let ok = !d.is_empty()
        && d.len() <= 253
        && d.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        });
    ok.then_some(d)
}

/// Checks a request and builds the settings to store. `has_secret`: a
/// secret is stored already. Every problem is named before anything is
/// written.
fn checked_oidc(
    req: &OidcUpdateRequest,
    has_secret: bool,
    public_url_set: bool,
    cipher: &crate::secrets::Cipher,
) -> Result<OidcSettings, ApiError> {
    let mut fields = BTreeMap::new();
    let mut bad = |field: &str, message: &str| {
        fields.insert(field.to_string(), message.to_string());
    };
    let label = req.label.trim().to_string();
    if label.is_empty()
        || label.chars().count() > MAX_LABEL_CHARS
        || label.chars().any(char::is_control)
    {
        bad("label", "must be from 1 to 40 characters");
    }
    let issuer = req.issuer.trim().to_string();
    if !issuer.is_empty() {
        if let Err(m) = check_provider_url(&issuer, true) {
            bad("issuer", m);
        }
    } else if req.enabled {
        bad("issuer", "is required to turn single sign-on on");
    }
    let client_id = req.client_id.trim().to_string();
    if client_id.len() > MAX_ID_BYTES || client_id.chars().any(char::is_control) {
        bad(
            "client_id",
            "must be at most 512 characters, without control characters",
        );
    } else if client_id.is_empty() && req.enabled {
        bad("client_id", "is required to turn single sign-on on");
    }
    let mut new_secret = None;
    match req.client_secret.as_deref() {
        Some(secret) if secret.is_empty() || secret.len() > MAX_SECRET_BYTES => {
            bad(
                "client_secret",
                "must be from 1 to 4096 bytes; leave it out to keep the stored one",
            );
        }
        Some(secret) => new_secret = Some(secret),
        None if req.enabled && !has_secret => {
            bad("client_secret", "is required to turn single sign-on on");
        }
        None => {}
    }
    if req.scopes.len() > MAX_SCOPES_BYTES || req.scopes.chars().any(char::is_control) {
        bad("scopes", "must be at most 1000 characters, on one line");
    }
    let scopes = req.scopes.split_whitespace().collect::<Vec<_>>().join(" ");
    let groups_claim = req.groups_claim.trim().to_string();
    if groups_claim.is_empty()
        || groups_claim.len() > MAX_CLAIM_BYTES
        || groups_claim.chars().any(char::is_control)
    {
        bad("groups_claim", "must be from 1 to 100 characters");
    }
    let admin_group = req.admin_group.trim().to_string();
    if admin_group.len() > MAX_GROUP_BYTES || admin_group.chars().any(char::is_control) {
        bad("admin_group", "must be at most 200 characters");
    }
    let mut allowed_domains: Vec<String> = Vec::new();
    let mut domains_bad = false;
    if req.allowed_domains.len() > MAX_DOMAINS {
        domains_bad = true;
        bad("allowed_domains", "at most 100 domains");
    }
    for d in req.allowed_domains.iter().take(MAX_DOMAINS) {
        match clean_domain(d) {
            Some(d) if allowed_domains.contains(&d) => {}
            Some(d) => allowed_domains.push(d),
            None => {
                domains_bad = true;
                bad("allowed_domains", "each must be a domain like example.com");
                break;
            }
        }
    }
    if req.auto_create && allowed_domains.is_empty() && !domains_bad {
        bad(
            "allowed_domains",
            "at least one domain is required to create users on first sign-in",
        );
    }
    if req.enabled && !public_url_set {
        bad(
            "enabled",
            "needs the gateway to be started with UF_PUBLIC_URL",
        );
    }
    if !fields.is_empty() {
        return Err(ApiError::validation(fields));
    }
    Ok(OidcSettings {
        enabled: req.enabled,
        label,
        issuer,
        client_id,
        client_secret_enc: new_secret.map(|s| hex::encode(cipher.encrypt(s.as_bytes()))),
        scopes,
        groups_claim,
        admin_group,
        link_by_email: req.link_by_email,
        auto_create: req.auto_create,
        allowed_domains,
    })
}

#[utoipa::path(
    get,
    path = "/settings/oidc",
    tag = "settings",
    operation_id = "settings_oidc_view",
    responses(
        (status = 200, description = "The single sign-on settings, without the secret.", body = OidcView),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn oidc_view(
    State(state): State<Arc<AppState>>,
    authed: Authed,
) -> Result<Response, ApiError> {
    require(&authed.principal, &Action::ManageSettings)?;
    let settings = state.store.oidc_settings().await?;
    Ok(Json(oidc_view_of(&state, &settings)).into_response())
}

#[utoipa::path(
    put,
    path = "/settings/oidc",
    tag = "settings",
    operation_id = "settings_oidc_update",
    request_body = OidcUpdateRequest,
    responses(
        (status = 200, description = "The settings after the change.", body = OidcView),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 413, description = "The request body is too large.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "A value is not valid; `fields` names each of them.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn oidc_update(
    State(state): State<Arc<AppState>>,
    authed: Authed,
    ApiJson(req): ApiJson<OidcUpdateRequest>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    require(me, &Action::ManageSettings)?;
    let before = state.store.oidc_settings().await?;
    let next = checked_oidc(
        &req,
        // A stored secret that cannot be read does not count: enabling
        // would not start a provider.
        state.oidc_client_secret(&before).is_some(),
        state.public_url.is_some(),
        &state.cipher,
    )?;
    let summary = format!(
        "Updated single sign-on settings; sign-in with {} is {}{}",
        next.label,
        if next.enabled { "on" } else { "off" },
        if next.client_secret_enc.is_some() {
            ", client secret replaced"
        } else {
            ""
        },
    );
    let mut tx = state.store.begin().await?;
    tx.set_oidc_settings(&next).await?;
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "settings.oidc_update",
        target_type: "settings",
        target_id: None,
        summary: &summary,
    })
    .await?;
    tx.commit().await?;
    state.reload_sign_in().await?;
    let settings = state.store.oidc_settings().await?;
    Ok(Json(oidc_view_of(&state, &settings)).into_response())
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct OidcTestRequest {
    /// The issuer to test. Left out or empty: the saved one.
    #[serde(default)]
    #[schema(nullable = false)]
    issuer: Option<String>,
}

#[derive(Debug, Default, Serialize, utoipa::ToSchema)]
pub struct OidcTestResult {
    /// Whether the discovery document and the key set could be used.
    pub ok: bool,
    /// The issuer the provider named, when its discovery document was read.
    pub issuer: Option<String>,
    pub authorization_endpoint: Option<String>,
    pub token_endpoint: Option<String>,
    /// How many keys the provider publishes to verify ID tokens.
    pub jwks_keys: Option<u32>,
    /// What is wrong, when `ok` is false. It never repeats a URL.
    pub error: Option<String>,
}

/// Reads one JSON document from the provider: no redirects, a bounded size.
async fn fetch_document(
    http: &reqwest::Client,
    url: reqwest::Url,
) -> Result<serde_json::Value, String> {
    let mut resp = http
        .get(url)
        .header("accept", "application/json")
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                "The provider did not answer in time.".to_string()
            } else {
                "Could not reach the provider.".to_string()
            }
        })?;
    let status = resp.status();
    if status.is_redirection() {
        return Err("The provider answered with a redirect, which is not followed.".to_string());
    }
    if !status.is_success() {
        return Err(format!("The provider answered HTTP {}.", status.as_u16()));
    }
    if resp
        .content_length()
        .is_some_and(|n| n > MAX_DOCUMENT_BYTES as u64)
    {
        return Err("The provider's answer is too large.".to_string());
    }
    let mut body = Vec::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|_| "The provider's answer could not be read.".to_string())?
    {
        if body.len() + chunk.len() > MAX_DOCUMENT_BYTES {
            return Err("The provider's answer is too large.".to_string());
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(|_| "The provider's answer is not JSON.".to_string())
}

/// Reads the discovery document and the key set of `issuer`.
async fn probe_issuer(http: &reqwest::Client, issuer: &str) -> OidcTestResult {
    let mut result = OidcTestResult::default();
    let fail = |mut r: OidcTestResult, m: &str| {
        r.error = Some(m.to_string());
        r
    };
    let well_known = format!(
        "{}/.well-known/openid-configuration",
        issuer.trim_end_matches('/')
    );
    let Ok(well_known) = reqwest::Url::parse(&well_known) else {
        return fail(result, "The issuer is not a valid URL.");
    };
    let doc = match fetch_document(http, well_known).await {
        Ok(doc) => doc,
        Err(m) => return fail(result, &m),
    };
    let text = |name: &str| doc.get(name).and_then(|v| v.as_str()).map(str::to_string);
    // What the provider calls itself is shown only once it is the issuer
    // that was entered: a document that names another is not repeated.
    if text("issuer").as_deref() != Some(issuer) {
        return fail(
            result,
            "The issuer in the discovery document is not the one you entered; they must match exactly.",
        );
    }
    result.issuer = text("issuer");
    result.authorization_endpoint = text("authorization_endpoint");
    result.token_endpoint = text("token_endpoint");
    let jwks_uri = text("jwks_uri");
    let endpoints = [
        (
            "authorization_endpoint",
            result.authorization_endpoint.clone(),
        ),
        ("token_endpoint", result.token_endpoint.clone()),
        ("jwks_uri", jwks_uri.clone()),
    ];
    for (name, value) in endpoints {
        match value.as_deref() {
            None => return fail(result, &format!("The discovery document has no {name}.")),
            Some(v) if check_provider_url(v, false).is_err() => {
                return fail(
                    result,
                    &format!("The {name} in the discovery document is not usable."),
                );
            }
            Some(_) => {}
        }
    }
    let Ok(jwks_uri) = reqwest::Url::parse(&jwks_uri.unwrap_or_default()) else {
        return fail(
            result,
            "The jwks_uri in the discovery document is not usable.",
        );
    };
    let keys = match fetch_document(http, jwks_uri).await {
        Ok(doc) => doc.get("keys").and_then(|k| k.as_array()).map(Vec::len),
        Err(m) => return fail(result, &format!("Key set: {m}")),
    };
    let Some(keys) = keys.filter(|n| *n > 0) else {
        return fail(result, "The provider publishes no signing keys.");
    };
    result.jwks_keys = Some(u32::try_from(keys).unwrap_or(u32::MAX));
    result.ok = true;
    result
}

#[utoipa::path(
    post,
    path = "/settings/oidc/test",
    tag = "settings",
    operation_id = "settings_oidc_test",
    request_body = OidcTestRequest,
    responses(
        (status = 200, description = "What the provider answered. A provider that cannot be used is `ok: false` with an `error`, not an HTTP error.", body = OidcTestResult),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 413, description = "The request body is too large.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "There is no issuer to test, or it is not valid; `fields` names it.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn oidc_test(
    State(state): State<Arc<AppState>>,
    authed: Authed,
    ApiJson(req): ApiJson<OidcTestRequest>,
) -> Result<Response, ApiError> {
    require(&authed.principal, &Action::ManageSettings)?;
    let issuer = match req
        .issuer
        .as_deref()
        .map(str::trim)
        .filter(|i| !i.is_empty())
    {
        Some(issuer) => issuer.to_string(),
        None => state.store.oidc_settings().await?.issuer,
    };
    if issuer.is_empty() {
        return Err(ApiError::invalid_field(
            "issuer",
            "enter an issuer, or save one first",
        ));
    }
    if let Err(message) = check_provider_url(&issuer, true) {
        return Err(ApiError::invalid_field("issuer", message));
    }
    Ok(Json(probe_issuer(&state.http, &issuer).await).into_response())
}
