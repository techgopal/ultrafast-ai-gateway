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

use super::auth::UserView;
use super::{path_id, refresh_snapshot, require, ApiError, ApiJson, Authed};
use crate::app::AppState;
use crate::identity::policy::{list_scope, Action, Scope};
use crate::identity::{normalize_email, Role, UserStatus};
use crate::secrets::{generate_secret, INVITE_PREFIX};
use crate::store::{after, AuditEntry, NewUser, Store, StoreError, Tx, UserRow};

/// Longest accepted user name, in characters.
const MAX_NAME_CHARS: usize = 100;
/// How long an invite link works.
const INVITE_SECONDS: i64 = 7 * 24 * 60 * 60;
/// The page that takes an invite token.
const INVITE_PAGE: &str = "/accept-invite?token=";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InviteRequest {
    email: String,
    name: String,
    role: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateRequest {
    name: Option<String>,
    role: Option<String>,
    status: Option<String>,
}

fn user_name(raw: &str) -> Result<&str, &'static str> {
    let name = raw.trim();
    let chars = name.chars().count();
    if chars == 0 || chars > MAX_NAME_CHARS || name.chars().any(char::is_control) {
        return Err("name must be 1 to 100 characters");
    }
    Ok(name)
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
    let users: Vec<UserView> = users.into_iter().map(UserView::from).collect();
    Ok(Json(json!({ "users": users })).into_response())
}

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
    let name = user_name(&req.name)
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
    let body = json!({ "user": UserView::from(user), "invite_link": invite_link });
    Ok((StatusCode::CREATED, Json(body)).into_response())
}

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
    Ok(Json(UserView::from(target)).into_response())
}

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
    let name = match req.name.as_deref().map(user_name) {
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

    let mut tx = store.begin().await?;
    // What is compared, checked and recorded is what the transaction sees.
    let was = tx
        .user_by_id(target.id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let name = name.filter(|name| *name != was.name);
    let role = role.filter(|role| *role != was.role);
    let status = status.filter(|status| *status != was.status);
    if status == Some(UserStatus::Active) && was.password_hash.is_none() {
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
        if req.status.is_some() {
            refresh_snapshot(&state).await?;
        }
        return Ok(Json(UserView::from(was)).into_response());
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
    // Only a change of status alters which keys work.
    if status.is_some() {
        refresh_snapshot(&state).await?;
    }

    let user = store
        .user_by_id(was.id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(UserView::from(user)).into_response())
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

fn delete_summary(email: &str, revoked_keys: u64) -> String {
    match revoked_keys {
        0 => format!("Deleted user {email}"),
        1 => format!("Deleted user {email}, revoked 1 key"),
        n => format!("Deleted user {email}, revoked {n} keys"),
    }
}

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

    let mut tx = store.begin().await?;
    let was = tx
        .user_by_id(target.id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    tx.delete_sessions_of(was.id).await?;
    tx.revoke_tokens_of(was.id).await?;
    // Deleting the user makes their keys ownerless. The keys of a user who
    // is not active do not work, and must not start to work that way.
    let revoked = if was.status == UserStatus::Active {
        0
    } else {
        tx.revoke_keys_of(was.id).await?
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
        summary: &delete_summary(&was.email, revoked),
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
    fn names_are_trimmed_and_bounded() {
        assert_eq!(user_name("  Lena "), Ok("Lena"));
        assert_eq!(user_name(&"é".repeat(100)), Ok("é".repeat(100).as_str()));
        for bad in ["", "   ", &"n".repeat(101), "a\nb"] {
            assert!(user_name(bad).is_err(), "{bad:?}");
        }
    }

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
    fn invites_last_seven_days() {
        assert_eq!(INVITE_SECONDS, 604_800);
    }
}
