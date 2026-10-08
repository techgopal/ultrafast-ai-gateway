//! Setup, sign-in, sign-out, the current user, invite acceptance and
//! password change.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, bail, Context};
use axum::extract::State;
use axum::http::header::SET_COOKIE;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::Semaphore;

use super::SESSION_COOKIE;
use super::{
    refresh_snapshot, require, trimmed_name, ApiError, ApiJson, AuthVia, Authed, ClientAddr,
};
use crate::app::AppState;
use crate::identity::password::{
    check_password_policy, hash_password, verify_dummy, verify_password,
};
use crate::identity::policy::Action;
use crate::identity::{normalize_email, Principal, Role, TeamRole, UserStatus};
use crate::secrets::{hash_key, setup_code_matches, INVITE_PREFIX};
use crate::store::{AuditEntry, NewSession, NewUser, Store, Tx, UserRow, UserTeam};

/// The name of an admin created from the environment at startup.
const BOOTSTRAP_NAME: &str = "Admin";

/// A user as `/api` shows it. It has no field for the password hash.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct UserView {
    pub id: i64,
    pub email: String,
    pub name: String,
    pub role: Role,
    pub status: UserStatus,
    /// How the user signs in: with a `password`, or through the single
    /// sign-on provider (`oidc`) they are linked to. Read only.
    pub auth_provider: AuthProviderView,
    pub created_at: String,
    #[schema(required)]
    pub last_active_at: Option<String>,
    /// The user's teams, ordered by name.
    pub teams: Vec<UserTeamView>,
}

/// How a user signs in, as `/api` shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum AuthProviderView {
    Password,
    Oidc,
}

impl AuthProviderView {
    fn of(stored: &str) -> Self {
        match stored {
            "oidc" => Self::Oidc,
            _ => Self::Password,
        }
    }
}

impl UserView {
    pub fn new(u: UserRow, teams: Vec<UserTeam>) -> Self {
        Self {
            id: u.id,
            email: u.email,
            name: u.name,
            role: u.role,
            status: u.status,
            auth_provider: AuthProviderView::of(&u.auth_provider),
            created_at: u.created_at,
            last_active_at: u.last_active_at,
            teams: teams.into_iter().map(UserTeamView::from).collect(),
        }
    }
}

/// The teams of `user_id` that `viewer` may see: all of them for an admin
/// and for the user themself, otherwise only those the viewer belongs to.
/// A team the viewer cannot open stays hidden here too.
pub fn teams_visible_to(viewer: &Principal, user_id: i64, teams: Vec<UserTeam>) -> Vec<UserTeam> {
    if viewer.is_admin() || viewer.user_id == user_id {
        return teams;
    }
    teams
        .into_iter()
        .filter(|t| viewer.team_role(t.team_id).is_some())
        .collect()
}

/// The view of a user as `viewer` may see them.
pub async fn user_view_for(
    store: &Store,
    viewer: &Principal,
    user: UserRow,
) -> anyhow::Result<UserView> {
    let mut teams = store.teams_of_users(&[user.id]).await?;
    let teams = teams_visible_to(viewer, user.id, teams.remove(&user.id).unwrap_or_default());
    Ok(UserView::new(user, teams))
}

/// The view of a single user, with all their teams: for the user
/// themself, and for answers only an admin gets.
pub async fn user_view(store: &Store, user: UserRow) -> anyhow::Result<UserView> {
    let mut teams = store.teams_of_users(&[user.id]).await?;
    let teams = teams.remove(&user.id).unwrap_or_default();
    Ok(UserView::new(user, teams))
}

