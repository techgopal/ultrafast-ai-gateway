//! The configuration of the gateway as one file: what `GET /api/config/export`
//! and `ultrafast config export` write, and what the import reads.
//!
//! The file holds names, never ids, so it can go to another gateway: models
//! are `provider/model`, grants and route access name teams and users (by
//! name and by email). It holds no secret: no credential of a provider, no
//! key, token, password, session, log or audit row. Users are not in it;
//! they must exist where it is imported. Keys have no limit or budget in it.
//!
//! The import creates what is missing and updates what exists, by name. It
//! never deletes. It is checked in full first; a file with any error
//! writes nothing. What it writes, it writes in one transaction.

use std::collections::{BTreeMap, HashMap, HashSet};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use ultrafast_translate::provider::ProviderKind;

use crate::alerts::rules as alert_rules;
use crate::alerts::sign::new_secret;
use crate::api::budgets::MAX_AMOUNT_MICROS;
use crate::api::guardrails::{
    check_description, check_fail_mode, check_rules_sync, check_timeout, rules_of,
    DEFAULT_TIMEOUT_MS, KINDS as GUARDRAIL_KINDS, MAX_ATTACHED,
};
use crate::api::limits::{checked as checked_limit, MAX_COUNT, MAX_TOKENS};
use crate::api::prompts::check_version;
use crate::api::providers::{check_api_version, checked as checked_provider};
use crate::api::routes::check_settings;
use crate::api::teams::valid_team_name;
use crate::api::trimmed_name;
use crate::budgets::{BudgetAction, Period};
use crate::cache::{CacheScope, RouteCache};
use crate::catalog::validate_model_name;
use crate::config::{same_host, validate_base_url, validate_provider_name};
use crate::guardrails::{Directions, RuleSpec};
use crate::limits::{LimitScope, RateLimit};
use crate::prompts::{self, Params, TemplateMessage, MAX_TEMPLATES, MAX_VERSIONS};
use crate::secrets::Cipher;
use crate::store::{
    AuditEntry, ConfigState, Grants, GuardrailPatch, NewGuardrail, NewVersion, RouteSettings,
    Store, TargetsInput, Tx, SESSION_HOURS_RANGE,
};

/// The `format` of the file.
pub const FORMAT: &str = "ultrafast-config";
/// The `version` of the file this gateway reads and writes.
pub const VERSION: u32 = 1;
/// The largest file the API reads.
pub const MAX_FILE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ProviderEntry {
    pub name: String,
    /// `openai`, `anthropic`, `gemini` or `azure`.
    pub kind: String,
    pub base_url: String,
    /// Azure OpenAI only.
    #[serde(default)]
    #[schema(required)]
    pub api_version: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GrantEntry {
    #[serde(default)]
    pub everyone: bool,
    /// Names of teams.
    #[serde(default)]
    pub teams: Vec<String>,
    /// Emails of users.
    #[serde(default)]
    pub users: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelEntry {
    /// The name of its provider.
    pub provider: String,
    pub name: String,
    pub enabled: bool,
    /// Per million tokens, in millionths of a dollar. `null`: not known.
    #[serde(default)]
    #[schema(required)]
    pub input_price_micros: Option<i64>,
    #[serde(default)]
    #[schema(required)]
    pub output_price_micros: Option<i64>,
    #[serde(default)]
    pub grants: GrantEntry,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TeamEntry {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PrimaryEntry {
    /// `provider/model`.
    pub model: String,
    pub weight: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RouteEntry {
    pub name: String,
    pub primaries: Vec<PrimaryEntry>,
    /// `provider/model`, in the order they are tried.
    #[serde(default)]
    pub fallbacks: Vec<String>,
    pub retries: i64,
    pub first_token_timeout_ms: i64,
    pub total_timeout_ms: i64,
    pub breaker_failures: i64,
    pub breaker_window_s: i64,
    pub breaker_open_s: i64,
    #[serde(default)]
    pub everyone: bool,
    /// Names of teams.
    #[serde(default)]
    pub teams: Vec<String>,
    #[serde(default)]
    pub cache_enabled: bool,
    #[serde(default = "default_cache_ttl")]
    pub cache_ttl_s: i64,
    #[serde(default = "default_cache_scope")]
    pub cache_scope: String,
    /// Names of guardrails, in the order they apply. Left out of the file
    /// for a route that has none; a file that leaves it out does not change
    /// what is attached (`[]` takes them all off).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guardrails: Option<Vec<String>>,
}

fn default_cache_ttl() -> i64 {
    crate::cache::DEFAULT_TTL_S
}

fn default_cache_scope() -> String {
    CacheScope::Team.as_str().to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct LimitEntry {
    /// `gateway`, `team` or `user`.
    pub scope: String,
    /// The team's name or the user's email; `null` for the gateway.
    #[serde(default)]
    #[schema(required)]
    pub name: Option<String>,
    #[serde(default)]
    #[schema(required)]
    pub requests_per_minute: Option<u64>,
    #[serde(default)]
    #[schema(required)]
    pub tokens_per_minute: Option<u64>,
    #[serde(default)]
    #[schema(required)]
    pub concurrent: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct BudgetEntry {
    /// `gateway`, `team` or `user`.
    pub scope: String,
    /// The team's name or the user's email; `null` for the gateway.
    #[serde(default)]
    #[schema(required)]
    pub name: Option<String>,
    pub amount_micros: u64,
    /// `daily`, `weekly` or `monthly`.
    pub period: String,
    /// `block` or `alert`.
    pub action: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SettingsEntry {
    /// 1 to 3650. Not in the file: not changed.
    #[serde(default)]
    #[schema(required)]
    pub log_retention_days: Option<i64>,
    /// 1 to 720. Not in the file: not changed.
    #[serde(default)]
    #[schema(required)]
    pub session_hours: Option<i64>,
}

/// An alert channel: its name and kind only. Its URL and secret are never in
/// a file; a channel an import creates is off until its URL is set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AlertChannelEntry {
    pub name: String,
    /// `webhook` or `slack`.
    pub kind: String,
}

/// An alert rule. Its channels are named; a `budget` rule names its budget by
/// `{scope, name, period}` (or `null`: every budget) instead of an id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AlertRuleEntry {
    pub name: String,
    /// `budget`, `error_rate` or `circuit_open`.
    pub kind: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// As the API takes them, except that a `budget` rule has
    /// `{"budget": {"scope", "name", "period"} or null, "percent"}`.
    #[schema(value_type = Object)]
    pub params: Value,
    /// Names of channels.
    #[serde(default)]
    pub channels: Vec<String>,
}

fn yes() -> bool {
    true
}

fn default_timeout() -> i64 {
    DEFAULT_TIMEOUT_MS
}

fn default_fail_mode() -> String {
    "open".to_string()
}

fn both() -> Directions {
    Directions::Both
}

/// How an external guardrail behaves. Its URL and signing secret are never
/// in a file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ExternalEntry {
    /// 1 000 to 10 000. Not in the file: 3 000.
    #[serde(default = "default_timeout")]
    pub timeout_ms: i64,
    /// `open` or `closed`. Not in the file: `open`.
    #[serde(default = "default_fail_mode")]
    pub fail_mode: String,
    /// What it is asked about. Not in the file: `both`.
    #[serde(default = "both")]
    pub directions: Directions,
}

/// A guardrail. The URL and the signing secret of an external one are never
/// in a file; one an import creates is off until its URL is set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GuardrailEntry {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// `rules` or `external`.
    pub kind: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// Applies to every call of the gateway.
    #[serde(default)]
    pub is_default: bool,
    /// The rules of a `rules` guardrail; empty for an external one.
    #[serde(default)]
    pub rules: Vec<RuleSpec>,
    /// An external guardrail's settings; left out for `rules`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external: Option<ExternalEntry>,
}

/// One version of a prompt template in a file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PromptVersionEntry {
    /// From 1, in order, without gaps.
    pub version: i64,
    pub messages: Vec<TemplateMessage>,
    /// Used when a call names no model.
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub params: Params,
}

/// A prompt template with all its versions. An import adds the versions a
/// gateway lacks and never rewrites one it has: a version that differs is
/// an error. Who made a template is not in a file; an import makes the
/// templates it creates the importing admin's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PromptEntry {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub versions: Vec<PromptVersionEntry>,
}

/// The configuration file. `format` and `version` come first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigFile {
    /// Always `ultrafast-config`.
    pub format: String,
    /// Always 1.
    pub version: u32,
    #[serde(default)]
    pub providers: Vec<ProviderEntry>,
    #[serde(default)]
    pub models: Vec<ModelEntry>,
    #[serde(default)]
    pub teams: Vec<TeamEntry>,
    #[serde(default)]
    pub routes: Vec<RouteEntry>,
    #[serde(default)]
    pub limits: Vec<LimitEntry>,
    #[serde(default)]
    pub budgets: Vec<BudgetEntry>,
    #[serde(default)]
    pub settings: SettingsEntry,
    /// Left out of the file when there are none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alert_channels: Vec<AlertChannelEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alert_rules: Vec<AlertRuleEntry>,
    /// Left out of the file when there are none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guardrails: Vec<GuardrailEntry>,
    /// Prompt templates with all their versions. Left out of the file when
    /// there are none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prompts: Vec<PromptEntry>,
}

/// Something the import did, or would do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct Item {
    /// `provider`, `team`, `model`, `guardrail`, `prompt`, `route`, `limit`,
    /// `budget`, `alert_channel`, `alert_rule` or `settings`.
    pub kind: String,
    pub name: String,
    /// For an update: the fields that change. Empty for a creation.
    pub changes: Vec<String>,
}

/// A place in the file and what is to be said about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct Issue {
    /// Where, as `models[0].grants.teams[1]`; `file` for the file as a whole.
    pub at: String,
    pub message: String,
}

