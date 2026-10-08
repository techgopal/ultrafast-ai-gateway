//! Alert channels: where notifications go. Admins only. A channel's URL and
//! secret go in (the secret is made here) and are shown once or never; the
//! list shows the host only.

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::anyhow;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{path_id, require, trimmed_name, ApiError, ApiJson, Authed};
use crate::alerts::rules::{self, Params};
use crate::alerts::sign::new_secret;
use crate::alerts::{payload, send_once, TRY_TIMEOUT};
use crate::app::AppState;
use crate::config::{url_origin, validate_webhook_url};
use crate::identity::policy::Action;
use crate::store::{
    now, AlertEventRow, AuditEntry, ChannelRow, NewAlertEvent, RuleRow, StateRow, Store, StoreError,
};

const KINDS: [&str; 2] = ["webhook", "slack"];

// The request types hold a URL, which is a credential, so they have neither
// `Debug` nor `Serialize`: there is no way to print one.

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateChannelRequest {
    name: String,
    /// `webhook` (the gateway's own JSON) or `slack` (`{"text": ...}`, which
    /// Slack and compatible incoming webhooks read).
    kind: String,
    /// Where to post. A query string is allowed. Kept encrypted and never
    /// shown again; only its scheme, host and port are.
    #[schema(write_only)]
    url: String,
    /// On when left out.
    enabled: Option<bool>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateChannelRequest {
    name: Option<String>,
    #[schema(write_only)]
    url: Option<String>,
    enabled: Option<bool>,
}

/// A rule that sends to a channel.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ChannelRule {
    pub id: i64,
    pub name: String,
}

/// A channel as `/api` shows it: the host of its URL, never the URL or the
/// secret.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ChannelView {
    pub id: i64,
    pub name: String,
    pub kind: String,
    /// Scheme, host and port of the URL, like `https://hooks.slack.com`.
    pub url_host: String,
    pub enabled: bool,
    pub created_at: String,
    /// The rules that send to this channel.
    pub rules: Vec<ChannelRule>,
}

impl ChannelView {
    /// Built field by field: the URL and the secret are looked at, not carried.
    fn of(c: &ChannelRow, links: &[(i64, i64, String)]) -> Self {
        Self {
            id: c.id,
            name: c.name.clone(),
            kind: c.kind.clone(),
            url_host: c.url_host.clone(),
            enabled: c.enabled,
            created_at: c.created_at.clone(),
            rules: links
                .iter()
                .filter(|(channel_id, _, _)| *channel_id == c.id)
                .map(|(_, id, name)| ChannelRule {
                    id: *id,
                    name: name.clone(),
                })
                .collect(),
        }
    }
}

/// What a test delivery came to.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct TestResult {
    pub ok: bool,
    /// The HTTP status the receiver answered with, if it answered.
    #[schema(required)]
    pub status: Option<u16>,
    /// Why it failed. Never holds the URL.
    #[schema(required)]
    pub error: Option<String>,
}

async fn channel_of(store: &Store, raw_id: &str) -> Result<ChannelRow, ApiError> {
    let id = path_id(raw_id)?;
    store
        .alert_channel_by_id(id)
        .await?
        .ok_or_else(ApiError::not_found)
}

async fn view_of(store: &Store, row: &ChannelRow) -> Result<ChannelView, ApiError> {
    let links = store.alert_channel_rules().await?;
    Ok(ChannelView::of(row, &links))
}

fn check_name(name: &str, fields: &mut BTreeMap<String, String>) {
    if let Err(message) = trimmed_name(name) {
        fields.insert("name".to_string(), message.to_string());
    }
}

/// The URL is valid and has a host to show; the message never repeats it.
fn check_url(url: &str, fields: &mut BTreeMap<String, String>) -> Option<String> {
    if let Err(e) = validate_webhook_url(url) {
        fields.insert("url".to_string(), e.to_string());
        return None;
    }
    let origin = url_origin(url);
    if origin.is_none() {
        fields.insert("url".to_string(), "URL is not valid".to_string());
    }
    origin
}

