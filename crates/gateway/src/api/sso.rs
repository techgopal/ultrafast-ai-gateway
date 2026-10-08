//! Sign-in through an identity provider outside the gateway: where the
//! browser starts (`/api/auth/oidc/start`), where it comes back
//! (`/api/auth/oidc/callback`), and how the person the provider vouches for
//! becomes one of the gateway's users.
//!
//! Both endpoints are plain browser navigations: GET, no CSRF header, no
//! JSON. A failure is never shown as an error body but as a redirect to
//! `/sign-in?sso_error=<code>`, which the console words; the codes are fixed
//! words and carry nothing the provider sent.

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Instant;

use axum::extract::{RawQuery, State};
use axum::http::header::{CACHE_CONTROL, LOCATION, SET_COOKIE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};

use super::auth::{open_session, safe_return_to, session_cookie_header};
use super::{cookie_value, refresh_snapshot, trimmed_name, ApiError, ClientAddr, MAX_NAME_CHARS};
use crate::app::AppState;
use crate::identity::external::{CallbackParams, ExternalError, ExternalIdentity};
use crate::identity::{Role, UserStatus};
use crate::store::{AuditEntry, NewUser, OidcSettings, StoreError, Tx, UserRow};

/// The cookie that carries the state of a sign-in attempt between the two
/// requests. Only `/api/auth/oidc` sees it.
const FLOW_COOKIE: &str = "uf_oidc";
const FLOW_COOKIE_PATH: &str = "/api/auth/oidc";
/// How long a sign-in attempt may take, in seconds.
const FLOW_MAX_AGE: i64 = 600;
/// The longest query string of a callback that is read.
const MAX_QUERY_BYTES: usize = 16 * 1024;

fn flow_cookie_header(value: &str, max_age: i64, secure: bool) -> anyhow::Result<HeaderValue> {
    let secure = if secure { "; Secure" } else { "" };
    let text = format!(
        "{FLOW_COOKIE}={value}; HttpOnly; SameSite=Lax; Path={FLOW_COOKIE_PATH}; Max-Age={max_age}{secure}"
    );
    // The error would quote the cookie, so it is not passed on.
    HeaderValue::from_str(&text)
        .map_err(|_| anyhow::anyhow!("the flow cookie is not a valid header"))
}

/// The pairs of a query string. Nothing is kept for a query that is too long.
fn query_pairs(raw: Option<&str>) -> Vec<(String, String)> {
    let Some(raw) = raw.filter(|r| r.len() <= MAX_QUERY_BYTES) else {
        return Vec::new();
    };
    let mut url = reqwest::Url::parse("http://callback.invalid/").expect("a constant URL parses");
    url.set_query(Some(raw));
    url.query_pairs().into_owned().collect()
}

/// A 302 to `location`. Never cached.
fn redirect(location: &str, cookies: Vec<HeaderValue>) -> anyhow::Result<Response> {
    let mut response = StatusCode::FOUND.into_response();
    let headers = response.headers_mut();
    headers.insert(
        LOCATION,
        HeaderValue::from_str(location)
            .map_err(|_| anyhow::anyhow!("the location is not valid"))?,
    );
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    for cookie in cookies {
        headers.append(SET_COOKIE, cookie);
    }
    Ok(response)
}

/// Back to the sign-in page with the reason. The flow cookie is cleared
/// when `clear_flow` is set, which every end of a callback does.
fn refused(state: &AppState, code: &'static str, clear_flow: bool) -> Response {
    let cookies = if clear_flow {
        flow_cookie_header("", 0, state.cookie_secure).map(|cleared| vec![cleared])
    } else {
        Ok(Vec::new())
    };
    let sent = cookies.and_then(|c| redirect(&format!("/sign-in?sso_error={code}"), c));
    sent.unwrap_or_else(|e| {
        tracing::error!(error = %e, "single sign-on redirect failed");
        ApiError::internal().into_response()
    })
}

/// The code the sign-in page words for a failure of the provider's half.
fn code_of(error: &ExternalError) -> &'static str {
    match error {
        ExternalError::NotConfigured | ExternalError::Discovery(_) => "config",
        ExternalError::BadState => "state",
        ExternalError::Expired => "expired",
        ExternalError::IdpError(_) => "idp",
        ExternalError::Token(_) | ExternalError::Exchange(_) => "token",
    }
}

