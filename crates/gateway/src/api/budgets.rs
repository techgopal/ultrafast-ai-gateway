//! Budgets: who sees them, and the admin who sets them.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use super::{path_id, refresh_snapshot, require, ApiError, ApiJson, Authed};
use crate::app::AppState;
use crate::budgets::{seed_from_logs, usd, Budget, BudgetAction, Period};
use crate::identity::policy::{limit_access, Action, LimitAccess};
use crate::limits::LimitScope;
use crate::store::{AuditEntry, BudgetRow};

/// The most a budget may allow: a billion dollars, in millionths.
pub(crate) const MAX_AMOUNT_MICROS: i64 = 1_000_000_000_000_000;

/// The budget of one team, user, key or of the gateway for one period.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct BudgetView {
    pub id: i64,
    /// `gateway`, `team`, `user` or `key`.
    pub scope: String,
    /// The id of the team, user or key; `null` for the gateway.
    #[schema(required)]
    pub scope_id: Option<i64>,
    /// How a message names it: `gateway`, `team 'Platform'`,
    /// `user 'lena@example.com'` or `key 'ci'`.
    pub label: String,
    /// The amount, in millionths of a dollar.
    pub amount_micros: u64,
    /// `daily`, `weekly` (from Monday) or `monthly`; UTC calendar periods.
    pub period: String,
    /// `block` refuses calls once the amount is spent; `alert` allows them
    /// and writes one audit entry per period.
    pub action: String,
    /// The UTC date the current period began on, `YYYY-MM-DD`.
    pub period_start: String,
    /// What the gateway counted as spent in the current period, in
    /// millionths of a dollar. `null` when the caller may not see it: a
    /// member sees the spend of their own user and keys and of no one else's,
    /// nor of the gateway or of a team they do not lead.
    #[schema(required)]
    pub spent_micros: Option<u64>,
}

fn view(state: &AppState, r: &BudgetRow, now: OffsetDateTime) -> BudgetView {
    let budget = budget_of(r);
    BudgetView {
        id: r.id,
        scope: r.scope.as_str().to_string(),
        scope_id: r.scope_id,
        label: r.label(),
        amount_micros: r.amount_micros,
        period: r.period.as_str().to_string(),
        action: r.action.as_str().to_string(),
        period_start: r.period.start_string(now),
        spent_micros: Some(state.budgets.spent(&budget, now)),
    }
}