/// What an import did, or with `dry_run` would do.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct ImportReport {
    pub created: Vec<Item>,
    pub updated: Vec<Item>,
    /// How many things of the file were there already, as they are.
    pub unchanged: u32,
    pub warnings: Vec<Issue>,
    /// With any error nothing is written.
    pub errors: Vec<Issue>,
}

impl ImportReport {
    fn error(at: &str, message: impl Into<String>) -> Self {
        Self {
            errors: vec![Issue {
                at: at.to_string(),
                message: message.into(),
            }],
            ..Self::default()
        }
    }

    /// Whether the file was, or would be, written.
    pub fn is_clean(&self) -> bool {
        self.errors.is_empty()
    }
}

impl ImportReport {
    /// The report as text for the command line.
    pub fn describe(&self, dry_run: bool) -> String {
        let mut lines = Vec::new();
        if !self.is_clean() {
            lines.push("The file has errors. Nothing was written.".to_string());
        } else if dry_run {
            lines
                .push("Dry run: nothing was written. This is what an import would do.".to_string());
        }
        for (verb, items) in [("created", &self.created), ("updated", &self.updated)] {
            for item in items {
                if item.changes.is_empty() {
                    lines.push(format!("{verb}: {} {}", item.kind, item.name));
                } else {
                    lines.push(format!(
                        "{verb}: {} {} ({})",
                        item.kind,
                        item.name,
                        item.changes.join(", ")
                    ));
                }
            }
        }
        for issue in &self.warnings {
            lines.push(format!("warning: {}: {}", issue.at, issue.message));
        }
        for issue in &self.errors {
            lines.push(format!("error: {}: {}", issue.at, issue.message));
        }
        if self.is_clean() {
            lines.push(format!(
                "{} created, {} updated, {} unchanged.",
                self.created.len(),
                self.updated.len(),
                self.unchanged
            ));
        }
        lines.join("\n")
    }
}

/// Reads a file. A file that is not one says so without quoting it.
pub fn parse(bytes: &[u8]) -> Result<ConfigFile, ImportReport> {
    serde_json::from_slice(bytes).map_err(|e| {
        ImportReport::error(
            "file",
            format!(
                "This is not a configuration file the gateway can read (line {}, column {}).",
                e.line(),
                e.column()
            ),
        )
    })
}

// ---------------------------------------------------------------- export

fn scope_rank(scope: LimitScope) -> u8 {
    match scope {
        LimitScope::Gateway => 0,
        LimitScope::Team => 1,
        LimitScope::User => 2,
        LimitScope::Key => 3,
    }
}

fn period_rank(period: Period) -> u8 {
    match period {
        Period::Daily => 0,
        Period::Weekly => 1,
        Period::Monthly => 2,
    }
}

/// Names of the teams and emails of the users that a model is granted to.
fn grants_by_model(state: &ConfigState) -> HashMap<i64, GrantEntry> {
    let team_names: HashMap<i64, &str> =
        state.teams.iter().map(|(i, n)| (*i, n.as_str())).collect();
    let user_emails: HashMap<i64, &str> =
        state.users.iter().map(|(i, e)| (*i, e.as_str())).collect();
    let mut out: HashMap<i64, GrantEntry> = HashMap::new();
    for grant in &state.model_grants {
        let entry = out.entry(grant.model_id).or_default();
        match (grant.team_id, grant.user_id) {
            (None, None) => entry.everyone = true,
            (Some(team), _) => {
                if let Some(name) = team_names.get(&team) {
                    entry.teams.push((*name).to_string());
                }
            }
            (None, Some(user)) => {
                if let Some(email) = user_emails.get(&user) {
                    entry.users.push((*email).to_string());
                }
            }
        }
    }
    for entry in out.values_mut() {
        entry.teams.sort();
        entry.teams.dedup();
        entry.users.sort();
        entry.users.dedup();
    }
    out
}

/// How a rule's budget is named in a file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BudgetRef {
    scope: String,
    #[serde(default)]
    name: Option<String>,
    period: String,
}

/// The parameters of a `budget` rule in a file.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileBudgetParams {
    #[serde(default)]
    budget: Option<BudgetRef>,
    percent: i64,
}

fn budget_ref_of(state: &ConfigState, id: i64) -> Option<BudgetRef> {
    let b = state
        .budgets
        .iter()
        .find(|b| b.id == id && b.scope != LimitScope::Key && b.has_subject())?;
    Some(BudgetRef {
        scope: b.scope.as_str().to_string(),
        name: b.name.clone(),
        period: b.period.as_str().to_string(),
    })
}

/// The channels and rules of the file. A rule on a budget of a key, or on a
/// budget that is gone, cannot be named in a file and is left out, as the
/// budgets of keys are.
fn alerts_of(state: &ConfigState) -> (Vec<AlertChannelEntry>, Vec<AlertRuleEntry>) {
    let mut channels: Vec<AlertChannelEntry> = state
        .alert_channels
        .iter()
        .map(|(_, name, kind)| AlertChannelEntry {
            name: name.clone(),
            kind: kind.clone(),
        })
        .collect();
    channels.sort_by(|a, b| a.name.cmp(&b.name));
    let mut rules: Vec<AlertRuleEntry> = state
        .alert_rules
        .iter()
        .filter_map(|r| {
            let mut params: Value = serde_json::from_str(&r.params).ok()?;
            if r.kind == "budget" {
                let budget = match params.get("budget_id").and_then(Value::as_i64) {
                    Some(id) => Some(budget_ref_of(state, id)?),
                    None => None,
                };
                params = json!({ "budget": budget, "percent": params.get("percent") });
            }
            let mut names: Vec<String> = r
                .channel_ids
                .iter()
                .filter_map(|id| state.alert_channels.iter().find(|(c, _, _)| c == id))
                .map(|(_, name, _)| name.clone())
                .collect();
            names.sort();
            Some(AlertRuleEntry {
                name: r.name.clone(),
                kind: r.kind.clone(),
                enabled: r.enabled,
                params,
                channels: names,
            })
        })
        .collect();
    rules.sort_by(|a, b| a.name.cmp(&b.name));
    (channels, rules)
}

/// The guardrails of the file, by name.
fn guardrails_of(state: &ConfigState) -> Vec<GuardrailEntry> {
    let mut entries: Vec<GuardrailEntry> = state
        .guardrails
        .iter()
        .map(|g| {
            let external = g.kind == "external";
            GuardrailEntry {
                name: g.name.clone(),
                description: g.description.clone(),
                kind: g.kind.clone(),
                enabled: g.enabled,
                is_default: g.is_default,
                rules: if external { Vec::new() } else { rules_of(g) },
                external: external.then(|| ExternalEntry {
                    timeout_ms: g.timeout_ms,
                    fail_mode: g.fail_mode.clone(),
                    directions: Directions::parse(&g.directions).unwrap_or(Directions::Both),
                }),
            }
        })
        .collect();
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries
}

/// The prompt templates of the file, by name, each with all its versions.
/// A template or version that cannot be read is left out of the file.
fn prompts_of(state: &ConfigState) -> Vec<PromptEntry> {
    let mut entries: Vec<PromptEntry> = state
        .prompt_templates
        .iter()
        .filter_map(|t| {
            let versions: Option<Vec<PromptVersionEntry>> = state
                .prompt_versions
                .iter()
                .filter(|v| v.template_id == t.id)
                .map(|v| {
                    Some(PromptVersionEntry {
                        version: v.version,
                        messages: serde_json::from_str(&v.messages).ok()?,
                        model: v.model.clone(),
                        params: serde_json::from_str(&v.params).ok()?,
                    })
                })
                .collect();
            Some(PromptEntry {
                name: t.name.clone(),
                description: t.description.clone(),
                versions: versions?,
            })
        })
        .collect();
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries
}

