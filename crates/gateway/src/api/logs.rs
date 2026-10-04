//! Request logs.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::format_description::well_known::Rfc3339;
use time::macros::format_description;
use time::{Date, OffsetDateTime, Time};

use super::{path_id, require, ApiError, Authed};
use crate::app::AppState;
use crate::identity::policy::{list_scope, Action, Scope};
use crate::store::{LogDetail, LogFilter, LogScope};
use crate::tags::{self, Tags};

/// Rows in one answer when `limit` is not given.
const DEFAULT_LIMIT: i64 = 50;
const MAX_LIMIT: i64 = 200;

#[derive(Deserialize)]
pub struct LogsQuery {
    limit: Option<String>,
    before: Option<String>,
    from: Option<String>,
    to: Option<String>,
    key_id: Option<String>,
    user_id: Option<String>,
    team_id: Option<String>,
    model: Option<String>,
    status: Option<String>,
    errors: Option<String>,
}

/// One logged call, as `/api` shows it.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct LogView {
    pub id: i64,
    /// UTC, `YYYY-MM-DD HH:MM:SS`.
    pub at: String,
    #[schema(required)]
    pub key_id: Option<i64>,
    /// `null` when the key was deleted.
    #[schema(required)]
    pub key_name: Option<String>,
    #[schema(required)]
    pub user_id: Option<i64>,
    #[schema(required)]
    pub user_email: Option<String>,
    #[schema(required)]
    pub team_id: Option<i64>,
    #[schema(required)]
    pub team_name: Option<String>,
    /// The model or route name the caller asked for.
    pub requested: String,
    pub endpoint: String,
    pub stream: bool,
    /// What the caller was answered.
    pub status: i64,
    /// The provider that answered; null when nothing answered (the model is then null too).
    #[schema(required)]
    pub provider: Option<String>,
    #[schema(required)]
    pub model: Option<String>,
    #[schema(required)]
    pub input_tokens: Option<i64>,
    #[schema(required)]
    pub output_tokens: Option<i64>,
    /// Cost in millionths of a dollar; 0 when `priced` is false.
    pub cost_micros: i64,
    pub priced: bool,
    /// Answered from the response cache: `cost_micros` is 0 and `priced`
    /// is true, and the tokens are those of the cached answer, so usage
    /// reports count them; no provider was called.
    pub cached: bool,
    /// The tokens and cost are an estimate: a stream that ended without the
    /// provider's report (the caller went away, or an error came after
    /// content was sent) is charged the input of the call and the streamed
    /// characters / 4, and `priced` stays true when the model has a price.
    pub estimated: bool,
    pub duration_ms: i64,
    /// The tags of the call: what it sent in `x-uf-tags` overlaid by its
    /// key's. Empty when none.
    #[schema(value_type = std::collections::BTreeMap<String, String>)]
    pub tags: Tags,
}

/// One target tried for a call.
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct LogAttempt {
    pub provider: String,
    pub model: String,
    /// `ok`, `retryable`, `fatal`, `circuit_open`, `skipped` or `cached` (answered from the response cache, no provider called).
    pub outcome: String,
    /// What the provider answered, when it did.
    #[schema(required)]
    pub status: Option<i64>,
    pub duration_ms: i64,
}

/// A call with the targets it tried, in order. The fields of a
/// [`LogView`], and `attempts`.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct LogDetailView {
    pub id: i64,
    /// UTC, `YYYY-MM-DD HH:MM:SS`.
    pub at: String,
    #[schema(required)]
    pub key_id: Option<i64>,
    /// `null` when the key was deleted.
    #[schema(required)]
    pub key_name: Option<String>,
    #[schema(required)]
    pub user_id: Option<i64>,
    #[schema(required)]
    pub user_email: Option<String>,
    #[schema(required)]
    pub team_id: Option<i64>,
    #[schema(required)]
    pub team_name: Option<String>,
    /// The model or route name the caller asked for.
    pub requested: String,
    pub endpoint: String,
    pub stream: bool,
    /// What the caller was answered.
    pub status: i64,
    /// The provider that answered; null when nothing answered (the model is then null too).
    #[schema(required)]
    pub provider: Option<String>,
    #[schema(required)]
    pub model: Option<String>,
    #[schema(required)]
    pub input_tokens: Option<i64>,
    #[schema(required)]
    pub output_tokens: Option<i64>,
    /// Cost in millionths of a dollar; 0 when `priced` is false.
    pub cost_micros: i64,
    pub priced: bool,
    /// Answered from the response cache: `cost_micros` is 0 and `priced`
    /// is true, and the tokens are those of the cached answer, so usage
    /// reports count them; no provider was called.
    pub cached: bool,
    /// The tokens and cost are an estimate: a stream that ended without the
    /// provider's report (the caller went away, or an error came after
    /// content was sent) is charged the input of the call and the streamed
    /// characters / 4, and `priced` stays true when the model has a price.
    pub estimated: bool,
    pub duration_ms: i64,
    /// The tags of the call: what it sent in `x-uf-tags` overlaid by its
    /// key's. Empty when none.
    #[schema(value_type = std::collections::BTreeMap<String, String>)]
    pub tags: Tags,
    pub attempts: Vec<LogAttempt>,
}