fn budget_of(r: &BudgetRow) -> Budget {
    Budget {
        id: r.id,
        scope: r.scope,
        scope_id: r.scope_id.unwrap_or(0),
        scope_label: r.label(),
        amount_micros: r.amount_micros,
        period: r.period,
        action: r.action,
    }
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct BudgetsPage {
    pub budgets: Vec<BudgetView>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetBudgetRequest {
    /// `gateway`, `team`, `user` or `key`.
    scope: String,
    /// The id of the team, user or key. Not sent for the gateway.
    scope_id: Option<i64>,
    /// The amount in millionths of a dollar: 1 to 1 000 000 000 000 000.
    amount_micros: i64,
    /// `daily`, `weekly` or `monthly`.
    period: String,
    /// `block` or `alert`.
    action: String,
}

/// The budgets the caller may see: all of them, with what each has spent in
/// its current period, for an admin. Anyone else sees the gateway's and
/// those of their teams without the spend (with it for a team they lead),
/// and those of themselves and their own keys with the spend.
#[utoipa::path(
    get,
    path = "/budgets",
    tag = "budgets",
    operation_id = "budgets_list",
    responses(
        (status = 200, description = "The budgets that apply to the caller.", body = BudgetsPage),
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
    require(me, &Action::ListBudgets)?;
    let now = OffsetDateTime::now_utc();
    let budgets: Vec<BudgetView> = state
        .store
        .list_budgets()
        .await?
        .iter()
        .filter(|b| b.has_subject())
        .filter_map(
            |b| match limit_access(me, b.scope, b.scope_id, b.key_owner) {
                LimitAccess::Hidden => None,
                LimitAccess::Spent => Some(view(&state, b, now)),
                LimitAccess::Figures => Some(BudgetView {
                    spent_micros: None,
                    ..view(&state, b, now)
                }),
            },
        )
        .collect();
    Ok(Json(BudgetsPage { budgets }).into_response())
}

fn field(fields: &mut BTreeMap<String, String>, name: &str, message: &str) {
    fields.insert(name.to_string(), message.to_string());
}

/// Sets the budget of a team, a user, a key or the gateway for a period:
/// the amount and what happens when it is spent. A second call for the same
/// subject and period changes that budget. Admin only. The change reaches
/// `/v1` at once.
#[utoipa::path(
    put,
    path = "/budgets",
    tag = "budgets",
    operation_id = "budgets_set",
    request_body = SetBudgetRequest,
    responses(
        (status = 200, description = "The budget after the change.", body = BudgetView),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 413, description = "The request body is too large.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "A value is not valid, or the team, user or key does not exist; `fields` names each of them.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn set(
    State(state): State<Arc<AppState>>,
    authed: Authed,
    ApiJson(req): ApiJson<SetBudgetRequest>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    require(me, &Action::ManageBudgets)?;

    let mut fields = BTreeMap::new();
    let scope = LimitScope::parse(&req.scope);
    match (scope, req.scope_id) {
        (None, _) => field(&mut fields, "scope", "must be gateway, team, user or key"),
        (Some(LimitScope::Gateway), Some(_)) => {
            field(&mut fields, "scope_id", "the gateway has no id");
        }
        (Some(LimitScope::Gateway), None) => {}
        (Some(_), None) => field(&mut fields, "scope_id", "is required"),
        (Some(_), Some(id)) if id < 1 => field(&mut fields, "scope_id", "does not exist"),
        (Some(_), Some(_)) => {}
    }
    let amount = match u64::try_from(req.amount_micros) {
        Ok(a) if (1..=MAX_AMOUNT_MICROS.unsigned_abs()).contains(&a) => Some(a),
        _ => {
            field(
                &mut fields,
                "amount_micros",
                &format!("must be from 1 to {MAX_AMOUNT_MICROS}"),
            );
            None
        }
    };
    let period = Period::parse(&req.period);
    if period.is_none() {
        field(&mut fields, "period", "must be daily, weekly or monthly");
    }
    let action = BudgetAction::parse(&req.action);
    if action.is_none() {
        field(&mut fields, "action", "must be block or alert");
    }
    let (Some(scope), Some(amount), Some(period), Some(action), true) =
        (scope, amount, period, action, fields.is_empty())
    else {
        return Err(ApiError::validation(fields));
    };

    let mut tx = state.store.begin().await?;
    // A new budget (a subject and period without one) counts what was
    // already spent; an edit of an existing one keeps its counter.
    let existed = tx
        .budget_id_of(scope, req.scope_id, period)
        .await?
        .is_some();
    let id = tx
        .upsert_budget(scope, req.scope_id, amount, period, action)
        .await?;
    let row = tx.budget_by_id(id).await?.ok_or_else(ApiError::internal)?;
    if !row.has_subject() {
        // Dropping the transaction takes the new row back.
        return Err(ApiError::invalid_field("scope_id", "does not exist"));
    }
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "budget.set",
        target_type: "budget",
        target_id: Some(id),
        summary: &format!(
            "Set the {} budget of {} to {}, action {}",
            period.as_str(),
            row.label(),
            usd(amount),
            action.as_str()
        ),
    })
    .await?;
    tx.commit().await?;
    refresh_snapshot(&state).await?;
    // A budget that is new counts what was already spent in its period.
    let now = OffsetDateTime::now_utc();
    if !existed {
        if let Err(e) = seed_from_logs(&state, &budget_of(&row), now).await {
            tracing::warn!(error = %e, "could not count the spend of a new budget from the logs");
        }
    }
    Ok(Json(view(&state, &row, now)).into_response())
}

/// Removes one budget. Admin only. The change reaches `/v1` at once.
#[utoipa::path(
    delete,
    path = "/budgets/{id}",
    tag = "budgets",
    operation_id = "budgets_delete",
    params(
        ("id" = i64, Path, description = "The id of the budget."),
    ),
    responses(
        (status = 204, description = "The budget is removed."),
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
    require(me, &Action::ManageBudgets)?;
    let id = path_id(&raw_id)?;

    let mut tx = state.store.begin().await?;
    let Some(row) = tx.budget_by_id(id).await? else {
        drop(tx);
        // An earlier call may have deleted it and failed to refresh.
        refresh_snapshot(&state).await?;
        return Err(ApiError::not_found());
    };
    if !tx.delete_budget(id).await? {
        return Err(ApiError::not_found());
    }
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "budget.delete",
        target_type: "budget",
        target_id: Some(id),
        summary: &format!(
            "Removed the {} budget of {}",
            row.period.as_str(),
            row.label()
        ),
    })
    .await?;
    tx.commit().await?;
    refresh_snapshot(&state).await?;
    state.budgets.forget(id);
    Ok(StatusCode::NO_CONTENT.into_response())
}
