//! Teams and their members.

use std::sync::Arc;

use anyhow::anyhow;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::json;

use super::{path_id, refresh_snapshot, require, ApiError, ApiJson, Authed};
use crate::app::AppState;
use crate::identity::policy::{list_scope, Action, Scope};
use crate::identity::{normalize_email, TeamRole, UserStatus};
use crate::store::{AuditEntry, MemberDetail, Store, StoreError, TeamRow, TeamSummary};

/// Longest accepted team name, in characters.
const MAX_TEAM_NAME_CHARS: usize = 60;

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TeamNameRequest {
    name: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MemberRequest {
    role: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AddMemberRequest {
    email: String,
}

/// A team name, trimmed: 1 to 60 characters, none of them a control one.
pub(crate) fn valid_team_name(raw: &str) -> Result<&str, &'static str> {
    let name = raw.trim();
    let chars = name.chars().count();
    if chars == 0 || chars > MAX_TEAM_NAME_CHARS || name.chars().any(char::is_control) {
        return Err("name must be 1 to 60 characters");
    }
    Ok(name)
}

fn team_name(raw: &str) -> Result<&str, ApiError> {
    valid_team_name(raw).map_err(|message| ApiError::invalid_field("name", message))
}

fn team_exists() -> ApiError {
    ApiError::conflict("team_exists", "A team with this name already exists.")
}

/// A taken name is the caller's mistake; anything else is ours.
fn name_error(e: anyhow::Error) -> ApiError {
    match e.downcast_ref::<StoreError>() {
        Some(StoreError::Duplicate) => team_exists(),
        None => e.into(),
    }
}

/// The team of a path, or the answer for a team that does not exist.
async fn team_of(store: &Store, raw_id: &str) -> Result<TeamRow, ApiError> {
    let id = path_id(raw_id)?;
    store.team_by_id(id).await?.ok_or_else(ApiError::not_found)
}

async fn summary_of(store: &Store, id: i64) -> Result<TeamSummary, ApiError> {
    Ok(store
        .team_summary(id)
        .await?
        .ok_or_else(|| anyhow!("the team is missing after the change"))?)
}

#[utoipa::path(
    get,
    path = "/teams",
    tag = "teams",
    operation_id = "teams_list",
    responses(
        (status = 200, description = "The teams the caller may see.", body = super::openapi::TeamList),
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
    require(me, &Action::ListTeams)?;
    let teams = match list_scope(me) {
        Scope::All => state.store.list_team_summaries().await?,
        // Teams are listed by membership, in any role.
        Scope::Teams { own_user_id, .. } => state.store.list_team_summaries_of(own_user_id).await?,
        Scope::Own { user_id } => state.store.list_team_summaries_of(user_id).await?,
    };
    Ok(Json(json!({ "teams": teams })).into_response())
}

#[utoipa::path(
    post,
    path = "/teams",
    tag = "teams",
    operation_id = "teams_create",
    request_body = TeamNameRequest,
    responses(
        (status = 201, description = "The new team.", body = TeamSummary),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 409, description = "`team_exists`: the name is taken.", body = super::openapi::ApiErrorBody),
        (status = 413, description = "The request body is too large.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "Some fields are not valid; `fields` names each of them.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn create(
    State(state): State<Arc<AppState>>,
    authed: Authed,
    ApiJson(req): ApiJson<TeamNameRequest>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    require(me, &Action::CreateTeam)?;
    let name = team_name(&req.name)?;

    let mut tx = state.store.begin().await?;
    let id = tx.insert_team(name).await.map_err(name_error)?;
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "team.create",
        target_type: "team",
        target_id: Some(id),
        summary: &format!("Created team {name}"),
    })
    .await?;
    tx.commit().await?;

    let team = summary_of(&state.store, id).await?;
    Ok((StatusCode::CREATED, Json(team)).into_response())
}