#[utoipa::path(
    get,
    path = "/auth/oidc/start",
    tag = "auth",
    operation_id = "auth_oidc_start",
    summary = "Start signing in with the identity provider",
    description = "A browser navigation, not a call for a script: a GET that needs no session and no CSRF header. Limited to 60 starts per client address in 15 minutes, counted apart from sign-in failures; over the limit the browser is sent to `/sign-in?sso_error=rate_limited` and a flow cookie already set is left alone.",
    params(
        ("return_to" = Option<String>, Query, description = "Where to send the browser after sign-in: a path inside the console that starts with a single `/`. Anything else means `/`."),
    ),
    responses(
        (
            status = 302,
            description = "The browser is sent to the identity provider. The flow cookie `uf_oidc` (HttpOnly, SameSite=Lax, Path=/api/auth/oidc, 10 minutes) is set. Without a provider that can be reached, to `/sign-in?sso_error=config`; over the start limit, to `/sign-in?sso_error=rate_limited` (no cookie is set).",
            headers(
                ("Location" = String, description = "The identity provider's authorization address."),
                ("Set-Cookie" = String, description = "The flow cookie `uf_oidc`."),
            ),
        ),
        (status = 404, description = "`oidc_disabled`: single sign-on is not turned on.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
)]
pub async fn oidc_start(
    State(state): State<Arc<AppState>>,
    ClientAddr(addr): ClientAddr,
    RawQuery(raw): RawQuery,
) -> Result<Response, ApiError> {
    let Some(provider) = state.sign_in.load_full() else {
        return Err(ApiError::not_found_with(
            "oidc_disabled",
            "Single sign-on is not turned on.",
        ));
    };
    // A start may make the gateway fetch the provider's discovery document:
    // limited per client address, in a bucket of its own. Any web page can
    // make a browser start one, so it must not use up the failures that
    // password sign-in and callbacks are limited by.
    if !state.limiter.try_begin_start(addr, Instant::now()) {
        tracing::info!(
            reason = "rate_limited",
            "single sign-on could not be started"
        );
        // The cookie of an attempt that is under way is left alone.
        return Ok(refused(&state, "rate_limited", false));
    }
    let return_to = query_pairs(raw.as_deref())
        .into_iter()
        .find(|(name, _)| name == "return_to")
        .map(|(_, value)| safe_return_to(&value))
        .unwrap_or_else(|| "/".to_string());
    match provider.begin(&return_to).await {
        Ok(begin) => {
            let cookie = flow_cookie_header(&begin.flow_cookie, FLOW_MAX_AGE, state.cookie_secure)?;
            Ok(redirect(&begin.redirect_to, vec![cookie])?)
        }
        Err(e) => {
            let code = code_of(&e);
            tracing::info!(reason = code, "single sign-on could not be started");
            Ok(refused(&state, code, false))
        }
    }
}

#[utoipa::path(
    get,
    path = "/auth/oidc/callback",
    tag = "auth",
    operation_id = "auth_oidc_callback",
    summary = "Finish signing in with the identity provider",
    description = "Where the identity provider sends the browser back: a GET that needs no session and no CSRF header, the flow cookie and `state` being what ties it to the start. Limited per client address with the same failure budget as password sign-in; over the limit the browser is sent to `/sign-in?sso_error=rate_limited`.",
    params(
        ("code" = Option<String>, Query, description = "The authorization code the identity provider made."),
        ("state" = Option<String>, Query, description = "The `state` of the attempt."),
        ("error" = Option<String>, Query, description = "The error code when the identity provider refused."),
        ("error_description" = Option<String>, Query, description = "Ignored."),
    ),
    responses(
        (
            status = 302,
            description = "Signed in: the session cookie is set and the browser is sent to the path given at the start (`/` when it was not a console path). Not signed in: the browser is sent to `/sign-in?sso_error=<code>` with code `state`, `expired`, `idp`, `token`, `not_allowed`, `disabled`, `rate_limited` or `config`. The flow cookie is cleared either way.",
            headers(
                ("Location" = String, description = "The console path, or the sign-in page with the reason."),
                ("Set-Cookie" = String, description = "The cleared flow cookie, and the session cookie when signed in."),
            ),
        ),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
)]
pub async fn oidc_callback(
    State(state): State<Arc<AppState>>,
    ClientAddr(addr): ClientAddr,
    headers: HeaderMap,
    RawQuery(raw): RawQuery,
) -> Response {
    match finish_sign_in(&state, addr, &headers, raw.as_deref()).await {
        Ok(done) => {
            state.metrics.oidc_signin("ok");
            let cookies = flow_cookie_header("", 0, state.cookie_secure).and_then(|cleared| {
                Ok(vec![cleared, session_cookie_header(&state, &done.session)?])
            });
            match cookies.and_then(|c| redirect(&done.return_to, c)) {
                Ok(response) => response,
                Err(e) => {
                    tracing::error!(error = %e, "single sign-on redirect failed");
                    ApiError::internal().into_response()
                }
            }
        }
        Err(code) => {
            state.metrics.oidc_signin(code);
            tracing::info!(reason = code, "single sign-on callback refused");
            refused(&state, code, true)
        }
    }
}

