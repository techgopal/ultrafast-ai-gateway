//! The MCP registry: servers, the tools they offer and who may call them.
//! A tool is granted like a model. A server's secret goes in and never
//! comes out.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Deserializer, Serialize};

use super::models::{key_team, GrantsView, KeyTeamQuery};
use super::providers::checked;
use super::{path_id, refresh_snapshot, require, ApiError, ApiJson, Authed};
use crate::access::{self, Viewer};
use crate::app::AppState;
use crate::config::{is_private_host, same_host, validate_base_url};
use crate::identity::policy::Action;
use crate::identity::Principal;
use crate::mcp::sync::{sync_server, Actor, SyncError};
use crate::mcp::{check_auth, encrypt_secret, server_usable, validate_server_name};
use crate::snapshot::{SnapMcpServer, SnapMcpTool};
use crate::store::{AuditEntry, Grants, McpServerRow, McpToolRow, NewMcpServer, Store, StoreError};

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateMcpServerRequest {
    /// 1 to 63 characters of `a-z`, `0-9` and `-`, starting with a letter or
    /// a digit. It is the name in the endpoint `/mcp/{name}` and in a key's
    /// allowlist (`mcp:{name}/{tool}`), and cannot be changed.
    name: String,
    /// The streamable-HTTP endpoint of the server.
    url: String,
    /// `none` (the default), `bearer` or `header`.
    auth_kind: Option<String>,
    /// The header that carries the secret, for `auth_kind` `header`.
    auth_header: Option<String>,
    /// Required with `bearer` or `header`.
    #[schema(write_only)]
    secret: Option<String>,
    description: Option<String>,
    /// On by default.
    enabled: Option<bool>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateMcpServerRequest {
    url: Option<String>,
    auth_kind: Option<String>,
    auth_header: Option<String>,
    /// Absent leaves the secret, `null` removes it, a string replaces it.
    /// Changing the host of a server that has a secret needs it again.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>, write_only)]
    secret: Option<Option<String>>,
    description: Option<String>,
    enabled: Option<bool>,
}

/// Reads a field that is there, so `null` differs from a missing field.
fn present<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(Some)
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateMcpToolRequest {
    enabled: bool,
}

/// A server as `/api` shows it. Everything but the name and description is
/// for admins: anybody else gets `null` (and a count of the tools they may
/// call).
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct McpServerView {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    /// How many tools: for an admin those the server offers, for anybody
    /// else those they may call.
    pub tool_count: i64,
    pub created_at: String,
    #[schema(required)]
    pub url: Option<String>,
    /// `none`, `bearer` or `header`.
    #[schema(required)]
    pub auth_kind: Option<String>,
    #[schema(required)]
    pub auth_header: Option<String>,
    #[schema(required)]
    pub has_secret: Option<bool>,
    /// The server can be called: it needs no secret, or its secret can be read.
    #[schema(required)]
    pub usable: Option<bool>,
    #[schema(required)]
    pub last_sync_at: Option<String>,
    /// Our summary of why the last sync failed; never text from the server.
    #[schema(required)]
    pub last_sync_error: Option<String>,
}

impl McpServerView {
    fn admin(row: &McpServerRow, tool_count: i64, usable: bool) -> Self {
        Self {
            id: row.id,
            name: row.name.clone(),
            description: row.description.clone(),
            enabled: row.enabled,
            tool_count,
            created_at: row.created_at.clone(),
            url: Some(row.url.clone()),
            auth_kind: Some(row.auth_kind.clone()),
            auth_header: row.auth_header.clone(),
            has_secret: Some(row.auth_secret_enc.is_some()),
            usable: Some(usable),
            last_sync_at: row.last_sync_at.clone(),
            last_sync_error: row.last_sync_error.clone(),
        }
    }
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct McpServerList {
    pub servers: Vec<McpServerView>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct McpToolView {
    pub id: i64,
    pub server_id: i64,
    pub name: String,
    #[schema(required)]
    pub title: Option<String>,
    pub description: String,
    /// JSON Schema of the arguments, as the server gave it.
    #[schema(value_type = Object)]
    pub input_schema: serde_json::Value,
    #[schema(value_type = Option<Object>, required)]
    pub output_schema: Option<serde_json::Value>,
    #[schema(value_type = Option<Object>, required)]
    pub annotations: Option<serde_json::Value>,
    pub enabled: bool,
    /// The server no longer offers it; its grants are kept.
    pub removed: bool,
    /// Who may call it. For everyone who is not an admin it is always empty.
    pub grants: GrantsView,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct McpToolList {
    pub tools: Vec<McpToolView>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct McpSyncResult {
    /// Tools that are new, now switched off.
    pub added: Vec<String>,
    pub updated: Vec<String>,
    /// Tools the server no longer offers; their grants are kept.
    pub removed: Vec<String>,
    /// Tools the server listed that were left out.
    pub skipped: u32,
}

fn json_of(text: &str) -> serde_json::Value {
    serde_json::from_str(text).unwrap_or_else(|_| serde_json::json!({ "type": "object" }))
}

impl McpToolView {
    fn of_row(t: &McpToolRow, grants: Grants) -> Self {
        Self {
            id: t.id,
            server_id: t.server_id,
            name: t.name.clone(),
            title: t.title.clone(),
            description: t.description.clone(),
            input_schema: json_of(&t.input_schema),
            output_schema: t
                .output_schema
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok()),
            annotations: t
                .annotations
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok()),
            enabled: t.enabled,
            removed: t.removed_at.is_some(),
            grants: grants.into(),
        }
    }

    /// What a caller who may call the tool sees: no grants.
    fn of_snapshot(t: &SnapMcpTool) -> Self {
        Self {
            id: t.id,
            server_id: t.server_id,
            name: t.name.clone(),
            title: t.title.clone(),
            description: t.description.clone(),
            input_schema: t.input_schema.clone(),
            output_schema: t.output_schema.clone(),
            annotations: t.annotations.clone(),
            enabled: true,
            removed: false,
            grants: GrantsView {
                everyone: false,
                team_ids: Vec::new(),
                user_ids: Vec::new(),
            },
        }
    }
}

/// The viewer for a non-admin caller: the team of `?key_team_id=` when
/// they ask for what a key they make for another member may call, else
/// themselves.
fn viewer_for<'a>(me: &Principal, teams: &'a [i64], team_key: Option<i64>) -> Viewer<'a> {
    match team_key {
        Some(team_id) => Viewer::Team { team_id },
        None => super::models::viewer_of(me, teams),
    }
}

fn exists_err() -> ApiError {
    ApiError::conflict(
        "mcp_server_exists",
        "An MCP server with this name already exists.",
    )
}

async fn tool_counts(store: &Store, row: &McpServerRow) -> Result<i64, ApiError> {
    let tools = store.list_tools(row.id).await?;
    Ok(tools.iter().filter(|t| t.removed_at.is_none()).count() as i64)
}

async fn admin_view(state: &AppState, row: &McpServerRow) -> Result<McpServerView, ApiError> {
    let count = tool_counts(&state.store, row).await?;
    Ok(McpServerView::admin(
        row,
        count,
        server_usable(row, &state.cipher),
    ))
}

async fn server_of(store: &Store, raw_id: &str) -> Result<McpServerRow, ApiError> {
    let id = path_id(raw_id)?;
    store
        .server_by_id(id)
        .await?
        .ok_or_else(ApiError::not_found)
}

/// How many tools of the snapshot's server the viewer may call.
fn callable_count(
    state: &AppState,
    viewer: Viewer<'_>,
    server: &SnapMcpServer,
) -> (i64, Vec<SnapMcpTool>) {
    let snapshot = state.snapshot.load();
    let tools: Vec<SnapMcpTool> = snapshot
        .mcp_tools_of(server.id)
        .filter(|t| access::may_call_tool(viewer, server, t))
        .cloned()
        .collect();
    (tools.len() as i64, tools)
}

#[utoipa::path(
    get,
    path = "/mcp/servers",
    tag = "mcp",
    operation_id = "mcp_servers_list",
    params(
        ("key_team_id" = Option<i64>, Query, description = "A team the caller leads: only the servers with a tool a key they make for another member of it may call, those granted to everyone or to the team. An admin's keys are not cut."),
    ),
    responses(
        (status = 200, description = "An admin gets every server with its settings. Everyone else gets the enabled servers that have a tool they may call, without URL or authentication.", body = McpServerList),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "`key_team_id` names a team the caller does not lead.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "Some fields are not valid; `fields` names each of them.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    authed: Authed,
    query: Result<Query<KeyTeamQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    require(me, &Action::ListMcp)?;
    let team_key = key_team(me, query)?;
    let mut servers = Vec::new();
    if me.is_admin() {
        for row in state.store.list_mcp_servers().await? {
            servers.push(admin_view(&state, &row).await?);
        }
    } else {
        let teams = me.team_ids();
        let viewer = viewer_for(me, &teams, team_key);
        let snapshot = state.snapshot.load_full();
        for server in snapshot.mcp_servers().filter(|s| s.enabled && s.usable) {
            let (count, _) = callable_count(&state, viewer, server);
            if count == 0 {
                continue;
            }
            // Gone since the snapshot: left out.
            let Some(row) = state.store.server_by_id(server.id).await? else {
                continue;
            };
            servers.push(McpServerView {
                id: row.id,
                name: row.name,
                description: row.description,
                enabled: true,
                tool_count: count,
                created_at: row.created_at,
                url: None,
                auth_kind: None,
                auth_header: None,
                has_secret: None,
                usable: None,
                last_sync_at: None,
                last_sync_error: None,
            });
        }
    }
    Ok(Json(McpServerList { servers }).into_response())
}

/// The URL of a server: well formed, and not at this machine or a private
/// network unless `UF_MCP_ALLOW_PRIVATE_UPSTREAMS=on`. A name is judged by its
/// spelling (see `is_private_host`).
fn check_url(state: &AppState, url: &str, field: &str, fields: &mut BTreeMap<String, String>) {
    checked(field, validate_base_url(url), fields);
    if !fields.contains_key(field) && !state.mcp.allow_private_upstreams && is_private_host(url) {
        fields.insert(
            field.to_string(),
            "The address is on this machine or a private network. Set UF_MCP_ALLOW_PRIVATE_UPSTREAMS=on to allow it.".to_string(),
        );
    }
}

fn check_secret(secret: Option<&str>, fields: &mut BTreeMap<String, String>) {
    if secret.is_some_and(|s| s.trim().is_empty()) {
        fields.insert("secret".to_string(), "must not be empty".to_string());
    }
}

#[utoipa::path(
    post,
    path = "/mcp/servers",
    tag = "mcp",
    operation_id = "mcp_servers_create",
    request_body = CreateMcpServerRequest,
    responses(
        (status = 201, description = "The new server. It has no tools until it is synced.", body = McpServerView),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 409, description = "`mcp_server_exists`: the name is taken.", body = super::openapi::ApiErrorBody),
        (status = 413, description = "The request body is too large.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "Some fields are not valid; `fields` names each of them.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn create(
    State(state): State<Arc<AppState>>,
    authed: Authed,
    ApiJson(req): ApiJson<CreateMcpServerRequest>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    require(me, &Action::ManageMcp)?;

    let kind = req.auth_kind.as_deref().unwrap_or("none");
    let mut fields = BTreeMap::new();
    checked("name", validate_server_name(&req.name), &mut fields);
    check_url(&state, &req.url, "url", &mut fields);
    check_auth(kind, req.auth_header.as_deref(), &mut fields);
    check_secret(req.secret.as_deref(), &mut fields);
    if kind == "none" && req.secret.is_some() {
        fields.insert(
            "secret".to_string(),
            "secret is only for auth_kind bearer or header".to_string(),
        );
    }
    if kind != "none" && req.secret.is_none() {
        fields.insert(
            "secret".to_string(),
            "secret is required with auth_kind bearer or header".to_string(),
        );
    }
    let description = req.description.as_deref().unwrap_or("");
    super::guardrails::check_description(description, &mut fields);
    if !fields.is_empty() {
        return Err(ApiError::validation(fields));
    }
    // From here on only the encrypted form exists.
    let secret_enc = req
        .secret
        .as_deref()
        .map(|s| encrypt_secret(&state.cipher, s));

    let mut tx = state.store.begin().await?;
    let inserted = tx
        .insert_server(NewMcpServer {
            name: &req.name,
            url: &req.url,
            auth_kind: kind,
            auth_header: req.auth_header.as_deref(),
            auth_secret_enc: secret_enc.as_deref(),
            enabled: req.enabled.unwrap_or(true),
            description,
        })
        .await;
    let id = match inserted {
        Ok(id) => id,
        Err(e) => {
            return Err(match e.downcast_ref::<StoreError>() {
                Some(StoreError::Duplicate) => exists_err(),
                None => e.into(),
            })
        }
    };
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "mcp.create",
        target_type: "mcp_server",
        target_id: Some(id),
        summary: &if secret_enc.is_some() {
            format!("Created MCP server {}, secret set", req.name)
        } else {
            format!("Created MCP server {}", req.name)
        },
    })
    .await?;
    tx.commit().await?;
    refresh_snapshot(&state).await?;
    let row = state
        .store
        .server_by_id(id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok((StatusCode::CREATED, Json(admin_view(&state, &row).await?)).into_response())
}

#[utoipa::path(
    get,
    path = "/mcp/servers/{id}",
    tag = "mcp",
    operation_id = "mcp_servers_view",
    params(
        ("id" = i64, Path, description = "The id of the server."),
    ),
    responses(
        (status = 200, description = "The server.", body = McpServerView),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this.", body = super::openapi::ApiErrorBody),
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
    require(&authed.principal, &Action::ManageMcp)?;
    let row = server_of(&state.store, &raw_id).await?;
    Ok(Json(admin_view(&state, &row).await?).into_response())
}

#[utoipa::path(
    patch,
    path = "/mcp/servers/{id}",
    tag = "mcp",
    operation_id = "mcp_servers_update",
    params(
        ("id" = i64, Path, description = "The id of the server."),
    ),
    request_body = UpdateMcpServerRequest,
    responses(
        (status = 200, description = "The server after the change.", body = McpServerView),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "It does not exist, or it is hidden from the caller.", body = super::openapi::ApiErrorBody),
        (status = 413, description = "The request body is too large.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "Some fields are not valid, or the host changed and the secret was not sent again; `fields` names each of them.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn update(
    State(state): State<Arc<AppState>>,
    Path(raw_id): Path<String>,
    authed: Authed,
    ApiJson(req): ApiJson<UpdateMcpServerRequest>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    // The answer does not depend on the server, so it comes before the id
    // is looked at.
    require(me, &Action::ManageMcp)?;
    let target = server_of(&state.store, &raw_id).await?;
    if req.url.is_none()
        && req.auth_kind.is_none()
        && req.auth_header.is_none()
        && req.secret.is_none()
        && req.description.is_none()
        && req.enabled.is_none()
    {
        return Err(ApiError::bad_request(
            "Send at least one of url, auth_kind, auth_header, secret, description and enabled.",
        ));
    }
    let mut fields = BTreeMap::new();
    if let Some(url) = req.url.as_deref().filter(|u| **u != target.url) {
        check_url(&state, url, "url", &mut fields);
    }
    check_secret(req.secret.as_ref().and_then(|s| s.as_deref()), &mut fields);
    if let Some(description) = &req.description {
        super::guardrails::check_description(description, &mut fields);
    }
    if !fields.is_empty() {
        return Err(ApiError::validation(fields));
    }

    let mut tx = state.store.begin_immediate().await?;
    // What is compared and recorded is what the transaction sees.
    let was = tx
        .server_by_id(target.id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let kind = req
        .auth_kind
        .clone()
        .unwrap_or_else(|| was.auth_kind.clone());
    let header = match &req.auth_header {
        Some(h) => Some(h.clone()),
        None if kind == "header" => was.auth_header.clone(),
        None => None,
    };
    check_auth(&kind, header.as_deref(), &mut fields);
    let url = req.url.clone().filter(|u| *u != was.url);
    let host_changed = url.as_deref().is_some_and(|u| !same_host(&was.url, u));
    // The new stored secret: kept, replaced or removed.
    let secret_enc: Option<String> = if kind == "none" {
        if matches!(req.secret, Some(Some(_))) {
            fields.insert(
                "secret".to_string(),
                "secret is only for auth_kind bearer or header".to_string(),
            );
        }
        None
    } else {
        match &req.secret {
            Some(Some(s)) => Some(encrypt_secret(&state.cipher, s)),
            Some(None) => {
                // Removing the secret is allowed only with `auth_kind` `none`.
                fields.insert(
                    "secret".to_string(),
                    "A secret is needed for this auth kind.".to_string(),
                );
                None
            }
            None => {
                if was.auth_secret_enc.is_some() && host_changed {
                    // The stored secret goes only to the host it was given for.
                    fields.insert(
                        "secret".to_string(),
                        "Enter the secret again: the host changed.".to_string(),
                    );
                } else if was.auth_secret_enc.is_none() && was.auth_kind == "none" {
                    fields.insert(
                        "secret".to_string(),
                        "secret is required with auth_kind bearer or header".to_string(),
                    );
                }
                was.auth_secret_enc.clone()
            }
        }
    };
    if !fields.is_empty() {
        return Err(ApiError::validation(fields));
    }

    let mut changes = Vec::new();
    if url.is_some() {
        changes.push("URL changed");
    }
    if kind != was.auth_kind || header != was.auth_header {
        changes.push("authentication changed");
    }
    match (&secret_enc, &was.auth_secret_enc) {
        (None, None) => {}
        (None, Some(_)) => changes.push("secret removed"),
        (Some(_), None) => changes.push("secret set"),
        (Some(new), Some(old)) if new != old => changes.push("secret replaced"),
        _ => {}
    }
    let description = req.description.clone().filter(|d| *d != was.description);
    if description.is_some() {
        changes.push("description changed");
    }
    let enabled = req.enabled.filter(|e| *e != was.enabled);
    match enabled {
        Some(true) => changes.push("enabled"),
        Some(false) => changes.push("disabled"),
        None => {}
    }
    if changes.is_empty() {
        // Nothing to change, so nothing to record.
        drop(tx);
        // An earlier call may have made the change and failed to refresh.
        refresh_snapshot(&state).await?;
        return Ok(Json(admin_view(&state, &was).await?).into_response());
    }

    let updated = tx
        .update_server(
            was.id,
            NewMcpServer {
                name: &was.name,
                url: url.as_deref().unwrap_or(&was.url),
                auth_kind: &kind,
                auth_header: header.as_deref(),
                auth_secret_enc: secret_enc.as_deref(),
                enabled: enabled.unwrap_or(was.enabled),
                description: description.as_deref().unwrap_or(&was.description),
            },
        )
        .await?;
    if !updated {
        return Err(ApiError::not_found());
    }
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "mcp.update",
        target_type: "mcp_server",
        target_id: Some(was.id),
        summary: &format!("Updated MCP server {}: {}", was.name, changes.join(", ")),
    })
    .await?;
    tx.commit().await?;
    refresh_snapshot(&state).await?;
    let row = state
        .store
        .server_by_id(was.id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(admin_view(&state, &row).await?).into_response())
}

#[utoipa::path(
    delete,
    path = "/mcp/servers/{id}",
    tag = "mcp",
    operation_id = "mcp_servers_delete",
    params(
        ("id" = i64, Path, description = "The id of the server."),
    ),
    responses(
        (status = 204, description = "The server, its tools and their grants are deleted."),
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
    require(me, &Action::ManageMcp)?;
    let target = match server_of(&state.store, &raw_id).await {
        Ok(target) => target,
        Err(e) => {
            // An earlier call may have deleted it and failed to refresh.
            refresh_snapshot(&state).await?;
            return Err(e);
        }
    };
    let mut tx = state.store.begin().await?;
    // The foreign keys take the tools and their grants along.
    if !tx.delete_server(target.id).await? {
        drop(tx);
        refresh_snapshot(&state).await?;
        return Err(ApiError::not_found());
    }
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "mcp.delete",
        target_type: "mcp_server",
        target_id: Some(target.id),
        summary: &format!("Deleted MCP server {}", target.name),
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
        message: "The server did not return its tools.".to_string(),
        fields: None,
    }
}

