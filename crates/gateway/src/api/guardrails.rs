//! Guardrails: rules that block, redact or flag the text going to and coming
//! from models, or an external webhook that decides. Admins only. An
//! external guardrail's URL goes in and its signing secret is made here;
//! the secret is shown once, the URL never (only its host).

use std::collections::BTreeMap;
use std::ops::RangeInclusive;
use std::sync::Arc;

use anyhow::anyhow;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::alerts::check_url;
use super::{path_id, refresh_snapshot, require, trimmed_name, ApiError, ApiJson, Authed};
use crate::alerts::sign::new_secret;
use crate::app::AppState;
use crate::guardrails::external::CallMeta;
use crate::guardrails::run::{Active, Hooks};
use crate::guardrails::{
    check_texts, Compiled, Direction, Directions, Outcome, RuleSpec, MAX_RULES,
};
use crate::identity::policy::Action;
use crate::snapshot::{external_of, SnapGuardrail, Snapshot};
use crate::store::{AuditEntry, GuardrailPatch, GuardrailRow, NewGuardrail, Store, StoreError, Tx};

/// The kinds of guardrail.
pub(crate) const KINDS: [&str; 2] = ["rules", "external"];
const FAIL_MODES: [&str; 2] = ["open", "closed"];
/// Longest description, in characters.
pub const MAX_DESCRIPTION_CHARS: usize = 500;
/// Longest rule id, in characters.
pub const MAX_RULE_ID_CHARS: usize = 64;
/// Most guardrails one route or key may have.
pub const MAX_ATTACHED: usize = 20;
/// How long an external guardrail may take, in milliseconds.
pub const TIMEOUT_RANGE: RangeInclusive<i64> = 1_000..=10_000;
/// The default timeout of an external guardrail.
pub const DEFAULT_TIMEOUT_MS: i64 = 3_000;
/// The longest an error message about a rule is, in characters.
const MAX_MESSAGE_CHARS: usize = 400;
/// The name the test endpoint gives rules sent with the request.
const TEST_NAME: &str = "Test rules";
/// The longest text the test endpoint takes, in characters.
const MAX_TEST_CHARS: usize = 20_000;
/// The model name a hook is told when it is called from the test.
const TEST_MODEL: &str = "guardrail-test";

// The request types of an external guardrail hold a URL, which is a
// credential, so they have neither `Debug` nor `Serialize`.

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateGuardrailRequest {
    /// Unique, 1 to 100 characters.
    name: String,
    /// Up to 500 characters. Left out: none.
    description: Option<String>,
    /// `rules` (keywords, regular expressions and PII detectors, run in the
    /// gateway) or `external` (a signed webhook that decides).
    kind: String,
    /// On when left out.
    enabled: Option<bool>,
    /// Applies to every call of the gateway. Off when left out.
    is_default: Option<bool>,
    /// `rules` only: 1 to 50 rules.
    #[schema(value_type = Option<Vec<RuleSpec>>)]
    rules: Option<Vec<serde_json::Value>>,
    /// `external` only, required: where to post the text. Kept encrypted and
    /// never shown again; only its scheme, host and port are.
    #[schema(write_only)]
    url: Option<String>,
    /// `external` only: how long to wait, 1 000 to 10 000. Left out: 3 000.
    timeout_ms: Option<i64>,
    /// `external` only: what to do when the call fails, `open` (let the text
    /// through and flag it) or `closed` (block). Left out: `open`.
    fail_mode: Option<String>,
    /// `external` only: what it is asked about. Left out: `both`.
    directions: Option<Directions>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateGuardrailRequest {
    name: Option<String>,
    description: Option<String>,
    enabled: Option<bool>,
    is_default: Option<bool>,
    /// `rules` only: replaces all the rules.
    #[schema(value_type = Option<Vec<RuleSpec>>)]
    rules: Option<Vec<serde_json::Value>>,
    /// `external` only: a new URL.
    #[schema(write_only)]
    url: Option<String>,
    timeout_ms: Option<i64>,
    fail_mode: Option<String>,
    directions: Option<Directions>,
}

/// A guardrail by id and name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct GuardrailRef {
    pub id: i64,
    pub name: String,
}

/// A guardrail as `/api` shows it: the host of an external one's URL, never
/// the URL or the secret.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct GuardrailView {
    pub id: i64,
    pub name: String,
    pub description: String,
    /// `rules` or `external`.
    pub kind: String,
    pub enabled: bool,
    /// Applies to every call of the gateway.
    pub is_default: bool,
    /// The rules of a `rules` guardrail; empty for an `external` one.
    pub rules: Vec<RuleSpec>,
    /// `external`: scheme, host and port of the URL, like
    /// `https://guard.example.com`; empty until a URL is set (a guardrail an
    /// import made). `null` for `rules`.
    #[schema(required)]
    pub url_host: Option<String>,
    /// `external` only; `null` for `rules`.
    #[schema(required)]
    pub timeout_ms: Option<i64>,
    /// `open` or `closed`; `external` only.
    #[schema(required)]
    pub fail_mode: Option<String>,
    /// What an external guardrail is asked about; `null` for `rules`.
    #[schema(required)]
    pub directions: Option<Directions>,
    pub created_at: String,
    /// The routes it is attached to.
    pub routes: Vec<GuardrailRef>,
    /// How many keys it is attached to.
    pub key_count: i64,
    /// Whether the gateway can use it as stored. `false` for an external
    /// guardrail whose URL or secret cannot be read (it fails by its mode on
    /// every check), and for an enabled rules guardrail whose rules the
    /// gateway is not running (they do not compile). A disabled one is `true`.
    pub usable: bool,
}