// Request types have no `Debug`: most of them hold a password or a token.

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetupRequest {
    email: String,
    name: String,
    #[schema(write_only)]
    password: String,
    /// The one-time code the gateway printed to its log when it started
    /// without users. Required; left out or wrong, the answer is 403.
    #[schema(write_only)]
    setup_code: Option<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct LoginRequest {
    email: String,
    #[schema(write_only)]
    password: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AcceptInviteRequest {
    /// The token of the invite link.
    #[schema(write_only)]
    token: String,
    #[schema(write_only)]
    password: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ChangePasswordRequest {
    #[schema(write_only)]
    current_password: String,
    #[schema(write_only)]
    new_password: String,
}

/// A team of a user, with their role in it.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct UserTeamView {
    team_id: i64,
    name: String,
    role: TeamRole,
}

impl From<UserTeam> for UserTeamView {
    fn from(t: UserTeam) -> Self {
        Self {
            team_id: t.team_id,
            name: t.name,
            role: t.role,
        }
    }
}

/// Runs `work` on a blocking thread while holding one permit of `hashing`.
/// The permit belongs to the work, not to the caller: a caller that goes
/// away while the work runs does not free it early.
async fn run_bounded<T: Send + 'static>(
    hashing: &Arc<Semaphore>,
    work: impl FnOnce() -> T + Send + 'static,
) -> anyhow::Result<T> {
    let permit = hashing
        .clone()
        .acquire_owned()
        .await
        .context("hashing is closed")?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        work()
    })
    .await
    .context("the hashing task failed")
}

/// Hashes off the async threads: Argon2 takes tens of milliseconds.
async fn hash_blocking(hashing: &Arc<Semaphore>, password: String) -> anyhow::Result<String> {
    run_bounded(hashing, move || hash_password(&password)).await?
}

/// Checks a password against a stored hash, off the async threads. Without
/// a hash it spends the same effort and answers `false`.
async fn verify_blocking(
    hashing: &Arc<Semaphore>,
    password: String,
    hash: Option<String>,
) -> anyhow::Result<bool> {
    run_bounded(hashing, move || match hash {
        Some(hash) => verify_password(&password, &hash),
        None => {
            verify_dummy(&password);
            false
        }
    })
    .await
}

/// The `Set-Cookie` value for the session cookie.
pub(super) fn cookie_header(
    value: &str,
    max_age: i64,
    secure: bool,
) -> anyhow::Result<HeaderValue> {
    let secure = if secure { "; Secure" } else { "" };
    let text = format!(
        "{SESSION_COOKIE}={value}; HttpOnly; SameSite=Strict; Path=/; Max-Age={max_age}{secure}"
    );
    // The error would quote the cookie, so it is not passed on.
    HeaderValue::from_str(&text).map_err(|_| anyhow!("the session cookie is not a valid header"))
}

/// Starts a session for `user` in `tx` and records `auth.login`. Every way
/// to sign in ends here, so a session is the same whatever made it: the same
/// lifetime (`session_hours`), CSRF token and audit entry. The caller commits
/// and sets the cookie with [`session_cookie_header`].
pub(super) async fn open_session(
    tx: &mut Tx<'_>,
    user: &UserRow,
    summary: &str,
) -> anyhow::Result<NewSession> {
    let session = tx.create_session(user.id).await?;
    tx.audit(AuditEntry {
        actor_user_id: Some(user.id),
        actor_email: &user.email,
        action: "auth.login",
        target_type: "user",
        target_id: Some(user.id),
        summary,
    })
    .await?;
    Ok(session)
}

/// The `Set-Cookie` header that gives the browser a new session.
pub(super) fn session_cookie_header(
    state: &AppState,
    session: &NewSession,
) -> anyhow::Result<HeaderValue> {
    cookie_header(&session.id, session.max_age_seconds, state.cookie_secure)
}

/// The longest return path kept.
const MAX_RETURN_TO_BYTES: usize = 2048;

/// Where the console may send the browser after a sign-in: a path inside
/// the console that starts with a single `/`. Anything else (a scheme, a
/// host, `//`, a backslash, a space or control character, anything outside
/// printable ASCII, or a path into `/api` or `/v1`) becomes `/`. Used when a
/// sign-in starts and again when it ends, so a path is never trusted for
/// having been checked once.
pub(super) fn safe_return_to(raw: &str) -> String {
    let printable = raw.bytes().all(|b| (0x21..=0x7e).contains(&b));
    let path = raw.split(['?', '#']).next().unwrap_or_default();
    let into_api = ["/api", "/v1"]
        .iter()
        .any(|p| path == *p || path.starts_with(&format!("{p}/")));
    let ok = raw.len() <= MAX_RETURN_TO_BYTES
        && printable
        && raw.starts_with('/')
        && !raw.starts_with("//")
        && !raw.contains('\\')
        && !into_api;
    if ok {
        raw.to_string()
    } else {
        "/".to_string()
    }
}

