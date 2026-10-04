//! Virtual keys.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use anyhow::anyhow;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{name_and_expiry, path_id, refresh_snapshot, require, ApiError, ApiJson, Authed};
use crate::access::Viewer;
use crate::app::AppState;
use crate::identity::policy::{self, list_scope, Action, Scope};
use crate::identity::{Principal, UserStatus};
use crate::secrets::generate_key;
use crate::store::{now, AuditEntry, KeyRow, Store};
use crate::tags::{self, Tags};

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateKeyRequest {
    name: String,
    team_id: Option<i64>,
    owner_id: Option<i64>,
    expires_at: Option<String>,
    /// The names the key may call: `provider/model` of a model in the
    /// catalog, or the name of a route. Left out, the key has no allowlist.
    allowed: Option<Vec<String>>,
    /// Tags every call of the key is recorded with, over those the call
    /// sends. At most 20; names of `A-Z a-z 0-9 _ . -`, names and values
    /// of 1 to 64 characters.
    #[schema(value_type = Option<std::collections::BTreeMap<String, String>>)]
    tags: Option<Tags>,
}

/// The new tags of a key.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateKeyRequest {
    /// Replaces all the tags of the key; `{}` removes them. The same limits
    /// as when the key is created.
    #[schema(value_type = std::collections::BTreeMap<String, String>)]
    tags: Tags,
}

/// Most names an allowlist may hold, and the longest of them.
const MAX_ALLOWED: usize = 500;
const MAX_ALLOWED_NAME_BYTES: usize = 300;

/// The allowlist as it is stored: without repeats, in the order given. The
/// error is the message for `fields.allowed`. `team_key` is the team of a
/// key a non-admin makes for another user: such a key calls only what is
/// everyone's or the team's, and may name nothing else.
async fn checked_allowed(
    store: &Store,
    me: &Principal,
    team_key: Option<i64>,
    asked: &[String],
) -> Result<Vec<String>, String> {
    if asked.is_empty() {
        return Err("must name at least one model or route; leave it out for no limit".into());
    }
    if asked.len() > MAX_ALLOWED {
        return Err(format!("must have at most {MAX_ALLOWED} names"));
    }
    let mut names: Vec<String> = Vec::new();
    for name in asked {
        if name.is_empty() || name.len() > MAX_ALLOWED_NAME_BYTES {
            return Err("every name must be 1 to 300 bytes".into());
        }
        if !names.contains(name) {
            names.push(name.clone());
        }
    }
    let failed = |_| "could not be checked".to_string();
    // Admins see every name. Anyone else may name only what they can call
    // or use themselves, and is told the same for a name that is hidden as
    // for one that does not exist.
    let admin = me.is_admin() && team_key.is_none();
    let my_teams = me.team_ids();
    let viewer = match team_key {
        Some(team_id) => Viewer::Team { team_id },
        None => super::models::viewer_of(me, &my_teams),
    };
    let mut grants = super::models::grouped(store.list_model_grants().await.map_err(failed)?);
    let models: HashSet<String> = store
        .list_models()
        .await
        .map_err(failed)?
        .into_iter()
        .filter(|m| {
            let g = grants.remove(&m.id).unwrap_or_default();
            admin || super::models::callable(viewer, m.enabled, &g)
        })
        .map(|m| format!("{}/{}", m.provider_name, m.name))
        .collect();
    let mut route_teams: std::collections::HashMap<i64, Vec<i64>> = Default::default();
    for (route, team) in store.list_route_grants().await.map_err(failed)? {
        route_teams.entry(route).or_default().push(team);
    }
    let mut route_targets: std::collections::HashMap<i64, Vec<String>> = Default::default();
    for t in store.list_route_targets().await.map_err(failed)? {
        route_targets
            .entry(t.route_id)
            .or_default()
            .push(format!("{}/{}", t.provider_name, t.model_name));
    }
    let routes: HashSet<String> = store
        .list_routes()
        .await
        .map_err(failed)?
        .into_iter()
        .filter(|r| {
            let teams = route_teams.remove(&r.id).unwrap_or_default();
            // As on /v1: the route is open to the caller and at least one
            // of its targets is a model they can call.
            admin
                || (super::routes::usable(viewer, r.everyone, &teams)
                    && route_targets
                        .get(&r.id)
                        .is_some_and(|names| names.iter().any(|n| models.contains(n))))
        })
        .map(|r| r.name)
        .collect();
    for name in &names {
        // A route name has no `/`; a model name always has one.
        let known = if name.contains('/') {
            models.contains(name)
        } else {
            routes.contains(name)
        };
        if !known {
            return Err(if admin {
                format!("'{name}' is not a model or route that exists")
            } else if team_key.is_some() {
                format!("'{name}' is not a model or route of everyone or of this team")
            } else {
                format!("'{name}' is not a model or route you can use")
            });
        }
    }
    Ok(names)
}