/// A created guardrail and, for an external one, its signing secret.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct CreatedGuardrail {
    pub guardrail: GuardrailView,
    /// The signing secret of an `external` guardrail. It is shown once, in
    /// this answer, and cannot be read again. `null` for `rules`.
    #[schema(required)]
    pub secret: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct GuardrailList {
    pub guardrails: Vec<GuardrailView>,
}

/// What a test sends: rules, or the id of a stored guardrail.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GuardrailTestRequest {
    /// Rules to try, as for a new guardrail. Send these or `guardrail_id`.
    #[schema(value_type = Option<Vec<RuleSpec>>)]
    rules: Option<Vec<serde_json::Value>>,
    /// A stored guardrail, enabled or not.
    guardrail_id: Option<i64>,
    /// `input` or `output`: which rules apply.
    direction: Direction,
    /// Up to 20 000 characters.
    text: String,
    /// Call the external guardrail `guardrail_id` for real, as a call would
    /// (signed, with its timeout and fail mode): the text is sent to its
    /// URL, and the outcome says what it decided or, in `flags`, why it
    /// could not (`external_error:<reason>`). The hook is asked only when
    /// the guardrail covers `direction`. Without this, an external guardrail
    /// is never called by a test, and sending its id is refused (422).
    /// Not for `rules`.
    #[serde(default)]
    call_external: bool,
}

/// A rule that flagged the text.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct FlagView {
    /// 0 for rules sent with the request.
    pub guardrail_id: i64,
    pub guardrail_name: String,
    pub rule_id: String,
}

/// What a check found. Holds counts and ids only, never matched text.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct OutcomeView {
    /// The first guardrail that blocks the text; `id` is 0 for rules sent
    /// with the request.
    #[schema(required)]
    pub blocked_by: Option<GuardrailRef>,
    /// Replacements made, by PII type (`EMAIL`) or by rule id.
    pub redactions: BTreeMap<String, u32>,
    pub flags: Vec<FlagView>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct GuardrailTestResult {
    pub outcome: OutcomeView,
    /// The text as the guardrail leaves it: redacted, or unchanged when it
    /// is blocked or only flagged.
    pub redacted_text: String,
}

pub(super) fn ref_of(row: &GuardrailRow) -> GuardrailRef {
    GuardrailRef {
        id: row.id,
        name: row.name.clone(),
    }
}

/// The rules of a stored guardrail. They were validated when written; a row
/// that cannot be read has none (and is left out of the snapshot).
pub(crate) fn rules_of(row: &GuardrailRow) -> Vec<RuleSpec> {
    match serde_json::from_str(&row.rules) {
        Ok(rules) => rules,
        Err(_) => {
            tracing::error!(
                guardrail_id = row.id,
                "the rules of a guardrail cannot be read"
            );
            Vec::new()
        }
    }
}

/// Whether the snapshot holds `row` the way it is stored and can use it.
fn usable_in(snapshot: &Snapshot, row: &GuardrailRow) -> bool {
    if !row.enabled {
        return true;
    }
    match snapshot.guardrail(row.id) {
        None => false,
        Some(g) => match &g.external {
            Some(external) => external.usable,
            None => g.rules_text == row.rules,
        },
    }
}

fn view_of(
    row: &GuardrailRow,
    routes: &[(i64, i64, String)],
    key_counts: &[(i64, i64)],
    snapshot: &Snapshot,
) -> GuardrailView {
    let external = row.kind == "external";
    GuardrailView {
        id: row.id,
        name: row.name.clone(),
        description: row.description.clone(),
        kind: row.kind.clone(),
        enabled: row.enabled,
        is_default: row.is_default,
        rules: if external { Vec::new() } else { rules_of(row) },
        url_host: external.then(|| row.url_host.clone().unwrap_or_default()),
        timeout_ms: external.then_some(row.timeout_ms),
        fail_mode: external.then(|| row.fail_mode.clone()),
        directions: external
            .then(|| Directions::parse(&row.directions))
            .flatten(),
        created_at: row.created_at.clone(),
        routes: routes
            .iter()
            .filter(|(guardrail, _, _)| *guardrail == row.id)
            .map(|(_, id, name)| GuardrailRef {
                id: *id,
                name: name.clone(),
            })
            .collect(),
        key_count: key_counts
            .iter()
            .find(|(guardrail, _)| *guardrail == row.id)
            .map_or(0, |(_, n)| *n),
        usable: usable_in(snapshot, row),
    }
}