/// The file for the configuration as it is stored.
pub fn file_of(state: &ConfigState) -> ConfigFile {
    let team_names: HashMap<i64, &str> =
        state.teams.iter().map(|(i, n)| (*i, n.as_str())).collect();
    let mut grants = grants_by_model(state);

    let mut providers: Vec<ProviderEntry> = state
        .providers
        .iter()
        .map(|p| ProviderEntry {
            name: p.name.clone(),
            kind: p.kind.clone(),
            base_url: p.base_url.clone(),
            api_version: p.api_version.clone(),
        })
        .collect();
    providers.sort_by(|a, b| a.name.cmp(&b.name));

    let mut models: Vec<ModelEntry> = state
        .models
        .iter()
        .map(|m| ModelEntry {
            provider: m.provider_name.clone(),
            name: m.name.clone(),
            enabled: m.enabled,
            input_price_micros: m.input_price_micros,
            output_price_micros: m.output_price_micros,
            grants: grants.remove(&m.id).unwrap_or_default(),
        })
        .collect();
    models.sort_by(|a, b| (&a.provider, &a.name).cmp(&(&b.provider, &b.name)));

    let mut teams: Vec<TeamEntry> = state
        .teams
        .iter()
        .map(|(_, name)| TeamEntry { name: name.clone() })
        .collect();
    teams.sort_by(|a, b| a.name.cmp(&b.name));

    let mut routes: Vec<RouteEntry> = state
        .routes
        .iter()
        .map(|r| {
            let targets = state.route_targets.iter().filter(|t| t.route_id == r.id);
            let reference =
                |t: &crate::store::TargetRow| format!("{}/{}", t.provider_name, t.model_name);
            let mut route_teams: Vec<String> = state
                .route_grants
                .iter()
                .filter(|(route, _)| *route == r.id)
                .filter_map(|(_, team)| team_names.get(team).map(|n| (*n).to_string()))
                .collect();
            route_teams.sort();
            let attached: Vec<String> = state
                .route_guardrails
                .iter()
                .filter(|(route, _, _)| *route == r.id)
                .map(|(_, _, name)| name.clone())
                .collect();
            RouteEntry {
                name: r.name.clone(),
                primaries: targets
                    .clone()
                    .filter(|t| t.primary)
                    .map(|t| PrimaryEntry {
                        model: reference(t),
                        weight: t.weight,
                    })
                    .collect(),
                fallbacks: targets.filter(|t| !t.primary).map(reference).collect(),
                retries: r.settings.retries,
                first_token_timeout_ms: r.settings.first_token_timeout_ms,
                total_timeout_ms: r.settings.total_timeout_ms,
                breaker_failures: r.settings.breaker_failures,
                breaker_window_s: r.settings.breaker_window_s,
                breaker_open_s: r.settings.breaker_open_s,
                everyone: r.everyone,
                teams: route_teams,
                cache_enabled: r.cache.enabled,
                cache_ttl_s: r.cache.ttl_s,
                cache_scope: r.cache.scope.as_str().to_string(),
                guardrails: (!attached.is_empty()).then_some(attached),
            }
        })
        .collect();
    routes.sort_by(|a, b| a.name.cmp(&b.name));

    // Limits and budgets of keys are left out, and so are those of a team or
    // user that is gone.
    let mut limits: Vec<(u8, LimitEntry)> = state
        .limits
        .iter()
        .filter(|l| l.scope != LimitScope::Key && l.has_subject())
        .map(|l| {
            (
                scope_rank(l.scope),
                LimitEntry {
                    scope: l.scope.as_str().to_string(),
                    name: l.name.clone(),
                    requests_per_minute: l.limit.requests_per_minute,
                    tokens_per_minute: l.limit.tokens_per_minute,
                    concurrent: l.limit.concurrent,
                },
            )
        })
        .collect();
    limits.sort_by(|a, b| (a.0, &a.1.name).cmp(&(b.0, &b.1.name)));

    let mut budgets: Vec<(u8, u8, BudgetEntry)> = state
        .budgets
        .iter()
        .filter(|b| b.scope != LimitScope::Key && b.has_subject())
        .map(|b| {
            (
                scope_rank(b.scope),
                period_rank(b.period),
                BudgetEntry {
                    scope: b.scope.as_str().to_string(),
                    name: b.name.clone(),
                    amount_micros: b.amount_micros,
                    period: b.period.as_str().to_string(),
                    action: b.action.as_str().to_string(),
                },
            )
        })
        .collect();
    budgets.sort_by(|a, b| (a.0, &a.2.name, a.1).cmp(&(b.0, &b.2.name, b.1)));

    let (alert_channels, alert_rules) = alerts_of(state);

    ConfigFile {
        format: FORMAT.to_string(),
        version: VERSION,
        alert_channels,
        alert_rules,
        guardrails: guardrails_of(state),
        prompts: prompts_of(state),
        providers,
        models,
        teams,
        routes,
        limits: limits.into_iter().map(|(_, l)| l).collect(),
        budgets: budgets.into_iter().map(|(_, _, b)| b).collect(),
        settings: SettingsEntry {
            log_retention_days: Some(state.log_retention_days),
            session_hours: Some(state.session_hours),
        },
    }
}

/// The configuration of the gateway as a file.
pub async fn export(store: &Store) -> Result<ConfigFile> {
    Ok(file_of(&store.config_state().await?))
}

// ---------------------------------------------------------------- import

/// Who imports: a user of the console, or the command line.
pub struct Actor<'a> {
    pub user_id: Option<i64>,
    pub email: &'a str,
    /// Encrypts what an import makes that holds a secret (the signing secret
    /// of an alert channel it creates). The command line has none: its import
    /// cannot create a channel.
    pub cipher: Option<&'a Cipher>,
}

/// What the import writes, in the order it writes it. Names, not ids: the
/// ids of what the import creates are known only as it writes.
enum Op {
    CreateProvider {
        entry: ProviderEntry,
        api_version: Option<String>,
    },
    UpdateProvider {
        id: i64,
        /// It moves to another host: its stored key is not sent there.
        drop_credential: bool,
        base_url: Option<String>,
        api_version: Option<Option<String>>,
    },
    CreateTeam {
        name: String,
    },
    CreateModel {
        entry: ModelEntry,
    },
    UpdateModel {
        id: i64,
        enabled: Option<bool>,
        input: Option<Option<i64>>,
        output: Option<Option<i64>>,
        grants: Option<GrantEntry>,
    },
    CreateGuardrail {
        entry: GuardrailEntry,
    },
    UpdateGuardrail {
        id: i64,
        entry: GuardrailEntry,
    },
    /// A prompt template: made with all its versions (`id` none), or given
    /// the versions after the first `have`.
    UpsertPrompt {
        id: Option<i64>,
        entry: PromptEntry,
        have: usize,
        description: bool,
    },
    UpsertRoute {
        id: Option<i64>,
        entry: RouteEntry,
    },
    SetLimit {
        scope: LimitScope,
        name: Option<String>,
        limit: RateLimit,
    },
    SetBudget {
        scope: LimitScope,
        name: Option<String>,
        amount: u64,
        period: Period,
        action: BudgetAction,
    },
    CreateAlertChannel {
        name: String,
        kind: String,
    },
    UpsertAlertRule {
        id: Option<i64>,
        entry: AlertRuleEntry,
        /// Its budget, if the rule names one; resolved when the rule is written.
        budget: Option<BudgetRef>,
        /// The parameters differ from those stored: its state is forgotten.
        params_changed: bool,
    },
    SetRetention(i64),
    SetSessionHours(i64),
}

struct Planned {
    op: Op,
    item: Item,
    created: bool,
}

struct Plan {
    report: ImportReport,
    ops: Vec<Planned>,
}

fn grants_of(entry: &GrantEntry) -> GrantEntry {
    let mut grants = entry.clone();
    grants.teams.sort();
    grants.teams.dedup();
    grants.users.sort();
    grants.users.dedup();
    grants
}

struct Planner<'a> {
    file: &'a ConfigFile,
    state: &'a ConfigState,
    /// The gateway's file form, built once.
    current: ConfigFile,
    report: ImportReport,
    ops: Vec<Planned>,
    /// Names the file or the gateway has.
    providers: HashSet<String>,
    teams: HashSet<String>,
    models: HashSet<String>,
    /// Names of the guardrails the gateway has or the file creates.
    guardrails: HashSet<String>,
    /// The faults of the rules of each guardrail of the file, found before
    /// the transaction (compiling is slow).
    rule_checks: Vec<BTreeMap<String, String>>,
    /// Whether the import can encrypt (the API can, the command line cannot).
    can_encrypt: bool,
}