/// Inserts the first admin unless a user exists, which is checked in the
/// transaction of the insert. Returns the new user's id.
async fn create_first_admin(
    store: &Store,
    email: &str,
    name: &str,
    password_hash: &str,
) -> anyhow::Result<Option<i64>> {
    let mut tx = store.begin().await?;
    if tx.count_users().await? != 0 {
        return Ok(None);
    }
    let id = tx
        .insert_user(NewUser {
            email,
            name,
            role: Role::Admin,
            status: UserStatus::Active,
            password_hash: Some(password_hash),
        })
        .await?;
    tx.audit(AuditEntry {
        actor_user_id: Some(id),
        actor_email: email,
        action: "setup.create_admin",
        target_type: "user",
        target_id: Some(id),
        summary: &format!("Created the first admin {email}"),
    })
    .await?;
    tx.commit().await?;
    Ok(Some(id))
}

/// Creates the first admin from `UF_ADMIN_EMAIL` and `UF_ADMIN_PASSWORD`
/// when no user exists. Errors name the variable and never show a value of
/// the password.
pub async fn bootstrap_admin(
    store: &Store,
    email: Option<String>,
    password: Option<String>,
) -> anyhow::Result<()> {
    let (email, password) = match (email, password) {
        (None, None) => return Ok(()),
        (Some(_), None) => bail!("UF_ADMIN_EMAIL is set but UF_ADMIN_PASSWORD is not"),
        (None, Some(_)) => bail!("UF_ADMIN_PASSWORD is set but UF_ADMIN_EMAIL is not"),
        (Some(email), Some(password)) => (email, password),
    };
    if store.count_users().await? != 0 {
        return Ok(());
    }
    let email = normalize_email(&email).map_err(|m| anyhow!("UF_ADMIN_EMAIL: {m}"))?;
    check_password_policy(&password).map_err(|m| anyhow!("UF_ADMIN_PASSWORD: {m}"))?;
    // Startup hashes one password, before the shared limit exists.
    let hash = hash_blocking(&Arc::new(Semaphore::new(1)), password).await?;
    if create_first_admin(store, &email, BOOTSTRAP_NAME, &hash)
        .await?
        .is_some()
    {
        tracing::info!(%email, "created the first admin from the environment");
    }
    Ok(())
}

#[utoipa::path(
    get,
    path = "/setup",
    tag = "auth",
    operation_id = "auth_setup_status",
    responses(
        (status = 200, description = "Whether the first admin still has to be created.", body = super::openapi::SetupStatus),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
)]
pub async fn setup_status(State(state): State<Arc<AppState>>) -> Result<Response, ApiError> {
    let needs_setup = state.store.count_users().await? == 0;
    Ok(Json(json!({ "needs_setup": needs_setup })).into_response())
}

fn already_set_up() -> ApiError {
    ApiError::conflict("already_set_up", "The gateway is already set up.")
}

#[utoipa::path(
    post,
    path = "/setup",
    tag = "auth",
    operation_id = "auth_setup",
    request_body = SetupRequest,
    responses(
        (status = 201, description = "The first admin.", body = UserView),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "`setup_code_invalid`: the setup code is missing or wrong.", body = super::openapi::ApiErrorBody),
        (status = 409, description = "`already_set_up`: a user exists.", body = super::openapi::ApiErrorBody),
        (status = 413, description = "The request body is too large.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "Some fields are not valid; `fields` names each of them.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
)]
pub async fn setup(
    State(state): State<Arc<AppState>>,
    ApiJson(req): ApiJson<SetupRequest>,
) -> Result<Response, ApiError> {
    let store = &state.store;
    // Answered before any hashing; the transaction below checks it again.
    if store.count_users().await? != 0 {
        return Err(already_set_up());
    }
    // Before the fields: who has not the code learns nothing from them.
    let code_ok = match (&state.setup_code, &req.setup_code) {
        (Some(expected), Some(given)) => setup_code_matches(expected, given),
        _ => false,
    };
    if !code_ok {
        return Err(ApiError::setup_code_invalid());
    }

    let mut fields = BTreeMap::new();
    let email = normalize_email(&req.email)
        .map_err(|m| fields.insert("email".to_string(), m.to_string()))
        .ok();
    let name = trimmed_name(&req.name)
        .map_err(|m| fields.insert("name".to_string(), m.to_string()))
        .ok();
    if let Err(m) = check_password_policy(&req.password) {
        fields.insert("password".to_string(), m.to_string());
    }
    let (Some(email), Some(name), true) = (email, name, fields.is_empty()) else {
        return Err(ApiError::validation(fields));
    };

    let hash = hash_blocking(&state.hashing, req.password).await?;
    let id = create_first_admin(store, &email, name, &hash)
        .await?
        .ok_or_else(already_set_up)?;
    let user = store
        .user_by_id(id)
        .await?
        .ok_or_else(|| anyhow!("the new admin is missing"))?;
    Ok((
        StatusCode::CREATED,
        Json(user_view(&state.store, user).await?),
    )
        .into_response())
}