/// A key as `/api` shows it. It has no field for the key or its hash.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct KeyView {
    pub id: i64,
    pub name: String,
    pub display: String,
    #[schema(required)]
    pub owner_id: Option<i64>,
    #[schema(required)]
    pub owner_email: Option<String>,
    #[schema(required)]
    pub team_id: Option<i64>,
    #[schema(required)]
    pub team_name: Option<String>,
    #[schema(required)]
    pub expires_at: Option<String>,
    #[schema(required)]
    pub revoked_at: Option<String>,
    pub created_at: String,
    /// The models and routes the key may call; `null` is no limit.
    #[schema(required)]
    pub allowed: Option<Vec<String>>,
    /// Added to every call of the key; the key's value wins over the call's
    /// for the same name. Empty when none.
    #[schema(value_type = std::collections::BTreeMap<String, String>)]
    pub tags: Tags,
    /// A non-admin made it for another user: it calls only what everyone
    /// or its team may use, and is revoked when its owner is deleted.
    pub team_only: bool,
    /// `revoked`, `expired`, `suspended` or `active`, the first that
    /// applies. `suspended`: the owner of the key is not active, so the key
    /// does not work until they are. Only an `active` key works.
    #[schema(value_type = String)]
    pub status: &'static str,
}

impl KeyView {
    /// `now` is the current time as the store writes it.
    fn new(k: KeyRow, now: &str) -> Self {
        let status = key_status(
            k.revoked_at.as_deref(),
            k.expires_at.as_deref(),
            k.owner_inactive,
            now,
        );
        Self {
            id: k.id,
            name: k.name,
            display: k.display,
            owner_id: k.user_id,
            owner_email: k.owner_email,
            team_id: k.team_id,
            team_name: k.team_name,
            expires_at: k.expires_at,
            revoked_at: k.revoked_at,
            created_at: k.created_at,
            allowed: k.allowed,
            tags: k.tags,
            team_only: k.team_only,
            status,
        }
    }
}

/// Revoked wins over expired, and both over suspended: they do not end.
/// A key stops working at `expires_at`, not after it, as in the lookup that
/// authenticates `/v1`.
fn key_status(
    revoked_at: Option<&str>,
    expires_at: Option<&str>,
    owner_inactive: bool,
    now: &str,
) -> &'static str {
    if revoked_at.is_some() {
        "revoked"
    } else if expires_at.is_some_and(|at| at <= now) {
        "expired"
    } else if owner_inactive {
        "suspended"
    } else {
        "active"
    }
}

/// The key of a path, or the answer for a key that does not exist.
async fn key_of(store: &Store, raw_id: &str) -> Result<KeyRow, ApiError> {
    let id = path_id(raw_id)?;
    store.key_by_id(id).await?.ok_or_else(ApiError::not_found)
}

#[utoipa::path(
    get,
    path = "/keys",
    tag = "keys",
    operation_id = "keys_list",
    responses(
        (status = 200, description = "The keys the caller may see.", body = super::openapi::KeyList),
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
    require(me, &Action::ListKeys)?;
    let store = &state.store;
    let keys = match list_scope(me) {
        Scope::All => store.list_keys().await?,
        Scope::Teams {
            team_ids,
            own_user_id,
        } => store.list_keys_in_teams(&team_ids, own_user_id).await?,
        Scope::Own { user_id } => store.list_keys_in_teams(&[], user_id).await?,
    };
    let now = now();
    let keys: Vec<KeyView> = keys.into_iter().map(|k| KeyView::new(k, &now)).collect();
    Ok(Json(json!({ "keys": keys })).into_response())
}