struct Done {
    session: crate::store::NewSession,
    return_to: String,
}

/// Runs the callback; the error is the `sso_error` code.
async fn finish_sign_in(
    state: &Arc<AppState>,
    addr: IpAddr,
    headers: &HeaderMap,
    raw_query: Option<&str>,
) -> Result<Done, &'static str> {
    // Counted before anything else, so callbacks sent at the same time
    // cannot each get a try; a success gives its attempt back.
    if !state.limiter.try_begin_address(addr, Instant::now()) {
        return Err("rate_limited");
    }
    let Some(provider) = state.sign_in.load_full() else {
        return Err("config");
    };
    let params: CallbackParams = query_pairs(raw_query).into_iter().collect();
    // A missing cookie is the provider's to judge: the provider's own
    // refusal still counts as such.
    let flow_cookie = cookie_value(headers, FLOW_COOKIE).unwrap_or_default();
    let completed = provider
        .complete(&params, flow_cookie)
        .await
        .map_err(|e| code_of(&e))?;
    let settings = state.store.oidc_settings().await.map_err(internal)?;
    let (session, changed) =
        sign_in_identity(state, &settings, provider.label(), completed.identity).await?;
    if changed {
        // Roles and statuses decide access; /v1 must see the change.
        let _ = refresh_snapshot(state).await;
    }
    // This attempt.
    state.limiter.forgive(addr);
    Ok(Done {
        session,
        return_to: safe_return_to(&completed.return_to),
    })
}

fn internal(e: anyhow::Error) -> &'static str {
    tracing::error!(error = %e, "single sign-on failed");
    "config"
}

/// The email's domain, lower case.
fn domain_of(email: &str) -> &str {
    email.rsplit_once('@').map_or("", |(_, domain)| domain)
}

/// A name for a user made on first sign-in: the provider's, else the part
/// of the email before the `@`.
fn display_name(identity: &ExternalIdentity) -> String {
    let from_provider = identity
        .name
        .as_deref()
        .and_then(|n| trimmed_name(n).ok())
        .map(str::to_string);
    from_provider.unwrap_or_else(|| {
        identity
            .email
            .split('@')
            .next()
            .unwrap_or_default()
            .chars()
            .filter(|c| !c.is_control())
            .take(MAX_NAME_CHARS)
            .collect()
    })
}

/// Finds or makes the gateway user for `identity`, applies the admin-group
/// rule, and opens a session, all in one transaction. The error is the
/// `sso_error` code. The flag says whether something `/v1` depends on
/// changed.
async fn sign_in_identity(
    state: &Arc<AppState>,
    settings: &OidcSettings,
    label: &str,
    identity: ExternalIdentity,
) -> Result<(crate::store::NewSession, bool), &'static str> {
    // Taken at once: the last-admin check reads before it writes.
    let mut tx = state.store.begin_immediate().await.map_err(internal)?;
    let mut changed = false;
    let mut user = match tx
        .user_by_external(identity.provider, &identity.external_id)
        .await
        .map_err(internal)?
    {
        Some(user) => user,
        None => {
            let (user, made_changes) = new_link(&mut tx, settings, label, &identity).await?;
            changed |= made_changes;
            user
        }
    };
    if user.status == UserStatus::Disabled {
        return Err("disabled");
    }
    changed |= apply_admin_group(&mut tx, settings, label, &identity, &mut user)
        .await
        .map_err(internal)?;
    let session = open_session(
        &mut tx,
        &user,
        &format!("{} signed in via OIDC ({label})", user.email),
    )
    .await
    .map_err(internal)?;
    tx.commit().await.map_err(internal)?;
    Ok((session, changed))
}