fn taken() -> ApiError {
    ApiError::conflict(
        "alert_channel_exists",
        "An alert channel with this name already exists.",
    )
}

#[utoipa::path(
    get,
    path = "/alerts/channels",
    tag = "alerts",
    operation_id = "alerts_channels_list",
    responses(
        (status = 200, description = "Every channel, by name.", body = super::openapi::ChannelList),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    authed: Authed,
) -> Result<Response, ApiError> {
    require(&authed.principal, &Action::ManageAlerts)?;
    let channels = state.store.list_alert_channels().await?;
    let links = state.store.alert_channel_rules().await?;
    let channels: Vec<ChannelView> = channels
        .iter()
        .map(|c| ChannelView::of(c, &links))
        .collect();
    Ok(Json(json!({ "channels": channels })).into_response())
}

#[utoipa::path(
    post,
    path = "/alerts/channels",
    tag = "alerts",
    operation_id = "alerts_channels_create",
    request_body = CreateChannelRequest,
    responses(
        (status = 201, description = "The new channel and its signing secret. The secret is shown once, here.", body = super::openapi::CreatedChannel),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 409, description = "`alert_channel_exists`: the name is taken.", body = super::openapi::ApiErrorBody),
        (status = 413, description = "The request body is too large.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "Some fields are not valid; `fields` names each of them.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn create(
    State(state): State<Arc<AppState>>,
    authed: Authed,
    ApiJson(req): ApiJson<CreateChannelRequest>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    require(me, &Action::ManageAlerts)?;
    let mut fields = BTreeMap::new();
    check_name(&req.name, &mut fields);
    if !KINDS.contains(&req.kind.as_str()) {
        fields.insert(
            "kind".to_string(),
            "kind must be webhook or slack".to_string(),
        );
    }
    let host = check_url(&req.url, &mut fields);
    let Some(host) = host.filter(|_| fields.is_empty()) else {
        return Err(ApiError::validation(fields));
    };
    let CreateChannelRequest {
        name,
        kind,
        url,
        enabled,
    } = req;
    let name = name.trim().to_string();
    // From here on only the encrypted forms exist; the secret is shown once.
    let secret = new_secret();
    let url_enc = state.cipher.encrypt(url.as_bytes());
    let secret_enc = state.cipher.encrypt(secret.as_bytes());

    let store = &state.store;
    let mut tx = store.begin().await?;
    let id = match tx
        .insert_alert_channel(
            &name,
            &kind,
            &url_enc,
            &host,
            &secret_enc,
            enabled.unwrap_or(true),
        )
        .await
    {
        Ok(id) => id,
        Err(e) => {
            return Err(match e.downcast_ref::<StoreError>() {
                Some(StoreError::Duplicate) => taken(),
                None => e.into(),
            })
        }
    };
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "alert_channel.create",
        target_type: "alert_channel",
        target_id: Some(id),
        summary: &format!("Created alert channel {name} ({kind}) for {host}"),
    })
    .await?;
    tx.commit().await?;

    let row = store
        .alert_channel_by_id(id)
        .await?
        .ok_or_else(|| anyhow!("the channel is missing after it was created"))?;
    // The only time the secret is sent.
    let body = json!({ "channel": view_of(store, &row).await?, "secret": secret });
    Ok((StatusCode::CREATED, Json(body)).into_response())
}

