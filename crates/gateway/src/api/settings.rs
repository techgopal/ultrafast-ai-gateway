//! Settings of the gateway. Only an admin reads or changes them.

use std::sync::Arc;

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use super::{require, ApiError, ApiJson, Authed};
use crate::app::AppState;
use crate::identity::policy::Action;
use crate::store::AuditEntry;

/// The fewest and the most days request logs may be kept.
const RETENTION_DAYS: std::ops::RangeInclusive<i64> = 1..=3650;

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SettingsView {
    /// How many days request logs are kept before they are deleted.
    pub log_retention_days: i64,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateSettingsRequest {
    /// 1 to 3650.
    log_retention_days: i64,
}

#[utoipa::path(
    get,
    path = "/settings",
    tag = "settings",
    operation_id = "settings_view",
    responses(
        (status = 200, description = "The settings.", body = SettingsView),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn view(
    State(state): State<Arc<AppState>>,
    authed: Authed,
) -> Result<Response, ApiError> {
    require(&authed.principal, &Action::ManageSettings)?;
    let log_retention_days = state.store.log_retention_days().await?;
    Ok(Json(SettingsView { log_retention_days }).into_response())
}

#[utoipa::path(
    patch,
    path = "/settings",
    tag = "settings",
    operation_id = "settings_update",
    request_body = UpdateSettingsRequest,
    responses(
        (status = 200, description = "The settings after the change.", body = SettingsView),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 413, description = "The request body is too large.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "A value is out of range; `fields` names each of them.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn update(
    State(state): State<Arc<AppState>>,
    authed: Authed,
    ApiJson(req): ApiJson<UpdateSettingsRequest>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    require(me, &Action::ManageSettings)?;
    if !RETENTION_DAYS.contains(&req.log_retention_days) {
        return Err(ApiError::invalid_field(
            "log_retention_days",
            "must be from 1 to 3650",
        ));
    }
    let mut tx = state.store.begin().await?;
    tx.set_log_retention_days(req.log_retention_days).await?;
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "settings.update",
        target_type: "settings",
        target_id: None,
        summary: &format!("Set log retention to {} days", req.log_retention_days),
    })
    .await?;
    tx.commit().await?;
    Ok(Json(SettingsView {
        log_retention_days: req.log_retention_days,
    })
    .into_response())
}
