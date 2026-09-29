//! Access tokens. A user manages only their own.

use std::sync::Arc;

use anyhow::anyhow;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::keys::name_and_expiry;
use super::{path_id, require, ApiError, ApiJson, Authed};
use crate::app::AppState;
use crate::identity::policy::Action;
use crate::secrets::{generate_secret, TOKEN_PREFIX};
use crate::store::{AuditEntry, TokenRow};

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateTokenRequest {
    name: String,
    expires_at: Option<String>,
}

/// A token as `/api` shows it. It has no field for the token or its hash.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct TokenView {
    pub id: i64,
    pub name: String,
    pub display: String,
    #[schema(required)]
    pub expires_at: Option<String>,
    #[schema(required)]
    pub revoked_at: Option<String>,
    #[schema(required)]
    pub last_used_at: Option<String>,
    pub created_at: String,
}

impl From<TokenRow> for TokenView {
    fn from(t: TokenRow) -> Self {
        Self {
            id: t.id,
            name: t.name,
            display: t.display,
            expires_at: t.expires_at,
            revoked_at: t.revoked_at,
            last_used_at: t.last_used_at,
            created_at: t.created_at,
        }
    }
}

#[utoipa::path(
    get,
    path = "/tokens",
    tag = "tokens",
    responses(
        (status = 200, description = "The caller's access tokens.", body = super::openapi::TokenList),
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
    require(me, &Action::ManageOwnTokens)?;
    let tokens = state.store.list_tokens_of(me.user_id).await?;
    let tokens: Vec<TokenView> = tokens.into_iter().map(TokenView::from).collect();
    Ok(Json(json!({ "tokens": tokens })).into_response())
}

#[utoipa::path(
    post,
    path = "/tokens",
    tag = "tokens",
    request_body = CreateTokenRequest,
    responses(
        (status = 201, description = "The new access token, with the token itself.", body = super::openapi::CreatedToken),
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
    ApiJson(req): ApiJson<CreateTokenRequest>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    require(me, &Action::ManageOwnTokens)?;
    let (name, expires_at) = name_and_expiry(&req.name, req.expires_at.as_deref())?;

    let store = &state.store;
    let token = generate_secret(TOKEN_PREFIX);
    let mut tx = store.begin().await?;
    let id = tx
        .insert_token(me.user_id, name, &token.hash, &token.display, expires_at)
        .await?;
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "token.create",
        target_type: "token",
        target_id: Some(id),
        summary: &format!("Created access token {name} ({})", token.display),
    })
    .await?;
    tx.commit().await?;

    let row = store
        .token_by_id(id)
        .await?
        .ok_or_else(|| anyhow!("the token is missing after it was created"))?;
    // The only time the token itself is sent.
    let body = json!({ "token": TokenView::from(row), "secret": token.full });
    Ok((StatusCode::CREATED, Json(body)).into_response())
}

#[utoipa::path(
    delete,
    path = "/tokens/{id}",
    tag = "tokens",
    params(
        ("id" = i64, Path, description = "The id of the access token."),
    ),
    responses(
        (status = 204, description = "The access token is revoked."),
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
    require(me, &Action::ManageOwnTokens)?;
    let id = path_id(&raw_id)?;
    let token = store.token_by_id(id).await?;
    let token = token.ok_or_else(ApiError::not_found)?;
    // The action covers the caller's own tokens only. Anyone else's is
    // answered like one that does not exist, for an admin too.
    if token.user_id != me.user_id {
        return Err(ApiError::not_found());
    }

    let mut tx = store.begin().await?;
    if !tx.revoke_token(token.id).await? {
        // Already revoked: nothing changed, so nothing to record.
        drop(tx);
        return Ok(StatusCode::NO_CONTENT.into_response());
    }
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "token.revoke",
        target_type: "token",
        target_id: Some(token.id),
        summary: &format!("Revoked access token {} ({})", token.name, token.display),
    })
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