#[utoipa::path(
    patch,
    path = "/alerts/channels/{id}",
    tag = "alerts",
    operation_id = "alerts_channels_update",
    params(
        ("id" = i64, Path, description = "The id of the channel."),
    ),
    request_body = UpdateChannelRequest,
    responses(
        (status = 200, description = "The channel after the change.", body = ChannelView),
        (status = 400, description = "The request is not of the expected form, or changes nothing.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "It does not exist.", body = super::openapi::ApiErrorBody),
        (status = 409, description = "`alert_channel_exists`: the name is taken.", body = super::openapi::ApiErrorBody),
        (status = 413, description = "The request body is too large.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "Some fields are not valid; `fields` names each of them.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn update(
    State(state): State<Arc<AppState>>,
    Path(raw_id): Path<String>,
    authed: Authed,
    ApiJson(req): ApiJson<UpdateChannelRequest>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    let store = &state.store;
    // The answer does not depend on the channel, so it comes before the id
    // is looked at.
    require(me, &Action::ManageAlerts)?;
    let was = channel_of(store, &raw_id).await?;
    if req.name.is_none() && req.url.is_none() && req.enabled.is_none() {
        return Err(ApiError::bad_request(
            "Send at least one of name, url and enabled.",
        ));
    }
    let mut fields = BTreeMap::new();
    if let Some(name) = &req.name {
        check_name(name, &mut fields);
    }
    let host = req.url.as_deref().and_then(|u| check_url(u, &mut fields));
    // A channel made by an import has no URL until one is set.
    if req.enabled == Some(true) && req.url.is_none() && was.url_host.is_empty() {
        fields.insert(
            "enabled".to_string(),
            "Set a URL before enabling this channel.".to_string(),
        );
    }
    if !fields.is_empty() {
        return Err(ApiError::validation(fields));
    }
    let name = req.name.as_deref().map(str::trim);
    let url = req
        .url
        .as_deref()
        .zip(host.as_deref())
        .map(|(url, host)| (state.cipher.encrypt(url.as_bytes()), host));

    let mut changes = Vec::new();
    if name.is_some_and(|n| n != was.name) {
        changes.push("name changed");
    }
    if url.is_some() {
        changes.push("URL changed");
    }
    if req.enabled.is_some_and(|e| e != was.enabled) {
        changes.push(if req.enabled == Some(true) {
            "enabled"
        } else {
            "disabled"
        });
    }
    if changes.is_empty() {
        return Ok(Json(view_of(store, &was).await?).into_response());
    }
    let mut tx = store.begin().await?;
    let updated = tx
        .update_alert_channel(
            was.id,
            name,
            url.as_ref().map(|(enc, host)| (enc.as_slice(), *host)),
            req.enabled,
        )
        .await;
    match updated {
        Ok(true) => {}
        Ok(false) => return Err(ApiError::not_found()),
        Err(e) => {
            return Err(match e.downcast_ref::<StoreError>() {
                Some(StoreError::Duplicate) => taken(),
                None => e.into(),
            })
        }
    }
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "alert_channel.update",
        target_type: "alert_channel",
        target_id: Some(was.id),
        summary: &format!("Updated alert channel {}: {}", was.name, changes.join(", ")),
    })
    .await?;
    tx.commit().await?;
    let row = store
        .alert_channel_by_id(was.id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(view_of(store, &row).await?).into_response())
}

#[utoipa::path(
    delete,
    path = "/alerts/channels/{id}",
    tag = "alerts",
    operation_id = "alerts_channels_delete",
    params(
        ("id" = i64, Path, description = "The id of the channel."),
    ),
    responses(
        (status = 204, description = "The channel is deleted; rules stop sending to it."),
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
    require(me, &Action::ManageAlerts)?;
    let target = channel_of(&state.store, &raw_id).await?;
    let mut tx = state.store.begin().await?;
    if !tx.delete_alert_channel(target.id).await? {
        return Err(ApiError::not_found());
    }
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "alert_channel.delete",
        target_type: "alert_channel",
        target_id: Some(target.id),
        summary: &format!("Deleted alert channel {}", target.name),
    })
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[utoipa::path(
    post,
    path = "/alerts/channels/{id}/rotate-secret",
    tag = "alerts",
    operation_id = "alerts_channels_rotate_secret",
    params(
        ("id" = i64, Path, description = "The id of the channel."),
    ),
    responses(
        (status = 200, description = "The new signing secret, shown once, here. The old one stops working at once.", body = super::openapi::RotatedSecret),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "It does not exist.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn rotate_secret(
    State(state): State<Arc<AppState>>,
    Path(raw_id): Path<String>,
    authed: Authed,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    require(me, &Action::ManageAlerts)?;
    let target = channel_of(&state.store, &raw_id).await?;
    let secret = new_secret();
    let secret_enc = state.cipher.encrypt(secret.as_bytes());
    let mut tx = state.store.begin().await?;
    if !tx.set_alert_channel_secret(target.id, &secret_enc).await? {
        return Err(ApiError::not_found());
    }
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "alert_channel.rotate_secret",
        target_type: "alert_channel",
        target_id: Some(target.id),
        summary: &format!(
            "Rotated the signing secret of alert channel {}",
            target.name
        ),
    })
    .await?;
    tx.commit().await?;
    Ok(Json(json!({ "secret": secret })).into_response())
}

