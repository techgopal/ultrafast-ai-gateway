//! The `/api` admin API: its router, its error type and the extractor that
//! authenticates every request.

pub mod audit;
pub mod auth;
pub mod budgets;
pub mod health;
pub mod keys;
pub mod limits;
pub mod logs;
pub mod models;
pub mod openapi;
pub mod providers;
pub mod routes;
pub mod settings;
pub mod teams;
pub mod tokens;
pub mod usage;
pub mod users;

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{ConnectInfo, DefaultBodyLimit, FromRequest, FromRequestParts, Request};
use axum::http::header::{AUTHORIZATION, COOKIE};
use axum::http::request::Parts;
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use ipnet::IpNet;
use serde::de::DeserializeOwned;
use serde_json::json;
use utoipa_axum::router::OpenApiRouter;
use utoipa_axum::routes;

use crate::app::AppState;
use crate::identity::policy::{authorize, Action, Decision};
use crate::identity::{Principal, UserStatus};
use crate::secrets::secrets_equal;
use crate::store::{after, check_timestamp, now, Store, UserRow};

/// The name of the session cookie.
pub const SESSION_COOKIE: &str = "uf_session";
/// The header that carries the CSRF token of a session.
pub const CSRF_HEADER: &str = "x-csrf-token";
/// The largest request body `/api` reads.
pub const MAX_BODY_BYTES: usize = 64 * 1024;
/// A user or token that was active this recently is not written again.
const TOUCH_INTERVAL_SECONDS: i64 = 60;
/// Longest accepted name of a user, a key or an access token, in characters.
pub const MAX_NAME_CHARS: usize = 100;

/// Every route of `/api` with its description. Paths are relative to
/// `/api`. The router and the OpenAPI spec are both made from this, so a
/// route registered here through `routes!` is in both.
///
/// Nothing enforces that for a route added any other way: one added with
/// `OpenApiRouter::route`, on the axum router after `split_for_parts`, or
/// at another registration site is served but is not in the spec, and so
/// escapes the role table test. Add routes only here, only with `routes!`.
pub(crate) fn documented() -> OpenApiRouter<Arc<AppState>> {
    OpenApiRouter::new()
        .routes(routes!(auth::setup_status, auth::setup))
        .routes(routes!(auth::login))
        .routes(routes!(auth::logout))
        .routes(routes!(auth::me))
        .routes(routes!(auth::accept_invite))
        .routes(routes!(auth::change_password))
        .routes(routes!(users::list, users::invite))
        .routes(routes!(users::view, users::update, users::delete))
        .routes(routes!(users::reinvite))
        .routes(routes!(teams::list, teams::create))
        .routes(routes!(teams::view, teams::rename, teams::delete))
        .routes(routes!(teams::add_member))
        .routes(routes!(teams::put_member, teams::remove_member))
        .routes(routes!(keys::list, keys::create))
        .routes(routes!(keys::view, keys::revoke))
        .routes(routes!(providers::list, providers::create))
        .routes(routes!(providers::update, providers::delete))
        .routes(routes!(models::sync))
        .routes(routes!(models::list, models::create))
        .routes(routes!(models::update, models::delete))
        .routes(routes!(models::put_grants))
        .routes(routes!(routes::list, routes::create))
        .routes(routes!(routes::view, routes::update, routes::delete))
        .routes(routes!(tokens::list, tokens::create))
        .routes(routes!(tokens::revoke))
        .routes(routes!(audit::list))
        .routes(routes!(health::routing_health))
        .routes(routes!(settings::view, settings::update))
        .routes(routes!(limits::list, limits::set))
        .routes(routes!(limits::delete))
        .routes(routes!(budgets::list, budgets::set))
        .routes(routes!(budgets::delete))
        .routes(routes!(logs::list))
        .routes(routes!(logs::view))
        .routes(routes!(usage::usage_view))
}

pub fn router() -> Router<Arc<AppState>> {
    let (router, _) = documented().split_for_parts();
    router
        .fallback(|| async { ApiError::not_found() })
        .method_not_allowed_fallback(|| async { ApiError::method_not_allowed() })
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
}

/// An error answer of `/api`. `message` and `fields` are sent to the caller,
/// so they must never hold a secret.
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
    pub fields: Option<BTreeMap<String, String>>,
}