#[utoipa::path(
    post,
    path = "/keys",
    tag = "keys",
    operation_id = "keys_create",
    request_body = CreateKeyRequest,
    responses(
        (status = 201, description = "The new key, with the key itself.", body = super::openapi::CreatedKey),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 413, description = "The request body is too large.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "Some fields are not valid; `fields` names each of them.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn create(
    State(state): State<Arc<AppState>>,
    authed: Authed,
    ApiJson(req): ApiJson<CreateKeyRequest>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    let owner_id = req.owner_id.unwrap_or(me.user_id);
    let team_id = req.team_id;
    // Decided on what was asked for, before anything is looked up, so a
    // refusal does not tell whether the team or the user exists.
    require(me, &Action::CreateKey { owner_id, team_id })?;
    let (name, expires_at) = name_and_expiry(&req.name, req.expires_at.as_deref())?;

    let store = &state.store;
    // A key a non-admin makes for another user is a key of its team: the
    // policy has required a team the caller leads.
    let team_key = if policy::key_for_another_is_team_key(me, owner_id) {
        team_id
    } else {
        None
    };
    // Before the transaction: it holds the connection the checks read with.
    let allowed = match &req.allowed {
        Some(asked) => Some(checked_allowed(store, me, team_key, asked).await),
        None => None,
    };
    let mut tx = store.begin().await?;
    let mut fields = BTreeMap::new();
    if let Some(reason) = req.tags.as_ref().and_then(tags::refusal) {
        fields.insert("tags".to_string(), reason.to_string());
    }
    let allowed = match allowed {
        Some(Ok(names)) => Some(names),
        Some(Err(message)) => {
            fields.insert("allowed".to_string(), message);
            None
        }
        None => None,
    };
    let owner = tx
        .user_by_id(owner_id)
        .await?
        .filter(|u| u.status == UserStatus::Active);
    if owner.is_none() {
        fields.insert(
            "owner_id".to_string(),
            "owner must be an active user".into(),
        );
    }
    let team = match team_id {
        Some(id) => {
            let team = tx.team_by_id(id).await?;
            if team.is_none() {
                fields.insert("team_id".to_string(), "team does not exist".into());
            } else if let Some(owner) = &owner {
                let role = tx.member_role(id, owner_id).await?;
                if role.is_none() {
                    let message = "owner is not a member of this team";
                    fields.insert("team_id".to_string(), message.into());
                } else if team_key.is_some() && !policy::may_own_team_key(owner.role, role) {
                    let message = "must be a member of the team, not a lead or an admin";
                    fields.insert("owner_id".to_string(), message.into());
                }
            }
            team
        }
        None => None,
    };
    let Some(owner) = owner.filter(|_| fields.is_empty()) else {
        return Err(ApiError::validation(fields));
    };

    let key = generate_key();
    let id = tx
        .insert_key(
            name,
            &key.hash,
            &key.display,
            expires_at,
            Some(owner.id),
            team.as_ref().map(|t| t.id),
        )
        .await?;
    tx.set_key_origin(id, me.user_id, team_key.is_some())
        .await?;
    if let Some(names) = &allowed {
        tx.set_key_allowed(id, Some(names)).await?;
    }
    if let Some(key_tags) = &req.tags {
        tx.set_key_tags(id, key_tags).await?;
    }
    let summary = match &team {
        Some(team) => format!(
            "Created key {name} ({}) for {} in team {}",
            key.display, owner.email, team.name
        ),
        None => format!("Created key {name} ({}) for {}", key.display, owner.email),
    };
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "key.create",
        target_type: "key",
        target_id: Some(id),
        summary: &summary,
    })
    .await?;
    tx.commit().await?;
    refresh_snapshot(&state).await?;

    let row = store
        .key_by_id(id)
        .await?
        .ok_or_else(|| anyhow!("the key is missing after it was created"))?;
    // The only time the key itself is sent.
    let body = json!({ "key": KeyView::new(row, &now()), "secret": key.full });
    Ok((StatusCode::CREATED, Json(body)).into_response())
}

