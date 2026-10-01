//! The model catalog: which models exist, which are enabled and who may
//! call them.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use super::{path_id, refresh_snapshot, require, ApiError, ApiJson, Authed};
use crate::app::AppState;
use crate::catalog::sync::{fetch_model_names, SyncError};
use crate::catalog::validate_model_name;
use crate::identity::policy::Action;
use crate::identity::Principal;
use crate::store::{grants_of_rows, AuditEntry, GrantRow, Grants, ModelRow, StoreError};

/// Most names one sync adds. A provider that lists more is cut here.
const MAX_SYNCED_NAMES: usize = 10_000;

/// Who may call a model. For everyone who is not an admin it is always
/// empty.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GrantsView {
    /// Every user may call the model. It cannot be combined with teams or
    /// users.
    pub everyone: bool,
    pub team_ids: Vec<i64>,
    pub user_ids: Vec<i64>,
}

impl GrantsView {
    fn empty() -> Self {
        Self {
            everyone: false,
            team_ids: Vec::new(),
            user_ids: Vec::new(),
        }
    }
}

impl From<Grants> for GrantsView {
    fn from(g: Grants) -> Self {
        Self {
            everyone: g.everyone,
            team_ids: g.team_ids,
            user_ids: g.user_ids,
        }
    }
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ModelView {
    pub id: i64,
    pub provider_id: i64,
    pub provider_name: String,
    /// The provider's own id for the model.
    pub name: String,
    pub enabled: bool,
    pub grants: GrantsView,
    pub created_at: String,
}

impl ModelView {
    fn of(m: ModelRow, grants: GrantsView) -> Self {
        Self {
            id: m.id,
            provider_id: m.provider_id,
            provider_name: m.provider_name,
            name: m.name,
            enabled: m.enabled,
            grants,
            created_at: m.created_at,
        }
    }
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ModelList {
    pub models: Vec<ModelView>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SyncResult {
    /// The names that were new, now in the catalog and disabled.
    pub added: Vec<String>,
    /// How many names the provider listed that were already there.
    pub existing: u32,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateModelRequest {
    provider_id: i64,
    /// The provider's id for the model, 1 to 200 characters, no whitespace.
    name: String,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateModelRequest {
    enabled: bool,
}

/// Whether the principal may call a model granted like this.
fn may_call(p: &Principal, grants: &Grants) -> bool {
    grants.everyone
        || grants.user_ids.contains(&p.user_id)
        || grants
            .team_ids
            .iter()
            .any(|t| p.teams.iter().any(|(id, _)| id == t))
}

fn grouped(rows: Vec<GrantRow>) -> HashMap<i64, Grants> {
    let mut by_model: HashMap<i64, Vec<GrantRow>> = HashMap::new();
    for row in rows {
        by_model.entry(row.model_id).or_default().push(row);
    }
    by_model
        .into_iter()
        .map(|(id, rows)| (id, grants_of_rows(rows.into_iter())))
        .collect()
}

/// The view of one model as this caller sees it.
async fn view_of(state: &AppState, me: &Principal, id: i64) -> Result<ModelView, ApiError> {
    let row = state
        .store
        .model_by_id(id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let grants = if me.is_admin() {
        state.store.grants_of(id).await?.into()
    } else {
        GrantsView::empty()
    };
    Ok(ModelView::of(row, grants))
}

async fn model_exists(state: &AppState, raw_id: &str) -> Result<i64, ApiError> {
    let id = path_id(raw_id)?;
    state
        .store
        .model_by_id(id)
        .await?
        .map(|m| m.id)
        .ok_or_else(ApiError::not_found)
}

#[utoipa::path(
    get,
    path = "/models",
    tag = "models",
    operation_id = "models_list",
    responses(
        (status = 200, description = "An admin gets every model with its grants. Everyone else gets the enabled models they may call, with empty grants.", body = ModelList),
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
    require(me, &Action::ListModels)?;
    let rows = state.store.list_models().await?;
    let mut grants = grouped(state.store.list_model_grants().await?);
    let models: Vec<ModelView> = rows
        .into_iter()
        .filter_map(|m| {
            let g = grants.remove(&m.id).unwrap_or_default();
            if me.is_admin() {
                Some(ModelView::of(m, g.into()))
            } else if m.enabled && may_call(me, &g) {
                Some(ModelView::of(m, GrantsView::empty()))
            } else {
                None
            }
        })
        .collect();
    Ok(Json(ModelList { models }).into_response())
}

#[utoipa::path(
    post,
    path = "/models",
    tag = "models",
    operation_id = "models_create",
    request_body = CreateModelRequest,
    responses(
        (status = 201, description = "The new model. It is disabled and granted to nobody.", body = ModelView),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 409, description = "`model_exists`: the provider already has a model of this name.", body = super::openapi::ApiErrorBody),
        (status = 413, description = "The request body is too large.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "Some fields are not valid; `fields` names each of them.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn create(
    State(state): State<Arc<AppState>>,
    authed: Authed,
    ApiJson(req): ApiJson<CreateModelRequest>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    require(me, &Action::ManageModels)?;

    let mut fields = BTreeMap::new();
    if let Err(message) = validate_model_name(&req.name) {
        fields.insert("name".to_string(), message.to_string());
    }
    let provider = state.store.provider_by_id(req.provider_id).await?;
    if provider.is_none() {
        fields.insert(
            "provider_id".to_string(),
            "provider does not exist".to_string(),
        );
    }
    let Some(provider) = provider.filter(|_| fields.is_empty()) else {
        return Err(ApiError::validation(fields));
    };

    let mut tx = state.store.begin().await?;
    let id = match tx.insert_model(provider.id, &req.name).await {
        Ok(id) => id,
        Err(e) => {
            return Err(match e.downcast_ref::<StoreError>() {
                Some(StoreError::Duplicate) => ApiError::conflict(
                    "model_exists",
                    "This provider already has a model of this name.",
                ),
                None => e.into(),
            })
        }
    };
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "model.create",
        target_type: "model",
        target_id: Some(id),
        summary: &format!("Created model {} of {}", req.name, provider.name),
    })
    .await?;
    tx.commit().await?;
    refresh_snapshot(&state).await?;
    let view = view_of(&state, me, id).await?;
    Ok((StatusCode::CREATED, Json(view)).into_response())
}

#[utoipa::path(
    patch,
    path = "/models/{id}",
    tag = "models",
    operation_id = "models_update",
    params(
        ("id" = i64, Path, description = "The id of the model."),
    ),
    request_body = UpdateModelRequest,
    responses(
        (status = 200, description = "The model after the change.", body = ModelView),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "It does not exist, or it is hidden from the caller.", body = super::openapi::ApiErrorBody),
        (status = 413, description = "The request body is too large.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn update(
    State(state): State<Arc<AppState>>,
    Path(raw_id): Path<String>,
    authed: Authed,
    ApiJson(req): ApiJson<UpdateModelRequest>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    // The answer does not depend on the model, so it comes before the id
    // is looked at.
    require(me, &Action::ManageModels)?;
    let id = model_exists(&state, &raw_id).await?;

    let mut tx = state.store.begin().await?;
    let model = tx.model_by_id(id).await?.ok_or_else(ApiError::not_found)?;
    if !tx.set_model_enabled(id, req.enabled).await? {
        return Err(ApiError::not_found());
    }
    let verb = if req.enabled { "Enabled" } else { "Disabled" };
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "model.update",
        target_type: "model",
        target_id: Some(id),
        summary: &format!("{verb} {}", model.name),
    })
    .await?;
    tx.commit().await?;
    refresh_snapshot(&state).await?;
    Ok(Json(view_of(&state, me, id).await?).into_response())
}

#[utoipa::path(
    put,
    path = "/models/{id}/grants",
    tag = "models",
    operation_id = "models_grants_put",
    params(
        ("id" = i64, Path, description = "The id of the model."),
    ),
    request_body = GrantsView,
    responses(
        (status = 200, description = "The model with its new grants. They replace the old ones.", body = ModelView),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "It does not exist, or it is hidden from the caller.", body = super::openapi::ApiErrorBody),
        (status = 413, description = "The request body is too large.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "A team or user does not exist, or everyone is combined with others; `fields` names each of them.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn put_grants(
    State(state): State<Arc<AppState>>,
    Path(raw_id): Path<String>,
    authed: Authed,
    ApiJson(req): ApiJson<GrantsView>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    require(me, &Action::ManageModels)?;
    let id = model_exists(&state, &raw_id).await?;

    let distinct = |ids: &[i64]| {
        let mut seen = HashSet::new();
        ids.iter()
            .copied()
            .filter(|i| seen.insert(*i))
            .collect::<Vec<_>>()
    };
    let grants = Grants {
        everyone: req.everyone,
        team_ids: distinct(&req.team_ids),
        user_ids: distinct(&req.user_ids),
    };

    let mut fields = BTreeMap::new();
    if grants.everyone && !(grants.team_ids.is_empty() && grants.user_ids.is_empty()) {
        fields.insert(
            "everyone".to_string(),
            "must not be combined with teams or users".to_string(),
        );
    }
    let mut tx = state.store.begin().await?;
    let model = tx.model_by_id(id).await?.ok_or_else(ApiError::not_found)?;
    for team_id in &grants.team_ids {
        if tx.team_by_id(*team_id).await?.is_none() {
            fields.insert("team_ids".to_string(), "a team does not exist".to_string());
            break;
        }
    }
    for user_id in &grants.user_ids {
        if tx.user_by_id(*user_id).await?.is_none() {
            fields.insert("user_ids".to_string(), "a user does not exist".to_string());
            break;
        }
    }
    if !fields.is_empty() {
        return Err(ApiError::validation(fields));
    }

    tx.replace_grants(id, &grants).await?;
    let who = if grants.everyone {
        "everyone".to_string()
    } else {
        format!(
            "{} teams and {} users",
            grants.team_ids.len(),
            grants.user_ids.len()
        )
    };
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "model.grants",
        target_type: "model",
        target_id: Some(id),
        summary: &format!("Granted {} to {who}", model.name),
    })
    .await?;
    tx.commit().await?;
    refresh_snapshot(&state).await?;
    Ok(Json(view_of(&state, me, id).await?).into_response())
}

#[utoipa::path(
    delete,
    path = "/models/{id}",
    tag = "models",
    operation_id = "models_delete",
    params(
        ("id" = i64, Path, description = "The id of the model."),
    ),
    responses(
        (status = 204, description = "The model and its grants are deleted."),
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
    require(me, &Action::ManageModels)?;
    let id = match model_exists(&state, &raw_id).await {
        Ok(id) => id,
        Err(e) => {
            // An earlier call may have deleted it and failed to refresh.
            refresh_snapshot(&state).await?;
            return Err(e);
        }
    };

    let mut tx = state.store.begin().await?;
    let model = tx.model_by_id(id).await?.ok_or_else(ApiError::not_found)?;
    if !tx.delete_model(id).await? {
        return Err(ApiError::not_found());
    }
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "model.delete",
        target_type: "model",
        target_id: Some(id),
        summary: &format!("Deleted model {} of {}", model.name, model.provider_name),
    })
    .await?;
    tx.commit().await?;
    refresh_snapshot(&state).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

fn sync_failed() -> ApiError {
    ApiError {
        status: StatusCode::BAD_GATEWAY,
        code: "sync_failed",
        message: "The provider did not return its models.".to_string(),
        fields: None,
    }
}

#[utoipa::path(
    post,
    path = "/providers/{id}/sync",
    tag = "providers",
    operation_id = "providers_sync",
    params(
        ("id" = i64, Path, description = "The id of the provider."),
    ),
    responses(
        (status = 200, description = "The names the provider listed that were new are added, disabled. Nothing is ever removed.", body = SyncResult),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "It does not exist, or it is hidden from the caller.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
        (status = 502, description = "`sync_failed`: the provider did not return its models.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn sync(
    State(state): State<Arc<AppState>>,
    Path(raw_id): Path<String>,
    authed: Authed,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    require(me, &Action::ManageModels)?;
    let id = path_id(&raw_id)?;
    let row = state
        .store
        .provider_by_id(id)
        .await?
        .ok_or_else(ApiError::not_found)?;

    // The snapshot is the one place credentials are decrypted.
    let provider = state
        .snapshot
        .load()
        .provider(&row.name)
        .filter(|p| p.id == row.id)
        .cloned();
    let Some(provider) = provider else {
        tracing::warn!(provider = %row.name, "model sync refused: provider is not usable");
        return Err(sync_failed());
    };
    let listed = match fetch_model_names(&state.http, &provider).await {
        Ok(names) => names,
        Err(e) => {
            let status = match &e {
                SyncError::Provider { status } => Some(*status),
                _ => None,
            };
            tracing::warn!(provider = %row.name, ?status, error = %e, "model sync failed");
            return Err(sync_failed());
        }
    };

    // Names that cannot be catalogued are left out, the rest keep their
    // order and appear once.
    let mut seen = HashSet::new();
    let names: Vec<String> = listed
        .into_iter()
        .filter(|n| validate_model_name(n).is_ok() && seen.insert(n.clone()))
        .take(MAX_SYNCED_NAMES)
        .collect();

    let mut tx = state.store.begin().await?;
    let have = tx.model_names_of(row.id).await?;
    let mut added = Vec::new();
    for name in &names {
        if have.contains(name) {
            continue;
        }
        tx.insert_model(row.id, name).await?;
        added.push(name.clone());
    }
    let existing = u32::try_from(names.len() - added.len()).unwrap_or(u32::MAX);
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "provider.sync",
        target_type: "provider",
        target_id: Some(row.id),
        summary: &format!("Synced {} new models from {}", added.len(), row.name),
    })
    .await?;
    tx.commit().await?;
    refresh_snapshot(&state).await?;
    Ok(Json(SyncResult { added, existing }).into_response())
}