impl ApiError {
    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            fields: None,
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "bad_request", message)
    }

    /// One entry per field that failed.
    pub fn validation(fields: BTreeMap<String, String>) -> Self {
        Self {
            fields: Some(fields),
            ..Self::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation_failed",
                "Some fields are not valid.",
            )
        }
    }

    /// A validation error for a single field.
    pub fn invalid_field(field: &str, message: &str) -> Self {
        Self::validation(BTreeMap::from([(field.to_string(), message.to_string())]))
    }

    pub fn unauthenticated() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "unauthenticated",
            "Sign in to continue.",
        )
    }

    /// The same answer whatever was wrong with the email or the password.
    pub fn invalid_credentials() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "invalid_credentials",
            "Email or password is incorrect.",
        )
    }

    pub fn csrf() -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "csrf_failed",
            "The CSRF token is missing or does not match.",
        )
    }

    pub fn forbidden() -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "forbidden",
            "You are not allowed to do this.",
        )
    }

    pub fn not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", "Not found.")
    }

    /// A 404 with a code of its own, for a thing the caller named that is
    /// not there.
    pub fn not_found_with(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, code, message)
    }

    pub fn method_not_allowed() -> Self {
        Self::new(
            StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
            "This method is not supported here.",
        )
    }

    pub fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, code, message)
    }

    pub fn payload_too_large() -> Self {
        Self::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "payload_too_large",
            "The request body is too large.",
        )
    }

    pub fn too_many_attempts() -> Self {
        Self::new(
            StatusCode::TOO_MANY_REQUESTS,
            "too_many_attempts",
            "Too many failed attempts. Try again later.",
        )
    }

    pub fn internal() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "Something went wrong.",
        )
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut error = json!({ "code": self.code, "message": self.message });
        if let Some(fields) = self.fields {
            error["fields"] = json!(fields);
        }
        (self.status, Json(json!({ "error": error }))).into_response()
    }
}

/// The error is logged and the caller is told nothing about it.
impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        tracing::error!(error = %e, "api request failed");
        Self::internal()
    }
}

/// Makes a committed change visible to `/v1`. Call it after `commit`, never
/// while a `Tx` is open. When it fails the change stays committed and the
/// next refresh picks it up.
///
/// The refresh runs as a task of its own, so it finishes even when the
/// caller disconnects and this future is dropped.
pub async fn refresh_snapshot(state: &Arc<AppState>) -> Result<(), ApiError> {
    let state = state.clone();
    let refreshed = tokio::spawn(async move {
        let result = state.refresh().await;
        // Logged here, so a failure is recorded with nobody waiting.
        if let Err(e) = &result {
            tracing::error!(error = %e, "snapshot refresh failed after a committed change");
        }
        result.is_ok()
    });
    match refreshed.await {
        Ok(true) => Ok(()),
        Ok(false) => Err(ApiError::internal()),
        Err(e) => {
            tracing::error!(error = %e, "snapshot refresh task failed");
            Err(ApiError::internal())
        }
    }
}

/// The id of a path segment. Anything but a positive integer written in
/// plain digits is answered like a row that does not exist.
pub fn path_id(raw: &str) -> Result<i64, ApiError> {
    if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) {
        return Err(ApiError::not_found());
    }
    raw.parse::<i64>()
        .ok()
        .filter(|id| *id > 0)
        .ok_or_else(ApiError::not_found)
}

/// The name of a user, a key or an access token, trimmed: 1 to
/// `MAX_NAME_CHARS` characters, none of them a control character.
pub fn trimmed_name(raw: &str) -> Result<&str, &'static str> {
    let name = raw.trim();
    let chars = name.chars().count();
    if chars == 0 || chars > MAX_NAME_CHARS || name.chars().any(char::is_control) {
        return Err("name must be 1 to 100 characters");
    }
    Ok(name)
}

/// When a key or an access token stops working: a real time in the future.
pub(crate) fn future_timestamp(raw: &str) -> Result<&str, &'static str> {
    if check_timestamp(raw).is_err() {
        return Err("expires_at must be a UTC time in the form YYYY-MM-DD HH:MM:SS");
    }
    if raw <= now().as_str() {
        return Err("expires_at must be in the future");
    }
    Ok(raw)
}