/// The user for an identity not seen before: an existing user the verified
/// email names, or a new one, or nobody (`not_allowed`).
async fn new_link(
    tx: &mut Tx<'_>,
    settings: &OidcSettings,
    label: &str,
    identity: &ExternalIdentity,
) -> Result<(UserRow, bool), &'static str> {
    // Only an address the provider vouches for names a user. An unverified
    // one is whatever the person typed at the provider.
    let existing = if settings.link_by_email && identity.email_verified {
        tx.user_by_email(&identity.email).await.map_err(internal)?
    } else {
        None
    };
    if let Some(user) = existing {
        if user.status == UserStatus::Disabled {
            return Err("disabled");
        }
        // Linked to this very provider under another subject: another
        // person with the same address (it was reassigned), not the same
        // one. A link to another issuer (it was changed) or a password
        // account may be linked.
        let same_issuer = format!("{}|", settings.issuer);
        if user.auth_provider == identity.provider
            && user
                .external_id
                .as_deref()
                .is_some_and(|id| id.starts_with(&same_issuer))
        {
            return Err("not_allowed");
        }
        link(tx, user.id, identity).await?;
        let activated = user.status == UserStatus::Invited;
        if activated {
            tx.set_user_status(user.id, UserStatus::Active)
                .await
                .map_err(internal)?;
            tx.delete_invites_of(user.id).await.map_err(internal)?;
        }
        tx.audit(AuditEntry {
            actor_user_id: Some(user.id),
            actor_email: &user.email,
            action: "user.link_oidc",
            target_type: "user",
            target_id: Some(user.id),
            summary: &format!(
                "Linked {} to sign-in with {label}{}",
                user.email,
                if activated {
                    " and activated the account"
                } else {
                    ""
                }
            ),
        })
        .await
        .map_err(internal)?;
        let linked = tx
            .user_by_id(user.id)
            .await
            .map_err(internal)?
            .ok_or("config")?;
        return Ok((linked, activated));
    }

    // A new account needs a verified address in an allowed domain: the
    // person must really own it, or they could hold it before its owner.
    let allowed = settings.auto_create
        && identity.email_verified
        && settings
            .allowed_domains
            .iter()
            .any(|d| d == domain_of(&identity.email));
    if !allowed {
        return Err("not_allowed");
    }
    let name = display_name(identity);
    let inserted = tx
        .insert_user(NewUser {
            email: &identity.email,
            name: &name,
            role: Role::Member,
            status: UserStatus::Active,
            password_hash: None,
        })
        .await;
    let id = match inserted {
        Ok(id) => id,
        Err(e) => {
            return Err(match e.downcast_ref::<StoreError>() {
                // The address belongs to a user the rules did not let in.
                Some(StoreError::Duplicate) => "not_allowed",
                None => internal(e),
            });
        }
    };
    link(tx, id, identity).await?;
    tx.audit(AuditEntry {
        actor_user_id: Some(id),
        actor_email: &identity.email,
        action: "user.create_oidc",
        target_type: "user",
        target_id: Some(id),
        summary: &format!("Created {} on first sign-in with {label}", identity.email),
    })
    .await
    .map_err(internal)?;
    let created = tx.user_by_id(id).await.map_err(internal)?.ok_or("config")?;
    Ok((created, true))
}

async fn link(
    tx: &mut Tx<'_>,
    user_id: i64,
    identity: &ExternalIdentity,
) -> Result<(), &'static str> {
    match tx
        .link_external(user_id, identity.provider, &identity.external_id)
        .await
    {
        Ok(true) => Ok(()),
        Ok(false) => Err("config"),
        Err(e) => Err(match e.downcast_ref::<StoreError>() {
            // Another user has this identity.
            Some(StoreError::Duplicate) => "not_allowed",
            None => internal(e),
        }),
    }
}

/// With an admin group set, the user is an admin exactly while the provider
/// says they are in it, except that the last active admin stays one. Without
/// an admin group roles are not touched. Returns whether the role changed.
async fn apply_admin_group(
    tx: &mut Tx<'_>,
    settings: &OidcSettings,
    label: &str,
    identity: &ExternalIdentity,
    user: &mut UserRow,
) -> anyhow::Result<bool> {
    if settings.admin_group.is_empty() {
        return Ok(false);
    }
    // Not told which groups the user is in (no groups claim): nothing is
    // known, so nothing is changed. An empty list is an answer.
    let Some(groups) = &identity.groups else {
        tracing::info!("groups claim missing; role left unchanged");
        return Ok(false);
    };
    let in_group = groups.contains(&settings.admin_group);
    let wanted = if in_group { Role::Admin } else { Role::Member };
    if wanted == user.role {
        return Ok(false);
    }
    if wanted == Role::Member && tx.count_active_admins().await? <= 1 {
        tx.audit(AuditEntry {
            actor_user_id: Some(user.id),
            actor_email: &user.email,
            action: "user.role_from_idp",
            target_type: "user",
            target_id: Some(user.id),
            summary: &format!(
                "Kept {} as admin although {label} no longer lists them in the admin group: the last active admin is never demoted",
                user.email
            ),
        })
        .await?;
        return Ok(false);
    }
    tx.set_user_role(user.id, wanted).await?;
    tx.audit(AuditEntry {
        actor_user_id: Some(user.id),
        actor_email: &user.email,
        action: "user.role_from_idp",
        target_type: "user",
        target_id: Some(user.id),
        summary: &format!(
            "Made {} {} as {label} {} them in the admin group",
            user.email,
            if in_group { "an admin" } else { "a member" },
            if in_group { "lists" } else { "no longer lists" },
        ),
    })
    .await?;
    user.role = wanted;
    Ok(true)
}
