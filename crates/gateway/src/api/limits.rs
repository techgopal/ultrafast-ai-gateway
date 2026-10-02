//! Rate limits: who sees them, and the admin who sets them.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use super::{path_id, refresh_snapshot, require, ApiError, ApiJson, Authed};
use crate::app::AppState;
use crate::identity::policy::{limit_applies_to, Action};
use crate::limits::{LimitScope, RateLimit};
use crate::store::{AuditEntry, LimitRow};

/// The most requests per minute or concurrent requests a limit may allow.
const MAX_COUNT: i64 = 1_000_000;
/// The most tokens per minute a limit may allow.
const MAX_TOKENS: i64 = 1_000_000_000_000;

/// The limits of one team, user, key or of the gateway. A limit that is
/// `null` is not set.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct LimitView {
    pub id: i64,
    /// `gateway`, `team`, `user` or `key`.
    pub scope: String,
    /// The id of the team, user or key; `null` for the gateway.
    #[schema(required)]
    pub scope_id: Option<i64>,
    /// How a refusal names it: `gateway`, `team 'Platform'`,
    /// `user 'lena@example.com'` or `key 'ci'`.
    pub label: String,
    #[schema(required)]
    pub requests_per_minute: Option<u64>,
    #[schema(required)]
    pub tokens_per_minute: Option<u64>,
    #[schema(required)]
    pub concurrent: Option<u64>,
}