/// The name and the expiry of a new key or access token, or every field
/// that is not valid.
pub(crate) fn name_and_expiry<'a>(
    name: &'a str,
    expires_at: Option<&'a str>,
) -> Result<(&'a str, Option<&'a str>), ApiError> {
    let mut fields = BTreeMap::new();
    let name = trimmed_name(name)
        .map_err(|m| fields.insert("name".to_string(), m.to_string()))
        .ok();
    let expires_at = match expires_at.map(future_timestamp) {
        Some(Err(m)) => {
            fields.insert("expires_at".to_string(), m.to_string());
            None
        }
        Some(Ok(at)) => Some(at),
        None => None,
    };
    match name {
        Some(name) if fields.is_empty() => Ok((name, expires_at)),
        _ => Err(ApiError::validation(fields)),
    }
}

/// Turns a policy decision into a result.
pub fn require(p: &Principal, action: &Action) -> Result<(), ApiError> {
    match authorize(p, action) {
        Decision::Allow => Ok(()),
        Decision::Forbidden => Err(ApiError::forbidden()),
        Decision::Hidden => Err(ApiError::not_found()),
    }
}

/// A JSON request body. Unlike `axum::Json` it answers in the `/api` error
/// shape, and never repeats any part of what was sent.
pub struct ApiJson<T>(pub T);

impl<S, T> FromRequest<S> for ApiJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(value)) => Ok(Self(value)),
            Err(rejection) => Err(json_error(&rejection)),
        }
    }
}

fn json_error(rejection: &JsonRejection) -> ApiError {
    if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
        return ApiError::payload_too_large();
    }
    // The parser's own message can quote the body, so it is not passed on.
    ApiError::bad_request(
        "The body must be JSON, sent as application/json, with exactly the expected fields.",
    )
}

/// The address of the client. It is the TCP peer, unless the peer is inside
/// one of `AppState::trusted_proxies`: then it is the `CF-Connecting-IP`
/// header if that holds an address, else the last address of
/// `X-Forwarded-For` that is not itself trusted. The headers of any other
/// peer are ignored. Without connection information, as in tests that call
/// the router directly, the peer is 127.0.0.1.
pub struct ClientAddr(pub IpAddr);

/// The client address for a request from `peer` with these headers.
pub fn client_addr(peer: IpAddr, headers: &HeaderMap, trusted: &[IpNet]) -> IpAddr {
    // A dual-stack listener shows an IPv4 peer as ::ffff:a.b.c.d, which an
    // IPv4 network does not contain; compare and return the plain form.
    let peer = peer.to_canonical();
    let is_trusted = |addr: &IpAddr| trusted.iter().any(|net| net.contains(addr));
    if !is_trusted(&peer) {
        return peer;
    }
    let forwarded = |name: &str| {
        headers
            .get_all(name)
            .iter()
            .filter_map(|value| value.to_str().ok())
    };
    if let Some(addr) = forwarded("cf-connecting-ip")
        .next()
        .and_then(|value| value.trim().parse::<IpAddr>().ok())
        .map(|addr| addr.to_canonical())
    {
        return addr;
    }
    // Each proxy appends the address it saw, so the chain is read from the
    // end: what is left of the last trusted hop is the client. Anything
    // further left was written by the client and can be anything.
    let chain: Vec<&str> = forwarded("x-forwarded-for")
        .flat_map(|value| value.split(','))
        .collect();
    for entry in chain.into_iter().rev() {
        match entry.trim().parse::<IpAddr>().map(|a| a.to_canonical()) {
            Ok(addr) if is_trusted(&addr) => continue,
            Ok(addr) => return addr,
            // A hop that is not an address cannot name the client.
            Err(_) => break,
        }
    }
    peer
}

impl FromRequestParts<Arc<AppState>> for ClientAddr {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map_or(IpAddr::V4(Ipv4Addr::LOCALHOST), |info| info.0.ip());
        Ok(Self(client_addr(
            peer,
            &parts.headers,
            &state.trusted_proxies,
        )))
    }
}

/// How the caller proved who they are. No `Debug`: it holds a CSRF token.
pub enum AuthVia {
    Session { session_id: i64, csrf_token: String },
    Token { token_id: i64 },
}

/// The authenticated caller. Extracting it authenticates the request and,
/// for cookie sessions on non-GET/HEAD requests, checks the CSRF header.
pub struct Authed {
    pub principal: Principal,
    pub via: AuthVia,
}