impl Planner<'_> {
    fn error(&mut self, at: String, message: impl Into<String>) {
        self.report.errors.push(Issue {
            at,
            message: message.into(),
        });
    }

    fn fields(&mut self, at: &str, fields: BTreeMap<String, String>) {
        for (field, message) in fields {
            self.error(format!("{at}.{field}"), message);
        }
    }

    fn push(&mut self, op: Op, kind: &str, name: String, changes: Vec<String>, created: bool) {
        let item = Item {
            kind: kind.to_string(),
            name,
            changes,
        };
        if created {
            self.report.created.push(item.clone());
        } else {
            self.report.updated.push(item.clone());
        }
        self.ops.push(Planned { op, item, created });
    }

    fn providers(&mut self) {
        let file = self.file;
        let state = self.state;
        let mut seen = HashSet::new();
        for (i, entry) in file.providers.iter().enumerate() {
            let at = format!("providers[{i}]");
            let before = self.report.errors.len();
            let mut fields = BTreeMap::new();
            checked_provider("name", validate_provider_name(&entry.name), &mut fields);
            if ProviderKind::parse(&entry.kind).is_none() {
                fields.insert(
                    "kind".to_string(),
                    "kind must be openai, anthropic, gemini or azure".to_string(),
                );
            }
            checked_provider("base_url", validate_base_url(&entry.base_url), &mut fields);
            let api_version =
                check_api_version(&entry.kind, entry.api_version.as_deref(), &mut fields);
            self.fields(&at, fields);
            if !seen.insert(entry.name.clone()) {
                self.error(
                    at.clone(),
                    format!("provider '{}' appears more than once", entry.name),
                );
            }
            if self.report.errors.len() > before {
                continue;
            }
            match state.providers.iter().find(|p| p.name == entry.name) {
                Some(existing) => {
                    if existing.kind != entry.kind {
                        self.error(
                            format!("{at}.kind"),
                            format!(
                                "provider '{}' is of kind {} and an import does not change a kind",
                                entry.name, existing.kind
                            ),
                        );
                        continue;
                    }
                    let mut changes = Vec::new();
                    let mut drop_credential = false;
                    if existing.base_url != entry.base_url {
                        changes.push("base_url".to_string());
                        // The stored key goes only to the host it was given for.
                        if existing.credential.is_some()
                            && !same_host(&existing.base_url, &entry.base_url)
                        {
                            drop_credential = true;
                            self.report.warnings.push(Issue {
                                at: format!("{at}.base_url"),
                                message: format!(
                                    "provider '{}' moves to another host: its stored credential is removed; set it again",
                                    entry.name
                                ),
                            });
                        }
                    }
                    if existing.api_version != api_version {
                        changes.push("api_version".to_string());
                    }
                    if changes.is_empty() {
                        self.report.unchanged += 1;
                    } else {
                        let op = Op::UpdateProvider {
                            id: existing.id,
                            drop_credential,
                            base_url: changes
                                .contains(&"base_url".to_string())
                                .then(|| entry.base_url.clone()),
                            api_version: changes
                                .contains(&"api_version".to_string())
                                .then(|| api_version.clone()),
                        };
                        self.push(op, "provider", entry.name.clone(), changes, false);
                    }
                }
                None => {
                    self.report.warnings.push(Issue {
                        at: at.clone(),
                        message: format!(
                            "provider '{}' is created with no credential; set one before it can be called",
                            entry.name
                        ),
                    });
                    let op = Op::CreateProvider {
                        entry: entry.clone(),
                        api_version,
                    };
                    self.push(op, "provider", entry.name.clone(), Vec::new(), true);
                }
            }
        }
    }

    fn teams(&mut self) {
        let file = self.file;
        let state = self.state;
        let mut seen = HashSet::new();
        for (i, entry) in file.teams.iter().enumerate() {
            let at = format!("teams[{i}]");
            match valid_team_name(&entry.name) {
                Err(message) => {
                    self.error(format!("{at}.name"), message);
                    continue;
                }
                Ok(trimmed) if trimmed != entry.name => {
                    self.error(
                        format!("{at}.name"),
                        "name must not start or end with whitespace",
                    );
                    continue;
                }
                Ok(_) => {}
            }
            if !seen.insert(entry.name.clone()) {
                self.error(at, format!("team '{}' appears more than once", entry.name));
                continue;
            }
            if state.teams.iter().any(|(_, n)| *n == entry.name) {
                self.report.unchanged += 1;
            } else {
                let op = Op::CreateTeam {
                    name: entry.name.clone(),
                };
                self.push(op, "team", entry.name.clone(), Vec::new(), true);
            }
        }
    }

    /// Checks the names of teams and emails of users that a grant names.
    fn check_grants(&mut self, at: &str, grants: &GrantEntry) {
        if grants.everyone && (!grants.teams.is_empty() || !grants.users.is_empty()) {
            self.error(
                format!("{at}.grants"),
                "must not be combined with teams or users",
            );
        }
        for (j, team) in grants.teams.iter().enumerate() {
            if !self.teams.contains(team) {
                self.error(
                    format!("{at}.grants.teams[{j}]"),
                    format!("team '{team}' does not exist"),
                );
            }
        }
        for (j, email) in grants.users.iter().enumerate() {
            if !self.state.users.iter().any(|(_, e)| e == email) {
                self.error(
                    format!("{at}.grants.users[{j}]"),
                    format!("user '{email}' does not exist"),
                );
            }
        }
    }

    fn models(&mut self) {
        let file = self.file;
        let state = self.state;
        let grants_in_state = grants_by_model(state);
        let mut seen = HashSet::new();
        for (i, entry) in file.models.iter().enumerate() {
            let at = format!("models[{i}]");
            let before = self.report.errors.len();
            if !self.providers.contains(&entry.provider) {
                self.error(
                    format!("{at}.provider"),
                    format!(
                        "provider '{}' is not in the file or in the gateway",
                        entry.provider
                    ),
                );
            }
            if let Err(message) = validate_model_name(&entry.name) {
                self.error(format!("{at}.name"), message);
            }
            for (field, price) in [
                ("input_price_micros", entry.input_price_micros),
                ("output_price_micros", entry.output_price_micros),
            ] {
                if price.is_some_and(|p| p < 0) {
                    self.error(format!("{at}.{field}"), "must not be negative");
                }
            }
            self.check_grants(&at, &entry.grants);
            let reference = format!("{}/{}", entry.provider, entry.name);
            if !seen.insert(reference.clone()) {
                self.error(
                    at.clone(),
                    format!("model '{reference}' appears more than once"),
                );
            }
            if self.report.errors.len() > before {
                continue;
            }
            let existing = state
                .models
                .iter()
                .find(|m| m.provider_name == entry.provider && m.name == entry.name);
            match existing {
                None => self.push(
                    Op::CreateModel {
                        entry: ModelEntry {
                            grants: grants_of(&entry.grants),
                            ..entry.clone()
                        },
                    },
                    "model",
                    reference,
                    Vec::new(),
                    true,
                ),
                Some(model) => {
                    let current = grants_in_state.get(&model.id).cloned().unwrap_or_default();
                    let wanted = grants_of(&entry.grants);
                    let mut changes = Vec::new();
                    if model.enabled != entry.enabled {
                        changes.push("enabled".to_string());
                    }
                    if model.input_price_micros != entry.input_price_micros {
                        changes.push("input_price_micros".to_string());
                    }
                    if model.output_price_micros != entry.output_price_micros {
                        changes.push("output_price_micros".to_string());
                    }
                    if current != wanted {
                        changes.push("grants".to_string());
                    }
                    if changes.is_empty() {
                        self.report.unchanged += 1;
                        continue;
                    }
                    let has = |name: &str| changes.iter().any(|c| c == name);
                    let op = Op::UpdateModel {
                        id: model.id,
                        enabled: has("enabled").then_some(entry.enabled),
                        input: has("input_price_micros").then_some(entry.input_price_micros),
                        output: has("output_price_micros").then_some(entry.output_price_micros),
                        grants: has("grants").then_some(wanted),
                    };
                    self.push(op, "model", reference, changes, false);
                }
            }
        }
    }

    fn guardrails(&mut self) {
        let file = self.file;
        let state = self.state;
        let mut seen = HashSet::new();
        for (i, entry) in file.guardrails.iter().enumerate() {
            let at = format!("guardrails[{i}]");
            let before = self.report.errors.len();
            if let Err(message) = trimmed_name(&entry.name) {
                self.error(format!("{at}.name"), message);
            } else if entry.name.trim() != entry.name {
                self.error(format!("{at}.name"), "must not start or end with a space");
            }
            if !seen.insert(entry.name.clone()) {
                self.error(at.clone(), "this name appears more than once");
            }
            let mut fields = BTreeMap::new();
            check_description(&entry.description, &mut fields);
            self.fields(&at, fields);
            let external = entry.kind == "external";
            if !GUARDRAIL_KINDS.contains(&entry.kind.as_str()) {
                self.error(format!("{at}.kind"), "kind must be rules or external");
            } else if external {
                if !entry.rules.is_empty() {
                    self.error(format!("{at}.rules"), "only for rules guardrails");
                }
                let mut fields = BTreeMap::new();
                if let Some(x) = &entry.external {
                    check_timeout(x.timeout_ms, &mut fields);
                    check_fail_mode(&x.fail_mode, &mut fields);
                }
                self.fields(&format!("{at}.external"), fields);
            } else {
                if entry.external.is_some() {
                    self.error(format!("{at}.external"), "only for external guardrails");
                }
                let fields = self.rule_checks.get(i).cloned().unwrap_or_default();
                self.fields(&at, fields);
            }
            let existing = state.guardrails.iter().find(|g| g.name == entry.name);
            if let Some(g) = existing {
                if g.kind != entry.kind {
                    self.error(
                        format!("{at}.kind"),
                        format!("guardrail '{}' exists with another kind", entry.name),
                    );
                }
            }
            self.guardrails.insert(entry.name.clone());
            if self.report.errors.len() > before {
                continue;
            }
            let mut wanted = entry.clone();
            if external && wanted.external.is_none() {
                // Left out: the defaults.
                wanted.external = Some(ExternalEntry {
                    timeout_ms: DEFAULT_TIMEOUT_MS,
                    fail_mode: default_fail_mode(),
                    directions: Directions::Both,
                });
            }
            let current = self
                .current
                .guardrails
                .iter()
                .find(|g| g.name == entry.name);
            let Some((existing, current)) = existing.zip(current) else {
                if external && !self.can_encrypt {
                    self.error(
                        at.clone(),
                        format!(
                            "guardrail '{}' does not exist; the command line cannot create an external one (import the file in the console)",
                            entry.name
                        ),
                    );
                    continue;
                }
                if external {
                    // Off until it has a URL.
                    wanted.enabled = false;
                    self.report.warnings.push(Issue {
                        at: at.clone(),
                        message: format!("guardrail '{}' needs a URL", entry.name),
                    });
                }
                self.push(
                    Op::CreateGuardrail { entry: wanted },
                    "guardrail",
                    entry.name.clone(),
                    Vec::new(),
                    true,
                );
                continue;
            };
            // A file never turns on an external guardrail that has no URL.
            if external && existing.url_host.as_deref().is_none_or(str::is_empty) {
                wanted.enabled = false;
            }
            if wanted == *current {
                self.report.unchanged += 1;
                continue;
            }
            let mut changes = Vec::new();
            let mut note = |changed: bool, name: &str| {
                if changed {
                    changes.push(name.to_string());
                }
            };
            note(current.description != wanted.description, "description");
            note(current.enabled != wanted.enabled, "enabled");
            note(current.is_default != wanted.is_default, "is_default");
            note(current.rules != wanted.rules, "rules");
            note(current.external != wanted.external, "external");
            self.push(
                Op::UpdateGuardrail {
                    id: existing.id,
                    entry: wanted,
                },
                "guardrail",
                entry.name.clone(),
                changes,
                false,
            );
        }
    }

    fn prompts(&mut self) {
        let file = self.file;
        let state = self.state;
        let mut seen = HashSet::new();
        let mut created_prompts = 0usize;
        for (i, entry) in file.prompts.iter().enumerate() {
            let at = format!("prompts[{i}]");
            let before = self.report.errors.len();
            if let Err(message) = trimmed_name(&entry.name) {
                self.error(format!("{at}.name"), message);
            } else if entry.name.trim() != entry.name {
                self.error(format!("{at}.name"), "must not start or end with a space");
            } else if entry.name.contains('@') {
                self.error(
                    format!("{at}.name"),
                    "name must not contain @ (the log writes name@version)",
                );
            }
            if !seen.insert(entry.name.clone()) {
                self.error(at.clone(), "this name appears more than once");
            }
            let mut fields = BTreeMap::new();
            check_description(&entry.description, &mut fields);
            self.fields(&at, fields);
            if entry.versions.is_empty() {
                self.error(format!("{at}.versions"), "add at least one version");
            } else if entry.versions.len() > MAX_VERSIONS {
                self.error(
                    format!("{at}.versions"),
                    format!("a template has at most {MAX_VERSIONS} versions"),
                );
            }
            for (j, v) in entry.versions.iter().enumerate().take(MAX_VERSIONS) {
                let vat = format!("{at}.versions[{j}]");
                if v.version != j as i64 + 1 {
                    self.error(
                        format!("{vat}.version"),
                        format!(
                            "expected version {}: versions are numbered from 1, in order, without gaps",
                            j + 1
                        ),
                    );
                }
                let messages = v
                    .messages
                    .iter()
                    .filter_map(|m| serde_json::to_value(m).ok())
                    .collect();
                let params = serde_json::to_value(&v.params).ok();
                let mut fields = BTreeMap::new();
                check_version(messages, v.model.clone(), params, &mut fields);
                self.fields(&vat, fields);
            }
            if self.report.errors.len() > before {
                continue;
            }
            let existing = state.prompt_templates.iter().find(|t| t.name == entry.name);
            let current = self
                .current
                .prompts
                .iter()
                .find(|p| p.name == entry.name)
                .cloned();
            let (Some(existing), Some(current)) = (existing, current.as_ref()) else {
                if existing.is_some() {
                    // Stored, but not readable as a file entry.
                    self.error(
                        at.clone(),
                        format!("prompt template '{}' cannot be read", entry.name),
                    );
                    continue;
                }
                if state.prompt_templates.len() + created_prompts >= MAX_TEMPLATES {
                    self.error(
                        at.clone(),
                        format!("there would be more than {MAX_TEMPLATES} templates"),
                    );
                    continue;
                }
                created_prompts += 1;
                self.push(
                    Op::UpsertPrompt {
                        id: None,
                        entry: entry.clone(),
                        have: 0,
                        description: false,
                    },
                    "prompt",
                    entry.name.clone(),
                    Vec::new(),
                    true,
                );
                continue;
            };
            let have = current.versions.len();
            for (j, stored) in current.versions.iter().enumerate() {
                if entry.versions.get(j).is_some_and(|v| v != stored) {
                    self.error(
                        format!("{at}.versions[{j}]"),
                        format!(
                            "version {} of '{}' exists and differs; versions never change (add a new version instead)",
                            j + 1,
                            entry.name
                        ),
                    );
                }
            }
            if self.report.errors.len() > before {
                continue;
            }
            let description = current.description != entry.description;
            let added = entry.versions.len().saturating_sub(have);
            if !description && added == 0 {
                self.report.unchanged += 1;
                continue;
            }
            let mut changes = Vec::new();
            if description {
                changes.push("description".to_string());
            }
            match added {
                0 => {}
                1 => changes.push(format!("version {}", have + 1)),
                n => changes.push(format!("versions {}-{}", have + 1, have + n)),
            }
            self.push(
                Op::UpsertPrompt {
                    id: Some(existing.id),
                    entry: entry.clone(),
                    have,
                    description,
                },
                "prompt",
                entry.name.clone(),
                changes,
                false,
            );
        }
    }

    fn routes(&mut self) {
        let file = self.file;
        let state = self.state;
        let mut seen = HashSet::new();
        for (i, entry) in file.routes.iter().enumerate() {
            let at = format!("routes[{i}]");
            let before = self.report.errors.len();
            let settings = RouteSettings {
                retries: entry.retries,
                first_token_timeout_ms: entry.first_token_timeout_ms,
                total_timeout_ms: entry.total_timeout_ms,
                breaker_failures: entry.breaker_failures,
                breaker_window_s: entry.breaker_window_s,
                breaker_open_s: entry.breaker_open_s,
            };
            let fields = check_settings(
                &entry.name,
                &settings,
                entry.cache_ttl_s,
                &entry.cache_scope,
            );
            self.fields(&at, fields);
            if !seen.insert(entry.name.clone()) {
                self.error(
                    at.clone(),
                    format!("route '{}' appears more than once", entry.name),
                );
            }
            if entry.primaries.is_empty() {
                self.error(format!("{at}.primaries"), "needs at least one model");
            }
            let mut in_route = HashSet::new();
            let unknown =
                |model: &str| format!("model '{model}' is not in the file or in the gateway");
            for (j, primary) in entry.primaries.iter().enumerate() {
                if !(1..=1000).contains(&primary.weight) {
                    self.error(
                        format!("{at}.primaries[{j}].weight"),
                        "a weight must be 1 to 1000",
                    );
                }
                if !self.models.contains(&primary.model) {
                    self.error(
                        format!("{at}.primaries[{j}].model"),
                        unknown(&primary.model),
                    );
                } else if !in_route.insert(primary.model.clone()) {
                    self.error(
                        format!("{at}.primaries[{j}].model"),
                        "a model may appear only once in a route",
                    );
                }
            }
            for (j, model) in entry.fallbacks.iter().enumerate() {
                if !self.models.contains(model) {
                    self.error(format!("{at}.fallbacks[{j}]"), unknown(model));
                } else if !in_route.insert(model.clone()) {
                    self.error(
                        format!("{at}.fallbacks[{j}]"),
                        "a model may appear only once in a route",
                    );
                }
            }
            if entry.everyone && !entry.teams.is_empty() {
                self.error(
                    format!("{at}.everyone"),
                    "must not be combined with teams or users",
                );
            }
            for (j, team) in entry.teams.iter().enumerate() {
                if !self.teams.contains(team) {
                    self.error(
                        format!("{at}.teams[{j}]"),
                        format!("team '{team}' does not exist"),
                    );
                }
            }
            if let Some(names) = &entry.guardrails {
                if names.len() > MAX_ATTACHED {
                    self.error(
                        format!("{at}.guardrails"),
                        format!("at most {MAX_ATTACHED} guardrails"),
                    );
                }
                for (j, guardrail) in names.iter().enumerate() {
                    if !self.guardrails.contains(guardrail) {
                        self.error(
                            format!("{at}.guardrails[{j}]"),
                            format!("guardrail '{guardrail}' does not exist"),
                        );
                    }
                }
            }
            if self.report.errors.len() > before {
                continue;
            }
            let mut wanted = entry.clone();
            wanted.teams.sort();
            wanted.teams.dedup();
            if let Some(names) = &mut wanted.guardrails {
                // The order is kept; a repeat counts where it first is.
                let mut seen = HashSet::new();
                names.retain(|n| seen.insert(n.clone()));
            }
            let Some(existing) = state.routes.iter().find(|r| r.name == entry.name) else {
                self.push(
                    Op::UpsertRoute {
                        id: None,
                        entry: wanted,
                    },
                    "route",
                    entry.name.clone(),
                    Vec::new(),
                    true,
                );
                continue;
            };
            let current = self
                .current
                .routes
                .iter()
                .find(|r| r.name == entry.name)
                .expect("the route is in the state");
            let mut changes = Vec::new();
            let mut note = |changed: bool, name: &str| {
                if changed {
                    changes.push(name.to_string());
                }
            };
            note(current.primaries != wanted.primaries, "primaries");
            note(current.fallbacks != wanted.fallbacks, "fallbacks");
            note(current.retries != wanted.retries, "retries");
            note(
                current.first_token_timeout_ms != wanted.first_token_timeout_ms,
                "first_token_timeout_ms",
            );
            note(
                current.total_timeout_ms != wanted.total_timeout_ms,
                "total_timeout_ms",
            );
            note(
                current.breaker_failures != wanted.breaker_failures,
                "breaker_failures",
            );
            note(
                current.breaker_window_s != wanted.breaker_window_s,
                "breaker_window_s",
            );
            note(
                current.breaker_open_s != wanted.breaker_open_s,
                "breaker_open_s",
            );
            note(current.everyone != wanted.everyone, "everyone");
            note(current.teams != wanted.teams, "teams");
            note(
                current.cache_enabled != wanted.cache_enabled,
                "cache_enabled",
            );
            note(current.cache_ttl_s != wanted.cache_ttl_s, "cache_ttl_s");
            note(current.cache_scope != wanted.cache_scope, "cache_scope");
            // A file that does not say leaves the guardrails alone.
            note(
                wanted
                    .guardrails
                    .as_ref()
                    .is_some_and(|w| *w != current.guardrails.clone().unwrap_or_default()),
                "guardrails",
            );
            if changes.is_empty() {
                self.report.unchanged += 1;
            } else {
                self.push(
                    Op::UpsertRoute {
                        id: Some(existing.id),
                        entry: wanted,
                    },
                    "route",
                    entry.name.clone(),
                    changes,
                    false,
                );
            }
        }
    }

    /// The scope and the subject of a limit or a budget: the scope that was
    /// written, and the label that names them.
    fn subject(
        &mut self,
        at: &str,
        scope: &str,
        name: Option<&String>,
    ) -> Option<(LimitScope, Option<String>)> {
        let parsed = match LimitScope::parse(scope) {
            Some(LimitScope::Key) => {
                self.error(
                    format!("{at}.scope"),
                    "limits and budgets of keys are not part of a configuration file",
                );
                return None;
            }
            Some(parsed) => parsed,
            None => {
                self.error(format!("{at}.scope"), "must be gateway, team or user");
                return None;
            }
        };
        match (parsed, name) {
            (LimitScope::Gateway, Some(_)) => {
                self.error(format!("{at}.name"), "the gateway has no name");
                None
            }
            (LimitScope::Gateway, None) => Some((parsed, None)),
            (_, None) => {
                self.error(format!("{at}.name"), "is required");
                None
            }
            (LimitScope::Team, Some(team)) => {
                if self.teams.contains(team) {
                    Some((parsed, Some(team.clone())))
                } else {
                    self.error(
                        format!("{at}.name"),
                        format!("team '{team}' does not exist"),
                    );
                    None
                }
            }
            (_, Some(email)) => {
                if self.state.users.iter().any(|(_, e)| e == email) {
                    Some((parsed, Some(email.clone())))
                } else {
                    self.error(
                        format!("{at}.name"),
                        format!("user '{email}' does not exist"),
                    );
                    None
                }
            }
        }
    }

    fn limits(&mut self) {
        let file = self.file;
        let state = self.state;
        let mut seen = HashSet::new();
        for (i, entry) in file.limits.iter().enumerate() {
            let at = format!("limits[{i}]");
            let before = self.report.errors.len();
            let subject = self.subject(&at, &entry.scope, entry.name.as_ref());
            let mut fields = BTreeMap::new();
            let as_i64 = |v: Option<u64>| v.map(|v| i64::try_from(v).unwrap_or(i64::MAX));
            let limit = RateLimit {
                requests_per_minute: checked_limit(
                    &mut fields,
                    "requests_per_minute",
                    as_i64(entry.requests_per_minute),
                    MAX_COUNT,
                ),
                tokens_per_minute: checked_limit(
                    &mut fields,
                    "tokens_per_minute",
                    as_i64(entry.tokens_per_minute),
                    MAX_TOKENS,
                ),
                concurrent: checked_limit(
                    &mut fields,
                    "concurrent",
                    as_i64(entry.concurrent),
                    MAX_COUNT,
                ),
            };
            self.fields(&at, fields);
            if self.report.errors.len() == before && limit.is_none() {
                self.error(at.clone(), "set at least one limit");
            }
            let Some((scope, name)) = subject else {
                continue;
            };
            if !seen.insert((scope, name.clone())) {
                self.error(at.clone(), "this subject appears more than once");
            }
            if self.report.errors.len() > before {
                continue;
            }
            let label = subject_label(scope, name.as_deref());
            let existing = state
                .limits
                .iter()
                .find(|l| l.scope == scope && l.name == name && l.has_subject());
            match existing {
                None => self.push(
                    Op::SetLimit { scope, name, limit },
                    "limit",
                    label,
                    Vec::new(),
                    true,
                ),
                Some(row) if row.limit == limit => self.report.unchanged += 1,
                Some(row) => {
                    let mut changes = Vec::new();
                    if row.limit.requests_per_minute != limit.requests_per_minute {
                        changes.push("requests_per_minute".to_string());
                    }
                    if row.limit.tokens_per_minute != limit.tokens_per_minute {
                        changes.push("tokens_per_minute".to_string());
                    }
                    if row.limit.concurrent != limit.concurrent {
                        changes.push("concurrent".to_string());
                    }
                    self.push(
                        Op::SetLimit { scope, name, limit },
                        "limit",
                        label,
                        changes,
                        false,
                    );
                }
            }
        }
    }

    fn budgets(&mut self) {
        let file = self.file;
        let state = self.state;
        let mut seen = HashSet::new();
        for (i, entry) in file.budgets.iter().enumerate() {
            let at = format!("budgets[{i}]");
            let before = self.report.errors.len();
            let subject = self.subject(&at, &entry.scope, entry.name.as_ref());
            if !(1..=MAX_AMOUNT_MICROS.unsigned_abs()).contains(&entry.amount_micros) {
                self.error(
                    format!("{at}.amount_micros"),
                    format!("must be from 1 to {MAX_AMOUNT_MICROS}"),
                );
            }
            let period = Period::parse(&entry.period);
            if period.is_none() {
                self.error(format!("{at}.period"), "must be daily, weekly or monthly");
            }
            let action = BudgetAction::parse(&entry.action);
            if action.is_none() {
                self.error(format!("{at}.action"), "must be block or alert");
            }
            let (Some((scope, name)), Some(period), Some(action)) = (subject, period, action)
            else {
                continue;
            };
            if !seen.insert((scope, name.clone(), period.as_str())) {
                self.error(at.clone(), "this subject and period appear more than once");
            }
            if self.report.errors.len() > before {
                continue;
            }
            let label = format!(
                "{} ({})",
                subject_label(scope, name.as_deref()),
                period.as_str()
            );
            let existing = state.budgets.iter().find(|b| {
                b.scope == scope && b.name == name && b.period == period && b.has_subject()
            });
            let op = Op::SetBudget {
                scope,
                name,
                amount: entry.amount_micros,
                period,
                action,
            };
            match existing {
                None => self.push(op, "budget", label, Vec::new(), true),
                Some(row) if row.amount_micros == entry.amount_micros && row.action == action => {
                    self.report.unchanged += 1;
                }
                Some(row) => {
                    let mut changes = Vec::new();
                    if row.amount_micros != entry.amount_micros {
                        changes.push("amount_micros".to_string());
                    }
                    if row.action != action {
                        changes.push("action".to_string());
                    }
                    self.push(op, "budget", label, changes, false);
                }
            }
        }
    }

    fn alerts(&mut self) {
        let file = self.file;
        let state = self.state;
        let mut channel_names: HashSet<String> = state
            .alert_channels
            .iter()
            .map(|(_, n, _)| n.clone())
            .collect();
        let mut seen = HashSet::new();
        for (i, entry) in file.alert_channels.iter().enumerate() {
            let at = format!("alert_channels[{i}]");
            let before = self.report.errors.len();
            if let Err(message) = trimmed_name(&entry.name) {
                self.error(format!("{at}.name"), message);
            } else if entry.name.trim() != entry.name {
                self.error(format!("{at}.name"), "must not start or end with a space");
            }
            if !["webhook", "slack"].contains(&entry.kind.as_str()) {
                self.error(format!("{at}.kind"), "kind must be webhook or slack");
            }
            if !seen.insert(entry.name.clone()) {
                self.error(at.clone(), "this name appears more than once");
            }
            let existing = state
                .alert_channels
                .iter()
                .find(|(_, n, _)| *n == entry.name);
            if let Some((_, _, kind)) = existing {
                if *kind != entry.kind {
                    self.error(
                        format!("{at}.kind"),
                        format!("channel '{}' exists with another kind", entry.name),
                    );
                }
            }
            channel_names.insert(entry.name.clone());
            if self.report.errors.len() > before {
                continue;
            }
            if existing.is_some() {
                self.report.unchanged += 1;
            } else if !self.can_encrypt {
                self.error(
                    at.clone(),
                    format!(
                        "channel '{}' does not exist; the command line cannot create one (import the file in the console)",
                        entry.name
                    ),
                );
            } else {
                self.report.warnings.push(Issue {
                    at: at.clone(),
                    message: format!("channel '{}' needs a URL", entry.name),
                });
                self.push(
                    Op::CreateAlertChannel {
                        name: entry.name.clone(),
                        kind: entry.kind.clone(),
                    },
                    "alert_channel",
                    entry.name.clone(),
                    Vec::new(),
                    true,
                );
            }
        }

        let mut seen = HashSet::new();
        for (i, entry) in file.alert_rules.iter().enumerate() {
            let at = format!("alert_rules[{i}]");
            let before = self.report.errors.len();
            if let Err(message) = trimmed_name(&entry.name) {
                self.error(format!("{at}.name"), message);
            } else if entry.name.trim() != entry.name {
                self.error(format!("{at}.name"), "must not start or end with a space");
            }
            if !seen.insert(entry.name.clone()) {
                self.error(at.clone(), "this name appears more than once");
            }
            let existing = state.alert_rules.iter().find(|r| r.name == entry.name);
            if let Some(row) = existing {
                if row.kind != entry.kind {
                    self.error(
                        format!("{at}.kind"),
                        format!("rule '{}' exists with another kind", entry.name),
                    );
                }
            }
            for (j, channel) in entry.channels.iter().enumerate() {
                if !channel_names.contains(channel) {
                    self.error(
                        format!("{at}.channels[{j}]"),
                        format!("channel '{channel}' does not exist"),
                    );
                }
            }
            // The parameters, in the form the file keeps them.
            let mut budget = None;
            let mut normal = None;
            if entry.kind == "budget" {
                match serde_json::from_value::<FileBudgetParams>(entry.params.clone()) {
                    Err(e) => self.error(format!("{at}.params"), e.to_string()),
                    Ok(p) => {
                        let reference = match &p.budget {
                            None => None,
                            Some(r) => self.budget_ref(&at, r),
                        };
                        if p.budget.is_some() && reference.is_none() {
                            // already reported
                        } else if let Err(e) = alert_rules::parse(
                            "budget",
                            &json!({ "budget_id": null, "percent": p.percent }),
                        ) {
                            self.error(format!("{at}.{}", e.field), e.message);
                        } else {
                            normal = Some(json!({ "budget": p.budget, "percent": p.percent }));
                            budget = reference;
                        }
                    }
                }
            } else {
                match alert_rules::parse(&entry.kind, &entry.params) {
                    Err(e) => self.error(format!("{at}.{}", e.field), e.message),
                    Ok(p) => normal = Some(p.to_value()),
                }
            }
            if self.report.errors.len() > before {
                continue;
            }
            let Some(params) = normal else { continue };
            let mut channels = entry.channels.clone();
            channels.sort();
            channels.dedup();
            let wanted = AlertRuleEntry {
                name: entry.name.clone(),
                kind: entry.kind.clone(),
                enabled: entry.enabled,
                params,
                channels,
            };
            let was = self
                .current
                .alert_rules
                .iter()
                .find(|r| r.name == entry.name);
            let op = |id: Option<i64>, params_changed: bool| Op::UpsertAlertRule {
                id,
                entry: wanted.clone(),
                budget: budget.clone(),
                params_changed,
            };
            match (existing, was) {
                (None, _) => self.push(
                    op(None, false),
                    "alert_rule",
                    entry.name.clone(),
                    Vec::new(),
                    true,
                ),
                (Some(_), Some(was)) if *was == wanted => self.report.unchanged += 1,
                (Some(row), was) => {
                    let mut changes = Vec::new();
                    let params_changed = was.is_none_or(|w| w.params != wanted.params);
                    if params_changed {
                        changes.push("params".to_string());
                    }
                    if was.is_none_or(|w| w.enabled != wanted.enabled) {
                        changes.push("enabled".to_string());
                    }
                    if was.is_none_or(|w| w.channels != wanted.channels) {
                        changes.push("channels".to_string());
                    }
                    self.push(
                        op(Some(row.id), params_changed),
                        "alert_rule",
                        entry.name.clone(),
                        changes,
                        false,
                    );
                }
            }
        }
    }

    /// A budget a rule names exists on the gateway or in the file.
    fn budget_ref(&mut self, at: &str, r: &BudgetRef) -> Option<BudgetRef> {
        let (scope, name) =
            self.subject(&format!("{at}.params.budget"), &r.scope, r.name.as_ref())?;
        let Some(period) = Period::parse(&r.period) else {
            self.error(
                format!("{at}.params.budget.period"),
                "must be daily, weekly or monthly",
            );
            return None;
        };
        let on_gateway =
            self.state.budgets.iter().any(|b| {
                b.scope == scope && b.name == name && b.period == period && b.has_subject()
            });
        let in_file = self
            .file
            .budgets
            .iter()
            .any(|b| b.scope == r.scope && b.name == name && b.period == r.period);
        if on_gateway || in_file {
            Some(r.clone())
        } else {
            self.error(
                format!("{at}.params.budget"),
                "no budget of this scope, name and period",
            );
            None
        }
    }

    fn settings(&mut self) {
        if let Some(days) = self.file.settings.log_retention_days {
            if !(1..=3650).contains(&days) {
                self.error(
                    "settings.log_retention_days".to_string(),
                    "must be from 1 to 3650",
                );
            } else if days == self.state.log_retention_days {
                self.report.unchanged += 1;
            } else {
                self.push(
                    Op::SetRetention(days),
                    "settings",
                    "log retention".to_string(),
                    vec!["log_retention_days".to_string()],
                    false,
                );
            }
        }
        if let Some(hours) = self.file.settings.session_hours {
            if !SESSION_HOURS_RANGE.contains(&hours) {
                self.error(
                    "settings.session_hours".to_string(),
                    "must be from 1 to 720",
                );
            } else if hours == self.state.session_hours {
                self.report.unchanged += 1;
            } else {
                self.push(
                    Op::SetSessionHours(hours),
                    "settings",
                    "session lifetime".to_string(),
                    vec!["session_hours".to_string()],
                    false,
                );
            }
        }
    }
}