async fn full_view(state: &AppState, row: &GuardrailRow) -> Result<GuardrailView, ApiError> {
    let routes = state.store.guardrail_routes().await?;
    let keys = state.store.guardrail_key_counts().await?;
    Ok(view_of(row, &routes, &keys, &state.snapshot.load()))
}

async fn guardrail_of(store: &Store, raw_id: &str) -> Result<GuardrailRow, ApiError> {
    let id = path_id(raw_id)?;
    store
        .guardrail_by_id(id)
        .await?
        .ok_or_else(ApiError::not_found)
}

fn taken() -> ApiError {
    ApiError::conflict(
        "guardrail_exists",
        "A guardrail with this name already exists.",
    )
}

fn cut(message: String) -> String {
    if message.chars().count() <= MAX_MESSAGE_CHARS {
        return message;
    }
    let mut cut: String = message.chars().take(MAX_MESSAGE_CHARS).collect();
    cut.push('…');
    cut
}

pub(crate) fn check_name(name: &str, fields: &mut BTreeMap<String, String>) {
    if let Err(message) = trimmed_name(name) {
        fields.insert("name".to_string(), message.to_string());
    }
}

pub(crate) fn check_description(description: &str, fields: &mut BTreeMap<String, String>) {
    if description.chars().count() > MAX_DESCRIPTION_CHARS
        || description.chars().any(|c| c.is_control() && c != '\n')
    {
        fields.insert(
            "description".to_string(),
            format!("description must be at most {MAX_DESCRIPTION_CHARS} characters"),
        );
    }
}

pub(crate) fn check_timeout(ms: i64, fields: &mut BTreeMap<String, String>) {
    if !TIMEOUT_RANGE.contains(&ms) {
        fields.insert(
            "timeout_ms".to_string(),
            format!(
                "must be {} to {}",
                TIMEOUT_RANGE.start(),
                TIMEOUT_RANGE.end()
            ),
        );
    }
}

pub(crate) fn check_fail_mode(mode: &str, fields: &mut BTreeMap<String, String>) {
    if !FAIL_MODES.contains(&mode) {
        fields.insert(
            "fail_mode".to_string(),
            "must be open or closed".to_string(),
        );
    }
}

/// Reads the rules of a request. A rule that cannot be read is named in
/// `fields` (`rules[i]`, or `rules[i].kind` for a matcher that is not
/// keywords, regex or pii, or `rules[i].types` for a PII type that is not
/// known); nothing sent is repeated in the message. Returns `Some(empty)`
/// then, and callers skip their own rule checks (see [`rules_faulty`]).
pub(crate) fn parse_rules(
    given: Option<Vec<serde_json::Value>>,
    fields: &mut BTreeMap<String, String>,
) -> Option<Vec<RuleSpec>> {
    let given = given?;
    let mut rules = Vec::new();
    for (i, value) in given.into_iter().enumerate() {
        match serde_json::from_value::<RuleSpec>(value) {
            Ok(rule) => rules.push(rule),
            Err(e) => {
                let text = e.to_string();
                let (key, message) = if text.contains("`keywords`") {
                    (
                        format!("rules[{i}].kind"),
                        "matcher must be keywords, regex or pii",
                    )
                } else if text.contains("`EMAIL`") {
                    (
                        format!("rules[{i}].types"),
                        "unknown PII type; use EMAIL, PHONE, CREDIT_CARD, IBAN, US_SSN, IPV4, IPV6 or SECRET",
                    )
                } else {
                    (
                        format!("rules[{i}]"),
                        "the rule is not valid: it needs id, matcher, action and directions, and nothing else",
                    )
                };
                fields.insert(key, message.to_string());
            }
        }
    }
    if rules_faulty(fields) {
        return Some(Vec::new());
    }
    Some(rules)
}

/// Whether a rule of the request was already refused.
pub(crate) fn rules_faulty(fields: &BTreeMap<String, String>) -> bool {
    fields.keys().any(|k| k.starts_with("rules["))
}

/// Checks the rules of a guardrail: that there are some, that each compiles
/// and that together they do. A rule that does not compile is named
/// (`rules[2]`); a fault of the set (a repeated id, too many) is `rules`.
pub(crate) fn check_rules_sync(rules: &[RuleSpec], fields: &mut BTreeMap<String, String>) {
    if rules.is_empty() {
        fields.insert("rules".to_string(), "add at least one rule".to_string());
    } else if rules.len() > MAX_RULES {
        fields.insert(
            "rules".to_string(),
            format!("a guardrail holds at most {MAX_RULES} rules"),
        );
    } else {
        fields.extend(rule_faults(rules));
    }
}

/// [`check_rules_sync`] off the async threads: compiling a large keyword
/// list can take a while.
pub(crate) async fn check_rules(
    rules: &[RuleSpec],
    fields: &mut BTreeMap<String, String>,
) -> Result<(), ApiError> {
    let owned = rules.to_vec();
    let found = tokio::task::spawn_blocking(move || {
        let mut found = BTreeMap::new();
        check_rules_sync(&owned, &mut found);
        found
    })
    .await
    .map_err(|e| anyhow!("the rule check failed: {e}"))?;
    fields.extend(found);
    Ok(())
}