#[utoipa::path(
    post,
    path = "/mcp/servers/{id}/sync",
    tag = "mcp",
    operation_id = "mcp_servers_sync",
    params(
        ("id" = i64, Path, description = "The id of the server."),
    ),
    responses(
        (status = 200, description = "New tools are added, switched off. Tools the server no longer offers are marked removed; their grants are kept.", body = McpSyncResult),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "It does not exist, or it is hidden from the caller.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "`server_unusable`: the server needs a secret that is missing or cannot be read.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
        (status = 502, description = "`sync_failed`: the server did not return its tools. The server's `last_sync_error` says why in our words.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn sync(
    State(state): State<Arc<AppState>>,
    Path(raw_id): Path<String>,
    authed: Authed,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    require(me, &Action::ManageMcp)?;
    let id = path_id(&raw_id)?;
    let actor = Actor::of(me);
    // A sync that has started finishes, even if the caller goes away.
    let worker = state.clone();
    let result = tokio::spawn(async move { sync_server(&worker, id, &actor).await })
        .await
        .map_err(|_| ApiError::internal())?;
    match result {
        Ok(done) => Ok(Json(McpSyncResult {
            added: done.delta.added,
            updated: done.delta.updated,
            removed: done.delta.removed,
            skipped: u32::try_from(done.skipped).unwrap_or(u32::MAX),
        })
        .into_response()),
        Err(SyncError::NotFound) => Err(ApiError::not_found()),
        Err(SyncError::Unusable) => Err(ApiError {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            code: "server_unusable",
            message: "Enter the secret of this server again.".to_string(),
            fields: None,
        }),
        Err(SyncError::Upstream(e)) => {
            tracing::warn!(server_id = id, error = %e, "MCP sync failed");
            Err(sync_failed())
        }
        Err(SyncError::Store(e)) => Err(e.into()),
    }
}

#[utoipa::path(
    get,
    path = "/mcp/servers/{id}/tools",
    tag = "mcp",
    operation_id = "mcp_tools_list",
    params(
        ("id" = i64, Path, description = "The id of the server."),
        ("key_team_id" = Option<i64>, Query, description = "A team the caller leads: only the tools a key they make for another member of it may call, those granted to everyone or to the team. An admin's keys are not cut."),
    ),
    responses(
        (status = 200, description = "An admin gets every tool of the server with its grants, removed ones included. Everyone else gets the tools they may call, with empty grants.", body = McpToolList),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "`key_team_id` names a team the caller does not lead.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "It does not exist, or the caller may call none of its tools.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "Some fields are not valid; `fields` names each of them.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn tools_list(
    State(state): State<Arc<AppState>>,
    Path(raw_id): Path<String>,
    authed: Authed,
    query: Result<Query<KeyTeamQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    require(me, &Action::ListMcp)?;
    let team_key = key_team(me, query)?;
    let id = path_id(&raw_id)?;
    if me.is_admin() {
        let server = state
            .store
            .server_by_id(id)
            .await?
            .ok_or_else(ApiError::not_found)?;
        let mut grants = state.store.mcp_grants_of_server(server.id).await?;
        let tools = state
            .store
            .list_tools(server.id)
            .await?
            .iter()
            .map(|t| McpToolView::of_row(t, grants.remove(&t.id).unwrap_or_default()))
            .collect();
        return Ok(Json(McpToolList { tools }).into_response());
    }
    let teams = me.team_ids();
    let viewer = viewer_for(me, &teams, team_key);
    let snapshot = state.snapshot.load_full();
    let server = snapshot
        .mcp_server_by_id(id)
        .filter(|s| s.enabled && s.usable)
        .ok_or_else(ApiError::not_found)?;
    let (count, tools) = callable_count(&state, viewer, server);
    if count == 0 {
        return Err(ApiError::not_found());
    }
    let tools = tools.iter().map(McpToolView::of_snapshot).collect();
    Ok(Json(McpToolList { tools }).into_response())
}

async fn tool_view(state: &AppState, id: i64) -> Result<McpToolView, ApiError> {
    let tool = state
        .store
        .tool_by_id(id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let grants = state.store.tool_grants_of(id).await?;
    Ok(McpToolView::of_row(&tool, grants))
}

#[utoipa::path(
    patch,
    path = "/mcp/tools/{id}",
    tag = "mcp",
    operation_id = "mcp_tools_update",
    params(
        ("id" = i64, Path, description = "The id of the tool."),
    ),
    request_body = UpdateMcpToolRequest,
    responses(
        (status = 200, description = "The tool after the change.", body = McpToolView),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "It does not exist, or it is hidden from the caller.", body = super::openapi::ApiErrorBody),
        (status = 413, description = "The request body is too large.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn tools_update(
    State(state): State<Arc<AppState>>,
    Path(raw_id): Path<String>,
    authed: Authed,
    ApiJson(req): ApiJson<UpdateMcpToolRequest>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    require(me, &Action::ManageMcp)?;
    let id = path_id(&raw_id)?;
    let mut tx = state.store.begin().await?;
    let tool = tx.tool_by_id(id).await?.ok_or_else(ApiError::not_found)?;
    let server = tx
        .server_by_id(tool.server_id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if !tx.set_tool_enabled(id, req.enabled).await? {
        return Err(ApiError::not_found());
    }
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "mcp.tool_update",
        target_type: "mcp_tool",
        target_id: Some(id),
        summary: &format!(
            "{} tool {} of {}",
            if req.enabled { "Enabled" } else { "Disabled" },
            tool.name,
            server.name
        ),
    })
    .await?;
    tx.commit().await?;
    refresh_snapshot(&state).await?;
    Ok(Json(tool_view(&state, id).await?).into_response())
}

#[utoipa::path(
    put,
    path = "/mcp/tools/{id}/grants",
    tag = "mcp",
    operation_id = "mcp_tools_grants_put",
    params(
        ("id" = i64, Path, description = "The id of the tool."),
    ),
    request_body = GrantsView,
    responses(
        (status = 200, description = "The tool with its new grants. They replace the old ones.", body = McpToolView),
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
pub async fn tools_put_grants(
    State(state): State<Arc<AppState>>,
    Path(raw_id): Path<String>,
    authed: Authed,
    ApiJson(req): ApiJson<GrantsView>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    require(me, &Action::ManageMcp)?;
    let id = path_id(&raw_id)?;
    state
        .store
        .tool_by_id(id)
        .await?
        .ok_or_else(ApiError::not_found)?;

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
    let tool = tx.tool_by_id(id).await?.ok_or_else(ApiError::not_found)?;
    let server = tx
        .server_by_id(tool.server_id)
        .await?
        .ok_or_else(ApiError::not_found)?;
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
    tx.replace_tool_grants(id, &grants).await?;
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
        action: "mcp.tool_grants",
        target_type: "mcp_tool",
        target_id: Some(id),
        summary: &format!("Granted tool {} of {} to {who}", tool.name, server.name),
    })
    .await?;
    tx.commit().await?;
    refresh_snapshot(&state).await?;
    Ok(Json(tool_view(&state, id).await?).into_response())
}

