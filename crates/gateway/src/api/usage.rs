//! `GET /api/usage`.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use time::format_description::FormatItem;
use time::macros::format_description;
use time::{Date, Duration, OffsetDateTime};

use super::logs::store_scope;
use super::{require, ApiError, Authed};
use crate::app::AppState;
use crate::identity::policy::{list_scope, Action};
use crate::store::{UsageGroup, UsageSums};

/// Days in the range when only one end is given or none.
const DEFAULT_DAYS: i64 = 30;
/// The most days one call may span, both ends counted.
const MAX_DAYS: i64 = 366;
const DAY: &[FormatItem<'static>] = format_description!("[year]-[month]-[day]");

#[derive(Deserialize)]
pub struct UsageQuery {
    from: Option<String>,
    to: Option<String>,
    group: Option<String>,
}

/// Sums over one group of calls.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct UsageRow {
    /// The day (`YYYY-MM-DD`), the model name, or the id of the key, user or
    /// team. Empty for calls that have no key, user or team. `total` in the
    /// total row.
    pub group: String,
    /// What to show: the day, the model name, the key or team name, the
    /// user's email, `(none)` for calls without a key, user or team,
    /// `(deleted)` when that object is gone, `Total` in the total row.
    pub label: String,
    pub requests: i64,
    /// Calls answered with a status of 400 or more, except 499.
    pub errors: i64,
    /// Calls the caller abandoned before they were answered (status 499): not
    /// errors of the gateway or of a provider.
    pub cancelled: i64,
    /// Includes the tokens of cached answers (`cached` rows of the logs),
    /// which cost nothing.
    pub input_tokens: i64,
    pub output_tokens: i64,
    /// In millionths of a dollar.
    pub cost_micros: i64,
    /// Calls that reported token usage but could not be priced, so their
    /// cost is missing from `cost_micros`.
    pub unpriced_requests: i64,
}

impl From<UsageSums> for UsageRow {
    fn from(s: UsageSums) -> Self {
        Self {
            group: s.group,
            label: s.label,
            requests: s.requests,
            errors: s.errors,
            cancelled: s.cancelled,
            input_tokens: s.input_tokens,
            output_tokens: s.output_tokens,
            cost_micros: s.cost_micros,
            unpriced_requests: s.unpriced_requests,
        }
    }
}

#[derive(Serialize)]
struct UsageAnswer {
    from: String,
    to: String,
    total: UsageRow,
    rows: Vec<UsageRow>,
}

fn day(raw: &str) -> Option<Date> {
    // `Date::parse` accepts a leading plus sign or short years in some
    // layouts; the answer must round-trip.
    let date = Date::parse(raw, DAY).ok()?;
    (date.format(DAY).ok()? == raw).then_some(date)
}

/// Sums of the calls in a range of days, by day, model, key, user or team.
///
/// What a caller sees depends on who they are, as in the log list. An admin
/// sees every call. A team lead sees the calls of the teams they lead, the
/// calls of the users who are members of those teams, and their own. Anyone
/// else sees their own calls, so grouping by user or team shows only
/// themselves and the teams on their own calls. A lead's scope uses the
/// CURRENT membership: a call linked to a team only by its user leaves the
/// lead's totals when that user leaves the team. Calls of keys without an
/// owner count for admins only. Days are UTC. The range is at most 366 days;
/// it defaults to the last 30 days.
#[utoipa::path(
    get,
    path = "/usage",
    tag = "usage",
    operation_id = "usage_view",
    params(
        ("from" = Option<String>, Query, description = "First day, `YYYY-MM-DD` (UTC). 29 days before `to` when left out."),
        ("to" = Option<String>, Query, description = "Last day, `YYYY-MM-DD` (UTC), counted whole. Today when left out."),
        ("group" = Option<String>, Query, description = "`day` (the default), `model`, `key`, `user` or `team`."),
    ),
    responses(
        (status = 200, description = "The sums, one row per group, and their total.", body = super::openapi::UsagePage),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "Some parameters are not valid; `fields` names each of them.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn usage_view(
    State(state): State<Arc<AppState>>,
    authed: Authed,
    query: Result<Query<UsageQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    require(me, &Action::ListUsage)?;
    let Ok(Query(q)) = query else {
        return Err(ApiError::bad_request("The query is not valid."));
    };

    let mut fields = BTreeMap::new();
    let group = match q.group.as_deref() {
        None | Some("day") => UsageGroup::Day,
        Some("model") => UsageGroup::Model,
        Some("key") => UsageGroup::Key,
        Some("user") => UsageGroup::User,
        Some("team") => UsageGroup::Team,
        Some(_) => {
            fields.insert(
                "group".to_string(),
                "must be day, model, key, user or team".to_string(),
            );
            UsageGroup::Day
        }
    };
    let mut date = |name: &str, raw: &Option<String>| {
        let raw = raw.as_deref()?;
        let parsed = day(raw);
        if parsed.is_none() {
            fields.insert(name.to_string(), "must be a date YYYY-MM-DD".to_string());
        }
        parsed
    };
    let from = date("from", &q.from);
    let to = date("to", &q.to);
    if !fields.is_empty() {
        return Err(ApiError::validation(fields));
    }

    let span = Duration::days(DEFAULT_DAYS - 1);
    let (from, to) = match (from, to) {
        (Some(f), Some(t)) => (f, t),
        (None, Some(t)) => (t - span, t),
        (Some(f), None) => (f, OffsetDateTime::now_utc().date()),
        (None, None) => {
            let today = OffsetDateTime::now_utc().date();
            (today - span, today)
        }
    };
    // Blame the bound the caller sent; the other one was filled in.
    let blamed = if q.to.is_some() { "to" } else { "from" };
    if to < from {
        return Err(ApiError::invalid_field(
            blamed,
            "the range must not end before it starts",
        ));
    }
    if (to - from).whole_days() + 1 > MAX_DAYS {
        return Err(ApiError::invalid_field(
            blamed,
            "the range must be at most 366 days",
        ));
    }
    // The store counts the whole last day by looking at the day after it.
    if to.next_day().is_none() {
        return Err(ApiError::invalid_field("to", "must be before 9999-12-31"));
    }
    let (from, to) = (
        from.format(DAY).map_err(|e| anyhow::anyhow!(e))?,
        to.format(DAY).map_err(|e| anyhow::anyhow!(e))?,
    );

    let sums = state
        .store
        .usage(&store_scope(list_scope(me)), &from, &to, group)
        .await?;
    let mut total = UsageRow {
        group: "total".into(),
        label: "Total".into(),
        requests: 0,
        errors: 0,
        cancelled: 0,
        input_tokens: 0,
        output_tokens: 0,
        cost_micros: 0,
        unpriced_requests: 0,
    };
    for s in &sums {
        total.requests += s.requests;
        total.errors += s.errors;
        total.cancelled += s.cancelled;
        total.input_tokens += s.input_tokens;
        total.output_tokens += s.output_tokens;
        total.cost_micros += s.cost_micros;
        total.unpriced_requests += s.unpriced_requests;
    }
    let rows = sums.into_iter().map(UsageRow::from).collect();
    Ok(Json(UsageAnswer {
        from,
        to,
        total,
        rows,
    })
    .into_response())
}