fn rule_faults(rules: &[RuleSpec]) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    for (i, rule) in rules.iter().enumerate() {
        if rule.id.chars().count() > MAX_RULE_ID_CHARS || rule.id.chars().any(char::is_control) {
            fields.insert(
                format!("rules[{i}]"),
                format!("rule id must be 1 to {MAX_RULE_ID_CHARS} characters"),
            );
        } else if rule.id.to_ascii_lowercase().starts_with("external") {
            // The log and the metrics name failures of external guardrails
            // `external...`; a rule must not be mistaken for one.
            fields.insert(
                format!("rules[{i}]"),
                "rule id must not start with 'external'".to_string(),
            );
        }
    }
    if !fields.is_empty() {
        return fields;
    }
    let Err(whole) = Compiled::compile(0, TEST_NAME, rules) else {
        return fields;
    };
    // Which rule? Each on its own; what is left is a fault of the set.
    for (i, rule) in rules.iter().enumerate() {
        if let Err(e) = Compiled::compile(0, TEST_NAME, std::slice::from_ref(rule)) {
            fields.insert(format!("rules[{i}]"), cut(e.to_string()));
        }
    }
    if fields.is_empty() {
        fields.insert("rules".to_string(), cut(whole.to_string()));
    }
    fields
}

/// The guardrails to attach: without repeats, in the order given, each one
/// that exists. The error is the message for `fields.guardrail_ids`.
pub(super) async fn resolve_ids(
    tx: &mut Tx<'_>,
    asked: &[i64],
) -> Result<Result<Vec<GuardrailRef>, String>, ApiError> {
    let mut ids: Vec<i64> = Vec::new();
    for id in asked {
        if !ids.contains(id) {
            ids.push(*id);
        }
    }
    if ids.len() > MAX_ATTACHED {
        return Ok(Err(format!("at most {MAX_ATTACHED} guardrails")));
    }
    let mut out = Vec::new();
    for id in ids {
        match tx.guardrail_by_id(id).await? {
            Some(row) => out.push(ref_of(&row)),
            None => return Ok(Err("a guardrail does not exist".to_string())),
        }
    }
    Ok(Ok(out))
}