/// The value of the session cookie, if the request has one.
fn session_cookie(headers: &HeaderMap) -> Option<&str> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == SESSION_COOKIE)
        .map(|(_, value)| value)
}

/// The token of an `Authorization: Bearer <token>` header value.
fn bearer_token(value: &axum::http::HeaderValue) -> Option<&str> {
    let (scheme, token) = value.to_str().ok()?.split_once(' ')?;
    // HTTP auth schemes are case-insensitive.
    scheme
        .eq_ignore_ascii_case("bearer")
        .then_some(token.trim())
}

/// Whether a stored timestamp is within the last minute.
fn is_recent(at: Option<&str>) -> bool {
    let cutoff = after(-TOUCH_INTERVAL_SECONDS);
    at.is_some_and(|at| at > cutoff.as_str())
}

/// Loads the user as they are now. `None` unless the user exists and is
/// active.
async fn active_user(store: &Store, user_id: i64) -> anyhow::Result<Option<UserRow>> {
    let user = store.user_by_id(user_id).await?;
    Ok(user.filter(|u| u.status == UserStatus::Active))
}

async fn principal_of(store: &Store, user: &UserRow) -> anyhow::Result<Principal> {
    let teams = store.memberships_of(user.id).await?;
    Ok(Principal {
        user_id: user.id,
        email: user.email.clone(),
        role: user.role,
        teams: teams.into_iter().map(|m| (m.team_id, m.role)).collect(),
    })
}

impl FromRequestParts<Arc<AppState>> for Authed {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let store = &state.store;

        // An Authorization header decides alone: the cookie is not a fallback.
        if let Some(value) = parts.headers.get(AUTHORIZATION) {
            let token = bearer_token(value).ok_or_else(ApiError::unauthenticated)?;
            let row = store
                .live_token(token)
                .await?
                .ok_or_else(ApiError::unauthenticated)?;
            let user = active_user(store, row.user_id)
                .await?
                .ok_or_else(ApiError::unauthenticated)?;
            let principal = principal_of(store, &user).await?;
            if !is_recent(row.last_used_at.as_deref()) {
                store.touch_token(row.id).await?;
            }
            if !is_recent(user.last_active_at.as_deref()) {
                store.touch_user(user.id).await?;
            }
            return Ok(Self {
                principal,
                via: AuthVia::Token { token_id: row.id },
            });
        }