impl From<&LimitRow> for LimitView {
    fn from(r: &LimitRow) -> Self {
        Self {
            id: r.id,
            scope: r.scope.as_str().to_string(),
            scope_id: r.scope_id,
            label: r.label(),
            requests_per_minute: r.limit.requests_per_minute,
            tokens_per_minute: r.limit.tokens_per_minute,
            concurrent: r.limit.concurrent,
        }
    }
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct LimitsPage {
    pub limits: Vec<LimitView>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetLimitRequest {
    /// `gateway`, `team`, `user` or `key`.
    scope: String,
    /// The id of the team, user or key. Not sent for the gateway.
    scope_id: Option<i64>,
    /// 1 to 1 000 000. Not sent: no limit.
    requests_per_minute: Option<i64>,
    /// 1 to 1 000 000 000 000. Not sent: no limit.
    tokens_per_minute: Option<i64>,
    /// 1 to 1 000 000. Not sent: no limit.
    concurrent: Option<i64>,
}

/// The limits that apply to the caller: all of them for an admin; for anyone
/// else the gateway's, those of their teams and of themselves, and those of
/// their keys.
#[utoipa::path(
    get,
    path = "/limits",
    tag = "limits",
    operation_id = "limits_list",
    responses(
        (status = 200, description = "The limits that apply to the caller.", body = LimitsPage),
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
    require(me, &Action::ListLimits)?;
    let limits: Vec<LimitView> = state
        .store
        .list_limits()
        .await?
        .iter()
        .filter(|l| {
            l.has_subject() && limit_applies_to(me, l.scope, l.scope_id, l.key_owner, l.key_team)
        })
        .map(LimitView::from)
        .collect();
    Ok(Json(LimitsPage { limits }).into_response())
}

fn field(fields: &mut BTreeMap<String, String>, name: &str, message: &str) {
    fields.insert(name.to_string(), message.to_string());
}

/// A limit that is sent: a whole number from 1 to `max`.
fn checked(
    fields: &mut BTreeMap<String, String>,
    name: &str,
    value: Option<i64>,
    max: i64,
) -> Option<u64> {
    let v = value?;
    if !(1..=max).contains(&v) {
        field(fields, name, &format!("must be from 1 to {max}"));
        return None;
    }
    u64::try_from(v).ok()
}

fn describe(limit: &RateLimit) -> String {
    let mut parts = Vec::new();
    if let Some(n) = limit.requests_per_minute {
        parts.push(format!("{n} requests per minute"));
    }
    if let Some(n) = limit.tokens_per_minute {
        parts.push(format!("{n} tokens per minute"));
    }
    if let Some(n) = limit.concurrent {
        parts.push(format!("{n} concurrent requests"));
    }
    parts.join(", ")
}

/// Sets the limits of a team, a user, a key or the gateway, replacing the
/// ones it had: a limit that is not sent is removed. Admin only. The change
/// reaches `/v1` at once.
#[utoipa::path(
    put,
    path = "/limits",
    tag = "limits",
    operation_id = "limits_set",
    request_body = SetLimitRequest,
    responses(
        (status = 200, description = "The limits of the subject after the change.", body = LimitView),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 413, description = "The request body is too large.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "A value is not valid, or the team, user or key does not exist; `fields` names each of them.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn set(
    State(state): State<Arc<AppState>>,
    authed: Authed,
    ApiJson(req): ApiJson<SetLimitRequest>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    require(me, &Action::ManageLimits)?;

    let mut fields = BTreeMap::new();
    let scope = LimitScope::parse(&req.scope);
    match (scope, req.scope_id) {
        (None, _) => field(&mut fields, "scope", "must be gateway, team, user or key"),
        (Some(LimitScope::Gateway), Some(_)) => {
            field(&mut fields, "scope_id", "the gateway has no id");
        }
        (Some(LimitScope::Gateway), None) => {}
        (Some(_), None) => field(&mut fields, "scope_id", "is required"),
        (Some(_), Some(id)) if id < 1 => field(&mut fields, "scope_id", "does not exist"),
        (Some(_), Some(_)) => {}
    }
    let limit = RateLimit {
        requests_per_minute: checked(
            &mut fields,
            "requests_per_minute",
            req.requests_per_minute,
            MAX_COUNT,
        ),
        tokens_per_minute: checked(
            &mut fields,
            "tokens_per_minute",
            req.tokens_per_minute,
            MAX_TOKENS,
        ),
        concurrent: checked(&mut fields, "concurrent", req.concurrent, MAX_COUNT),
    };
    if fields.is_empty() && limit.is_none() {
        field(
            &mut fields,
            "requests_per_minute",
            "set at least one limit; delete the limits to remove them",
        );
    }
    let Some(scope) = scope.filter(|_| fields.is_empty()) else {
        return Err(ApiError::validation(fields));
    };

    let mut tx = state.store.begin().await?;
    let id = tx.upsert_limit(scope, req.scope_id, &limit).await?;
    let row = tx.limit_by_id(id).await?.ok_or_else(ApiError::internal)?;
    if !row.has_subject() {
        // Dropping the transaction takes the new row back.
        return Err(ApiError::invalid_field("scope_id", "does not exist"));
    }
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "limit.set",
        target_type: "limit",
        target_id: Some(id),
        summary: &format!("Set the limits of {}: {}", row.label(), describe(&limit)),
    })
    .await?;
    tx.commit().await?;
    refresh_snapshot(&state).await?;
    Ok(Json(LimitView::from(&row)).into_response())
}

/// Removes the limits of one subject. Admin only. The change reaches `/v1`
/// at once.
#[utoipa::path(
    delete,
    path = "/limits/{id}",
    tag = "limits",
    operation_id = "limits_delete",
    params(
        ("id" = i64, Path, description = "The id of the limit."),
    ),
    responses(
        (status = 204, description = "The limits are removed."),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "It does not exist.", body = super::openapi::ApiErrorBody),
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
    require(me, &Action::ManageLimits)?;
    let id = path_id(&raw_id)?;

    let mut tx = state.store.begin().await?;
    let Some(row) = tx.limit_by_id(id).await? else {
        drop(tx);
        // An earlier call may have deleted it and failed to refresh.
        refresh_snapshot(&state).await?;
        return Err(ApiError::not_found());
    };
    if !tx.delete_limit(id).await? {
        return Err(ApiError::not_found());
    }
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "limit.delete",
        target_type: "limit",
        target_id: Some(id),
        summary: &format!("Removed the limits of {}", row.label()),
    })
    .await?;
    tx.commit().await?;
    refresh_snapshot(&state).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
