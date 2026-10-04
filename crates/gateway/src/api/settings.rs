//! Settings of the gateway. Only an admin reads or changes them.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use super::{require, ApiError, ApiJson, Authed};
use crate::app::AppState;
use crate::identity::limiter;
use crate::identity::policy::Action;
use crate::store::{AuditEntry, SESSION_HOURS_RANGE};

/// The fewest and the most days request logs may be kept.
const RETENTION_DAYS: std::ops::RangeInclusive<i64> = 1..=3650;

/// The limits of failed sign-ins, as they are built in.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct LoginLimits {
    /// Failures older than this no longer count.
    pub window_minutes: u64,
    /// Failures one email may have inside the window.
    pub max_per_email: u64,
    /// Failures one client address may have inside the window.
    pub max_per_address: u64,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SettingsView {
    /// How many days request logs are kept before they are deleted.
    pub log_retention_days: i64,
    /// How many hours a session lives, from sign-in. Sessions that exist
    /// keep the lifetime they were made with.
    pub session_hours: i64,
    /// The networks (CIDR) whose forwarding headers are believed, as the
    /// gateway was started. Read only: it is a flag of `ultrafast serve`.
    pub trusted_proxies: Vec<String>,
    /// Read only: built in.
    pub login_limits: LoginLimits,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateSettingsRequest {
    /// 1 to 3650.
    #[serde(default)]
    log_retention_days: Option<i64>,
    /// 1 to 720. Applies to sessions made from now on.
    #[serde(default)]
    session_hours: Option<i64>,
}

async fn view_of(state: &AppState) -> Result<SettingsView, ApiError> {
    Ok(SettingsView {
        log_retention_days: state.store.log_retention_days().await?,
        session_hours: state.store.session_hours().await?,
        trusted_proxies: state
            .trusted_proxies
            .iter()
            .map(ToString::to_string)
            .collect(),
        login_limits: LoginLimits {
            window_minutes: limiter::WINDOW.as_secs() / 60,
            max_per_email: limiter::MAX_PER_EMAIL as u64,
            max_per_address: limiter::MAX_PER_ADDRESS as u64,
        },
    })
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
    Ok(Json(view_of(&state).await?).into_response())
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
    if req.log_retention_days.is_none() && req.session_hours.is_none() {
        return Err(ApiError::bad_request(
            "Send at least one of log_retention_days and session_hours.",
        ));
    }
    // Every field is checked before any is written.
    let mut fields = BTreeMap::new();
    if req
        .log_retention_days
        .is_some_and(|days| !RETENTION_DAYS.contains(&days))
    {
        fields.insert(
            "log_retention_days".to_string(),
            "must be from 1 to 3650".to_string(),
        );
    }
    if req
        .session_hours
        .is_some_and(|hours| !SESSION_HOURS_RANGE.contains(&hours))
    {
        fields.insert(
            "session_hours".to_string(),
            "must be from 1 to 720".to_string(),
        );
    }
    if !fields.is_empty() {
        return Err(ApiError::validation(fields));
    }
    let mut tx = state.store.begin().await?;
    if let Some(days) = req.log_retention_days {
        tx.set_log_retention_days(days).await?;
        tx.audit(AuditEntry {
            actor_user_id: Some(me.user_id),
            actor_email: &me.email,
            action: "settings.update",
            target_type: "settings",
            target_id: None,
            summary: &format!("Set log retention to {days} days"),
        })
        .await?;
    }
    if let Some(hours) = req.session_hours {
        tx.set_session_hours(hours).await?;
        tx.audit(AuditEntry {
            actor_user_id: Some(me.user_id),
            actor_email: &me.email,
            action: "settings.update",
            target_type: "settings",
            target_id: None,
            summary: &format!("Set session lifetime to {hours} hours"),
        })
        .await?;
    }
    tx.commit().await?;
    Ok(Json(view_of(&state).await?).into_response())
}
