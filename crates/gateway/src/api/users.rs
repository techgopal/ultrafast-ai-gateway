//! Users and their invites.

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::anyhow;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::json;

use super::auth::{guardrail_ids_of_users, teams_visible_to, user_view_for, UserView};
use super::{path_id, refresh_snapshot, require, trimmed_name, ApiError, ApiJson, Authed};
use crate::app::AppState;
use crate::identity::policy::{list_scope, Action, Scope};
use crate::identity::{normalize_email, Role, UserStatus};
use crate::secrets::{generate_secret, INVITE_PREFIX, PASSWORD_LINK_PREFIX};
use crate::store::{after, AuditEntry, NewUser, Store, StoreError, Tx, UserRow};

/// How long an invite link works.
const INVITE_SECONDS: i64 = 7 * 24 * 60 * 60;
/// How long a password link works.
const PASSWORD_LINK_SECONDS: i64 = 24 * 60 * 60;
/// The page that takes an invite token.
/// The token goes in the fragment, which a browser never sends to a server.
const INVITE_PAGE: &str = "/accept-invite#token=";

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct InviteRequest {
    email: String,
    name: String,
    role: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateRequest {
    name: Option<String>,
    role: Option<String>,
    status: Option<String>,
}

fn last_admin() -> ApiError {
    ApiError::conflict("last_admin", "At least one active admin is required.")
}

/// The user of a path, or the answer for a user that does not exist.
async fn user_of(store: &Store, raw_id: &str) -> Result<UserRow, ApiError> {
    let id = path_id(raw_id)?;
    store.user_by_id(id).await?.ok_or_else(ApiError::not_found)
}

/// Stores a new invite for the user and returns its link. The link is
/// shown once: only the hash of its token is kept.
async fn new_invite(tx: &mut Tx<'_>, user_id: i64) -> anyhow::Result<String> {
    let token = generate_secret(INVITE_PREFIX);
    tx.insert_invite(user_id, &token.hash, &after(INVITE_SECONDS))
        .await?;
    Ok(format!("{INVITE_PAGE}{}", token.full))
}

/// Refuses a change that left no active admin. Called in the transaction
/// of the change, after it: returning the error drops the transaction,
/// which takes the change back.
async fn keep_an_admin(tx: &mut Tx<'_>, was: &UserRow) -> Result<(), ApiError> {
    let was_active_admin = was.role == Role::Admin && was.status == UserStatus::Active;
    if was_active_admin && tx.count_active_admins().await? == 0 {
        return Err(last_admin());
    }
    Ok(())
}

#[utoipa::path(
    get,
    path = "/users",
    tag = "users",
    operation_id = "users_list",
    responses(
        (status = 200, description = "The users the caller may see.", body = super::openapi::UserList),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    authed: Authed,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    require(me, &Action::ListUsers)?;
    let store = &state.store;
    let users = match list_scope(me) {
        Scope::All => store.list_users().await?,
        Scope::Teams {
            team_ids,
            own_user_id,
        } => store.list_users_in_teams(&team_ids, own_user_id).await?,
        Scope::Own { user_id } => store.list_users_in_teams(&[], user_id).await?,
    };
    // One query for the teams of everyone listed.
    let ids: Vec<i64> = users.iter().map(|u| u.id).collect();
    let mut teams = store.teams_of_users(&ids).await?;
    let mut attached = guardrail_ids_of_users(store).await?;
    let users: Vec<UserView> = users
        .into_iter()
        .map(|u| {
            let teams = teams_visible_to(me, u.id, teams.remove(&u.id).unwrap_or_default());
            let guardrail_ids = attached.remove(&u.id).unwrap_or_default();
            UserView::new(u, teams).with_guardrail_ids(guardrail_ids)
        })
        .collect();
    Ok(Json(json!({ "users": users })).into_response())
}

#[utoipa::path(
    post,
    path = "/users",
    tag = "users",
    operation_id = "users_invite",
    request_body = InviteRequest,
    responses(
        (status = 201, description = "The invited user and the invite link.", body = super::openapi::InviteResponse),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 409, description = "`user_exists`: the email is taken.", body = super::openapi::ApiErrorBody),
        (status = 413, description = "The request body is too large.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "Some fields are not valid; `fields` names each of them.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn invite(
    State(state): State<Arc<AppState>>,
    authed: Authed,
    ApiJson(req): ApiJson<InviteRequest>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    let role = Role::parse(&req.role);
    // A role that does not exist is judged as the one that needs the most.
    require(
        me,
        &Action::InviteUser {
            role: role.unwrap_or(Role::Admin),
        },
    )?;

    let mut fields = BTreeMap::new();
    let email = normalize_email(&req.email)
        .map_err(|m| fields.insert("email".to_string(), m.to_string()))
        .ok();
    let name = trimmed_name(&req.name)
        .map_err(|m| fields.insert("name".to_string(), m.to_string()))
        .ok();
    if role.is_none() {
        fields.insert("role".to_string(), "role must be admin or member".into());
    }
    let (Some(email), Some(name), Some(role)) = (email, name, role) else {
        return Err(ApiError::validation(fields));
    };

    let store = &state.store;
    let mut tx = store.begin().await?;
    let inserted = tx
        .insert_user(NewUser {
            email: &email,
            name,
            role,
            status: UserStatus::Invited,
            password_hash: None,
        })
        .await;
    let id = match inserted {
        Ok(id) => id,
        Err(e) => {
            return Err(match e.downcast_ref::<StoreError>() {
                Some(StoreError::Duplicate) => {
                    ApiError::conflict("user_exists", "A user with this email already exists.")
                }
                None => e.into(),
            })
        }
    };
    let invite_link = new_invite(&mut tx, id).await?;
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "user.invite",
        target_type: "user",
        target_id: Some(id),
        summary: &format!("Invited {email} as {}", role.as_str()),
    })
    .await?;
    tx.commit().await?;

    let user = store
        .user_by_id(id)
        .await?
        .ok_or_else(|| anyhow!("the invited user is missing"))?;
    let body = json!({ "user": user_view_for(store, me, user).await?, "invite_link": invite_link });
    Ok((StatusCode::CREATED, Json(body)).into_response())
}