/// What the audit log says about an attachment.
pub(super) fn attach_summary(subject: &str, names: &[GuardrailRef]) -> String {
    let list = if names.is_empty() {
        "none".to_string()
    } else {
        names
            .iter()
            .map(|g| g.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!("Set the guardrails of {subject} to {list}")
}

/// The body that sets the guardrails of a team or a user.
#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AttachRequest {
    /// The guardrails to apply, in this order, to every key of the team, or
    /// every key the user owns. `[]` takes them all off. At most 20, each
    /// one an existing guardrail.
    pub guardrail_ids: Vec<i64>,
}

/// The guardrails attached to a team or a user, in order.
#[derive(Serialize, utoipa::ToSchema)]
pub struct Attached {
    pub guardrail_ids: Vec<i64>,
}

/// What guardrails are attached to.
#[derive(Clone, Copy)]
pub(super) enum Holder {
    Team,
    User,
}

/// Replaces the guardrails of a team or a user. Admins only; the caller has
/// been checked.
pub(super) async fn set_attached(
    state: &Arc<AppState>,
    me: &crate::identity::Principal,
    holder: Holder,
    id: i64,
    asked: &[i64],
) -> Result<Response, ApiError> {
    let mut tx = state.store.begin_immediate().await?;
    let (kind, name) = match holder {
        Holder::Team => (
            "team",
            tx.team_by_id(id)
                .await?
                .map(|t| t.name)
                .ok_or_else(ApiError::not_found)?,
        ),
        Holder::User => (
            "user",
            tx.user_by_id(id)
                .await?
                .map(|u| u.email)
                .ok_or_else(ApiError::not_found)?,
        ),
    };
    let attach = match resolve_ids(&mut tx, asked).await? {
        Ok(found) => found,
        Err(message) => return Err(ApiError::invalid_field("guardrail_ids", &message)),
    };
    let ids: Vec<i64> = attach.iter().map(|g| g.id).collect();
    let before = match holder {
        Holder::Team => tx.team_guardrail_ids(id).await?,
        Holder::User => tx.user_guardrail_ids(id).await?,
    };
    if before != ids {
        match holder {
            Holder::Team => tx.replace_team_guardrails(id, &ids).await,
            Holder::User => tx.replace_user_guardrails(id, &ids).await,
        }
        .map_err(gone)?;
        tx.audit(AuditEntry {
            actor_user_id: Some(me.user_id),
            actor_email: &me.email,
            action: "guardrail.attach",
            target_type: kind,
            target_id: Some(id),
            summary: &attach_summary(&format!("{kind} {name}"), &attach),
        })
        .await?;
    }
    tx.commit().await?;
    refresh_snapshot(state).await?;
    Ok(Json(Attached { guardrail_ids: ids }).into_response())
}

/// A guardrail deleted since it was checked is the caller's mistake, not ours.
pub(super) fn gone(e: anyhow::Error) -> ApiError {
    if crate::store::is_missing_reference(&e) {
        ApiError::invalid_field("guardrail_ids", "a guardrail does not exist any more")
    } else {
        e.into()
    }
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

#[utoipa::path(
    get,
    path = "/guardrails",
    tag = "guardrails",
    operation_id = "guardrails_list",
    responses(
        (status = 200, description = "Every guardrail, by name.", body = GuardrailList),
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
    require(&authed.principal, &Action::ManageGuardrails)?;
    let rows = state.store.list_guardrails().await?;
    let routes = state.store.guardrail_routes().await?;
    let keys = state.store.guardrail_key_counts().await?;
    let snapshot = state.snapshot.load();
    let guardrails: Vec<GuardrailView> = rows
        .iter()
        .map(|g| view_of(g, &routes, &keys, &snapshot))
        .collect();
    Ok(Json(GuardrailList { guardrails }).into_response())
}

#[utoipa::path(
    get,
    path = "/guardrails/{id}",
    tag = "guardrails",
    operation_id = "guardrails_view",
    params(
        ("id" = i64, Path, description = "The id of the guardrail."),
    ),
    responses(
        (status = 200, description = "The guardrail.", body = GuardrailView),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "It does not exist.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn view(
    State(state): State<Arc<AppState>>,
    Path(raw_id): Path<String>,
    authed: Authed,
) -> Result<Response, ApiError> {
    require(&authed.principal, &Action::ManageGuardrails)?;
    let row = guardrail_of(&state.store, &raw_id).await?;
    Ok(Json(full_view(&state, &row).await?).into_response())
}

#[utoipa::path(
    post,
    path = "/guardrails",
    tag = "guardrails",
    operation_id = "guardrails_create",
    request_body = CreateGuardrailRequest,
    responses(
        (status = 201, description = "The new guardrail and, for an external one, its signing secret. The secret is shown once, here.", body = CreatedGuardrail),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 409, description = "`guardrail_exists`: the name is taken.", body = super::openapi::ApiErrorBody),
        (status = 413, description = "The request body is too large.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "Some fields are not valid; `fields` names each of them. A rule that does not compile is `rules[0]`, `rules[1]`, ...", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn create(
    State(state): State<Arc<AppState>>,
    authed: Authed,
    ApiJson(mut req): ApiJson<CreateGuardrailRequest>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    require(me, &Action::ManageGuardrails)?;
    let mut fields = BTreeMap::new();
    check_name(&req.name, &mut fields);
    if let Some(description) = &req.description {
        check_description(description, &mut fields);
    }
    let rules = parse_rules(req.rules.take(), &mut fields);
    let only = |fields: &mut BTreeMap<String, String>, name: &str, kind: &str| {
        fields.insert(name.to_string(), format!("only for {kind} guardrails"));
    };
    let mut host = None;
    match req.kind.as_str() {
        "rules" => {
            for (name, present) in [
                ("url", req.url.is_some()),
                ("timeout_ms", req.timeout_ms.is_some()),
                ("fail_mode", req.fail_mode.is_some()),
                ("directions", req.directions.is_some()),
            ] {
                if present {
                    only(&mut fields, name, "external");
                }
            }
            match &rules {
                Some(_) if rules_faulty(&fields) => {}
                Some(rules) => check_rules(rules, &mut fields).await?,
                None => {
                    fields.insert("rules".to_string(), "add at least one rule".to_string());
                }
            }
        }
        "external" => {
            if rules.is_some() {
                only(&mut fields, "rules", "rules");
            }
            match &req.url {
                Some(url) => host = check_url(url, &mut fields),
                None => {
                    fields.insert("url".to_string(), "a URL is required".to_string());
                }
            }
            check_timeout(req.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS), &mut fields);
            check_fail_mode(req.fail_mode.as_deref().unwrap_or("open"), &mut fields);
        }
        _ => {
            fields.insert(
                "kind".to_string(),
                "kind must be rules or external".to_string(),
            );
        }
    }
    if !fields.is_empty() {
        return Err(ApiError::validation(fields));
    }
    let CreateGuardrailRequest {
        name,
        description,
        kind,
        enabled,
        is_default,
        url,
        timeout_ms,
        fail_mode,
        directions,
        rules: _,
    } = req;
    let name = name.trim().to_string();
    let description = description.unwrap_or_default();
    let rules_json = match &rules {
        Some(rules) => serde_json::to_string(rules).map_err(|e| anyhow!(e))?,
        None => "[]".to_string(),
    };
    // From here on only the encrypted forms exist; the secret is shown once.
    let external = url.zip(host.as_deref()).map(|(url, host)| {
        let secret = new_secret();
        (
            state.cipher.encrypt(url.as_bytes()),
            host.to_string(),
            state.cipher.encrypt(secret.as_bytes()),
            secret,
        )
    });
    let store = &state.store;
    let mut tx = store.begin().await?;
    let id = match tx
        .insert_guardrail(NewGuardrail {
            name: &name,
            description: &description,
            kind: &kind,
            rules: &rules_json,
            url: external
                .as_ref()
                .map(|(enc, host, _, _)| (enc.as_slice(), host.as_str())),
            secret_enc: external.as_ref().map(|(_, _, enc, _)| enc.as_slice()),
            timeout_ms: timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS),
            fail_mode: fail_mode.as_deref().unwrap_or("open"),
            directions: directions.unwrap_or(Directions::Both).as_str(),
            enabled: enabled.unwrap_or(true),
            is_default: is_default.unwrap_or(false),
        })
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
    let summary = match (&rules, &host) {
        (Some(rules), _) => format!(
            "Created guardrail {name} (rules, {})",
            plural(rules.len(), "rule")
        ),
        (None, Some(host)) => format!("Created guardrail {name} (external, {host})"),
        (None, None) => format!("Created guardrail {name}"),
    };
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "guardrail.create",
        target_type: "guardrail",
        target_id: Some(id),
        summary: &summary,
    })
    .await?;
    tx.commit().await?;
    refresh_snapshot(&state).await?;

    let row = store
        .guardrail_by_id(id)
        .await?
        .ok_or_else(|| anyhow!("the guardrail is missing after it was created"))?;
    // The only time the secret is sent.
    let body = CreatedGuardrail {
        guardrail: full_view(&state, &row).await?,
        secret: external.map(|(_, _, _, secret)| secret),
    };
    Ok((StatusCode::CREATED, Json(body)).into_response())
}

