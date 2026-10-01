//! The audit log.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::json;

use super::{require, ApiError, Authed};
use crate::app::AppState;
use crate::identity::policy::Action;

/// Entries in one answer when `limit` is not given.
const DEFAULT_LIMIT: i64 = 50;

#[derive(Deserialize)]
pub struct AuditQuery {
    limit: Option<String>,
    before: Option<String>,
}

/// A positive integer written in plain digits.
fn positive(raw: &str) -> Option<i64> {
    if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    raw.parse::<i64>().ok().filter(|n| *n > 0)
}

/// Newest first. `limit` is capped by the store; `before` is the id of the
/// last entry of the page before.
#[utoipa::path(
    get,
    path = "/audit",
    tag = "audit",
    operation_id = "audit_list",
    params(
        ("limit" = Option<i64>, Query, description = "How many entries to return, 1 to 200. 50 when left out."),
        ("before" = Option<i64>, Query, description = "The id of the last entry of the page before."),
    ),
    responses(
        (status = 200, description = "Audit entries, newest first.", body = super::openapi::AuditPage),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "Some fields are not valid; `fields` names each of them.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    authed: Authed,
    query: Result<Query<AuditQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    require(&authed.principal, &Action::ViewAudit)?;
    let Ok(Query(query)) = query else {
        return Err(ApiError::bad_request("The query is not valid."));
    };

    let mut fields = BTreeMap::new();
    let mut read = |name: &str, raw: Option<&String>| match raw.map(|raw| positive(raw)) {
        None => None,
        Some(Some(n)) => Some(n),
        Some(None) => {
            fields.insert(name.to_string(), "must be a positive integer".to_string());
            None
        }
    };
    let limit = read("limit", query.limit.as_ref());
    let before = read("before", query.before.as_ref());
    if !fields.is_empty() {
        return Err(ApiError::validation(fields));
    }

    let entries = state
        .store
        .list_audit(limit.unwrap_or(DEFAULT_LIMIT), before)
        .await?;
    Ok(Json(json!({ "entries": entries })).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_positive_and_plain() {
        assert_eq!(positive("1"), Some(1));
        assert_eq!(positive("200"), Some(200));
        for bad in ["", "0", "-1", "+1", "1.0", "abc", "99999999999999999999"] {
            assert_eq!(positive(bad), None, "{bad:?}");
        }
    }
}