#[utoipa::path(
    post,
    path = "/users/{id}/invite",
    tag = "users",
    operation_id = "users_reinvite",
    params(
        ("id" = i64, Path, description = "The id of the user."),
    ),
    responses(
        (status = 201, description = "A new invite link. Earlier links stop working.", body = super::openapi::ReinviteResponse),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "It does not exist, or it is hidden from the caller.", body = super::openapi::ApiErrorBody),
        (status = 409, description = "`not_invited`: the user has accepted an invite already.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn reinvite(
    State(state): State<Arc<AppState>>,
    Path(raw_id): Path<String>,
    authed: Authed,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    let store = &state.store;
    // Whether the caller may invite at all does not depend on the target,
    // so it is settled before anything is said about the id.
    require(me, &Action::InviteUser { role: Role::Member })?;
    let target = user_of(store, &raw_id).await?;
    require(me, &Action::InviteUser { role: target.role })?;

    let mut tx = store.begin().await?;
    // Read again: the invite may have been accepted in the meantime.
    let user = tx
        .user_by_id(target.id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if user.status != UserStatus::Invited {
        return Err(ApiError::conflict(
            "not_invited",
            "Only a user who has not accepted an invite can get a new one.",
        ));
    }
    tx.delete_invites_of(user.id).await?;
    let invite_link = new_invite(&mut tx, user.id).await?;
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "user.reinvite",
        target_type: "user",
        target_id: Some(user.id),
        summary: &format!("Sent a new invite to {}", user.email),
    })
    .await?;
    tx.commit().await?;
    let body = json!({ "invite_link": invite_link });
    Ok((StatusCode::CREATED, Json(body)).into_response())
}