#[utoipa::path(
    post,
    path = "/alerts/channels/{id}/test",
    tag = "alerts",
    operation_id = "alerts_channels_test",
    params(
        ("id" = i64, Path, description = "The id of the channel."),
    ),
    responses(
        (status = 200, description = "A test event was sent, once, and stored with the state `test`. `ok` says whether the receiver answered 2xx.", body = TestResult),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "It does not exist.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn test(
    State(state): State<Arc<AppState>>,
    Path(raw_id): Path<String>,
    authed: Authed,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    let store = &state.store;
    require(me, &Action::ManageAlerts)?;
    let channel = channel_of(store, &raw_id).await?;
    let read = |bytes: &[u8]| {
        state
            .cipher
            .decrypt(bytes)
            .ok()
            .and_then(|plain| String::from_utf8(plain).ok())
    };
    let (Some(url), Some(secret)) = (read(&channel.url_enc), read(&channel.secret_enc)) else {
        return Err(anyhow!("the URL or the secret of a channel could not be read").into());
    };

    // The event is stored first, so the receiver is told an id that exists.
    let mut tx = store.begin().await?;
    let event_id = tx
        .insert_alert_event(NewAlertEvent {
            rule_id: None,
            rule_name: "Test",
            kind: "test",
            subject: &format!("channel:{}", channel.id),
            state: "test",
            summary: "Test notification from the Ultrafast gateway",
            details: "{}",
            at: &now(),
        })
        .await?;
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "alert_channel.test",
        target_type: "alert_channel",
        target_id: Some(channel.id),
        summary: &format!("Sent a test to alert channel {}", channel.name),
    })
    .await?;
    tx.commit().await?;
    let event = store
        .alert_event(event_id)
        .await?
        .ok_or_else(|| anyhow!("the test event is missing after it was stored"))?;

    // One try, here: the caller is waiting to hear whether it works.
    let body = payload(&channel.kind, &event);
    let attempt = send_once(&state.http, &url, &secret, &body, TRY_TIMEOUT).await;
    let deliveries = json!([{
        "channel_id": channel.id,
        "channel_name": channel.name,
        "ok": attempt.ok,
        "status": attempt.status,
        "tries": 1,
        "error": attempt.error,
    }]);
    store
        .set_alert_event_deliveries(event_id, &deliveries.to_string())
        .await?;
    let result = TestResult {
        ok: attempt.ok,
        status: attempt.status,
        error: attempt.error,
    };
    Ok(Json(result).into_response())
}

// ------------------------------------------------------------------ rules

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateRuleRequest {
    name: String,
    /// `budget`, `error_rate` or `circuit_open`.
    kind: String,
    /// By kind. `budget`: `{"budget_id": id or null, "percent": 1-100}`.
    /// `error_rate`: `{"scope": "gateway"|"route"|"provider"|"key",
    /// "subject": name or null, "percent": 1-100, "window_minutes": 5-60
    /// (5), "min_requests": 1-100000 (20)}`. `circuit_open`:
    /// `{"provider": name or null, "model": name or null}`. Unknown fields
    /// are refused.
    #[schema(value_type = Object)]
    params: Value,
    /// The channels that are told, by id.
    channel_ids: Vec<i64>,
    /// On when left out.
    enabled: Option<bool>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateRuleRequest {
    name: Option<String>,
    /// Only the kind the rule has; it cannot change.
    kind: Option<String>,
    /// Replaces the parameters. What the rule is firing for is forgotten.
    #[schema(value_type = Option<Object>)]
    params: Option<Value>,
    channel_ids: Option<Vec<i64>>,
    enabled: Option<bool>,
}

/// A subject a rule is firing for.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Firing {
    /// `budget:<id>:<period start>`, `route:<name>`, `provider:<name>`,
    /// `key:<id>`, `gateway` or `target:<provider>/<model>`.
    pub subject: String,
    /// UTC, `YYYY-MM-DD HH:MM:SS`.
    pub since: String,
}