/// The limiter's name for whatever was sent as the email. It has a bounded
/// length even when the input is not an email.
fn limiter_key(raw_email: &str, normalized: Option<&str>) -> String {
    match normalized {
        Some(email) => email.to_string(),
        None => hash_key(&raw_email.trim().to_lowercase()),
    }
}

/// Counts an attempt against the limits, or refuses it. The attempt stays
/// counted as a failure unless `attempt_succeeded` is called for it.
fn begin_attempt(state: &AppState, key: &str, addr: IpAddr) -> Result<(), ApiError> {
    if !state.limiter.try_begin(key, addr, Instant::now()) {
        return Err(ApiError::too_many_attempts());
    }
    Ok(())
}

/// Takes back what `begin_attempt` counted, and the failures of the email
/// from this address.
fn attempt_succeeded(state: &AppState, key: &str, addr: IpAddr) {
    state.limiter.record_success(key, addr);
    state.limiter.forgive(addr);
}

#[utoipa::path(
    post,
    path = "/auth/login",
    tag = "auth",
    operation_id = "auth_login",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Signed in. The session cookie is set.", body = super::openapi::LoginResponse),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "`invalid_credentials`: the email or the password is incorrect.", body = super::openapi::ApiErrorBody),
        (status = 413, description = "The request body is too large.", body = super::openapi::ApiErrorBody),
        (status = 429, description = "Too many failed attempts.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
)]
pub async fn login(
    State(state): State<Arc<AppState>>,
    ClientAddr(addr): ClientAddr,
    ApiJson(req): ApiJson<LoginRequest>,
) -> Result<Response, ApiError> {
    let store = &state.store;
    let email = normalize_email(&req.email).ok();
    let key = limiter_key(&req.email, email.as_deref());
    // Counted before the password is checked, so requests sent at the same
    // time cannot each get a guess.
    begin_attempt(&state, &key, addr)?;

    let user = match &email {
        Some(email) => store.user_by_email(email).await?,
        None => None,
    };
    // Every way to fail takes the same path: a hash is always computed.
    let user = user.filter(|u| u.status == UserStatus::Active);
    let hash = user.as_ref().and_then(|u| u.password_hash.clone());
    let verified = verify_blocking(&state.hashing, req.password, hash).await?;
    let Some(user) = user.filter(|_| verified) else {
        return Err(ApiError::invalid_credentials());
    };

    let mut tx = store.begin().await?;
    let session = open_session(&mut tx, &user, &format!("{} signed in", user.email)).await?;
    tx.commit().await?;
    attempt_succeeded(&state, &key, addr);

    let cookie = session_cookie_header(&state, &session)?;
    let user = user_view(&state.store, user).await?;
    let body = json!({ "user": user, "csrf_token": session.csrf_token });
    Ok(([(SET_COOKIE, cookie)], Json(body)).into_response())
}

#[utoipa::path(
    get,
    path = "/auth/methods",
    tag = "auth",
    operation_id = "auth_methods",
    responses(
        (status = 200, description = "How people can sign in: always with a password, and with the configured single sign-on provider when it is on.", body = super::openapi::SignInMethods),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
)]
pub async fn methods(State(state): State<Arc<AppState>>) -> Result<Response, ApiError> {
    let oidc = state
        .sign_in
        .load_full()
        .map(|provider| json!({ "label": provider.label() }));
    Ok(Json(json!({ "password": true, "oidc": oidc })).into_response())
}