/// The catalog of MCP servers the project publishes, as the console shows it.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct McpCatalogView {
    pub entries: Vec<crate::mcp::catalog::Entry>,
    /// Where `entries` came from: fetched since the gateway started
    /// (`remote`), kept from an earlier fetch (`cache`), or the copy that
    /// ships in the binary (`builtin`).
    pub source: crate::mcp::catalog::Source,
    #[schema(required)]
    pub fetched_at: Option<String>,
    /// Why the newest attempt did not give a newer copy, in our words.
    #[schema(required)]
    pub error: Option<String>,
    /// Whether the gateway fetches the catalog at all (`UF_MCP_CATALOG`).
    pub fetch_enabled: bool,
    /// Whether servers that run as programs on this host may be added
    /// (`UF_MCP_LOCAL=on`).
    pub local_enabled: bool,
}

async fn catalog_response(state: &AppState) -> Response {
    let view = state.catalog.view().await;
    Json(McpCatalogView {
        entries: view.entries,
        source: view.source,
        fetched_at: view.fetched_at,
        error: view.error,
        fetch_enabled: !state.catalog.is_off(),
        local_enabled: crate::mcp::local_enabled(),
    })
    .into_response()
}

#[utoipa::path(
    get,
    path = "/mcp/catalog",
    tag = "mcp",
    operation_id = "mcp_catalog_view",
    responses(
        (status = 200, description = "The catalog entries with where they came from.", body = McpCatalogView),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn catalog_view(
    State(state): State<Arc<AppState>>,
    authed: Authed,
) -> Result<Response, ApiError> {
    require(&authed.principal, &Action::ManageMcp)?;
    Ok(catalog_response(&state).await)
}

#[utoipa::path(
    post,
    path = "/mcp/catalog/refresh",
    tag = "mcp",
    operation_id = "mcp_catalog_refresh",
    responses(
        (status = 200, description = "The catalog after fetching it again. With fetching switched off nothing is requested.", body = McpCatalogView),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn catalog_refresh(
    State(state): State<Arc<AppState>>,
    authed: Authed,
) -> Result<Response, ApiError> {
    require(&authed.principal, &Action::ManageMcp)?;
    state.catalog.refresh(time::OffsetDateTime::now_utc()).await;
    Ok(catalog_response(&state).await)
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct FromCatalogRequest {
    /// The `id` of a catalog entry.
    catalog_id: String,
    /// The values of the entry's `env` items, by name. A secret item is
    /// required; the value of the one an HTTP entry authenticates with is
    /// the server's secret.
    #[serde(default)]
    #[schema(write_only)]
    secrets: BTreeMap<String, String>,
    /// The name of the server. The entry's id by default.
    name: Option<String>,
}

fn refused(field: &str, message: &str) -> ApiError {
    ApiError::validation(BTreeMap::from([(field.to_string(), message.to_string())]))
}

#[utoipa::path(
    post,
    path = "/mcp/servers/from-catalog",
    tag = "mcp",
    operation_id = "mcp_servers_from_catalog",
    request_body = FromCatalogRequest,
    responses(
        (status = 201, description = "The new server, made from the catalog entry and synced once. If that sync failed, `last_sync_error` says so; the server exists either way.", body = McpServerView),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 409, description = "`mcp_server_exists`: the name is taken.", body = super::openapi::ApiErrorBody),
        (status = 413, description = "The request body is too large.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "The entry does not exist, needs something this gateway does not have (local servers, OAuth), a secret is missing or not named by the entry, or the address is private; `fields` names each of them.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn from_catalog(
    State(state): State<Arc<AppState>>,
    authed: Authed,
    ApiJson(req): ApiJson<FromCatalogRequest>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    require(me, &Action::ManageMcp)?;
    let Some(entry) = state.catalog.entry(&req.catalog_id).await else {
        return Err(refused("catalog_id", "There is no such catalog entry."));
    };
    let name = req.name.clone().unwrap_or_else(|| entry.id.clone());
    let mut fields = BTreeMap::new();
    checked("name", validate_server_name(&name), &mut fields);
    if entry.transport != "http" {
        // Programs that run on the gateway host come with local servers.
        return Err(refused(
            "catalog_id",
            "This entry runs a program on the gateway host, which this gateway cannot do yet.",
        ));
    }
    if entry.auth == "oauth" {
        return Err(refused(
            "catalog_id",
            "This entry signs in with OAuth. Add it by hand.",
        ));
    }
    let url = entry.url.clone().unwrap_or_default();
    check_url(&state, &url, "catalog_id", &mut fields);
    // Names the entry does not list are not accepted; secrets it lists are
    // required, other values are optional.
    for given in req.secrets.keys() {
        if !entry.env.iter().any(|e| e.name == *given) {
            fields.insert(
                format!("secrets.{given}"),
                "The entry has no such item.".to_string(),
            );
        }
    }
    for item in &entry.env {
        let value = req.secrets.get(&item.name);
        if value.is_some_and(|v| v.trim().is_empty()) || (item.secret && value.is_none()) {
            fields.insert(
                format!("secrets.{}", item.name),
                "A value is required.".to_string(),
            );
        }
    }
    // The catalog names exactly one secret item for a server that signs in.
    let auth_kind = entry.auth.as_str();
    let secret = entry
        .env
        .iter()
        .find(|e| e.secret)
        .filter(|_| auth_kind != "none")
        .and_then(|e| req.secrets.get(&e.name));
    if !fields.is_empty() {
        return Err(ApiError::validation(fields));
    }
    let secret_enc = secret.map(|s| encrypt_secret(&state.cipher, s));

    let mut tx = state.store.begin().await?;
    let inserted = tx
        .insert_server(NewMcpServer {
            name: &name,
            url: &url,
            auth_kind,
            auth_header: entry
                .auth_header
                .as_deref()
                .filter(|_| auth_kind == "header"),
            auth_secret_enc: secret_enc.as_deref(),
            enabled: true,
            description: &entry.description,
        })
        .await;
    let id = match inserted {
        Ok(id) => id,
        Err(e) => {
            return Err(match e.downcast_ref::<StoreError>() {
                Some(StoreError::Duplicate) => exists_err(),
                None => e.into(),
            })
        }
    };
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "mcp.create",
        target_type: "mcp_server",
        target_id: Some(id),
        summary: &format!(
            "Created MCP server {name} from catalog entry {}{}",
            entry.id,
            if secret_enc.is_some() {
                ", secret set"
            } else {
                ""
            }
        ),
    })
    .await?;
    tx.commit().await?;
    refresh_snapshot(&state).await?;
    // One sync, so the tools are there to switch on. Its failure is recorded
    // on the server; the registration stands.
    let worker = state.clone();
    let actor = Actor::of(me);
    let _ = tokio::spawn(async move { sync_server(&worker, id, &actor).await }).await;
    let row = state
        .store
        .server_by_id(id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok((StatusCode::CREATED, Json(admin_view(&state, &row).await?)).into_response())
}
