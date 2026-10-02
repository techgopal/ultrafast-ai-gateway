//! Routes: names that spread requests over models, with fallbacks,
//! timeouts, a circuit breaker and the teams that may use them.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use super::{path_id, refresh_snapshot, require, ApiError, ApiJson, Authed};
use crate::access;
use crate::app::AppState;
use crate::cache::{CacheScope, RouteCache, DEFAULT_TTL_S, TTL_RANGE};
use crate::identity::policy::Action;
use crate::identity::Principal;
use crate::store::{
    is_missing_reference, AuditEntry, RouteRow, RouteSettings, StoreError, TargetRow, TargetsInput,
    Tx,
};

const MAX_NAME_CHARS: usize = 64;

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PrimaryRequest {
    pub model_id: i64,
    /// Share of the traffic among the primaries, 1 to 1000.
    pub weight: i64,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RouteRequest {
    /// 1 to 64 characters of `a-z`, `0-9`, `.`, `_`, `-`, starting with a
    /// letter or digit. No `/`, so a route never reads as `provider/model`.
    pub name: String,
    /// At least one. A model appears at most once in the route.
    pub primaries: Vec<PrimaryRequest>,
    /// Model ids, tried in this order when the primaries fail.
    pub fallbacks: Vec<i64>,
    /// 0 to 5.
    pub retries: i64,
    /// 1 000 to 300 000.
    pub first_token_timeout_ms: i64,
    /// 1 000 to 3 600 000, not below the first token timeout.
    pub total_timeout_ms: i64,
    /// 1 to 100.
    pub breaker_failures: i64,
    /// 5 to 3 600.
    pub breaker_window_s: i64,
    /// 5 to 3 600.
    pub breaker_open_s: i64,
    /// Every user may use the route. It cannot be combined with teams.
    /// Without it and without teams only admins may use the route.
    pub everyone: bool,
    /// Teams that may use the route.
    pub team_ids: Vec<i64>,
    /// Keep the answers of calls to this route and give them again to the
    /// same call, without a provider. Streams and calls with a temperature
    /// above 0.5 are never kept. Not sent: off.
    #[serde(default)]
    pub cache_enabled: bool,
    /// How long an answer is kept, 1 to 86 400 seconds. Not sent: 300.
    #[serde(default = "default_cache_ttl_s")]
    pub cache_ttl_s: i64,
    /// Whom a kept answer is given to: `team` (the team of the key; a key
    /// with no team uses `user`, then `key`), `key` or `user`. Never across
    /// teams. Not sent: `team`.
    #[serde(default = "default_cache_scope")]
    pub cache_scope: String,
}

fn default_cache_ttl_s() -> i64 {
    DEFAULT_TTL_S
}

fn default_cache_scope() -> String {
    CacheScope::Team.as_str().to_string()
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct PrimaryView {
    /// Zero for a caller who is not an admin.
    pub model_id: i64,
    /// `provider_name/model_name`.
    pub model: String,
    /// Zero for a caller who is not an admin.
    pub weight: i64,
    /// Whether the model is enabled.
    pub enabled: bool,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct FallbackView {
    /// Zero for a caller who is not an admin.
    pub model_id: i64,
    /// `provider_name/model_name`.
    pub model: String,
    pub enabled: bool,
}

/// A route. For a caller who is not an admin, `model_id`, `weight`, every
/// setting, `everyone` and `team_ids` are hidden: they read as 0, false or
/// empty whatever they are. Only the names and flags of the targets are real.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct RouteView {
    pub id: i64,
    pub name: String,
    pub primaries: Vec<PrimaryView>,
    pub fallbacks: Vec<FallbackView>,
    pub retries: i64,
    pub first_token_timeout_ms: i64,
    pub total_timeout_ms: i64,
    pub breaker_failures: i64,
    pub breaker_window_s: i64,
    pub breaker_open_s: i64,
    /// Every user may use the route. Hidden (false) for a non-admin.
    pub everyone: bool,
    /// Hidden (empty) for a non-admin.
    pub team_ids: Vec<i64>,
    /// The route keeps answers. Hidden (false) for a non-admin.
    pub cache_enabled: bool,
    /// Seconds an answer is kept. Hidden (0) for a non-admin.
    pub cache_ttl_s: i64,
    /// `team`, `key` or `user`. Hidden (`team`) for a non-admin.
    pub cache_scope: String,
    /// No target of the route is enabled, so it cannot serve a request.
    pub broken: bool,
    pub created_at: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct RouteList {
    pub routes: Vec<RouteView>,
}

fn view_of(row: RouteRow, targets: Vec<TargetRow>, team_ids: Vec<i64>, admin: bool) -> RouteView {
    let broken = !targets.iter().any(|t| t.enabled);
    let mut primaries = Vec::new();
    let mut fallbacks = Vec::new();
    for t in targets {
        let model = format!("{}/{}", t.provider_name, t.model_name);
        let model_id = if admin { t.model_id } else { 0 };
        if t.primary {
            primaries.push(PrimaryView {
                model_id,
                model,
                weight: if admin { t.weight } else { 0 },
                enabled: t.enabled,
            });
        } else {
            fallbacks.push(FallbackView {
                model_id,
                model,
                enabled: t.enabled,
            });
        }
    }
    let s = if admin {
        row.settings
    } else {
        RouteSettings {
            retries: 0,
            first_token_timeout_ms: 0,
            total_timeout_ms: 0,
            breaker_failures: 0,
            breaker_window_s: 0,
            breaker_open_s: 0,
        }
    };
    let cache = if admin {
        row.cache
    } else {
        RouteCache {
            enabled: false,
            ttl_s: 0,
            scope: CacheScope::Team,
        }
    };
    RouteView {
        id: row.id,
        name: row.name,
        primaries,
        fallbacks,
        retries: s.retries,
        first_token_timeout_ms: s.first_token_timeout_ms,
        total_timeout_ms: s.total_timeout_ms,
        breaker_failures: s.breaker_failures,
        breaker_window_s: s.breaker_window_s,
        breaker_open_s: s.breaker_open_s,
        everyone: admin && row.everyone,
        team_ids: if admin { team_ids } else { Vec::new() },
        cache_enabled: cache.enabled,
        cache_ttl_s: cache.ttl_s,
        cache_scope: cache.scope.as_str().to_string(),
        broken,
        created_at: row.created_at,
    }
}

/// Whether the principal may use a route open to everyone or to these teams:
/// the same rule `/v1` applies (`access::route_usable`).
pub(super) fn may_use(p: &Principal, everyone: bool, team_ids: &[i64]) -> bool {
    let teams = p.team_ids();
    access::route_usable(
        access::Viewer::User {
            id: p.user_id,
            admin: p.is_admin(),
            team_ids: &teams,
        },
        &access::RouteFacts { everyone, team_ids },
    )
}

/// The view of one route as this caller sees it; `None` when it is hidden.
async fn load(state: &AppState, me: &Principal, id: i64) -> Result<Option<RouteView>, ApiError> {
    let Some(row) = state.store.route_by_id(id).await? else {
        return Ok(None);
    };
    let team_ids = state.store.route_team_ids(id).await?;
    if !me.is_admin() && !may_use(me, row.everyone, &team_ids) {
        return Ok(None);
    }
    let targets = state.store.route_targets_of(id).await?;
    Ok(Some(view_of(row, targets, team_ids, me.is_admin())))
}

fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    let first_ok = chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    first_ok
        && name.len() <= MAX_NAME_CHARS
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
}

fn in_range(field: &str, v: i64, lo: i64, hi: i64, fields: &mut BTreeMap<String, String>) {
    if !(lo..=hi).contains(&v) {
        fields.insert(field.to_string(), format!("must be {lo} to {hi}"));
    }
}

/// Checks everything that needs no database.
fn check(req: &RouteRequest) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    if !valid_name(&req.name) {
        fields.insert(
            "name".to_string(),
            "must be 1 to 64 characters of a-z, 0-9, '.', '_' and '-', starting with a letter or digit"
                .to_string(),
        );
    }
    if req.primaries.is_empty() {
        fields.insert(
            "primaries".to_string(),
            "needs at least one model".to_string(),
        );
    } else if req
        .primaries
        .iter()
        .any(|p| !(1..=1000).contains(&p.weight))
    {
        fields.insert(
            "primaries".to_string(),
            "a weight must be 1 to 1000".to_string(),
        );
    }
    let mut seen = HashSet::new();
    if !req.primaries.iter().all(|p| seen.insert(p.model_id)) {
        fields
            .entry("primaries".to_string())
            .or_insert_with(|| "a model may appear only once in a route".to_string());
    }
    if !req.fallbacks.iter().all(|m| seen.insert(*m)) {
        fields.insert(
            "fallbacks".to_string(),
            "a model may appear only once in a route".to_string(),
        );
    }
    in_range("retries", req.retries, 0, 5, &mut fields);
    in_range(
        "first_token_timeout_ms",
        req.first_token_timeout_ms,
        1_000,
        300_000,
        &mut fields,
    );
    in_range(
        "total_timeout_ms",
        req.total_timeout_ms,
        1_000,
        3_600_000,
        &mut fields,
    );
    if !fields.contains_key("total_timeout_ms")
        && !fields.contains_key("first_token_timeout_ms")
        && req.total_timeout_ms < req.first_token_timeout_ms
    {
        fields.insert(
            "total_timeout_ms".to_string(),
            "must not be below the first token timeout".to_string(),
        );
    }
    in_range(
        "breaker_failures",
        req.breaker_failures,
        1,
        100,
        &mut fields,
    );
    in_range(
        "breaker_window_s",
        req.breaker_window_s,
        5,
        3_600,
        &mut fields,
    );
    in_range("breaker_open_s", req.breaker_open_s, 5, 3_600, &mut fields);
    if !TTL_RANGE.contains(&req.cache_ttl_s) {
        fields.insert(
            "cache_ttl_s".to_string(),
            format!("must be {} to {}", TTL_RANGE.start(), TTL_RANGE.end()),
        );
    }
    if CacheScope::parse(&req.cache_scope).is_none() {
        fields.insert(
            "cache_scope".to_string(),
            "must be team, key or user".to_string(),
        );
    }
    if req.everyone && !req.team_ids.is_empty() {
        fields.insert(
            "everyone".to_string(),
            "must not be combined with teams or users".to_string(),
        );
    }
    fields
}

/// Checks the ids against the database, in the transaction that writes.
async fn check_ids(
    tx: &mut Tx<'_>,
    req: &RouteRequest,
    team_ids: &[i64],
    fields: &mut BTreeMap<String, String>,
) -> Result<(), ApiError> {
    for p in &req.primaries {
        if tx.model_by_id(p.model_id).await?.is_none() {
            fields
                .entry("primaries".to_string())
                .or_insert_with(|| "a model does not exist".to_string());
            break;
        }
    }
    for m in &req.fallbacks {
        if tx.model_by_id(*m).await?.is_none() {
            fields
                .entry("fallbacks".to_string())
                .or_insert_with(|| "a model does not exist".to_string());
            break;
        }
    }
    for t in team_ids {
        if tx.team_by_id(*t).await?.is_none() {
            fields.insert("team_ids".to_string(), "a team does not exist".to_string());
            break;
        }
    }
    Ok(())
}

fn settings_of(req: &RouteRequest) -> RouteSettings {
    RouteSettings {
        retries: req.retries,
        first_token_timeout_ms: req.first_token_timeout_ms,
        total_timeout_ms: req.total_timeout_ms,
        breaker_failures: req.breaker_failures,
        breaker_window_s: req.breaker_window_s,
        breaker_open_s: req.breaker_open_s,
    }
}

fn distinct(ids: &[i64]) -> Vec<i64> {
    let mut seen = HashSet::new();
    ids.iter().copied().filter(|i| seen.insert(*i)).collect()
}

fn route_exists() -> ApiError {
    ApiError::conflict("route_exists", "A route of this name already exists.")
}

#[utoipa::path(
    get,
    path = "/routes",
    tag = "routes",
    operation_id = "routes_list",
    responses(
        (status = 200, description = "An admin gets every route in full. Everyone else gets the routes they may use, with the names and flags of their targets only.", body = RouteList),
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
    require(me, &Action::ListRoutes)?;
    let rows = state.store.list_routes().await?;
    let mut targets: HashMap<i64, Vec<TargetRow>> = HashMap::new();
    for t in state.store.list_route_targets().await? {
        targets.entry(t.route_id).or_default().push(t);
    }
    let mut grants: HashMap<i64, Vec<i64>> = HashMap::new();
    for (route, team) in state.store.list_route_grants().await? {
        grants.entry(route).or_default().push(team);
    }
    let routes = rows
        .into_iter()
        .filter_map(|r| {
            let teams = grants.remove(&r.id).unwrap_or_default();
            if !me.is_admin() && !may_use(me, r.everyone, &teams) {
                return None;
            }
            let t = targets.remove(&r.id).unwrap_or_default();
            Some(view_of(r, t, teams, me.is_admin()))
        })
        .collect();
    Ok(Json(RouteList { routes }).into_response())
}

#[utoipa::path(
    get,
    path = "/routes/{id}",
    tag = "routes",
    operation_id = "routes_view",
    params(
        ("id" = i64, Path, description = "The id of the route."),
    ),
    responses(
        (status = 200, description = "The route as this caller sees it.", body = RouteView),
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
    let me = &authed.principal;
    require(me, &Action::ListRoutes)?;
    let id = path_id(&raw_id)?;
    let view = load(&state, me, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(view).into_response())
}

#[utoipa::path(
    post,
    path = "/routes",
    tag = "routes",
    operation_id = "routes_create",
    request_body = RouteRequest,
    responses(
        (status = 201, description = "The new route.", body = RouteView),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 409, description = "`route_exists`: a route of this name exists.", body = super::openapi::ApiErrorBody),
        (status = 413, description = "The request body is too large.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "Some fields are not valid; `fields` names each of them.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn create(
    State(state): State<Arc<AppState>>,
    authed: Authed,
    ApiJson(req): ApiJson<RouteRequest>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    require(me, &Action::ManageRoutes)?;
    let mut fields = check(&req);
    let team_ids = distinct(&req.team_ids);

    let mut tx = state.store.begin().await?;
    check_ids(&mut tx, &req, &team_ids, &mut fields).await?;
    if !fields.is_empty() {
        return Err(ApiError::validation(fields));
    }
    let id = match tx
        .insert_route(&req.name, &settings_of(&req), req.everyone)
        .await
    {
        Ok(id) => id,
        Err(e) => {
            return Err(match e.downcast_ref::<StoreError>() {
                Some(StoreError::Duplicate) => route_exists(),
                None => e.into(),
            })
        }
    };
    write_parts(&mut tx, id, &req, &team_ids).await?;
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "route.create",
        target_type: "route",
        target_id: Some(id),
        summary: &format!("Created route {}", req.name),
    })
    .await?;
    tx.commit().await?;
    refresh_snapshot(&state).await?;
    let view = load(&state, me, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok((StatusCode::CREATED, Json(view)).into_response())
}

async fn write_parts(
    tx: &mut Tx<'_>,
    id: i64,
    req: &RouteRequest,
    team_ids: &[i64],
) -> Result<(), ApiError> {
    let targets = TargetsInput {
        primaries: req
            .primaries
            .iter()
            .map(|p| (p.model_id, p.weight))
            .collect(),
        fallbacks: req.fallbacks.clone(),
    };
    // A model or team deleted since it was checked is the caller's
    // mistake, not ours.
    let gone = |e: anyhow::Error, field: &str| {
        if is_missing_reference(&e) {
            let fields =
                BTreeMap::from([(field.to_string(), "does not exist any more".to_string())]);
            ApiError::validation(fields)
        } else {
            e.into()
        }
    };
    tx.replace_targets(id, &targets)
        .await
        .map_err(|e| gone(e, "primaries"))?;
    tx.replace_route_grants(id, team_ids)
        .await
        .map_err(|e| gone(e, "team_ids"))?;
    // Checked by `check`: a scope that does not parse never gets here.
    let scope = CacheScope::parse(&req.cache_scope).unwrap_or(CacheScope::Team);
    tx.set_route_cache(
        id,
        &RouteCache {
            enabled: req.cache_enabled,
            ttl_s: req.cache_ttl_s,
            scope,
        },
    )
    .await?;
    Ok(())
}

#[utoipa::path(
    put,
    path = "/routes/{id}",
    tag = "routes",
    operation_id = "routes_update",
    params(
        ("id" = i64, Path, description = "The id of the route."),
    ),
    request_body = RouteRequest,
    responses(
        (status = 200, description = "The route after the change. Everything is replaced.", body = RouteView),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "It does not exist, or it is hidden from the caller.", body = super::openapi::ApiErrorBody),
        (status = 409, description = "`route_exists`: another route has this name.", body = super::openapi::ApiErrorBody),
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
    ApiJson(req): ApiJson<RouteRequest>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    require(me, &Action::ManageRoutes)?;
    let id = path_id(&raw_id)?;
    let mut fields = check(&req);
    let team_ids = distinct(&req.team_ids);

    let mut tx = state.store.begin().await?;
    tx.route_by_id(id).await?.ok_or_else(ApiError::not_found)?;
    check_ids(&mut tx, &req, &team_ids, &mut fields).await?;
    if !fields.is_empty() {
        return Err(ApiError::validation(fields));
    }
    match tx
        .update_route(id, &req.name, &settings_of(&req), req.everyone)
        .await
    {
        Ok(true) => {}
        Ok(false) => return Err(ApiError::not_found()),
        Err(e) => {
            return Err(match e.downcast_ref::<StoreError>() {
                Some(StoreError::Duplicate) => route_exists(),
                None => e.into(),
            })
        }
    }
    write_parts(&mut tx, id, &req, &team_ids).await?;
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "route.update",
        target_type: "route",
        target_id: Some(id),
        summary: &format!("Updated route {}", req.name),
    })
    .await?;
    tx.commit().await?;
    refresh_snapshot(&state).await?;
    let view = load(&state, me, id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(view).into_response())
}

#[utoipa::path(
    delete,
    path = "/routes/{id}",
    tag = "routes",
    operation_id = "routes_delete",
    params(
        ("id" = i64, Path, description = "The id of the route."),
    ),
    responses(
        (status = 204, description = "The route, its targets and its grants are deleted."),
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
    require(me, &Action::ManageRoutes)?;
    let id = path_id(&raw_id)?;

    let mut tx = state.store.begin().await?;
    let Some(route) = tx.route_by_id(id).await? else {
        drop(tx);
        // An earlier call may have deleted it and failed to refresh.
        refresh_snapshot(&state).await?;
        return Err(ApiError::not_found());
    };
    if !tx.delete_route(id).await? {
        return Err(ApiError::not_found());
    }
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "route.delete",
        target_type: "route",
        target_id: Some(id),
        summary: &format!("Deleted route {}", route.name),
    })
    .await?;
    tx.commit().await?;
    refresh_snapshot(&state).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