        let cookie = session_cookie(&parts.headers).ok_or_else(ApiError::unauthenticated)?;
        let session = store
            .live_session(cookie)
            .await?
            .ok_or_else(ApiError::unauthenticated)?;
        let Some(user) = active_user(store, session.user_id).await? else {
            store.delete_session(cookie).await?;
            return Err(ApiError::unauthenticated());
        };
        if parts.method != Method::GET && parts.method != Method::HEAD {
            let sent = parts
                .headers
                .get(CSRF_HEADER)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(ApiError::csrf)?;
            if !secrets_equal(sent, &session.csrf_token) {
                return Err(ApiError::csrf());
            }
        }
        let principal = principal_of(store, &user).await?;
        if !is_recent(user.last_active_at.as_deref()) {
            store.touch_user(user.id).await?;
        }
        Ok(Self {
            principal,
            via: AuthVia::Session {
                session_id: session.id,
                csrf_token: session.csrf_token,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::Role;

    async fn body_of(error: ApiError) -> (StatusCode, serde_json::Value) {
        let response = error.into_response();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn errors_have_one_shape() {
        let cases = [
            (ApiError::bad_request("no"), 400, "bad_request"),
            (ApiError::unauthenticated(), 401, "unauthenticated"),
            (ApiError::invalid_credentials(), 401, "invalid_credentials"),
            (ApiError::csrf(), 403, "csrf_failed"),
            (ApiError::forbidden(), 403, "forbidden"),
            (ApiError::not_found(), 404, "not_found"),
            (ApiError::method_not_allowed(), 405, "method_not_allowed"),
            (ApiError::conflict("taken", "no"), 409, "taken"),
            (ApiError::payload_too_large(), 413, "payload_too_large"),
            (ApiError::too_many_attempts(), 429, "too_many_attempts"),
            (ApiError::internal(), 500, "internal_error"),
        ];
        for (error, status, code) in cases {
            let (got, body) = body_of(error).await;
            assert_eq!(got.as_u16(), status);
            assert_eq!(body["error"]["code"], code);
            assert!(!body["error"]["message"].as_str().unwrap().is_empty());
            assert_eq!(body["error"].as_object().unwrap().len(), 2, "{code}");
            assert_eq!(body.as_object().unwrap().len(), 1);
        }
    }

    #[tokio::test]
    async fn validation_errors_list_fields() {
        let (status, body) = body_of(ApiError::invalid_field("name", "too long")).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            body,
            json!({ "error": {
                "code": "validation_failed",
                "message": "Some fields are not valid.",
                "fields": { "name": "too long" }
            }})
        );
    }

    #[tokio::test]
    async fn internal_errors_hide_the_cause() {
        let (status, body) = body_of(anyhow::anyhow!("disk is on fire").into()).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(!body.to_string().contains("fire"));
    }

    #[test]
    fn names_are_trimmed_and_bounded() {
        assert_eq!(trimmed_name("  ci "), Ok("ci"));
        assert_eq!(trimmed_name(&"é".repeat(100)), Ok("é".repeat(100).as_str()));
        for bad in ["", "   ", &"n".repeat(101), "a\nb"] {
            assert!(trimmed_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn expiry_is_a_real_time_in_the_future() {
        let soon = after(60);
        assert_eq!(future_timestamp(&soon), Ok(soon.as_str()));
        assert!(future_timestamp("2999-12-31 23:59:59").is_ok());
        for bad in [
            after(-60).as_str(),
            "2000-01-01 00:00:00",
            "2999-02-31 00:00:00",
            "2999-01-01",
            "2999-01-01T00:00:00Z",
            "",
        ] {
            assert!(future_timestamp(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn every_invalid_field_is_named() {
        let err = name_and_expiry("", Some("never")).unwrap_err();
        let fields = err.fields.unwrap();
        assert_eq!(fields.len(), 2);
        assert!(fields.contains_key("name") && fields.contains_key("expires_at"));
        assert!(name_and_expiry(" a ", None).is_ok());
    }

    #[test]
    fn require_follows_the_policy() {
        let member = Principal {
            user_id: 1,
            email: "maya@example.com".into(),
            role: Role::Member,
            teams: vec![],
        };
        assert!(require(&member, &Action::ListUsers).is_ok());
        let err = require(&member, &Action::CreateTeam).unwrap_err();
        assert_eq!((err.status, err.code), (StatusCode::FORBIDDEN, "forbidden"));
        let err = require(&member, &Action::DeleteUser { user_id: 2 }).unwrap_err();
        assert_eq!((err.status, err.code), (StatusCode::NOT_FOUND, "not_found"));
    }

    #[test]
    fn path_ids_are_positive_integers() {
        assert_eq!(path_id("1").unwrap(), 1);
        assert_eq!(path_id("9223372036854775807").unwrap(), i64::MAX);
        for bad in [
            "",
            "0",
            "-1",
            "+1",
            "1.5",
            "abc",
            " 1",
            "1 ",
            "9223372036854775808",
            "١",
        ] {
            let err = path_id(bad).unwrap_err();
            assert_eq!((err.status, err.code), (StatusCode::NOT_FOUND, "not_found"));
        }
    }

    #[test]
    fn session_cookie_is_found_among_others() {
        let mut headers = HeaderMap::new();
        assert_eq!(session_cookie(&headers), None);
        headers.append(COOKIE, "theme=dark; xuf_session=no".parse().unwrap());
        assert_eq!(session_cookie(&headers), None);
        headers.append(COOKIE, "a=b;  uf_session=abc123 ; c=d".parse().unwrap());
        assert_eq!(session_cookie(&headers), Some("abc123"));
    }

    #[test]
    fn bearer_scheme_is_case_insensitive() {
        let value = |s: &str| axum::http::HeaderValue::from_str(s).unwrap();
        assert_eq!(bearer_token(&value("Bearer uf-at-1")), Some("uf-at-1"));
        assert_eq!(bearer_token(&value("bEARER  uf-at-1 ")), Some("uf-at-1"));
        assert_eq!(bearer_token(&value("Basic uf-at-1")), None);
        assert_eq!(bearer_token(&value("Bearer")), None);
        assert_eq!(bearer_token(&value("")), None);
    }

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.append(*name, value.parse().unwrap());
        }
        map
    }

    #[test]
    fn client_address_follows_trusted_proxies_only() {
        let trusted: Vec<IpNet> = vec!["10.0.0.0/8".parse().unwrap(), "fd00::/8".parse().unwrap()];
        let ip = |s: &str| s.parse::<IpAddr>().unwrap();
        let via = |peer: &str, pairs: &[(&'static str, &str)]| {
            client_addr(ip(peer), &headers(pairs), &trusted)
        };

        // No trusted proxy configured: nothing is read.
        let cf = headers(&[("cf-connecting-ip", "203.0.113.9")]);
        assert_eq!(client_addr(ip("10.0.0.1"), &cf, &[]), ip("10.0.0.1"));

        // An untrusted peer's headers are ignored.
        let spoof = [
            ("cf-connecting-ip", "1.1.1.1"),
            ("x-forwarded-for", "2.2.2.2"),
        ];
        assert_eq!(via("198.51.100.7", &spoof), ip("198.51.100.7"));

        // A trusted peer: Cloudflare's header first.
        assert_eq!(via("10.0.0.1", &spoof), ip("1.1.1.1"));
        assert_eq!(
            via("fd00::1", &[("cf-connecting-ip", " 2001:db8::5 ")]),
            ip("2001:db8::5")
        );
        // Not an address: it falls through to the chain, then to the peer.
        let bad = [
            ("cf-connecting-ip", "nonsense"),
            ("x-forwarded-for", "2.2.2.2"),
        ];
        assert_eq!(via("10.0.0.1", &bad), ip("2.2.2.2"));
        assert_eq!(
            via("10.0.0.1", &[("cf-connecting-ip", "x")]),
            ip("10.0.0.1")
        );
        assert_eq!(via("10.0.0.1", &[]), ip("10.0.0.1"));

        // The chain: the last address that is not trusted, whatever the
        // client wrote before it, over one header or several.
        let chain = [("x-forwarded-for", "9.9.9.9, 198.51.100.1, 10.0.0.2")];
        assert_eq!(via("10.0.0.1", &chain), ip("198.51.100.1"));
        let split = [
            ("x-forwarded-for", "9.9.9.9, 198.51.100.1"),
            ("x-forwarded-for", "10.0.0.2"),
        ];
        assert_eq!(via("10.0.0.1", &split), ip("198.51.100.1"));
        let two = [("x-forwarded-for", "198.51.100.1, 203.0.113.3")];
        assert_eq!(via("10.0.0.1", &two), ip("203.0.113.3"));
        // Every hop trusted, or a hop that is not an address: the peer.
        assert_eq!(
            via("10.0.0.1", &[("x-forwarded-for", "10.0.0.3, 10.0.0.2")]),
            ip("10.0.0.1")
        );
        assert_eq!(
            via(
                "10.0.0.1",
                &[("x-forwarded-for", "198.51.100.1, junk, 10.0.0.2")]
            ),
            ip("10.0.0.1")
        );
    }

    #[test]
    fn client_address_sees_through_ipv4_mapped_addresses() {
        let trusted: Vec<IpNet> = vec!["10.0.0.0/8".parse().unwrap()];
        let ip = |s: &str| s.parse::<IpAddr>().unwrap();
        // A dual-stack listener reports an IPv4 peer as ::ffff:a.b.c.d.
        let chain = headers(&[(
            "x-forwarded-for",
            "9.9.9.9, ::ffff:198.51.100.1, ::ffff:10.0.0.2",
        )]);
        assert_eq!(
            client_addr(ip("::ffff:10.0.0.1"), &chain, &trusted),
            ip("198.51.100.1")
        );
        // An untrusted mapped peer is returned in its plain form.
        assert_eq!(
            client_addr(ip("::ffff:198.51.100.7"), &chain, &trusted),
            ip("198.51.100.7")
        );
        let cf = headers(&[("cf-connecting-ip", "::ffff:203.0.113.9")]);
        assert_eq!(
            client_addr(ip("::ffff:10.0.0.1"), &cf, &trusted),
            ip("203.0.113.9")
        );
    }

    #[test]
    fn recent_means_within_a_minute() {
        assert!(!is_recent(None));
        assert!(is_recent(Some(&after(0))));
        assert!(is_recent(Some(&after(-50))));
        assert!(!is_recent(Some(&after(-70))));
        assert!(!is_recent(Some("2000-01-01 00:00:00")));
    }
}
