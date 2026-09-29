//! Virtual keys.

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::anyhow;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{path_id, refresh_snapshot, require, ApiError, ApiJson, Authed};
use crate::app::AppState;
use crate::identity::policy::{list_scope, Action, Scope};
use crate::identity::UserStatus;
use crate::secrets::generate_key;
use crate::store::{check_timestamp, now, AuditEntry, KeyRow, Store};

/// Longest accepted name of a key or an access token, in characters.
const MAX_NAME_CHARS: usize = 100;

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateKeyRequest {
    name: String,
    team_id: Option<i64>,
    owner_id: Option<i64>,
    expires_at: Option<String>,
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
    /// `active`, `expired` or `revoked`.
    #[schema(value_type = String)]
    pub status: &'static str,
}

impl KeyView {
    /// `now` is the current time as the store writes it.
    fn new(k: KeyRow, now: &str) -> Self {
        let status = key_status(k.revoked_at.as_deref(), k.expires_at.as_deref(), now);
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
            status,
        }
    }
}

/// Revoked wins over expired. A key stops working at `expires_at`, not
/// after it, as in the lookup that authenticates `/v1`.
fn key_status(revoked_at: Option<&str>, expires_at: Option<&str>, now: &str) -> &'static str {
    if revoked_at.is_some() {
        "revoked"
    } else if expires_at.is_some_and(|at| at <= now) {
        "expired"
    } else {
        "active"
    }
}

/// The name of a key or an access token, trimmed.
pub fn secret_name(raw: &str) -> Result<&str, &'static str> {
    let name = raw.trim();
    let chars = name.chars().count();
    if chars == 0 || chars > MAX_NAME_CHARS || name.chars().any(char::is_control) {
        return Err("name must be 1 to 100 characters");
    }
    Ok(name)
}

/// When a key or an access token stops working: a real time in the future.
pub(super) fn future_timestamp(raw: &str) -> Result<&str, &'static str> {
    if check_timestamp(raw).is_err() {
        return Err("expires_at must be a UTC time in the form YYYY-MM-DD HH:MM:SS");
    }
    if raw <= now().as_str() {
        return Err("expires_at must be in the future");
    }
    Ok(raw)
}

/// The name and the expiry of a request, or every field that is not valid.
pub(super) fn name_and_expiry<'a>(
    name: &'a str,
    expires_at: Option<&'a str>,
) -> Result<(&'a str, Option<&'a str>), ApiError> {
    let mut fields = BTreeMap::new();
    let name = secret_name(name)
        .map_err(|m| fields.insert("name".to_string(), m.to_string()))
        .ok();
    let expires_at = match expires_at.map(future_timestamp) {
        Some(Err(m)) => {
            fields.insert("expires_at".to_string(), m.to_string());
            None
        }
        Some(Ok(at)) => Some(at),
        None => None,
    };
    match name {
        Some(name) if fields.is_empty() => Ok((name, expires_at)),
        _ => Err(ApiError::validation(fields)),
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
    let mut tx = store.begin().await?;
    let mut fields = BTreeMap::new();
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
            } else if owner.is_some() && tx.member_role(id, owner_id).await?.is_none() {
                let message = "owner is not a member of this team";
                fields.insert("team_id".to_string(), message.into());
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
    delete,
    path = "/keys/{id}",
    tag = "keys",
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
    use crate::store::after;

    #[test]
    fn names_are_trimmed_and_bounded() {
        assert_eq!(secret_name("  ci "), Ok("ci"));
        assert_eq!(secret_name(&"é".repeat(100)), Ok("é".repeat(100).as_str()));
        for bad in ["", "   ", &"n".repeat(101), "a\nb"] {
            assert!(secret_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn expiry_is_a_real_time_in_the_future() {
        let soon = after(60);
        assert_eq!(future_timestamp(&soon), Ok(soon.as_str()));
        assert!(future_timestamp("2999-12-31 23:59:59").is_ok());
        for bad in [
            after(-60).as_str(),
            "2000-01-01 00:00:00",
            "2999-02-31 00:00:00",
            "2999-01-01",
            "2999-01-01T00:00:00Z",
            "",
        ] {
            assert!(future_timestamp(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn every_invalid_field_is_named() {
        let err = name_and_expiry("", Some("never")).unwrap_err();
        let fields = err.fields.unwrap();
        assert_eq!(fields.len(), 2);
        assert!(fields.contains_key("name") && fields.contains_key("expires_at"));
        assert!(name_and_expiry(" a ", None).is_ok());
    }

    #[test]
    fn status_prefers_revoked() {
        let now = "2026-01-01 00:00:00";
        let past = Some("2025-01-01 00:00:00");
        let future = Some("2027-01-01 00:00:00");
        assert_eq!(key_status(None, None, now), "active");
        assert_eq!(key_status(None, future, now), "active");
        assert_eq!(key_status(None, past, now), "expired");
        assert_eq!(key_status(None, Some(now), now), "expired");
        assert_eq!(key_status(past, None, now), "revoked");
        assert_eq!(key_status(past, past, now), "revoked");
    }
}