impl LogDetailView {
    fn new(l: LogView, attempts: Vec<LogAttempt>) -> Self {
        Self {
            id: l.id,
            at: l.at,
            key_id: l.key_id,
            key_name: l.key_name,
            user_id: l.user_id,
            user_email: l.user_email,
            team_id: l.team_id,
            team_name: l.team_name,
            requested: l.requested,
            endpoint: l.endpoint,
            stream: l.stream,
            status: l.status,
            provider: l.provider,
            model: l.model,
            input_tokens: l.input_tokens,
            output_tokens: l.output_tokens,
            cost_micros: l.cost_micros,
            priced: l.priced,
            cached: l.cached,
            estimated: l.estimated,
            duration_ms: l.duration_ms,
            tags: l.tags,
            attempts,
        }
    }
}

impl From<&LogDetail> for LogView {
    fn from(d: &LogDetail) -> Self {
        let r = d.row.clone();
        Self {
            id: r.id,
            at: r.at,
            key_id: r.key_id,
            key_name: d.key_name.clone(),
            user_id: r.user_id,
            user_email: d.user_email.clone(),
            team_id: r.team_id,
            team_name: d.team_name.clone(),
            requested: r.requested,
            endpoint: r.endpoint,
            stream: r.stream,
            status: r.status,
            provider: r.provider,
            model: r.model,
            input_tokens: r.input_tokens,
            output_tokens: r.output_tokens,
            cost_micros: r.cost_micros,
            priced: r.priced,
            cached: r.cached,
            estimated: r.estimated,
            duration_ms: r.duration_ms,
            tags: tags::parse_stored(r.tags.as_deref()),
        }
    }
}

/// `name:value`: the name ends at the first colon.
fn tag_filter(raw: &str) -> Result<(String, String), &'static str> {
    let Some((name, value)) = raw.split_once(':') else {
        return Err("must be name:value");
    };
    if tags::refusal_of_name(name).is_some() || tags::refusal_of_value(value).is_some() {
        return Err("must be name:value, with a name of A-Z a-z 0-9 _ . - and a value of 1 to 64 characters");
    }
    Ok((name.to_string(), value.to_string()))
}

/// A positive integer written in plain digits.
fn positive(raw: &str) -> Option<i64> {
    if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    raw.parse::<i64>().ok().filter(|n| *n > 0)
}

/// A time as the store writes it. `end` makes a plain date mean the last
/// second of that day, for an upper bound.
fn bound(raw: &str, end: bool) -> Option<String> {
    let layout = format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");
    let at = if let Ok(t) = OffsetDateTime::parse(raw, &Rfc3339) {
        t.to_offset(time::UtcOffset::UTC)
    } else {
        let date = Date::parse(raw, format_description!("[year]-[month]-[day]")).ok()?;
        let time = if end {
            Time::from_hms(23, 59, 59).ok()?
        } else {
            Time::MIDNIGHT
        };
        date.with_time(time).assume_utc()
    };
    at.format(&layout).ok()
}

/// The scope as the store takes it.
pub(super) fn store_scope(scope: Scope) -> LogScope {
    match scope {
        Scope::All => LogScope::All,
        Scope::Teams {
            team_ids,
            own_user_id,
        } => LogScope::Teams {
            team_ids,
            own_user_id,
        },
        Scope::Own { user_id } => LogScope::Own { user_id },
    }
}