fn subject_label(scope: LimitScope, name: Option<&str>) -> String {
    match name {
        None => scope.as_str().to_string(),
        Some(name) => format!("{} {name}", scope.as_str()),
    }
}

/// Checks the whole file against the configuration as it is, and works out
/// what the import would write.
fn plan(
    file: &ConfigFile,
    state: &ConfigState,
    can_encrypt: bool,
    rule_checks: Vec<BTreeMap<String, String>>,
) -> Plan {
    let mut report = ImportReport::default();
    if file.format != FORMAT {
        report.errors.push(Issue {
            at: "format".to_string(),
            message: format!("this is not an {FORMAT} file"),
        });
    }
    if file.version != VERSION {
        report.errors.push(Issue {
            at: "version".to_string(),
            message: format!(
                "this gateway reads version {VERSION} of the file, not version {}",
                file.version
            ),
        });
    }
    if !report.errors.is_empty() {
        return Plan {
            report,
            ops: Vec::new(),
        };
    }
    let providers = state
        .providers
        .iter()
        .map(|p| p.name.clone())
        .chain(file.providers.iter().map(|p| p.name.clone()))
        .collect();
    let teams = state
        .teams
        .iter()
        .map(|(_, n)| n.clone())
        .chain(file.teams.iter().map(|t| t.name.clone()))
        .collect();
    let providers: HashSet<String> = providers;
    // A model of the file counts when its provider is known: a route that
    // names one of a provider that is not there is wrong too.
    let models = state
        .models
        .iter()
        .map(|m| format!("{}/{}", m.provider_name, m.name))
        .chain(
            file.models
                .iter()
                .filter(|m| providers.contains(&m.provider))
                .map(|m| format!("{}/{}", m.provider, m.name)),
        )
        .collect();
    let mut planner = Planner {
        file,
        state,
        current: file_of(state),
        report,
        ops: Vec::new(),
        providers,
        teams,
        models,
        guardrails: state.guardrails.iter().map(|g| g.name.clone()).collect(),
        rule_checks,
        can_encrypt,
    };
    planner.providers();
    planner.teams();
    planner.models();
    planner.guardrails();
    planner.prompts();
    planner.routes();
    planner.limits();
    planner.budgets();
    planner.alerts();
    planner.settings();
    let mut report = planner.report;
    let mut ops = planner.ops;
    if !report.is_clean() {
        // Nothing is written, so nothing is said to be created or updated.
        report.created.clear();
        report.updated.clear();
        report.unchanged = 0;
        ops.clear();
    }
    Plan { report, ops }
}