#[utoipa::path(
    patch,
    path = "/guardrails/{id}",
    tag = "guardrails",
    operation_id = "guardrails_update",
    params(
        ("id" = i64, Path, description = "The id of the guardrail."),
    ),
    request_body = UpdateGuardrailRequest,
    responses(
        (status = 200, description = "The guardrail after the change.", body = GuardrailView),
        (status = 400, description = "The request is not of the expected form, or changes nothing.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "It does not exist.", body = super::openapi::ApiErrorBody),
        (status = 409, description = "`guardrail_exists`: the name is taken.", body = super::openapi::ApiErrorBody),
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
    ApiJson(mut req): ApiJson<UpdateGuardrailRequest>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    let store = &state.store;
    // The answer does not depend on the guardrail, so it comes before the
    // id is looked at.
    require(me, &Action::ManageGuardrails)?;
    let was = guardrail_of(store, &raw_id).await?;
    let mut fields = BTreeMap::new();
    let rules = parse_rules(req.rules.take(), &mut fields);
    if req.name.is_none()
        && req.description.is_none()
        && req.enabled.is_none()
        && req.is_default.is_none()
        && rules.is_none()
        && req.url.is_none()
        && req.timeout_ms.is_none()
        && req.fail_mode.is_none()
        && req.directions.is_none()
    {
        return Err(ApiError::bad_request("Send at least one field to change."));
    }
    let external = was.kind == "external";
    if let Some(name) = &req.name {
        check_name(name, &mut fields);
    }
    if let Some(description) = &req.description {
        check_description(description, &mut fields);
    }
    let mut host = None;
    if external {
        if rules.is_some() {
            fields.insert("rules".to_string(), "only for rules guardrails".to_string());
        }
        host = req.url.as_deref().and_then(|u| check_url(u, &mut fields));
        if let Some(ms) = req.timeout_ms {
            check_timeout(ms, &mut fields);
        }
        if let Some(mode) = &req.fail_mode {
            check_fail_mode(mode, &mut fields);
        }
        // A guardrail made by an import has no URL until one is set.
        let no_url = was.url_host.as_deref().is_none_or(str::is_empty);
        if req.enabled == Some(true) && req.url.is_none() && no_url {
            fields.insert(
                "enabled".to_string(),
                "Set a URL before enabling this guardrail.".to_string(),
            );
        }
    } else {
        for (name, present) in [
            ("url", req.url.is_some()),
            ("timeout_ms", req.timeout_ms.is_some()),
            ("fail_mode", req.fail_mode.is_some()),
            ("directions", req.directions.is_some()),
        ] {
            if present {
                fields.insert(name.to_string(), "only for external guardrails".to_string());
            }
        }
        if let Some(rules) = rules.as_ref().filter(|_| !rules_faulty(&fields)) {
            check_rules(rules, &mut fields).await?;
        }
    }
    if !fields.is_empty() {
        return Err(ApiError::validation(fields));
    }

    let name = req.name.as_deref().map(str::trim);
    let rules_json = match &rules {
        Some(rules) => Some(serde_json::to_string(rules).map_err(|e| anyhow!(e))?),
        None => None,
    };
    let url = req
        .url
        .as_deref()
        .zip(host.as_deref())
        .map(|(url, host)| (state.cipher.encrypt(url.as_bytes()), host));
    let directions = req.directions.map(Directions::as_str);

    let mut changes = Vec::new();
    let mut note = |changed: bool, what: &'static str| {
        if changed {
            changes.push(what);
        }
    };
    note(name.is_some_and(|n| n != was.name), "name changed");
    note(
        req.description
            .as_deref()
            .is_some_and(|d| d != was.description),
        "description changed",
    );
    note(
        rules_json.as_deref().is_some_and(|r| r != was.rules),
        "rules changed",
    );
    note(url.is_some(), "URL changed");
    note(
        req.timeout_ms.is_some_and(|t| t != was.timeout_ms),
        "timeout changed",
    );
    note(
        req.fail_mode.as_deref().is_some_and(|m| m != was.fail_mode),
        "fail mode changed",
    );
    note(
        directions.is_some_and(|d| d != was.directions),
        "directions changed",
    );
    note(
        req.enabled.is_some_and(|e| e != was.enabled),
        if req.enabled == Some(true) {
            "enabled"
        } else {
            "disabled"
        },
    );
    note(
        req.is_default.is_some_and(|d| d != was.is_default),
        if req.is_default == Some(true) {
            "made a default"
        } else {
            "no longer a default"
        },
    );
    if changes.is_empty() {
        return Ok(Json(full_view(&state, &was).await?).into_response());
    }
    let mut tx = store.begin().await?;
    let updated = tx
        .update_guardrail(
            was.id,
            GuardrailPatch {
                name,
                description: req.description.as_deref(),
                rules: rules_json.as_deref(),
                url: url.as_ref().map(|(enc, host)| (enc.as_slice(), *host)),
                timeout_ms: req.timeout_ms,
                fail_mode: req.fail_mode.as_deref(),
                directions,
                enabled: req.enabled,
                is_default: req.is_default,
            },
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
        action: "guardrail.update",
        target_type: "guardrail",
        target_id: Some(was.id),
        summary: &format!("Updated guardrail {}: {}", was.name, changes.join(", ")),
    })
    .await?;
    tx.commit().await?;
    refresh_snapshot(&state).await?;
    let row = store
        .guardrail_by_id(was.id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(full_view(&state, &row).await?).into_response())
}