/// A rule as `/api` shows it.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct RuleView {
    pub id: i64,
    pub name: String,
    pub kind: String,
    /// The parameters with every default written out.
    #[schema(value_type = Object)]
    pub params: Value,
    pub enabled: bool,
    pub created_at: String,
    /// The channels it sends to.
    pub channels: Vec<ChannelRule>,
    /// What it is firing for now.
    pub firing: Vec<Firing>,
}

fn rule_view(r: &RuleRow, channels: &[ChannelRow], states: &[StateRow]) -> RuleView {
    RuleView {
        id: r.id,
        name: r.name.clone(),
        kind: r.kind.clone(),
        params: serde_json::from_str(&r.params).unwrap_or_else(|_| json!({})),
        enabled: r.enabled,
        created_at: r.created_at.clone(),
        channels: r
            .channel_ids
            .iter()
            .filter_map(|id| channels.iter().find(|c| c.id == *id))
            .map(|c| ChannelRule {
                id: c.id,
                name: c.name.clone(),
            })
            .collect(),
        firing: states
            .iter()
            .filter(|s| s.rule_id == r.id)
            .map(|s| Firing {
                subject: s.subject.clone(),
                since: s.since.clone(),
            })
            .collect(),
    }
}

async fn rule_response(store: &Store, id: i64) -> Result<RuleView, ApiError> {
    let row = store
        .alert_rule_by_id(id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let channels = store.list_alert_channels().await?;
    let states = store.alert_states().await?;
    Ok(rule_view(&row, &channels, &states))
}

fn rule_taken() -> ApiError {
    ApiError::conflict(
        "alert_rule_exists",
        "An alert rule with this name already exists.",
    )
}

/// The engine reads its rules again; returns when it has.
async fn reload_engine(state: &AppState) {
    if let Some(engine) = &state.alert_engine {
        engine.reload().await;
    }
}

/// Checks the parameters of a rule and what they point at. The message names
/// the problem, never more.
async fn checked_params(
    store: &Store,
    kind: &str,
    params: &Value,
    fields: &mut BTreeMap<String, String>,
) -> Option<String> {
    match rules::parse(kind, params) {
        Err(e) => {
            fields.insert(e.field, e.message);
            None
        }
        Ok(parsed) => {
            if let Params::Budget {
                budget_id: Some(id),
                ..
            } = &parsed
            {
                match store.list_budgets().await {
                    Ok(budgets) if budgets.iter().any(|b| b.id == *id) => {}
                    Ok(_) => {
                        fields.insert("params.budget_id".to_string(), "no such budget".to_string());
                        return None;
                    }
                    Err(_) => return None,
                }
            }
            Some(parsed.to_value().to_string())
        }
    }
}

async fn checked_channels(
    store: &Store,
    ids: &[i64],
    fields: &mut BTreeMap<String, String>,
) -> Result<Vec<i64>, ApiError> {
    let known = store.list_alert_channels().await?;
    let mut out: Vec<i64> = Vec::new();
    for id in ids {
        if !known.iter().any(|c| c.id == *id) {
            fields.insert("channel_ids".to_string(), "No such channel.".to_string());
        } else if !out.contains(id) {
            out.push(*id);
        }
    }
    Ok(out)
}

#[utoipa::path(
    get,
    path = "/alerts/rules",
    tag = "alerts",
    operation_id = "alerts_rules_list",
    responses(
        (status = 200, description = "Every rule, by name, with what it is firing for.", body = super::openapi::RuleList),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn rules_list(
    State(state): State<Arc<AppState>>,
    authed: Authed,
) -> Result<Response, ApiError> {
    require(&authed.principal, &Action::ManageAlerts)?;
    let rows = state.store.list_alert_rules().await?;
    let channels = state.store.list_alert_channels().await?;
    let states = state.store.alert_states().await?;
    let rules: Vec<RuleView> = rows
        .iter()
        .map(|r| rule_view(r, &channels, &states))
        .collect();
    Ok(Json(json!({ "rules": rules })).into_response())
}

#[utoipa::path(
    post,
    path = "/alerts/rules",
    tag = "alerts",
    operation_id = "alerts_rules_create",
    request_body = CreateRuleRequest,
    responses(
        (status = 201, description = "The new rule.", body = RuleView),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 409, description = "`alert_rule_exists`: the name is taken.", body = super::openapi::ApiErrorBody),
        (status = 413, description = "The request body is too large.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "Some fields are not valid; `fields` names each of them.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn rules_create(
    State(state): State<Arc<AppState>>,
    authed: Authed,
    ApiJson(req): ApiJson<CreateRuleRequest>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    require(me, &Action::ManageAlerts)?;
    let store = &state.store;
    let mut fields = BTreeMap::new();
    check_name(&req.name, &mut fields);
    if !rules::KINDS.contains(&req.kind.as_str()) {
        fields.insert(
            "kind".to_string(),
            "kind must be budget, error_rate or circuit_open".to_string(),
        );
    }
    let params = if fields.contains_key("kind") {
        None
    } else {
        checked_params(store, &req.kind, &req.params, &mut fields).await
    };
    let channel_ids = checked_channels(store, &req.channel_ids, &mut fields).await?;
    let Some(params) = params.filter(|_| fields.is_empty()) else {
        return Err(ApiError::validation(fields));
    };
    let name = req.name.trim().to_string();
    let mut tx = store.begin().await?;
    let id = match tx
        .insert_alert_rule(&name, &req.kind, &params, req.enabled.unwrap_or(true))
        .await
    {
        Ok(id) => id,
        Err(e) => {
            return Err(match e.downcast_ref::<StoreError>() {
                Some(StoreError::Duplicate) => rule_taken(),
                None => e.into(),
            })
        }
    };
    tx.set_alert_rule_channels(id, &channel_ids).await?;
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "alert_rule.create",
        target_type: "alert_rule",
        target_id: Some(id),
        summary: &format!("Created alert rule {name} ({})", req.kind),
    })
    .await?;
    tx.commit().await?;
    reload_engine(&state).await;
    Ok((StatusCode::CREATED, Json(rule_response(store, id).await?)).into_response())
}

#[utoipa::path(
    patch,
    path = "/alerts/rules/{id}",
    tag = "alerts",
    operation_id = "alerts_rules_update",
    params(
        ("id" = i64, Path, description = "The id of the rule."),
    ),
    request_body = UpdateRuleRequest,
    responses(
        (status = 200, description = "The rule after the change.", body = RuleView),
        (status = 400, description = "The request is not of the expected form, or changes nothing.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "It does not exist.", body = super::openapi::ApiErrorBody),
        (status = 409, description = "`alert_rule_exists`: the name is taken.", body = super::openapi::ApiErrorBody),
        (status = 413, description = "The request body is too large.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "Some fields are not valid; `fields` names each of them.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn rules_update(
    State(state): State<Arc<AppState>>,
    Path(raw_id): Path<String>,
    authed: Authed,
    ApiJson(req): ApiJson<UpdateRuleRequest>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    let store = &state.store;
    require(me, &Action::ManageAlerts)?;
    let id = path_id(&raw_id)?;
    let was = store
        .alert_rule_by_id(id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if req.name.is_none()
        && req.params.is_none()
        && req.channel_ids.is_none()
        && req.enabled.is_none()
        && req.kind.is_none()
    {
        return Err(ApiError::bad_request(
            "Send at least one of name, params, channel_ids and enabled.",
        ));
    }
    let mut fields = BTreeMap::new();
    if let Some(name) = &req.name {
        check_name(name, &mut fields);
    }
    if req.kind.as_deref().is_some_and(|k| k != was.kind) {
        fields.insert("kind".to_string(), "kind cannot be changed".to_string());
    }
    let params = match &req.params {
        Some(p) => checked_params(store, &was.kind, p, &mut fields).await,
        None => None,
    };
    let channel_ids = match &req.channel_ids {
        Some(ids) => Some(checked_channels(store, ids, &mut fields).await?),
        None => None,
    };
    if !fields.is_empty() {
        return Err(ApiError::validation(fields));
    }
    let name = req.name.as_deref().map(str::trim);

    let mut changes = Vec::new();
    if name.is_some_and(|n| n != was.name) {
        changes.push("name changed");
    }
    if params.as_deref().is_some_and(|p| {
        serde_json::from_str::<Value>(p).ok() != serde_json::from_str::<Value>(&was.params).ok()
    }) {
        changes.push("parameters changed");
    }
    if channel_ids.as_ref().is_some_and(|c| {
        let mut a = c.clone();
        let mut b = was.channel_ids.clone();
        a.sort_unstable();
        b.sort_unstable();
        a != b
    }) {
        changes.push("channels changed");
    }
    if req.enabled.is_some_and(|e| e != was.enabled) {
        changes.push(if req.enabled == Some(true) {
            "enabled"
        } else {
            "disabled"
        });
    }
    if changes.is_empty() {
        return Ok(Json(rule_response(store, id).await?).into_response());
    }
    let params_changed = changes.contains(&"parameters changed");
    let mut tx = store.begin().await?;
    let updated = tx
        .update_alert_rule(
            id,
            name,
            params.as_deref().filter(|_| params_changed),
            req.enabled,
        )
        .await;
    match updated {
        Ok(true) => {}
        Ok(false) => return Err(ApiError::not_found()),
        Err(e) => {
            return Err(match e.downcast_ref::<StoreError>() {
                Some(StoreError::Duplicate) => rule_taken(),
                None => e.into(),
            })
        }
    }
    if let Some(ids) = &channel_ids {
        tx.set_alert_rule_channels(id, ids).await?;
    }
    if req.enabled == Some(false) && was.enabled {
        // Off: what it was firing for is forgotten, without a notice that it
        // resolved. Switched on again, it starts fresh.
        tx.clear_alert_states(id).await?;
    } else if params_changed {
        // A new condition is a new episode: what the old one fired for is
        // forgotten, without a notice that it resolved.
        tx.clear_alert_states(id).await?;
    }
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "alert_rule.update",
        target_type: "alert_rule",
        target_id: Some(id),
        summary: &format!("Updated alert rule {}: {}", was.name, changes.join(", ")),
    })
    .await?;
    tx.commit().await?;
    reload_engine(&state).await;
    Ok(Json(rule_response(store, id).await?).into_response())
}

#[utoipa::path(
    delete,
    path = "/alerts/rules/{id}",
    tag = "alerts",
    operation_id = "alerts_rules_delete",
    params(
        ("id" = i64, Path, description = "The id of the rule."),
    ),
    responses(
        (status = 204, description = "The rule is deleted; its events stay."),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "It does not exist.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn rules_delete(
    State(state): State<Arc<AppState>>,
    Path(raw_id): Path<String>,
    authed: Authed,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    require(me, &Action::ManageAlerts)?;
    let id = path_id(&raw_id)?;
    let was = state
        .store
        .alert_rule_by_id(id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let mut tx = state.store.begin().await?;
    if !tx.delete_alert_rule(id).await? {
        return Err(ApiError::not_found());
    }
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "alert_rule.delete",
        target_type: "alert_rule",
        target_id: Some(id),
        summary: &format!("Deleted alert rule {}", was.name),
    })
    .await?;
    tx.commit().await?;
    reload_engine(&state).await;
    Ok(StatusCode::NO_CONTENT.into_response())
}

// ----------------------------------------------------------------- events

const DEFAULT_EVENTS: i64 = 50;
const MAX_EVENTS: i64 = 200;

#[derive(Deserialize)]
pub struct EventsQuery {
    rule_id: Option<String>,
    state: Option<String>,
    limit: Option<String>,
    before_id: Option<String>,
}

/// One notification, as `/api` shows it. Metadata only.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct EventView {
    pub id: i64,
    /// `null` once the rule is deleted.
    #[schema(required)]
    pub rule_id: Option<i64>,
    pub rule_name: String,
    pub kind: String,
    pub subject: String,
    /// `firing`, `resolved` or `test`.
    pub state: String,
    pub summary: String,
    /// What the alert is about, by kind.
    #[schema(value_type = Object)]
    pub details: Value,
    /// UTC, `YYYY-MM-DD HH:MM:SS`.
    pub at: String,
    /// One entry per channel once every delivery finished:
    /// `{channel_id, channel_name, ok, status, tries, error}`. Empty before.
    #[schema(value_type = Vec<Object>)]
    pub deliveries: Value,
}

fn event_view(e: &AlertEventRow) -> EventView {
    EventView {
        id: e.id,
        rule_id: e.rule_id,
        rule_name: e.rule_name.clone(),
        kind: e.kind.clone(),
        subject: e.subject.clone(),
        state: e.state.clone(),
        summary: e.summary.clone(),
        details: serde_json::from_str(&e.details).unwrap_or_else(|_| json!({})),
        at: e.at.clone(),
        deliveries: serde_json::from_str(&e.deliveries).unwrap_or_else(|_| json!([])),
    }
}

fn positive(raw: &str) -> Option<i64> {
    if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    raw.parse::<i64>().ok().filter(|n| *n > 0)
}

#[utoipa::path(
    get,
    path = "/alerts/events",
    tag = "alerts",
    operation_id = "alerts_events_list",
    params(
        ("rule_id" = Option<i64>, Query, description = "Only the events of this rule."),
        ("state" = Option<String>, Query, description = "`firing`, `resolved` or `test`."),
        ("limit" = Option<i64>, Query, description = "From 1 to 200; 50 when left out."),
        ("before_id" = Option<i64>, Query, description = "Only events older than this id: the next page."),
    ),
    responses(
        (status = 200, description = "Events, newest first.", body = super::openapi::EventList),
        (status = 400, description = "The query is not valid.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "Some query fields are not valid; `fields` names each of them.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn events_list(
    State(state): State<Arc<AppState>>,
    authed: Authed,
    query: Result<Query<EventsQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    require(&authed.principal, &Action::ManageAlerts)?;
    let Ok(Query(q)) = query else {
        return Err(ApiError::bad_request("The query is not valid."));
    };
    let mut fields = BTreeMap::new();
    let mut id = |name: &str, raw: &Option<String>| match raw.as_deref().map(positive) {
        None => None,
        Some(Some(n)) => Some(n),
        Some(None) => {
            fields.insert(name.to_string(), "must be a positive integer".to_string());
            None
        }
    };
    let rule_id = id("rule_id", &q.rule_id);
    let before_id = id("before_id", &q.before_id);
    let limit = match q.limit.as_deref().map(positive) {
        None => DEFAULT_EVENTS,
        Some(Some(n)) if n <= MAX_EVENTS => n,
        Some(_) => {
            fields.insert("limit".to_string(), "must be from 1 to 200".to_string());
            DEFAULT_EVENTS
        }
    };
    let event_state = match q.state.as_deref() {
        None => None,
        Some(s @ ("firing" | "resolved" | "test")) => Some(s),
        Some(_) => {
            fields.insert(
                "state".to_string(),
                "must be firing, resolved or test".to_string(),
            );
            None
        }
    };
    if !fields.is_empty() {
        return Err(ApiError::validation(fields));
    }
    let rows = state
        .store
        .alert_events_page(rule_id, event_state, before_id, limit)
        .await?;
    let events: Vec<EventView> = rows.iter().map(event_view).collect();
    Ok(Json(json!({ "events": events })).into_response())
}