/// The ids of what exists, and of what the import creates as it goes.
struct Ids {
    providers: HashMap<String, i64>,
    teams: HashMap<String, i64>,
    users: HashMap<String, i64>,
    models: HashMap<String, i64>,
    alert_channels: HashMap<String, i64>,
    guardrails: HashMap<String, i64>,
}

impl Ids {
    fn of(state: &ConfigState) -> Self {
        Self {
            providers: state
                .providers
                .iter()
                .map(|p| (p.name.clone(), p.id))
                .collect(),
            teams: state.teams.iter().map(|(i, n)| (n.clone(), *i)).collect(),
            users: state.users.iter().map(|(i, e)| (e.clone(), *i)).collect(),
            models: state
                .models
                .iter()
                .map(|m| (format!("{}/{}", m.provider_name, m.name), m.id))
                .collect(),
            alert_channels: state
                .alert_channels
                .iter()
                .map(|(id, name, _)| (name.clone(), *id))
                .collect(),
            guardrails: state
                .guardrails
                .iter()
                .map(|g| (g.name.clone(), g.id))
                .collect(),
        }
    }

    fn guardrail(&self, name: &str) -> Result<i64> {
        self.guardrails
            .get(name)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("a guardrail of the plan is missing"))
    }

    /// Checked by the plan: every name the ops use is known.
    fn team(&self, name: &str) -> Result<i64> {
        self.teams
            .get(name)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("a team of the plan is missing"))
    }

    fn model(&self, reference: &str) -> Result<i64> {
        self.models
            .get(reference)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("a model of the plan is missing"))
    }

    fn grants(&self, entry: &GrantEntry) -> Result<Grants> {
        let team_ids = entry
            .teams
            .iter()
            .map(|t| self.team(t))
            .collect::<Result<Vec<_>>>()?;
        let user_ids = entry
            .users
            .iter()
            .map(|e| {
                self.users
                    .get(e)
                    .copied()
                    .ok_or_else(|| anyhow::anyhow!("a user of the plan is missing"))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Grants {
            everyone: entry.everyone,
            team_ids,
            user_ids,
        })
    }

    fn subject(&self, scope: LimitScope, name: Option<&str>) -> Result<Option<i64>> {
        Ok(match (scope, name) {
            (LimitScope::Team, Some(n)) => Some(self.team(n)?),
            (LimitScope::User, Some(e)) => self.users.get(e).copied(),
            _ => None,
        })
    }
}