/// Newest first. `before` is the id of the last row of the page before.
/// Filters only narrow what the caller may see.
///
/// What a caller sees depends on who they are. An admin sees every row. A
/// team lead sees the rows of the teams they lead, the rows of the users who
/// are members of those teams, and their own. Anyone else sees their own
/// rows. A lead's scope uses the CURRENT membership: a row linked to a team
/// only by its user leaves the lead's view when that user leaves the team.
/// Rows of keys without an owner are visible to admins only.
#[utoipa::path(
    get,
    path = "/logs",
    tag = "logs",
    operation_id = "logs_list",
    params(
        ("limit" = Option<i64>, Query, description = "How many rows to return, 1 to 200. 50 when left out."),
        ("before" = Option<i64>, Query, description = "The id of the last row of the page before."),
        ("from" = Option<String>, Query, description = "Only calls at or after this time: RFC 3339, or a date `YYYY-MM-DD` (UTC, from its start)."),
        ("to" = Option<String>, Query, description = "Only calls at or before this time: RFC 3339, or a date `YYYY-MM-DD` (UTC, to its end)."),
        ("key_id" = Option<i64>, Query, description = "Only calls made with this key."),
        ("user_id" = Option<i64>, Query, description = "Only calls of this user."),
        ("team_id" = Option<i64>, Query, description = "Only calls of this team."),
        ("model" = Option<String>, Query, description = "Only calls answered by, or asking for, this model name."),
        ("status" = Option<i64>, Query, description = "Only calls answered with this HTTP status, 100 to 599."),
        ("errors" = Option<bool>, Query, description = "`true`: only calls answered with a status of 400 or more. Combines with the other filters."),
        ("tag" = Option<Vec<String>>, Query, description = "Only calls with this tag, written `name:value` (the name ends at the first colon). Repeat it to require several tags: all must match."),
    ),
    responses(
        (status = 200, description = "The calls the caller may see, newest first.", body = super::openapi::LogPage),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "Some parameters are not valid; `fields` names each of them.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    authed: Authed,
    query: Result<Query<LogsQuery>, QueryRejection>,
    pairs: Result<Query<Vec<(String, String)>>, QueryRejection>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    require(me, &Action::ListLogs)?;
    let (Ok(Query(q)), Ok(Query(pairs))) = (query, pairs) else {
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
    let before = id("before", &q.before);
    let key_id = id("key_id", &q.key_id);
    let user_id = id("user_id", &q.user_id);
    let team_id = id("team_id", &q.team_id);
    let mut time = |name: &str, raw: &Option<String>, end: bool| {
        let raw = raw.as_deref()?;
        let parsed = bound(raw, end);
        if parsed.is_none() {
            fields.insert(
                name.to_string(),
                "must be an RFC 3339 time or a date YYYY-MM-DD".to_string(),
            );
        }
        parsed
    };
    let from = time("from", &q.from, false);
    let to = time("to", &q.to, true);
    let limit = match q.limit.as_deref().map(positive) {
        None => DEFAULT_LIMIT,
        Some(Some(n)) if n <= MAX_LIMIT => n,
        Some(_) => {
            fields.insert("limit".into(), "must be from 1 to 200".into());
            DEFAULT_LIMIT
        }
    };
    let status = match q.status.as_deref().map(positive) {
        None => None,
        Some(Some(n)) if (100..=599).contains(&n) => Some(n),
        Some(_) => {
            fields.insert("status".into(), "must be an HTTP status, 100 to 599".into());
            None
        }
    };
    let errors = match q.errors.as_deref() {
        None | Some("false") => false,
        Some("true") => true,
        Some(_) => {
            fields.insert("errors".into(), "must be true or false".into());
            false
        }
    };
    // `tag` may be repeated, which a struct cannot take.
    let mut tag_filters = Vec::new();
    for (_, raw) in pairs.iter().filter(|(name, _)| name == "tag") {
        match tag_filter(raw) {
            Ok(pair) => tag_filters.push(pair),
            Err(reason) => {
                fields.insert("tag".to_string(), reason.to_string());
            }
        }
    }
    if tag_filters.len() > tags::MAX_TAGS {
        fields.insert("tag".into(), "must be given at most 20 times".into());
    }
    if !fields.is_empty() {
        return Err(ApiError::validation(fields));
    }

    let filter = LogFilter {
        tags: tag_filters,
        errors,
        before,
        from,
        to,
        key_id,
        user_id,
        team_id,
        model: q.model.filter(|m| !m.is_empty()),
        status,
    };
    let rows = state
        .store
        .list_logs(&store_scope(list_scope(me)), &filter, limit)
        .await?;
    let logs: Vec<LogView> = rows.iter().map(LogView::from).collect();
    Ok(Json(json!({ "logs": logs })).into_response())
}

/// One call with its attempts. The same scope as the list applies; a call
/// the caller may not see is answered like one that does not exist. A lead's
/// view uses the current team membership, and rows of keys without an owner
/// are visible to admins only.
#[utoipa::path(
    get,
    path = "/logs/{id}",
    tag = "logs",
    operation_id = "logs_view",
    params(("id" = i64, Path, description = "The id of the log row.")),
    responses(
        (status = 200, description = "The call with its attempts.", body = LogDetailView),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "There is no such call, or the caller may not see it.", body = super::openapi::ApiErrorBody),
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
    let id = path_id(&raw_id)?;
    let store = &state.store;
    let detail = store.log_by_id(id).await?.ok_or_else(ApiError::not_found)?;
    let user_in_led_team = match detail.row.user_id {
        Some(user_id) => store.is_member_of_any(user_id, &me.led_teams()).await?,
        None => false,
    };
    require(
        me,
        &Action::ViewLog {
            user_id: detail.row.user_id,
            team_id: detail.row.team_id,
            user_in_led_team,
        },
    )?;
    let attempts: Vec<LogAttempt> = serde_json::from_str(&detail.row.attempts).unwrap_or_default();
    Ok(Json(LogDetailView::new(LogView::from(&detail), attempts)).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_are_utc_text() {
        assert_eq!(
            bound("2026-01-03", false).as_deref(),
            Some("2026-01-03 00:00:00")
        );
        assert_eq!(
            bound("2026-01-03", true).as_deref(),
            Some("2026-01-03 23:59:59")
        );
        assert_eq!(
            bound("2026-01-03T10:00:00+02:00", false).as_deref(),
            Some("2026-01-03 08:00:00")
        );
        assert_eq!(
            bound("2026-01-03T10:00:00.9Z", true).as_deref(),
            Some("2026-01-03 10:00:00")
        );
        for bad in ["", "x", "2026-13-01", "2026-01-03 10:00:00", "2026-1-3"] {
            assert_eq!(bound(bad, false), None, "{bad:?}");
        }
    }
}