#[utoipa::path(
    delete,
    path = "/guardrails/{id}",
    tag = "guardrails",
    operation_id = "guardrails_delete",
    params(
        ("id" = i64, Path, description = "The id of the guardrail."),
    ),
    responses(
        (status = 204, description = "The guardrail is deleted; routes and keys lose it."),
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
    require(me, &Action::ManageGuardrails)?;
    let target = guardrail_of(&state.store, &raw_id).await?;
    let mut tx = state.store.begin().await?;
    if !tx.delete_guardrail(target.id).await? {
        drop(tx);
        // An earlier call may have deleted it and failed to refresh.
        refresh_snapshot(&state).await?;
        return Err(ApiError::not_found());
    }
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "guardrail.delete",
        target_type: "guardrail",
        target_id: Some(target.id),
        summary: &format!("Deleted guardrail {}", target.name),
    })
    .await?;
    tx.commit().await?;
    refresh_snapshot(&state).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[utoipa::path(
    post,
    path = "/guardrails/{id}/rotate-secret",
    tag = "guardrails",
    operation_id = "guardrails_rotate_secret",
    params(
        ("id" = i64, Path, description = "The id of the guardrail."),
    ),
    responses(
        (status = 200, description = "The new signing secret, shown once, here. The old one stops working at once.", body = super::openapi::RotatedSecret),
        (status = 400, description = "The guardrail is not an external one, so it has no secret.", body = super::openapi::ApiErrorBody),
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
    require(me, &Action::ManageGuardrails)?;
    let target = guardrail_of(&state.store, &raw_id).await?;
    if target.kind != "external" {
        return Err(ApiError::bad_request(
            "Only an external guardrail has a signing secret.",
        ));
    }
    let secret = new_secret();
    let secret_enc = state.cipher.encrypt(secret.as_bytes());
    let mut tx = state.store.begin().await?;
    if !tx.set_guardrail_secret(target.id, &secret_enc).await? {
        return Err(ApiError::not_found());
    }
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "guardrail.rotate_secret",
        target_type: "guardrail",
        target_id: Some(target.id),
        summary: &format!("Rotated the signing secret of guardrail {}", target.name),
    })
    .await?;
    tx.commit().await?;
    // The snapshot holds the secret an external guardrail signs with.
    refresh_snapshot(&state).await?;
    Ok(Json(json!({ "secret": secret })).into_response())
}