async fn apply(tx: &mut Tx<'_>, state: &ConfigState, plan: Plan, actor: &Actor<'_>) -> Result<()> {
    let mut ids = Ids::of(state);
    for planned in plan.ops {
        match planned.op {
            Op::CreateProvider { entry, api_version } => {
                let id = tx
                    .insert_provider_versioned(
                        &entry.name,
                        &entry.kind,
                        &entry.base_url,
                        None,
                        api_version.as_deref(),
                    )
                    .await?;
                ids.providers.insert(entry.name, id);
            }
            Op::UpdateProvider {
                id,
                drop_credential,
                base_url,
                api_version,
            } => {
                if let Some(url) = base_url {
                    let credential = drop_credential.then_some(None);
                    tx.update_provider(id, Some(&url), credential).await?;
                }
                if let Some(version) = api_version {
                    tx.set_provider_api_version(id, version.as_deref()).await?;
                }
            }
            Op::CreateTeam { name } => {
                let id = tx.insert_team(&name).await?;
                ids.teams.insert(name, id);
            }
            Op::CreateModel { entry } => {
                let provider = ids
                    .providers
                    .get(&entry.provider)
                    .copied()
                    .ok_or_else(|| anyhow::anyhow!("a provider of the plan is missing"))?;
                let id = tx.insert_model(provider, &entry.name).await?;
                tx.set_model_enabled(id, entry.enabled).await?;
                tx.set_model_input_price(id, entry.input_price_micros)
                    .await?;
                tx.set_model_output_price(id, entry.output_price_micros)
                    .await?;
                tx.replace_grants(id, &ids.grants(&entry.grants)?).await?;
                ids.models
                    .insert(format!("{}/{}", entry.provider, entry.name), id);
            }
            Op::UpdateModel {
                id,
                enabled,
                input,
                output,
                grants,
            } => {
                if let Some(enabled) = enabled {
                    tx.set_model_enabled(id, enabled).await?;
                }
                if let Some(price) = input {
                    tx.set_model_input_price(id, price).await?;
                }
                if let Some(price) = output {
                    tx.set_model_output_price(id, price).await?;
                }
                if let Some(grants) = grants {
                    tx.replace_grants(id, &ids.grants(&grants)?).await?;
                }
            }
            Op::CreateGuardrail { entry } => {
                let rules = serde_json::to_string(&entry.rules)?;
                let external = entry.external.as_ref();
                // An external one is off, with no URL (the encrypted empty
                // text); its signing secret is made now and shown by
                // "rotate secret".
                let cipher =
                    if external.is_some() {
                        Some(actor.cipher.ok_or_else(|| {
                            anyhow::anyhow!("the import has no key to encrypt with")
                        })?)
                    } else {
                        None
                    };
                let (url_enc, secret_enc) = match cipher {
                    Some(c) => (
                        Some(c.encrypt(b"")),
                        Some(c.encrypt(new_secret().as_bytes())),
                    ),
                    None => (None, None),
                };
                let id = tx
                    .insert_guardrail(NewGuardrail {
                        name: &entry.name,
                        description: &entry.description,
                        kind: &entry.kind,
                        rules: &rules,
                        url: url_enc.as_deref().map(|enc| (enc, "")),
                        secret_enc: secret_enc.as_deref(),
                        timeout_ms: external.map_or(DEFAULT_TIMEOUT_MS, |x| x.timeout_ms),
                        fail_mode: external.map_or("open", |x| x.fail_mode.as_str()),
                        directions: external.map_or(Directions::Both, |x| x.directions).as_str(),
                        enabled: entry.enabled,
                        is_default: entry.is_default,
                    })
                    .await?;
                ids.guardrails.insert(entry.name, id);
            }
            Op::UpdateGuardrail { id, entry } => {
                let rules = (entry.kind == "rules")
                    .then(|| serde_json::to_string(&entry.rules))
                    .transpose()?;
                let external = entry.external.as_ref();
                tx.update_guardrail(
                    id,
                    GuardrailPatch {
                        description: Some(&entry.description),
                        rules: rules.as_deref(),
                        timeout_ms: external.map(|x| x.timeout_ms),
                        fail_mode: external.map(|x| x.fail_mode.as_str()),
                        directions: external.map(|x| x.directions.as_str()),
                        enabled: Some(entry.enabled),
                        is_default: Some(entry.is_default),
                        ..Default::default()
                    },
                )
                .await?;
            }
            Op::UpsertPrompt {
                id,
                entry,
                have,
                description,
            } => {
                let id = match id {
                    Some(id) => {
                        if description {
                            tx.set_prompt_description(id, &entry.description).await?;
                        }
                        id
                    }
                    None => {
                        tx.insert_prompt_template(&entry.name, &entry.description, actor.user_id)
                            .await?
                    }
                };
                for (j, v) in entry.versions.iter().enumerate().skip(have) {
                    let messages = serde_json::to_string(&v.messages)?;
                    let variables = serde_json::to_string(&prompts::variables_in(
                        v.messages.iter().map(|m| m.content.as_str()),
                    ))?;
                    let params = serde_json::to_string(&v.params)?;
                    let number = tx
                        .insert_prompt_version(
                            id,
                            NewVersion {
                                messages: &messages,
                                variables: &variables,
                                model: v.model.as_deref(),
                                params: &params,
                            },
                            actor.user_id,
                        )
                        .await?;
                    if number != j as i64 + 1 {
                        anyhow::bail!("a prompt version of the plan is out of order");
                    }
                }
            }
            Op::UpsertRoute { id, entry } => {
                let settings = RouteSettings {
                    retries: entry.retries,
                    first_token_timeout_ms: entry.first_token_timeout_ms,
                    total_timeout_ms: entry.total_timeout_ms,
                    breaker_failures: entry.breaker_failures,
                    breaker_window_s: entry.breaker_window_s,
                    breaker_open_s: entry.breaker_open_s,
                };
                let id = match id {
                    Some(id) => {
                        tx.update_route(id, &entry.name, &settings, entry.everyone)
                            .await?;
                        id
                    }
                    None => {
                        tx.insert_route(&entry.name, &settings, entry.everyone)
                            .await?
                    }
                };
                let targets = TargetsInput {
                    primaries: entry
                        .primaries
                        .iter()
                        .map(|p| Ok((ids.model(&p.model)?, p.weight)))
                        .collect::<Result<_>>()?,
                    fallbacks: entry
                        .fallbacks
                        .iter()
                        .map(|m| ids.model(m))
                        .collect::<Result<_>>()?,
                };
                tx.replace_targets(id, &targets).await?;
                let team_ids = entry
                    .teams
                    .iter()
                    .map(|t| ids.team(t))
                    .collect::<Result<Vec<_>>>()?;
                tx.replace_route_grants(id, &team_ids).await?;
                if let Some(names) = &entry.guardrails {
                    let guardrail_ids = names
                        .iter()
                        .map(|n| ids.guardrail(n))
                        .collect::<Result<Vec<_>>>()?;
                    tx.replace_route_guardrails(id, &guardrail_ids).await?;
                }
                tx.set_route_cache(
                    id,
                    &RouteCache {
                        enabled: entry.cache_enabled,
                        ttl_s: entry.cache_ttl_s,
                        scope: CacheScope::parse(&entry.cache_scope).unwrap_or(CacheScope::Team),
                    },
                )
                .await?;
            }
            Op::SetLimit { scope, name, limit } => {
                let subject = ids.subject(scope, name.as_deref())?;
                tx.upsert_limit(scope, subject, &limit).await?;
            }
            Op::SetBudget {
                scope,
                name,
                amount,
                period,
                action,
            } => {
                let subject = ids.subject(scope, name.as_deref())?;
                tx.upsert_budget(scope, subject, amount, period, action)
                    .await?;
            }
            Op::CreateAlertChannel { name, kind } => {
                // Off, with no URL: the encrypted empty text. The signing
                // secret is made now and shown by "rotate secret".
                let cipher = actor
                    .cipher
                    .ok_or_else(|| anyhow::anyhow!("the import has no key to encrypt with"))?;
                let id = tx
                    .insert_alert_channel(
                        &name,
                        &kind,
                        &cipher.encrypt(b""),
                        "",
                        &cipher.encrypt(new_secret().as_bytes()),
                        false,
                    )
                    .await?;
                ids.alert_channels.insert(name, id);
            }
            Op::UpsertAlertRule {
                id,
                entry,
                budget,
                params_changed,
            } => {
                let params = match entry.kind.as_str() {
                    "budget" => {
                        let percent = entry.params.get("percent").and_then(Value::as_u64);
                        let budget_id = match &budget {
                            None => None,
                            Some(r) => {
                                let scope = LimitScope::parse(&r.scope);
                                let period = Period::parse(&r.period);
                                let found =
                                    tx.config_state().await?.budgets.into_iter().find(|b| {
                                        Some(b.scope) == scope
                                            && b.name == r.name
                                            && Some(b.period) == period
                                            && b.has_subject()
                                    });
                                Some(
                                    found
                                        .ok_or_else(|| {
                                            anyhow::anyhow!("a budget of the plan is missing")
                                        })?
                                        .id,
                                )
                            }
                        };
                        json!({ "budget_id": budget_id, "percent": percent })
                    }
                    _ => entry.params.clone(),
                };
                let text = params.to_string();
                let rule_id = match id {
                    Some(id) => {
                        tx.update_alert_rule(id, None, Some(&text), Some(entry.enabled))
                            .await?;
                        if params_changed || !entry.enabled {
                            tx.clear_alert_states(id).await?;
                        }
                        id
                    }
                    None => {
                        tx.insert_alert_rule(&entry.name, &entry.kind, &text, entry.enabled)
                            .await?
                    }
                };
                let channel_ids = entry
                    .channels
                    .iter()
                    .map(|n| {
                        ids.alert_channels
                            .get(n)
                            .copied()
                            .ok_or_else(|| anyhow::anyhow!("a channel of the plan is missing"))
                    })
                    .collect::<Result<Vec<_>>>()?;
                tx.set_alert_rule_channels(rule_id, &channel_ids).await?;
            }
            Op::SetRetention(days) => tx.set_log_retention_days(days).await?,
            Op::SetSessionHours(hours) => tx.set_session_hours(hours).await?,
        }
        let verb = if planned.created {
            "Created"
        } else {
            "Updated"
        };
        let summary = if planned.item.changes.is_empty() {
            format!("{verb} {} {}", planned.item.kind, planned.item.name)
        } else {
            format!(
                "{verb} {} {} ({})",
                planned.item.kind,
                planned.item.name,
                planned.item.changes.join(", ")
            )
        };
        tx.audit(AuditEntry {
            actor_user_id: actor.user_id,
            actor_email: actor.email,
            action: &format!("{}.import", planned.item.kind),
            target_type: &planned.item.kind,
            target_id: None,
            summary: &summary,
        })
        .await?;
    }
    Ok(())
}