#[utoipa::path(
    get,
    path = "/keys/{id}",
    tag = "keys",
    operation_id = "keys_view",
    params(
        ("id" = i64, Path, description = "The id of the key."),
    ),
    responses(
        (status = 200, description = "The key.", body = KeyView),
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
    let key = key_of(&state.store, &raw_id).await?;
    require(
        &authed.principal,
        &Action::ViewKey {
            owner_id: key.user_id,
            team_id: key.team_id,
        },
    )?;
    Ok(Json(KeyView::new(key, &now())).into_response())
}

#[utoipa::path(
    patch,
    path = "/keys/{id}",
    tag = "keys",
    operation_id = "keys_update",
    params(
        ("id" = i64, Path, description = "The id of the key."),
    ),
    request_body = UpdateKeyRequest,
    responses(
        (status = 200, description = "The key with its new tags.", body = KeyView),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not an admin, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "It does not exist, or it is hidden from the caller.", body = super::openapi::ApiErrorBody),
        (status = 413, description = "The request body is too large.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "Some fields are not valid; `fields` names each of them.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
/// Replaces the tags of a key. Admins only: the key's tags win over a call's.
pub async fn update(
    State(state): State<Arc<AppState>>,
    Path(raw_id): Path<String>,
    authed: Authed,
    ApiJson(req): ApiJson<UpdateKeyRequest>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    let store = &state.store;
    let key = key_of(store, &raw_id).await?;
    require(
        me,
        &Action::EditKeyTags {
            owner_id: key.user_id,
            team_id: key.team_id,
        },
    )?;
    if let Some(reason) = tags::refusal(&req.tags) {
        return Err(ApiError::invalid_field("tags", reason));
    }

    let mut tx = store.begin().await?;
    tx.set_key_tags(key.id, &req.tags).await?;
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "key.tags",
        target_type: "key",
        target_id: Some(key.id),
        summary: &format!("Changed the tags of key {} ({})", key.name, key.display),
    })
    .await?;
    tx.commit().await?;
    refresh_snapshot(&state).await?;
    let row = store
        .key_by_id(key.id)
        .await?
        .ok_or_else(|| anyhow!("the key is missing after its tags were changed"))?;
    Ok(Json(KeyView::new(row, &now())).into_response())
}

#[utoipa::path(
    delete,
    path = "/keys/{id}",
    tag = "keys",
    operation_id = "keys_revoke",
    params(
        ("id" = i64, Path, description = "The id of the key."),
    ),
    responses(
        (status = 204, description = "The key is revoked."),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "It does not exist, or it is hidden from the caller.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn revoke(
    State(state): State<Arc<AppState>>,
    Path(raw_id): Path<String>,
    authed: Authed,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    let store = &state.store;
    let key = key_of(store, &raw_id).await?;
    require(
        me,
        &Action::RevokeKey {
            owner_id: key.user_id,
            team_id: key.team_id,
        },
    )?;

    let mut tx = store.begin().await?;
    if !tx.revoke_key(key.id).await? {
        // Already revoked: nothing changed, so nothing to record.
        drop(tx);
        // An earlier call may have revoked it and failed to refresh.
        refresh_snapshot(&state).await?;
        return Ok(StatusCode::NO_CONTENT.into_response());
    }
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "key.revoke",
        target_type: "key",
        target_id: Some(key.id),
        summary: &format!("Revoked key {} ({})", key.name, key.display),
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
    fn status_prefers_revoked() {
        let now = "2026-01-01 00:00:00";
        let past = Some("2025-01-01 00:00:00");
        let future = Some("2027-01-01 00:00:00");
        for owner_inactive in [false, true] {
            assert_eq!(key_status(None, past, owner_inactive, now), "expired");
            assert_eq!(key_status(None, Some(now), owner_inactive, now), "expired");
            assert_eq!(key_status(past, None, owner_inactive, now), "revoked");
            assert_eq!(key_status(past, past, owner_inactive, now), "revoked");
            assert_eq!(key_status(past, future, owner_inactive, now), "revoked");
        }
        assert_eq!(key_status(None, None, false, now), "active");
        assert_eq!(key_status(None, future, false, now), "active");
        assert_eq!(key_status(None, None, true, now), "suspended");
        assert_eq!(key_status(None, future, true, now), "suspended");
    }
}
