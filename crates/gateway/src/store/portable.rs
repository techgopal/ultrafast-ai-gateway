//! What the configuration export and import read: the configuration tables
//! as they are at one moment, with the names that identify them.

use anyhow::Result;
use sqlx::AnyConnection;
use sqlx::Row;

use super::dialect::Dialected;
use super::{
    budgets, limits, models, providers, routes, settings, BudgetRow, GrantRow, LimitRow, ModelRow,
    ProviderRow, RouteRow, Store, TargetRow, Tx, DEFAULT_ORG,
};

/// The configuration of the gateway, as stored. No key, token, password,
/// session, log or audit row is read: only what the export file holds.
pub struct ConfigState {
    pub providers: Vec<ProviderRow>,
    pub models: Vec<ModelRow>,
    pub model_grants: Vec<GrantRow>,
    /// `(id, name)`.
    pub teams: Vec<(i64, String)>,
    /// `(id, email)`.
    pub users: Vec<(i64, String)>,
    pub routes: Vec<RouteRow>,
    pub route_targets: Vec<TargetRow>,
    /// `(route_id, team_id)`.
    pub route_grants: Vec<(i64, i64)>,
    pub limits: Vec<LimitRow>,
    pub budgets: Vec<BudgetRow>,
    pub log_retention_days: i64,
    pub session_hours: i64,
    /// `(id, name, kind)`: never a URL or a secret.
    pub alert_channels: Vec<(i64, String, String)>,
    pub alert_rules: Vec<super::alerts::RuleRow>,
    pub guardrails: Vec<super::GuardrailRow>,
    /// `(route id, guardrail id, guardrail name)`, in each route's order.
    pub route_guardrails: Vec<(i64, i64, String)>,
    /// `(team id, guardrail id, guardrail name)`, in each team's order.
    pub team_guardrails: Vec<(i64, i64, String)>,
    /// Every prompt template, by name, and all their versions.
    pub prompt_templates: Vec<super::TemplateRow>,
    pub prompt_versions: Vec<super::VersionRow>,
}

async fn read(conn: &mut AnyConnection) -> Result<ConfigState> {
    let providers = providers::list_providers_in(conn).await?;
    let models = models::list_models_in(conn).await?;
    let model_grants = models::list_model_grants_in(conn).await?;
    let teams = conn
        .q("SELECT id, name FROM teams WHERE org_id = ? ORDER BY name")
        .bind(DEFAULT_ORG)
        .fetch_all(&mut *conn)
        .await?
        .iter()
        .map(|r| (r.get("id"), r.get("name")))
        .collect();
    let users = conn
        .q("SELECT id, email FROM users WHERE org_id = ? ORDER BY email")
        .bind(DEFAULT_ORG)
        .fetch_all(&mut *conn)
        .await?
        .iter()
        .map(|r| (r.get("id"), r.get("email")))
        .collect();
    let routes = routes::list_routes_in(conn).await?;
    let route_targets = routes::list_route_targets_in(conn).await?;
    let route_grants = routes::list_route_grants_in(conn).await?;
    let limits = limits::list_limits_in(conn).await?;
    let budgets = budgets::list_budgets_in(conn).await?;
    let log_retention_days = settings::log_retention_days_in(conn).await?;
    let session_hours = settings::session_hours_in(conn).await?;
    let alert_channels = super::alerts::list_channel_names_in(conn).await?;
    let alert_rules = super::alerts::list_alert_rules_in(conn).await?;
    let guardrails = super::guardrails::list_guardrails_in(conn).await?;
    let route_guardrails = super::guardrails::route_guardrail_refs_in(conn).await?;
    let team_guardrails = super::guardrails::team_guardrail_refs_in(conn).await?;
    let prompt_templates = super::prompts::list_templates_in(conn).await?;
    let prompt_versions = super::prompts::list_versions_in(conn).await?;
    Ok(ConfigState {
        providers,
        models,
        model_grants,
        teams,
        users,
        routes,
        route_targets,
        route_grants,
        limits,
        budgets,
        log_retention_days,
        session_hours,
        alert_channels,
        alert_rules,
        guardrails,
        route_guardrails,
        team_guardrails,
        prompt_templates,
        prompt_versions,
    })
}

impl Store {
    /// The configuration, read in one transaction.
    pub async fn config_state(&self) -> Result<ConfigState> {
        let mut tx = self.begin_read().await?;
        let state = read(&mut tx).await?;
        tx.commit().await?;
        Ok(state)
    }
}

impl Tx<'_> {
    /// The configuration, as this transaction sees it.
    pub async fn config_state(&mut self) -> Result<ConfigState> {
        read(self.conn()).await
    }
}