/// The faults of the rules of every `rules` guardrail of the file, in file
/// order (empty for the others). Compiling is slow, so the import does this
/// off the async threads and before it takes the write lock.
fn rule_checks_of(file: &ConfigFile) -> Vec<BTreeMap<String, String>> {
    file.guardrails
        .iter()
        .map(|g| {
            let mut fields = BTreeMap::new();
            if g.kind == "rules" {
                check_rules_sync(&g.rules, &mut fields);
            }
            fields
        })
        .collect()
}

/// Checks the file and, unless `dry_run` or it has errors, writes it, in
/// one transaction with its audit rows. The report says what was done or,
/// for a dry run, what would be. The caller refreshes the snapshot.
pub async fn import(
    store: &Store,
    file: &ConfigFile,
    actor: &Actor<'_>,
    dry_run: bool,
) -> Result<ImportReport> {
    let owned = file.clone();
    let rule_checks = tokio::task::spawn_blocking(move || rule_checks_of(&owned))
        .await
        .map_err(|e| anyhow::anyhow!("the rule check failed: {e}"))?;
    let mut tx = store.begin_immediate().await?;
    let state = tx.config_state().await?;
    let planned = plan(file, &state, actor.cipher.is_some(), rule_checks);
    let report = planned.report.clone();
    if dry_run || !report.is_clean() {
        // Nothing was written: the transaction is dropped, not committed.
        return Ok(report);
    }
    apply(&mut tx, &state, planned, actor).await?;
    let changed = report.created.len() + report.updated.len();
    if changed > 0 || report.unchanged > 0 {
        tx.audit(AuditEntry {
            actor_user_id: actor.user_id,
            actor_email: actor.email,
            action: "config.import",
            target_type: "config",
            target_id: None,
            summary: &format!(
                "Imported configuration: {} created, {} updated, {} unchanged",
                report.created.len(),
                report.updated.len(),
                report.unchanged
            ),
        })
        .await?;
    }
    tx.commit().await?;
    Ok(report)
}