#[utoipa::path(
    post,
    path = "/users/{id}/password-link",
    tag = "users",
    operation_id = "users_password_link",
    summary = "Make a link that lets a user of single sign-on set a password",
    description = "For an active user who signs in through the identity provider and has no password. The link works once and for 24 hours; a new one ends the earlier one. Setting the password keeps the user's single sign-on link and role. Admins only.",
    params(
        ("id" = i64, Path, description = "The id of the user."),
    ),
    responses(
        (status = 201, description = "The link, shown once.", body = super::openapi::PasswordLinkResponse),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "It does not exist.", body = super::openapi::ApiErrorBody),
        (status = 409, description = "`not_sso_user`: the user signs in with a password. `has_password`: the user has one. `not_active`: the user is not active.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn password_link(
    State(state): State<Arc<AppState>>,
    Path(raw_id): Path<String>,
    authed: Authed,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    let store = &state.store;
    // As for a new invite: settled before anything is said about the id.
    require(me, &Action::InviteUser { role: Role::Member })?;
    let target = user_of(store, &raw_id).await?;
    require(me, &Action::InviteUser { role: target.role })?;

    let mut tx = store.begin_immediate().await?;
    // Read again: the user may have changed in the meantime.
    let user = tx
        .user_by_id(target.id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if user.external_id.is_none() {
        return Err(ApiError::conflict(
            "not_sso_user",
            "This user signs in with a password. They can change it in their account.",
        ));
    }
    if user.password_hash.is_some() {
        return Err(ApiError::conflict(
            "has_password",
            "This user has a password already.",
        ));
    }
    if user.status != UserStatus::Active {
        return Err(ApiError::conflict(
            "not_active",
            "Only an active user can get a password link.",
        ));
    }
    // One link at a time: a new one ends the earlier.
    tx.delete_invites_of(user.id).await?;
    let token = generate_secret(PASSWORD_LINK_PREFIX);
    let expires_at = after(PASSWORD_LINK_SECONDS);
    tx.insert_invite_of_kind(user.id, &token.hash, &expires_at, "set_password")
        .await?;
    // The issuance is recorded, never the token or the link.
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "user.password_link",
        target_type: "user",
        target_id: Some(user.id),
        summary: &format!("Made a password link for {}", user.email),
    })
    .await?;
    tx.commit().await?;
    let body = json!({ "url": format!("{INVITE_PAGE}{}", token.full), "expires_at": expires_at });
    Ok((StatusCode::CREATED, Json(body)).into_response())
}

#[utoipa::path(
    get,
    path = "/users/{id}",
    tag = "users",
    operation_id = "users_view",
    params(
        ("id" = i64, Path, description = "The id of the user."),
    ),
    responses(
        (status = 200, description = "The user.", body = UserView),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "It does not exist, or it is hidden from the caller.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn view(
    State(state): State<Arc<AppState>>,
    Path(raw_id): Path<String>,
    authed: Authed,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    let store = &state.store;
    let target = user_of(store, &raw_id).await?;
    let shares_led_team = store.shares_led_team(me.user_id, target.id).await?;
    require(
        me,
        &Action::ViewUser {
            user_id: target.id,
            shares_led_team,
        },
    )?;
    Ok(Json(user_view_for(store, me, target).await?).into_response())
}