#[utoipa::path(
    post,
    path = "/auth/logout",
    tag = "auth",
    operation_id = "auth_logout",
    responses(
        (status = 204, description = "Signed out. The session cookie is cleared."),
        (status = 400, description = "The caller used an access token. Only a browser session can be signed out.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn logout(
    State(state): State<Arc<AppState>>,
    authed: Authed,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    require(
        me,
        &Action::UpdateUser {
            user_id: me.user_id,
            changes_role_or_status: false,
        },
    )?;
    let AuthVia::Session { session_id, .. } = authed.via else {
        return Err(ApiError::bad_request(
            "Only a browser session can be signed out.",
        ));
    };
    let mut tx = state.store.begin().await?;
    // False when another request ended the session in the meantime: then
    // nothing changed and nothing is recorded.
    if tx.delete_session_by_id(session_id).await? {
        tx.audit(AuditEntry {
            actor_user_id: Some(me.user_id),
            actor_email: &me.email,
            action: "auth.logout",
            target_type: "user",
            target_id: Some(me.user_id),
            summary: &format!("{} signed out", me.email),
        })
        .await?;
        tx.commit().await?;
    } else {
        drop(tx);
    }
    let cleared = cookie_header("", 0, state.cookie_secure)?;
    Ok((StatusCode::NO_CONTENT, [(SET_COOKIE, cleared)]).into_response())
}

#[utoipa::path(
    get,
    path = "/auth/me",
    tag = "auth",
    operation_id = "auth_me",
    responses(
        (status = 200, description = "The caller and their teams.", body = super::openapi::MeResponse),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn me(State(state): State<Arc<AppState>>, authed: Authed) -> Result<Response, ApiError> {
    let me = &authed.principal;
    require(
        me,
        &Action::ViewUser {
            user_id: me.user_id,
            shares_led_team: false,
        },
    )?;
    let store = &state.store;
    let user = store
        .user_by_id(me.user_id)
        .await?
        .ok_or_else(ApiError::unauthenticated)?;
    let mut teams = Vec::with_capacity(me.teams.len());
    for (team_id, role) in &me.teams {
        // A team deleted since the principal was built is left out.
        if let Some(team) = store.team_by_id(*team_id).await? {
            teams.push(UserTeamView {
                team_id: team.id,
                name: team.name,
                role: *role,
            });
        }
    }
    let csrf_token = match authed.via {
        AuthVia::Session { csrf_token, .. } => Some(csrf_token),
        AuthVia::Token { .. } => None,
    };
    let body = json!({
        "user": user_view(store, user).await?,
        "teams": teams,
        "csrf_token": csrf_token,
    });
    Ok(Json(body).into_response())
}

#[utoipa::path(
    post,
    path = "/auth/accept-invite",
    tag = "auth",
    operation_id = "auth_accept_invite",
    request_body = AcceptInviteRequest,
    responses(
        (status = 204, description = "The password is set and the user is active."),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "The invite does not exist, has expired or was used.", body = super::openapi::ApiErrorBody),
        (status = 413, description = "The request body is too large.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "Some fields are not valid; `fields` names each of them.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
)]
pub async fn accept_invite(
    State(state): State<Arc<AppState>>,
    ApiJson(req): ApiJson<AcceptInviteRequest>,
) -> Result<Response, ApiError> {
    let store = &state.store;
    if !req.token.starts_with(INVITE_PREFIX) {
        return Err(ApiError::not_found());
    }
    let invite = store
        .invite_by_hash(&hash_key(&req.token))
        .await?
        .ok_or_else(ApiError::not_found)?;
    let user = store
        .user_by_id(invite.user_id)
        .await?
        // Only a user who has not signed up yet. For an active user this
        // would be a password reset that ends no session.
        .filter(|u| u.status == UserStatus::Invited)
        .ok_or_else(ApiError::not_found)?;
    check_password_policy(&req.password).map_err(|m| ApiError::invalid_field("password", m))?;
    let hash = hash_blocking(&state.hashing, req.password).await?;

    let mut tx = store.begin().await?;
    // False when another request used the invite in the meantime.
    if !tx.use_invite(invite.id).await? {
        return Err(ApiError::not_found());
    }
    if !tx.set_user_password(user.id, &hash).await? {
        return Err(ApiError::not_found());
    }
    tx.set_user_status(user.id, UserStatus::Active).await?;
    tx.audit(AuditEntry {
        actor_user_id: Some(user.id),
        actor_email: &user.email,
        action: "user.accept_invite",
        target_type: "user",
        target_id: Some(user.id),
        summary: &format!("{} accepted their invite", user.email),
    })
    .await?;
    tx.commit().await?;
    refresh_snapshot(&state).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[utoipa::path(
    post,
    path = "/auth/password",
    tag = "auth",
    operation_id = "auth_change_password",
    request_body = ChangePasswordRequest,
    responses(
        (status = 204, description = "The password is changed. Other sessions and all access tokens of the caller end."),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token, or `invalid_credentials`: the current password is incorrect.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 413, description = "The request body is too large.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "Some fields are not valid; `fields` names each of them.", body = super::openapi::ApiErrorBody),
        (status = 429, description = "Too many failed attempts.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn change_password(
    State(state): State<Arc<AppState>>,
    ClientAddr(addr): ClientAddr,
    authed: Authed,
    ApiJson(req): ApiJson<ChangePasswordRequest>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    require(
        me,
        &Action::UpdateUser {
            user_id: me.user_id,
            changes_role_or_status: false,
        },
    )?;
    let store = &state.store;
    // A stolen session must not allow unlimited guesses at the password.
    begin_attempt(&state, &me.email, addr)?;
    let user = store
        .user_by_id(me.user_id)
        .await?
        .ok_or_else(ApiError::unauthenticated)?;
    if !verify_blocking(&state.hashing, req.current_password, user.password_hash).await? {
        return Err(ApiError::invalid_credentials());
    }
    attempt_succeeded(&state, &me.email, addr);
    check_password_policy(&req.new_password)
        .map_err(|m| ApiError::invalid_field("new_password", m))?;
    let hash = hash_blocking(&state.hashing, req.new_password).await?;

    let mut tx = store.begin().await?;
    if !tx.set_user_password(me.user_id, &hash).await? {
        return Err(ApiError::unauthenticated());
    }
    match authed.via {
        AuthVia::Session { session_id, .. } => {
            tx.delete_other_sessions_of(me.user_id, session_id).await?;
        }
        AuthVia::Token { .. } => {
            tx.delete_sessions_of(me.user_id).await?;
        }
    }
    tx.revoke_tokens_of(me.user_id).await?;
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "user.change_password",
        target_type: "user",
        target_id: Some(me.user_id),
        summary: &format!("{} changed their password", me.email),
    })
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use super::*;

    const PASSWORD: &str = "correct horse battery";

    fn some(s: &str) -> Option<String> {
        Some(s.to_string())
    }

    #[tokio::test]
    async fn bootstrap_creates_the_admin_once() {
        let store = Store::open_in_memory().await.unwrap();
        bootstrap_admin(&store, None, None).await.unwrap();
        assert_eq!(store.count_users().await.unwrap(), 0);

        bootstrap_admin(&store, some(" Maya@Example.com "), some(PASSWORD))
            .await
            .unwrap();
        let user = store
            .user_by_email("maya@example.com")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(user.role, Role::Admin);
        assert_eq!(user.status, UserStatus::Active);
        assert!(verify_password(PASSWORD, &user.password_hash.unwrap()));
        let audit = store.list_audit(10, None).await.unwrap();
        assert_eq!(audit.len(), 1);
        assert_eq!(audit[0].action, "setup.create_admin");
        assert!(!audit[0].summary.contains(PASSWORD));

        // With a user present the variables are ignored, valid or not.
        bootstrap_admin(&store, some("omar@example.com"), some(PASSWORD))
            .await
            .unwrap();
        bootstrap_admin(&store, some("bad"), some("short"))
            .await
            .unwrap();
        assert_eq!(store.count_users().await.unwrap(), 1);
        assert_eq!(store.list_audit(10, None).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn bootstrap_failures_name_the_variable_and_hide_the_password() {
        let store = Store::open_in_memory().await.unwrap();
        let cases = [
            (some("maya@example.com"), None, "UF_ADMIN_PASSWORD"),
            (None, some("secret-pass-value"), "UF_ADMIN_EMAIL"),
            (
                some("not-an-email"),
                some("secret-pass-value"),
                "UF_ADMIN_EMAIL",
            ),
            (
                some("maya@example.com"),
                some("secret"),
                "UF_ADMIN_PASSWORD",
            ),
        ];
        for (email, password, named) in cases {
            let err = bootstrap_admin(&store, email, password)
                .await
                .expect_err("must fail");
            let shown = format!("{err} {err:?}");
            assert!(shown.contains(named), "{shown}");
            assert!(!shown.contains("secret"), "{shown}");
        }
        assert_eq!(store.count_users().await.unwrap(), 0);
    }

    /// Counts the closures running at once and the most seen so far.
    #[derive(Default)]
    struct Running {
        now: AtomicUsize,
        most: AtomicUsize,
        started: AtomicUsize,
    }

    fn slow_work(running: Arc<Running>) -> impl FnOnce() + Send + 'static {
        move || {
            let now = running.now.fetch_add(1, Ordering::SeqCst) + 1;
            running.most.fetch_max(now, Ordering::SeqCst);
            running.started.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(300));
            running.now.fetch_sub(1, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn a_dropped_caller_keeps_its_permit_until_the_hash_ends() {
        const PERMITS: usize = 2;
        let hashing = Arc::new(Semaphore::new(PERMITS));
        let running = Arc::new(Running::default());
        let start = |count: usize| -> Vec<tokio::task::JoinHandle<()>> {
            (0..count)
                .map(|_| {
                    let (hashing, running) = (hashing.clone(), running.clone());
                    tokio::spawn(async move {
                        run_bounded(&hashing, slow_work(running)).await.unwrap();
                    })
                })
                .collect()
        };

        // The first callers go away while their work runs.
        let dropped = start(PERMITS);
        while running.started.load(Ordering::SeqCst) < PERMITS {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        for task in dropped {
            task.abort();
            assert!(task.await.unwrap_err().is_cancelled());
        }
        // More callers than permits arrive at once.
        for task in start(PERMITS * 2) {
            task.await.unwrap();
        }

        assert_eq!(running.started.load(Ordering::SeqCst), PERMITS * 3);
        assert_eq!(running.most.load(Ordering::SeqCst), PERMITS);
        assert_eq!(hashing.available_permits(), PERMITS);
    }

    #[test]
    fn cookie_attributes() {
        let secure = cookie_header("abc", 43200, true).unwrap();
        assert_eq!(
            secure,
            "uf_session=abc; HttpOnly; SameSite=Strict; Path=/; Max-Age=43200; Secure"
        );
        let plain = cookie_header("", 0, false).unwrap();
        assert_eq!(
            plain,
            "uf_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0"
        );
    }

    #[test]
    fn return_paths_are_relative_console_paths() {
        for ok in [
            "/",
            "/keys",
            "/ok?x=1",
            "/keys/12?tab=a&b=c#top",
            "/a//b",
            "/%2Fevil.com",
        ] {
            assert_eq!(safe_return_to(ok), ok);
        }
        for bad in [
            "",
            "keys",
            "//evil.com",
            "//",
            "/\\evil.com",
            "\\\\evil.com",
            "https://evil.com",
            "javascript:alert(1)",
            "/ok\r\nSet-Cookie: x=y",
            "/ok\tx",
            "/with space",
            "/caf\u{e9}",
            "/api/backup",
            "/api",
            "/v1/models",
            " /keys",
        ] {
            assert_eq!(safe_return_to(bad), "/", "{bad:?}");
        }
        assert_eq!(safe_return_to(&format!("/{}", "a".repeat(3000))), "/");
        assert_eq!(safe_return_to(&format!("/{}", "a".repeat(100))).len(), 101);
    }

    #[test]
    fn limiter_keys_are_bounded() {
        assert_eq!(
            limiter_key(" Maya@Example.com", Some("maya@example.com")),
            "maya@example.com"
        );
        let long = "x".repeat(100_000);
        assert_eq!(limiter_key(&long, None).len(), 64);
        assert_eq!(
            limiter_key(" Not An Email ", None),
            limiter_key("not an email", None)
        );
    }

    #[test]
    fn user_view_has_no_password_hash() {
        let view = UserView::new(
            UserRow {
                id: 1,
                email: "maya@example.com".into(),
                name: "Maya".into(),
                role: Role::Admin,
                status: UserStatus::Active,
                password_hash: Some("$argon2id$secret-hash".into()),
                auth_provider: "password".into(),
                external_id: None,
                created_at: "2026-01-01 00:00:00".into(),
                last_active_at: None,
            },
            vec![],
        );
        let text = serde_json::to_string(&view).unwrap();
        assert!(!text.contains("argon2"));
        assert!(!text.contains("password_hash"));
    }
}
