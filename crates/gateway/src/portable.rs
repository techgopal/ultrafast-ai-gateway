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
use ultrafast_translate::provider::ProviderKind;

use crate::api::budgets::MAX_AMOUNT_MICROS;
use crate::api::limits::{checked as checked_limit, MAX_COUNT, MAX_TOKENS};
use crate::api::providers::{check_api_version, checked as checked_provider};
use crate::api::routes::check_settings;
use crate::api::teams::valid_team_name;
use crate::budgets::{BudgetAction, Period};
use crate::cache::{CacheScope, RouteCache};
use crate::catalog::validate_model_name;
use crate::config::{validate_base_url, validate_provider_name};
use crate::limits::{LimitScope, RateLimit};
use crate::store::{AuditEntry, ConfigState, Grants, RouteSettings, Store, TargetsInput, Tx};

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
}

/// Something the import did, or would do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct Item {
    /// `provider`, `team`, `model`, `route`, `limit`, `budget` or `settings`.
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

    ConfigFile {
        format: FORMAT.to_string(),
        version: VERSION,
        providers,
        models,
        teams,
        routes,
        limits: limits.into_iter().map(|(_, l)| l).collect(),
        budgets: budgets.into_iter().map(|(_, _, b)| b).collect(),
        settings: SettingsEntry {
            log_retention_days: Some(state.log_retention_days),
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
    SetRetention(i64),
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
    report: ImportReport,
    ops: Vec<Planned>,
    /// Names the file or the gateway has.
    providers: HashSet<String>,
    teams: HashSet<String>,
    models: HashSet<String>,
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
                    if existing.base_url != entry.base_url {
                        changes.push("base_url".to_string());
                    }
                    if existing.api_version != api_version {
                        changes.push("api_version".to_string());
                    }
                    if changes.is_empty() {
                        self.report.unchanged += 1;
                    } else {
                        let op = Op::UpdateProvider {
                            id: existing.id,
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
            if self.report.errors.len() > before {
                continue;
            }
            let mut wanted = entry.clone();
            wanted.teams.sort();
            wanted.teams.dedup();
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
            let current = file_of(state)
                .routes
                .into_iter()
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

    fn settings(&mut self) {
        let Some(days) = self.file.settings.log_retention_days else {
            return;
        };
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
}

fn subject_label(scope: LimitScope, name: Option<&str>) -> String {
    match name {
        None => scope.as_str().to_string(),
        Some(name) => format!("{} {name}", scope.as_str()),
    }
}

/// Checks the whole file against the configuration as it is, and works out
/// what the import would write.
fn plan(file: &ConfigFile, state: &ConfigState) -> Plan {
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
        report,
        ops: Vec::new(),
        providers,
        teams,
        models,
    };
    planner.providers();
    planner.teams();
    planner.models();
    planner.routes();
    planner.limits();
    planner.budgets();
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
        }
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
                base_url,
                api_version,
            } => {
                if let Some(url) = base_url {
                    tx.update_provider(id, Some(&url), None).await?;
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
            Op::SetRetention(days) => tx.set_log_retention_days(days).await?,
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

/// Checks the file and, unless `dry_run` or it has errors, writes it, in
/// one transaction with its audit rows. The report says what was done or,
/// for a dry run, what would be. The caller refreshes the snapshot.
pub async fn import(
    store: &Store,
    file: &ConfigFile,
    actor: &Actor<'_>,
    dry_run: bool,
) -> Result<ImportReport> {
    let mut tx = store.begin().await?;
    let state = tx.config_state().await?;
    let planned = plan(file, &state);
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