#[utoipa::path(
    post,
    path = "/guardrails/test",
    tag = "guardrails",
    operation_id = "guardrails_test",
    request_body = GuardrailTestRequest,
    responses(
        (status = 200, description = "What the rules do to the text, or with `call_external` what an external guardrail decided. Nothing is stored or logged.", body = GuardrailTestResult),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 413, description = "The request body is too large.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "Some fields are not valid; `fields` names each of them.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn test(
    State(state): State<Arc<AppState>>,
    authed: Authed,
    ApiJson(mut req): ApiJson<GuardrailTestRequest>,
) -> Result<Response, ApiError> {
    require(&authed.principal, &Action::ManageGuardrails)?;
    let mut fields = BTreeMap::new();
    let mut external: Option<Arc<SnapGuardrail>> = None;
    let given_rules = parse_rules(req.rules.take(), &mut fields);
    if req.text.chars().count() > MAX_TEST_CHARS {
        fields.insert(
            "text".to_string(),
            format!("text must be at most {MAX_TEST_CHARS} characters"),
        );
    }
    let (id, name, rules) = match (given_rules, req.guardrail_id) {
        (Some(_), Some(_)) => {
            fields.insert(
                "guardrail_id".to_string(),
                "send rules or guardrail_id, not both".to_string(),
            );
            return Err(ApiError::validation(fields));
        }
        (None, None) => {
            fields.insert(
                "rules".to_string(),
                "send rules or guardrail_id".to_string(),
            );
            return Err(ApiError::validation(fields));
        }
        (Some(rules), None) => {
            if req.call_external {
                fields.insert(
                    "call_external".to_string(),
                    "only for the guardrail_id of an external guardrail".to_string(),
                );
            }
            if !rules_faulty(&fields) {
                check_rules(&rules, &mut fields).await?;
            }
            (0, TEST_NAME.to_string(), rules)
        }
        (None, Some(guardrail_id)) => {
            let row = state.store.guardrail_by_id(guardrail_id).await?;
            match row {
                None => {
                    fields.insert(
                        "guardrail_id".to_string(),
                        "the guardrail does not exist".to_string(),
                    );
                    (0, String::new(), Vec::new())
                }
                Some(row) if row.kind == "external" => {
                    if req.call_external {
                        let ext = external_of(&row, &state.cipher);
                        if !ext.usable {
                            fields.insert(
                                "call_external".to_string(),
                                "the guardrail has no URL to call".to_string(),
                            );
                        } else if req.text.is_empty() {
                            fields.insert(
                                "text".to_string(),
                                "nothing to check: the text is empty".to_string(),
                            );
                        } else {
                            external = Some(Arc::new(SnapGuardrail {
                                id: row.id,
                                name: row.name.clone(),
                                rules_text: String::new(),
                                rules: None,
                                external: Some(ext),
                            }));
                        }
                    } else {
                        fields.insert(
                            "guardrail_id".to_string(),
                            "an external guardrail is not called by a test; send call_external to call it"
                                .to_string(),
                        );
                    }
                    (0, String::new(), Vec::new())
                }
                Some(row) => {
                    if req.call_external {
                        fields.insert(
                            "call_external".to_string(),
                            "only for the guardrail_id of an external guardrail".to_string(),
                        );
                    }
                    (row.id, row.name.clone(), rules_of(&row))
                }
            }
        }
    };
    if !fields.is_empty() {
        return Err(ApiError::validation(fields));
    }
    if let Some(g) = external {
        // The hook is called for real, as a call would: same request, same
        // signature, same timeout, same fail mode.
        let hooks = Hooks {
            http: state.http.clone(),
            gates: state.hook_gates.clone(),
            meta: Arc::new(CallMeta {
                endpoint: "test",
                model: TEST_MODEL.to_string(),
                route: None,
                key_id: None,
                team_id: None,
                user_id: None,
            }),
        };
        let name = g.name.clone();
        let active = Active::of(&[g], Some(hooks));
        let mut text = req.text;
        let outcome = active
            .check(req.direction, vec![&mut text], None)
            .await
            .map_err(|_| ApiError::internal())?;
        return Ok(Json(test_result(outcome, text, &name)).into_response());
    }
    let (direction, text) = (req.direction, req.text);
    let guardrail_name = name.clone();
    let result = tokio::task::spawn_blocking(move || {
        let compiled = Compiled::compile(id, &guardrail_name, &rules)?;
        let mut texts = [text];
        let outcome = check_texts(&[Arc::new(compiled)], direction, &mut texts);
        let [text] = texts;
        Ok::<_, crate::guardrails::GuardrailError>((outcome, text))
    })
    .await
    .map_err(|e| anyhow!("the test failed: {e}"))?;
    let (outcome, redacted_text) = match result {
        Ok(done) => done,
        Err(e) => {
            // A stored guardrail was validated when it was written.
            tracing::error!(error = %e, "a stored guardrail does not compile");
            return Err(ApiError::internal());
        }
    };
    let body = test_result(outcome, redacted_text, &name);
    Ok(Json(body).into_response())
}

/// What the test endpoint answers for the outcome of a check.
fn test_result(outcome: Outcome, redacted_text: String, name: &str) -> GuardrailTestResult {
    let all_flags = outcome.all_flags();
    GuardrailTestResult {
        outcome: OutcomeView {
            blocked_by: outcome
                .blocked_by
                .map(|(id, name)| GuardrailRef { id, name }),
            redactions: outcome.redactions,
            flags: all_flags
                .into_iter()
                .map(|(guardrail_id, rule_id)| FlagView {
                    guardrail_id,
                    guardrail_name: name.to_string(),
                    rule_id,
                })
                .collect(),
        },
        redacted_text,
    }
}