#[utoipa::path(
    put,
    path = "/users/{id}/guardrails",
    tag = "users",
    operation_id = "users_set_guardrails",
    params(
        ("id" = i64, Path, description = "The id of the user."),
    ),
    request_body = super::guardrails::AttachRequest,
    responses(
        (status = 200, description = "The guardrails of the user after the change.", body = super::guardrails::Attached),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "It does not exist.", body = super::openapi::ApiErrorBody),
        (status = 413, description = "The request body is too large.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "Some fields are not valid; `fields` names each of them.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
/// Replaces the guardrails applied to every key the user owns. Admins only.
pub async fn set_guardrails(
    State(state): State<Arc<AppState>>,
    Path(raw_id): Path<String>,
    authed: Authed,
    ApiJson(req): ApiJson<super::guardrails::AttachRequest>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    require(me, &Action::ManageGuardrails)?;
    let id = path_id(&raw_id)?;
    super::guardrails::set_attached(
        &state,
        me,
        super::guardrails::Holder::User,
        id,
        &req.guardrail_ids,
    )
    .await
}

#[utoipa::path(
    patch,
    path = "/users/{id}",
    tag = "users",
    operation_id = "users_update",
    params(
        ("id" = i64, Path, description = "The id of the user."),
    ),
    request_body = UpdateRequest,
    responses(
        (status = 200, description = "The user after the change.", body = UserView),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "It does not exist, or it is hidden from the caller.", body = super::openapi::ApiErrorBody),
        (status = 409, description = "`last_admin`: no active admin would be left. `no_password`: the user has no password yet.", body = super::openapi::ApiErrorBody),
        (status = 413, description = "The request body is too large.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "Some fields are not valid; `fields` names each of them.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn update(
    State(state): State<Arc<AppState>>,
    Path(raw_id): Path<String>,
    authed: Authed,
    ApiJson(req): ApiJson<UpdateRequest>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    let store = &state.store;
    let target = user_of(store, &raw_id).await?;
    // Asking for a role or a status counts, even for the value they have.
    require(
        me,
        &Action::UpdateUser {
            user_id: target.id,
            changes_role_or_status: req.role.is_some() || req.status.is_some(),
        },
    )?;
    if req.name.is_none() && req.role.is_none() && req.status.is_none() {
        return Err(ApiError::bad_request(
            "Send at least one of name, role and status.",
        ));
    }

    let mut fields = BTreeMap::new();
    let name = match req.name.as_deref().map(trimmed_name) {
        Some(Err(m)) => {
            fields.insert("name".to_string(), m.to_string());
            None
        }
        Some(Ok(name)) => Some(name),
        None => None,
    };
    let role = match req.role.as_deref().map(Role::parse) {
        Some(None) => {
            fields.insert("role".to_string(), "role must be admin or member".into());
            None
        }
        Some(role) => role,
        None => None,
    };
    let status = match req.status.as_deref().map(UserStatus::parse) {
        Some(Some(UserStatus::Invited) | None) => {
            let message = "status must be active or disabled";
            fields.insert("status".to_string(), message.into());
            None
        }
        Some(status) => status,
        None => None,
    };
    if !fields.is_empty() {
        return Err(ApiError::validation(fields));
    }

    // The last-admin check reads after the write: take the write lock first, so two
    // changes at once cannot each see the other admin still there.
    let mut tx = store.begin_immediate().await?;
    // What is compared, checked and recorded is what the transaction sees.
    let was = tx
        .user_by_id(target.id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let name = name.filter(|name| *name != was.name);
    let role = role.filter(|role| *role != was.role);
    let status = status.filter(|status| *status != was.status);
    // Only a user who signs in with a password needs one to be active: a
    // user of the identity provider never has one.
    if status == Some(UserStatus::Active)
        && was.password_hash.is_none()
        && was.external_id.is_none()
    {
        return Err(ApiError::conflict(
            "no_password",
            "This user has no password yet. Send them an invite instead.",
        ));
    }

    // Each change as (field, old value, new value).
    let mut changes: Vec<(&str, &str, &str)> = Vec::new();
    if let Some(name) = name {
        tx.set_user_name(was.id, name).await?;
        changes.push(("name", &was.name, name));
    }
    if let Some(role) = role {
        tx.set_user_role(was.id, role).await?;
        changes.push(("role", was.role.as_str(), role.as_str()));
    }
    if let Some(status) = status {
        tx.set_user_status(was.id, status).await?;
        changes.push(("status", was.status.as_str(), status.as_str()));
    }
    if changes.is_empty() {
        // Nothing to change, so nothing to record.
        drop(tx);
        // An earlier call may have committed this status and failed to
        // refresh.
        if req.status.is_some() || req.role.is_some() {
            refresh_snapshot(&state).await?;
        }
        return Ok(Json(user_view_for(store, me, was).await?).into_response());
    }
    keep_an_admin(&mut tx, &was).await?;

    let disabled = status == Some(UserStatus::Disabled);
    if role.is_some() || disabled {
        tx.delete_sessions_of(was.id).await?;
    }
    if disabled {
        tx.revoke_tokens_of(was.id).await?;
        // An invite that was never accepted must not outlive the account.
        tx.delete_invites_of(was.id).await?;
    }
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "user.update",
        target_type: "user",
        target_id: Some(was.id),
        summary: &update_summary(&was.email, &changes),
    })
    .await?;
    tx.commit().await?;
    // A change of status or of role alters which keys work and what they may
    // call.
    if status.is_some() || role.is_some() {
        refresh_snapshot(&state).await?;
    }

    let user = store
        .user_by_id(was.id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(user_view_for(store, me, user).await?).into_response())
}

/// For example `Changed role of lena@example.com from member to admin`.
/// Further changes follow after commas, without the email.
fn update_summary(email: &str, changes: &[(&str, &str, &str)]) -> String {
    let parts: Vec<String> = changes
        .iter()
        .enumerate()
        .map(|(i, (field, old, new))| {
            if i == 0 {
                format!("{field} of {email} from {old} to {new}")
            } else {
                format!("{field} from {old} to {new}")
            }
        })
        .collect();
    format!("Changed {}", parts.join(", "))
}

/// What deleting a user did to the keys they owned.
#[derive(Clone, Copy)]
enum KeysOnDelete {
    /// The user was not active: their unrevoked keys were revoked.
    Revoked(u64),
    /// The user was active: their team keys were revoked (the first), the
    /// others go on working, ownerless (the second).
    LeftWorking(u64, i64),
}

fn delete_summary(email: &str, keys: KeysOnDelete) -> String {
    let count = |n: u64, one: &str, many: &str| match n {
        1 => format!("1 {one}"),
        n => format!("{n} {many}"),
    };
    let mut parts = vec![format!("Deleted user {email}")];
    match keys {
        KeysOnDelete::Revoked(0) => {}
        KeysOnDelete::Revoked(n) => parts.push(format!("revoked {}", count(n, "key", "keys"))),
        KeysOnDelete::LeftWorking(team, left) => {
            if team > 0 {
                parts.push(format!("revoked {}", count(team, "team key", "team keys")));
            }
            let left = u64::try_from(left).unwrap_or(0);
            if left > 0 {
                parts.push(format!(
                    "left {} working without an owner",
                    count(left, "key", "keys")
                ));
            }
        }
    }
    parts.join(", ")
}

#[utoipa::path(
    delete,
    path = "/users/{id}",
    tag = "users",
    operation_id = "users_delete",
    params(
        ("id" = i64, Path, description = "The id of the user."),
    ),
    responses(
        (status = 204, description = "The user is deleted."),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "It does not exist, or it is hidden from the caller.", body = super::openapi::ApiErrorBody),
        (status = 409, description = "`cannot_delete_self`, or `last_admin`: no active admin would be left.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn delete(
    State(state): State<Arc<AppState>>,
    Path(raw_id): Path<String>,
    authed: Authed,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    let store = &state.store;
    let target = user_of(store, &raw_id).await?;
    require(me, &Action::DeleteUser { user_id: target.id })?;
    if target.id == me.user_id {
        return Err(ApiError::conflict(
            "cannot_delete_self",
            "You cannot delete your own account.",
        ));
    }

    // The last-admin check reads after the write: take the write lock first, so two
    // changes at once cannot each see the other admin still there.
    let mut tx = store.begin_immediate().await?;
    let was = tx
        .user_by_id(target.id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    tx.delete_sessions_of(was.id).await?;
    tx.revoke_tokens_of(was.id).await?;
    // Deleting the user makes their keys ownerless. The keys of a user who
    // is not active do not work, and must not start to work that way. The
    // keys of an active user go on working, which the audit entry states,
    // but for their team keys: such a key needs its owner in its team.
    let keys = if was.status == UserStatus::Active {
        let team = tx.revoke_team_keys_of(was.id).await?;
        KeysOnDelete::LeftWorking(team, tx.count_live_keys_of(was.id).await?)
    } else {
        KeysOnDelete::Revoked(tx.revoke_keys_of(was.id).await?)
    };
    if !tx.delete_user(was.id).await? {
        return Err(ApiError::not_found());
    }
    keep_an_admin(&mut tx, &was).await?;
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "user.delete",
        target_type: "user",
        target_id: Some(was.id),
        summary: &delete_summary(&was.email, keys),
    })
    .await?;
    tx.commit().await?;
    refresh_snapshot(&state).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summaries_state_old_and_new_values() {
        assert_eq!(
            update_summary("lena@example.com", &[("role", "member", "admin")]),
            "Changed role of lena@example.com from member to admin"
        );
        assert_eq!(
            update_summary(
                "lena@example.com",
                &[("name", "Lena", "Lena K"), ("status", "active", "disabled")]
            ),
            "Changed name of lena@example.com from Lena to Lena K, status from active to disabled"
        );
    }

    #[test]
    fn delete_summaries_state_what_happened_to_the_keys() {
        let email = "lena@example.com";
        let cases = [
            (KeysOnDelete::Revoked(0), "Deleted user lena@example.com"),
            (
                KeysOnDelete::Revoked(1),
                "Deleted user lena@example.com, revoked 1 key",
            ),
            (
                KeysOnDelete::Revoked(2),
                "Deleted user lena@example.com, revoked 2 keys",
            ),
            (
                KeysOnDelete::LeftWorking(0, 0),
                "Deleted user lena@example.com",
            ),
            (
                KeysOnDelete::LeftWorking(0, 1),
                "Deleted user lena@example.com, left 1 key working without an owner",
            ),
            (
                KeysOnDelete::LeftWorking(0, 2),
                "Deleted user lena@example.com, left 2 keys working without an owner",
            ),
            (
                KeysOnDelete::LeftWorking(1, 0),
                "Deleted user lena@example.com, revoked 1 team key",
            ),
            (
                KeysOnDelete::LeftWorking(2, 1),
                "Deleted user lena@example.com, revoked 2 team keys, left 1 key working without an owner",
            ),
        ];
        for (keys, expected) in cases {
            assert_eq!(delete_summary(email, keys), expected);
        }
    }

    #[test]
    fn invites_last_seven_days() {
        assert_eq!(INVITE_SECONDS, 604_800);
    }

    #[test]
    fn password_links_last_a_day() {
        assert_eq!(PASSWORD_LINK_SECONDS, 86_400);
    }
}