#[utoipa::path(
    get,
    path = "/teams/{id}",
    tag = "teams",
    operation_id = "teams_view",
    params(
        ("id" = i64, Path, description = "The id of the team."),
    ),
    responses(
        (status = 200, description = "The team and its members.", body = super::openapi::TeamDetail),
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
    let store = &state.store;
    let team = team_of(store, &raw_id).await?;
    require(&authed.principal, &Action::ViewTeam { team_id: team.id })?;
    let summary = store
        .team_summary(team.id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let members = store.member_details(team.id).await?;
    Ok(Json(json!({ "team": summary, "members": members })).into_response())
}

#[utoipa::path(
    patch,
    path = "/teams/{id}",
    tag = "teams",
    operation_id = "teams_rename",
    params(
        ("id" = i64, Path, description = "The id of the team."),
    ),
    request_body = TeamNameRequest,
    responses(
        (status = 200, description = "The team after the change.", body = TeamSummary),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "It does not exist, or it is hidden from the caller.", body = super::openapi::ApiErrorBody),
        (status = 409, description = "`team_exists`: the name is taken.", body = super::openapi::ApiErrorBody),
        (status = 413, description = "The request body is too large.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "Some fields are not valid; `fields` names each of them.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn rename(
    State(state): State<Arc<AppState>>,
    Path(raw_id): Path<String>,
    authed: Authed,
    ApiJson(req): ApiJson<TeamNameRequest>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    let store = &state.store;
    let team = team_of(store, &raw_id).await?;
    require(me, &Action::RenameTeam { team_id: team.id })?;
    let name = team_name(&req.name)?;

    let mut tx = store.begin().await?;
    // Read again: the name in the summary must be the one replaced.
    let current = tx
        .team_by_id(team.id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if current.name != name {
        if !tx.rename_team(team.id, name).await.map_err(name_error)? {
            return Err(ApiError::not_found());
        }
        tx.audit(AuditEntry {
            actor_user_id: Some(me.user_id),
            actor_email: &me.email,
            action: "team.rename",
            target_type: "team",
            target_id: Some(team.id),
            summary: &format!("Renamed team {} to {name}", current.name),
        })
        .await?;
        tx.commit().await?;
    } else {
        drop(tx);
    }

    let team = summary_of(store, team.id).await?;
    Ok(Json(team).into_response())
}

#[utoipa::path(
    delete,
    path = "/teams/{id}",
    tag = "teams",
    operation_id = "teams_delete",
    params(
        ("id" = i64, Path, description = "The id of the team."),
    ),
    responses(
        (status = 204, description = "The team is deleted."),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "It does not exist, or it is hidden from the caller.", body = super::openapi::ApiErrorBody),
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
    let team = team_of(store, &raw_id).await?;
    require(me, &Action::DeleteTeam { team_id: team.id })?;

    let mut tx = store.begin().await?;
    let current = tx
        .team_by_id(team.id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if !tx.delete_team(team.id).await? {
        return Err(ApiError::not_found());
    }
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "team.delete",
        target_type: "team",
        target_id: Some(team.id),
        summary: &format!("Deleted team {}", current.name),
    })
    .await?;
    tx.commit().await?;
    // Its keys are detached from it.
    refresh_snapshot(&state).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[utoipa::path(
    post,
    path = "/teams/{id}/members",
    tag = "teams",
    operation_id = "teams_member_add",
    params(
        ("id" = i64, Path, description = "The id of the team."),
    ),
    request_body = AddMemberRequest,
    responses(
        (status = 201, description = "The user is now a member of the team.", body = MemberDetail),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "The team does not exist or is hidden from the caller, or `user_not_found`: no active user has that email.", body = super::openapi::ApiErrorBody),
        (status = 409, description = "`already_member`: the user is in the team already, in any role. Nothing is changed.", body = super::openapi::ApiErrorBody),
        (status = 413, description = "The request body is too large.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "Some fields are not valid; `fields` names each of them.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn add_member(
    State(state): State<Arc<AppState>>,
    Path(raw_team): Path<String>,
    authed: Authed,
    ApiJson(req): ApiJson<AddMemberRequest>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    let store = &state.store;
    let team = team_of(store, &raw_team).await?;
    require(me, &Action::AddMember { team_id: team.id })?;
    let email = normalize_email(&req.email).map_err(|m| ApiError::invalid_field("email", m))?;

    let mut tx = store.begin().await?;
    let team = tx
        .team_by_id(team.id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    // The same answer for a user who does not exist and for one who is not
    // active, so a lead cannot find out which accounts are disabled.
    let user = tx
        .user_by_email(&email)
        .await?
        .filter(|u| u.status == UserStatus::Active)
        .ok_or_else(|| {
            ApiError::not_found_with("user_not_found", "No active user with that email.")
        })?;
    if tx.member_role(team.id, user.id).await?.is_some() {
        return Err(ApiError::conflict(
            "already_member",
            "Already in this team.",
        ));
    }
    tx.put_member(team.id, user.id, TeamRole::Member).await?;
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "team.member_add",
        target_type: "team",
        target_id: Some(team.id),
        summary: &format!("Added {} to team {} as member", user.email, team.name),
    })
    .await?;
    tx.commit().await?;
    refresh_snapshot(&state).await?;

    let member = MemberDetail {
        user_id: user.id,
        email: user.email,
        name: user.name,
        role: TeamRole::Member,
    };
    Ok((StatusCode::CREATED, Json(member)).into_response())
}

#[utoipa::path(
    put,
    path = "/teams/{id}/members/{user_id}",
    tag = "teams",
    operation_id = "teams_member_put",
    params(
        ("id" = i64, Path, description = "The id of the team."),
        ("user_id" = i64, Path, description = "The id of the user."),
    ),
    request_body = MemberRequest,
    responses(
        (status = 204, description = "The user is a member of the team with this role."),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not an admin, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "It does not exist, or it is hidden from the caller.", body = super::openapi::ApiErrorBody),
        (status = 409, description = "`user_disabled`: a disabled user cannot be added.", body = super::openapi::ApiErrorBody),
        (status = 413, description = "The request body is too large.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "Some fields are not valid; `fields` names each of them.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn put_member(
    State(state): State<Arc<AppState>>,
    Path((raw_team, raw_user)): Path<(String, String)>,
    authed: Authed,
    ApiJson(req): ApiJson<MemberRequest>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    let store = &state.store;
    let team = team_of(store, &raw_team).await?;
    // Admins only, so only an admin learns that a role is not valid.
    require(me, &Action::PutMember { team_id: team.id })?;
    let user_id = path_id(&raw_user)?;
    let role = TeamRole::parse(&req.role)
        .ok_or_else(|| ApiError::invalid_field("role", "role must be lead or member"))?;

    let mut tx = store.begin().await?;
    let team = tx
        .team_by_id(team.id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let user = tx
        .user_by_id(user_id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if user.status == UserStatus::Disabled {
        return Err(ApiError::conflict(
            "user_disabled",
            "A disabled user cannot be added to a team.",
        ));
    }
    let summary = match tx.member_role(team.id, user.id).await? {
        // Nothing to change, so nothing to record.
        Some(old) if old == role => {
            drop(tx);
            return Ok(StatusCode::NO_CONTENT.into_response());
        }
        Some(old) => format!(
            "Changed role of {} in team {} from {} to {}",
            user.email,
            team.name,
            old.as_str(),
            role.as_str()
        ),
        None => format!(
            "Added {} to team {} as {}",
            user.email,
            team.name,
            role.as_str()
        ),
    };
    tx.put_member(team.id, user.id, role).await?;
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "team.member_put",
        target_type: "team",
        target_id: Some(team.id),
        summary: &summary,
    })
    .await?;
    tx.commit().await?;
    refresh_snapshot(&state).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[utoipa::path(
    delete,
    path = "/teams/{id}/members/{user_id}",
    tag = "teams",
    operation_id = "teams_member_remove",
    params(
        ("id" = i64, Path, description = "The id of the team."),
        ("user_id" = i64, Path, description = "The id of the user."),
    ),
    responses(
        (status = 204, description = "The user is no longer a member of the team."),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this (a lead of the team may remove members and themselves, not another lead), or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "It does not exist, or it is hidden from the caller.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn remove_member(
    State(state): State<Arc<AppState>>,
    Path((raw_team, raw_user)): Path<(String, String)>,
    authed: Authed,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    let store = &state.store;
    let team = team_of(store, &raw_team).await?;
    let user_id = path_id(&raw_user)?;

    // Decided on a plain read first, so a caller who may not do this never
    // takes the write lock; decided again inside the transaction, on what is
    // actually removed.
    let role_now = store
        .memberships_of(user_id)
        .await?
        .into_iter()
        .find(|m| m.team_id == team.id)
        .map(|m| m.role);
    let action = |target_role| Action::RemoveMember {
        team_id: team.id,
        target_user_id: user_id,
        target_role,
    };
    require(me, &action(role_now))?;

    let mut tx = store.begin().await?;
    let team = tx
        .team_by_id(team.id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    require(me, &action(tx.member_role(team.id, user_id).await?))?;
    let user = tx
        .user_by_id(user_id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if !tx.remove_member(team.id, user.id).await? {
        return Err(ApiError::not_found());
    }
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "team.member_remove",
        target_type: "team",
        target_id: Some(team.id),
        summary: &format!("Removed {} from team {}", user.email, team.name),
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
    fn team_names_are_trimmed_and_bounded() {
        assert_eq!(team_name("  Platform ").unwrap(), "Platform");
        assert_eq!(team_name(&"é".repeat(60)).unwrap(), "é".repeat(60));
        for bad in ["", "   ", &"n".repeat(61), "a\nb"] {
            let err = team_name(bad).unwrap_err();
            assert_eq!(err.status, StatusCode::UNPROCESSABLE_ENTITY);
            assert!(err.fields.unwrap().contains_key("name"));
        }
    }
}
