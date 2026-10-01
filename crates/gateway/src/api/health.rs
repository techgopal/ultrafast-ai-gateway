//! The health of the targets `/v1` has called.

use std::sync::Arc;

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::Json;

use super::{require, ApiError, Authed};
use crate::app::AppState;
use crate::identity::policy::Action;

/// Every target that has been called since the gateway started, with the
/// state of its circuit breaker. It is kept in memory: a restart clears it.
#[utoipa::path(
    get,
    path = "/routing/health",
    tag = "routing",
    operation_id = "routing_health",
    responses(
        (status = 200, description = "The state of the circuit breaker of each target that was called.", body = super::openapi::RoutingHealth),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn routing_health(
    State(state): State<Arc<AppState>>,
    authed: Authed,
) -> Result<Response, ApiError> {
    require(&authed.principal, &Action::ViewRoutingHealth)?;
    Ok(Json(serde_json::json!({ "targets": state.health.view() })).into_response())
}
