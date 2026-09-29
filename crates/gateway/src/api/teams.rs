//! Teams and their members.

use std::sync::Arc;

use anyhow::anyhow;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::json;

use super::{path_id, require, ApiError, ApiJson, Authed};
use crate::app::AppState;
use crate::identity::policy::{list_scope, Action, Scope};
use crate::identity::{TeamRole, UserStatus};
use crate::store::{AuditEntry, Store, StoreError, TeamRow, TeamSummary};

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

fn team_name(raw: &str) -> Result<&str, ApiError> {
    let name = raw.trim();
    let chars = name.chars().count();
    if chars == 0 || chars > MAX_TEAM_NAME_CHARS || name.chars().any(char::is_control) {
        return Err(ApiError::invalid_field(
            "name",
            "name must be 1 to 60 characters",
        ));
    }
    Ok(name)
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
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[utoipa::path(
    put,
    path = "/teams/{id}/members/{user_id}",
    tag = "teams",
    params(
        ("id" = i64, Path, description = "The id of the team."),
        ("user_id" = i64, Path, description = "The id of the user."),
    ),
    request_body = MemberRequest,
    responses(
        (status = 204, description = "The user is a member of the team with this role."),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
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
    let role = TeamRole::parse(&req.role);
    // A role that does not exist is judged as the one that needs the most,
    // so only a caller who may set any role learns that it is not valid.
    require(
        me,
        &Action::PutMember {
            team_id: team.id,
            role: role.unwrap_or(TeamRole::Lead),
        },
    )?;
    let user_id = path_id(&raw_user)?;
    let role =
        role.ok_or_else(|| ApiError::invalid_field("role", "role must be lead or member"))?;

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
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[utoipa::path(
    delete,
    path = "/teams/{id}/members/{user_id}",
    tag = "teams",
    params(
        ("id" = i64, Path, description = "The id of the team."),
        ("user_id" = i64, Path, description = "The id of the user."),
    ),
    responses(
        (status = 204, description = "The user is no longer a member of the team."),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
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
    require(me, &Action::RemoveMember { team_id: team.id })?;
    let user_id = path_id(&raw_user)?;

    let mut tx = store.begin().await?;
    let team = tx
        .team_by_id(team.id)
        .await?
        .ok_or_else(ApiError::not_found)?;
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
