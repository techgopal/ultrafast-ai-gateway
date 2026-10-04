//! The configuration as a file: export and import. Admin only.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::header::{CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;

use super::{refresh_snapshot, require, ApiError, Authed};
use crate::app::AppState;
use crate::identity::policy::Action;
use crate::portable::{self, Actor, ConfigFile, ImportReport, MAX_FILE_BYTES};
use crate::store::{now, AuditEntry};

/// The configuration of the gateway as one JSON file: providers (without
/// their credentials), models with their grants, teams, routes, limits and
/// budgets (those of keys left out) and settings. It holds no key, token,
/// password, session, log or audit row.
#[utoipa::path(
    get,
    path = "/config/export",
    tag = "config",
    operation_id = "config_export",
    responses(
        (status = 200, description = "The configuration file, as a download.", body = ConfigFile),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn export(
    State(state): State<Arc<AppState>>,
    authed: Authed,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    require(me, &Action::ManageSettings)?;
    let file = portable::export(&state.store).await?;
    let mut tx = state.store.begin().await?;
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "config.export",
        target_type: "config",
        target_id: None,
        summary: "Exported the configuration",
    })
    .await?;
    tx.commit().await?;
    let bytes = serde_json::to_vec_pretty(&file).map_err(anyhow::Error::from)?;
    // `YYYY-MM-DD HH:MM:SS` as `YYYYMMDD-HHMMSS`.
    let stamp: String = now()
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == ' ')
        .map(|c| if c == ' ' { '-' } else { c })
        .collect();
    Ok((
        [
            (CONTENT_TYPE, "application/json".to_string()),
            (
                CONTENT_DISPOSITION,
                format!("attachment; filename=\"ultrafast-config-{stamp}.json\""),
            ),
        ],
        bytes,
    )
        .into_response())
}

#[derive(Deserialize)]
pub struct ImportQuery {
    dry_run: Option<String>,
}

/// Checks a configuration file and, with `dry_run=false`, writes it. Without
/// `dry_run` it is a dry run. Missing things are created and existing ones,
/// by name, are updated; nothing is deleted. A file with any error writes
/// nothing: the answer is 422 with the report. A new provider has no
/// credential until an admin sets one.
#[utoipa::path(
    post,
    path = "/config/import",
    tag = "config",
    operation_id = "config_import",
    params(
        ("dry_run" = Option<bool>, Query, description = "`true` (the default) only reports; `false` writes."),
    ),
    request_body = ConfigFile,
    responses(
        (status = 200, description = "What was written, or with a dry run what would be.", body = ImportReport),
        (status = 400, description = "`dry_run` is not `true` or `false`.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 413, description = "The file is larger than 8 MiB.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "The file has errors, which the report lists. Nothing was written.", body = ImportReport),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn import(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ImportQuery>,
    authed: Authed,
    body: Body,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    require(me, &Action::ManageSettings)?;
    let dry_run = match query.dry_run.as_deref() {
        None | Some("true") => true,
        Some("false") => false,
        Some(_) => return Err(ApiError::bad_request("dry_run must be true or false.")),
    };
    let bytes = axum::body::to_bytes(body, MAX_FILE_BYTES)
        .await
        .map_err(|e| {
            if e.into_inner().is::<http_body_util::LengthLimitError>() {
                ApiError::payload_too_large()
            } else {
                ApiError::bad_request("The request body could not be read.")
            }
        })?;
    let report = match portable::parse(&bytes) {
        Err(report) => report,
        Ok(file) => {
            let actor = Actor {
                user_id: Some(me.user_id),
                email: &me.email,
            };
            let report = portable::import(&state.store, &file, &actor, dry_run).await?;
            if !dry_run && report.is_clean() {
                refresh_snapshot(&state).await?;
            }
            report
        }
    };
    let status = if report.is_clean() {
        StatusCode::OK
    } else {
        StatusCode::UNPROCESSABLE_ENTITY
    };
    Ok((status, Json(report)).into_response())
}
