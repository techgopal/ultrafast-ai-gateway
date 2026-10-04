//! The configuration export and import: the file, its secrets, the report,
//! the transaction, the audit and the snapshot.

mod common;

use axum::http::StatusCode;
use common::{call, error_code, org, org_with_sink, seed_user, Org, ORG_PASSWORD};
use serde_json::{json, Value};
use ultrafast_gateway::budgets::{BudgetAction, Period};
use ultrafast_gateway::cache::{CacheScope, RouteCache};
use ultrafast_gateway::identity::Role;
use ultrafast_gateway::limits::{LimitScope, RateLimit};
use ultrafast_gateway::portable::{self, Actor, ConfigFile};
use ultrafast_gateway::secrets::{generate_key, generate_secret, TOKEN_PREFIX};
use ultrafast_gateway::store::{Grants, RouteSettings, Store, TargetsInput};

const CREDENTIAL: &str = "sk-live-credential-of-the-main-provider";

/// What the world seeds that must never be in an export.
struct Secrets {
    texts: Vec<String>,
}

struct World {
    org: Org,
    secrets: Secrets,
}

fn route_settings(retries: i64) -> RouteSettings {
    RouteSettings {
        retries,
        first_token_timeout_ms: 20_000,
        total_timeout_ms: 120_000,
        breaker_failures: 4,
        breaker_window_s: 30,
        breaker_open_s: 15,
    }
}

/// A gateway with a provider that has a credential, one that has none, models
/// with grants and prices, routes, limits, budgets and a setting, and with
/// secrets of every kind in the other tables.
async fn world() -> World {
    let org = org().await;
    let store = &org.api.store;
    let cipher = &org.api.state.cipher;
    let credential = cipher.encrypt(CREDENTIAL.as_bytes());
    let main = store
        .insert_provider(
            "main",
            "openai",
            "https://api.openai.example/v1",
            Some(&credential),
        )
        .await
        .unwrap();
    let mut tx = store.begin().await.unwrap();
    let azure = tx
        .insert_provider_versioned(
            "azure1",
            "azure",
            "https://az.example",
            None,
            Some("2025-03-01-preview"),
        )
        .await
        .unwrap();
    let gpt4o = tx.insert_model(main, "gpt-4o").await.unwrap();
    tx.set_model_enabled(gpt4o, true).await.unwrap();
    tx.set_model_input_price(gpt4o, Some(2_500_000))
        .await
        .unwrap();
    tx.set_model_output_price(gpt4o, Some(10_000_000))
        .await
        .unwrap();
    tx.replace_grants(
        gpt4o,
        &Grants {
            everyone: false,
            team_ids: vec![org.platform],
            user_ids: vec![org.tomas],
        },
    )
    .await
    .unwrap();
    let mini = tx.insert_model(main, "gpt-4o-mini").await.unwrap();
    tx.set_model_enabled(mini, true).await.unwrap();
    tx.replace_grants(
        mini,
        &Grants {
            everyone: true,
            ..Grants::default()
        },
    )
    .await
    .unwrap();
    let dep = tx.insert_model(azure, "dep1").await.unwrap();
    let chat = tx
        .insert_route("chat", &route_settings(2), false)
        .await
        .unwrap();
    tx.replace_targets(
        chat,
        &TargetsInput {
            primaries: vec![(gpt4o, 3), (mini, 1)],
            fallbacks: vec![dep],
        },
    )
    .await
    .unwrap();
    tx.replace_route_grants(chat, &[org.research])
        .await
        .unwrap();
    tx.set_route_cache(
        chat,
        &RouteCache {
            enabled: true,
            ttl_s: 60,
            scope: CacheScope::User,
        },
    )
    .await
    .unwrap();
    let open = tx
        .insert_route("open", &route_settings(0), true)
        .await
        .unwrap();
    tx.replace_targets(
        open,
        &TargetsInput {
            primaries: vec![(mini, 1)],
            fallbacks: vec![],
        },
    )
    .await
    .unwrap();
    // Limits and budgets, with one of each on a key, which is not exported.
    let key = generate_key();
    let key_id = tx
        .insert_key("ci", &key.hash, &key.display, None, Some(org.lena), None)
        .await
        .unwrap();
    for (scope, id, rpm, tpm, conc) in [
        (LimitScope::Gateway, None, Some(100), None, None),
        (
            LimitScope::Team,
            Some(org.platform),
            None,
            Some(50_000),
            None,
        ),
        (LimitScope::User, Some(org.lena), None, None, Some(3)),
        (LimitScope::Key, Some(key_id), Some(7), None, None),
    ] {
        tx.upsert_limit(
            scope,
            id,
            &RateLimit {
                requests_per_minute: rpm,
                tokens_per_minute: tpm,
                concurrent: conc,
            },
        )
        .await
        .unwrap();
    }
    for (scope, id, amount, period, action) in [
        (
            LimitScope::Gateway,
            None,
            100_000_000,
            Period::Daily,
            BudgetAction::Alert,
        ),
        (
            LimitScope::Team,
            Some(org.research),
            20_000_000,
            Period::Monthly,
            BudgetAction::Block,
        ),
        (
            LimitScope::User,
            Some(org.tomas),
            5_000_000,
            Period::Weekly,
            BudgetAction::Block,
        ),
        (
            LimitScope::Key,
            Some(key_id),
            1_000_000,
            Period::Daily,
            BudgetAction::Block,
        ),
    ] {
        tx.upsert_budget(scope, id, amount, period, action)
            .await
            .unwrap();
    }
    tx.set_log_retention_days(45).await.unwrap();
    // Secrets of every other kind.
    let token = generate_secret(TOKEN_PREFIX);
    tx.insert_token(org.maya, "ci", &token.hash, &token.display, None)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let session = store.create_session(org.maya).await.unwrap();
    store
        .insert_logs(&[ultrafast_gateway::store::NewLog {
            at: "2026-01-01 10:00:00".into(),
            key_id: Some(key_id),
            user_id: Some(org.lena),
            team_id: None,
            requested: "a-secret-request-name".into(),
            endpoint: "chat".into(),
            stream: false,
            status: 200,
            provider: Some("main".into()),
            model: Some("gpt-4o".into()),
            input_tokens: Some(1),
            output_tokens: Some(1),
            cost_micros: 0,
            priced: false,
            cached: false,
            estimated: false,
            duration_ms: 1,
            attempts: "[]".into(),
        }])
        .await
        .unwrap();
    org.api.state.refresh().await.unwrap();
    let hash = {
        // The stored hash of a user's password is a secret too.
        let user = store.user_by_id(org.maya).await.unwrap().unwrap();
        user.password_hash.unwrap()
    };
    let secrets = Secrets {
        texts: vec![
            CREDENTIAL.to_string(),
            hex::encode(cipher.encrypt(CREDENTIAL.as_bytes())),
            key.full,
            key.hash,
            key.display,
            token.full,
            token.hash,
            token.display,
            session.id,
            session.csrf_token,
            hash,
            "a-secret-request-name".to_string(),
            ORG_PASSWORD.to_string(),
        ],
    };
    World { org, secrets }
}

impl World {
    async fn admin(&self) -> common::Signed {
        self.org.sign_in("maya").await
    }

    async fn export(&self) -> Value {
        let (status, _, body) = call(
            &self.org.api.app,
            "GET",
            "/api/config/export",
            Some(&self.admin().await),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }
}

fn expected_export() -> Value {
    json!({
        "format": "ultrafast-config",
        "version": 1,
        "providers": [
            { "name": "azure1", "kind": "azure", "base_url": "https://az.example", "api_version": "2025-03-01-preview" },
            { "name": "main", "kind": "openai", "base_url": "https://api.openai.example/v1", "api_version": null },
        ],
        "models": [
            { "provider": "azure1", "name": "dep1", "enabled": false,
              "input_price_micros": null, "output_price_micros": null,
              "grants": { "everyone": false, "teams": [], "users": [] } },
            { "provider": "main", "name": "gpt-4o", "enabled": true,
              "input_price_micros": 2_500_000, "output_price_micros": 10_000_000,
              "grants": { "everyone": false, "teams": ["Platform"], "users": ["tomas@example.com"] } },
            { "provider": "main", "name": "gpt-4o-mini", "enabled": true,
              "input_price_micros": null, "output_price_micros": null,
              "grants": { "everyone": true, "teams": [], "users": [] } },
        ],
        "teams": [
            { "name": "Growth" }, { "name": "Platform" }, { "name": "Research" },
        ],
        "routes": [
            { "name": "chat",
              "primaries": [ { "model": "main/gpt-4o", "weight": 3 }, { "model": "main/gpt-4o-mini", "weight": 1 } ],
              "fallbacks": [ "azure1/dep1" ],
              "retries": 2, "first_token_timeout_ms": 20000, "total_timeout_ms": 120000,
              "breaker_failures": 4, "breaker_window_s": 30, "breaker_open_s": 15,
              "everyone": false, "teams": ["Research"],
              "cache_enabled": true, "cache_ttl_s": 60, "cache_scope": "user" },
            { "name": "open",
              "primaries": [ { "model": "main/gpt-4o-mini", "weight": 1 } ],
              "fallbacks": [],
              "retries": 0, "first_token_timeout_ms": 20000, "total_timeout_ms": 120000,
              "breaker_failures": 4, "breaker_window_s": 30, "breaker_open_s": 15,
              "everyone": true, "teams": [],
              "cache_enabled": false, "cache_ttl_s": 300, "cache_scope": "team" },
        ],
        "limits": [
            { "scope": "gateway", "name": null, "requests_per_minute": 100, "tokens_per_minute": null, "concurrent": null },
            { "scope": "team", "name": "Platform", "requests_per_minute": null, "tokens_per_minute": 50000, "concurrent": null },
            { "scope": "user", "name": "lena@example.com", "requests_per_minute": null, "tokens_per_minute": null, "concurrent": 3 },
        ],
        "budgets": [
            { "scope": "gateway", "name": null, "amount_micros": 100_000_000, "period": "daily", "action": "alert" },
            { "scope": "team", "name": "Research", "amount_micros": 20_000_000, "period": "monthly", "action": "block" },
            { "scope": "user", "name": "tomas@example.com", "amount_micros": 5_000_000, "period": "weekly", "action": "block" },
        ],
        "settings": { "log_retention_days": 45 },
    })
}

#[tokio::test]
async fn the_export_has_what_the_spec_lists_and_nothing_of_keys() {
    let w = world().await;
    let body = w.export().await;
    assert_eq!(body, expected_export());
}

#[tokio::test]
async fn the_export_holds_no_secret() {
    let w = world().await;
    let (status, headers, _) = call(
        &w.org.api.app,
        "GET",
        "/api/config/export",
        Some(&w.admin().await),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let disposition = headers["content-disposition"].to_str().unwrap();
    assert!(
        disposition.starts_with("attachment; filename=\"ultrafast-config"),
        "{disposition}"
    );
    let text = w.export().await.to_string();
    for secret in &w.secrets.texts {
        assert!(
            !text.contains(secret.as_str()),
            "the export holds {}...",
            &secret[..6.min(secret.len())]
        );
    }
    // No long run of hex digits is in it, which is what a key, a hash or a
    // master key looks like.
    let longest = text
        .split(|c: char| !c.is_ascii_hexdigit())
        .map(str::len)
        .max()
        .unwrap_or(0);
    assert!(longest < 32, "a run of {longest} hex digits");
    // Nor a field that names a secret.
    for word in [
        "credential",
        "api_key",
        "password",
        "csrf",
        "key_hash",
        "token_hash",
        "session",
        "secret",
        "bearer",
    ] {
        assert!(
            !text.to_lowercase().contains(word),
            "{word} is in the export"
        );
    }
}

#[tokio::test]
async fn an_export_imported_into_a_fresh_database_exports_the_same() {
    let w = world().await;
    let first = w.export().await;

    // The users are not part of the file: they exist where it is imported.
    let fresh = org().await;
    let maya = fresh.sign_in("maya").await;
    // Teams of the same names are created by the import.
    let (status, _, body) = call(
        &fresh.api.app,
        "POST",
        "/api/config/import?dry_run=false",
        Some(&maya),
        Some(first.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["errors"].as_array().unwrap().is_empty());
    let (_, _, second) = call(
        &fresh.api.app,
        "GET",
        "/api/config/export",
        Some(&maya),
        None,
    )
    .await;
    assert_eq!(second, first);
}

fn file_of(value: Value) -> ConfigFile {
    serde_json::from_value(value).expect("a configuration file")
}

fn actor() -> Actor<'static> {
    Actor {
        user_id: None,
        email: "cli",
    }
}

#[tokio::test]
async fn a_second_import_changes_nothing() {
    let w = world().await;
    let file = file_of(expected_export());
    let store = &w.org.api.store;
    let report = portable::import(store, &file, &actor(), false)
        .await
        .unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert!(report.created.is_empty());
    assert!(report.updated.is_empty(), "{:?}", report.updated);
    // Every thing of the file is there already: 2 providers, 3 models,
    // 3 teams, 2 routes, 3 limits, 3 budgets and the settings.
    assert_eq!(report.unchanged, 17);
    // Only the summary of the import is audited when nothing changed.
    let actions: Vec<String> = store
        .list_audit(50, None)
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.action)
        .collect();
    assert!(actions.iter().all(|a| a != "model.import"), "{actions:?}");
}

#[tokio::test]
async fn import_creates_what_is_missing_updates_what_differs_and_deletes_nothing() {
    let w = world().await;
    let store = &w.org.api.store;
    let mut changed = expected_export();
    // One more of everything, a change in each existing kind, and one
    // thing that is in the gateway and not in the file.
    changed["providers"] = json!([
        { "name": "main", "kind": "openai", "base_url": "https://eu.openai.example/v1", "api_version": null },
        { "name": "extra", "kind": "anthropic", "base_url": "https://api.anthropic.example", "api_version": null },
    ]);
    changed["models"] = json!([
        { "provider": "main", "name": "gpt-4o", "enabled": false,
          "input_price_micros": 3_000_000, "output_price_micros": 10_000_000,
          "grants": { "everyone": true, "teams": [], "users": [] } },
        { "provider": "extra", "name": "claude", "enabled": true,
          "input_price_micros": null, "output_price_micros": null,
          "grants": { "everyone": false, "teams": ["Design"], "users": ["priya@example.com"] } },
    ]);
    changed["teams"] = json!([{ "name": "Design" }]);
    changed["routes"] = json!([
        { "name": "chat",
          "primaries": [ { "model": "extra/claude", "weight": 2 } ],
          "fallbacks": [ "main/gpt-4o" ],
          "retries": 1, "first_token_timeout_ms": 10000, "total_timeout_ms": 60000,
          "breaker_failures": 3, "breaker_window_s": 20, "breaker_open_s": 10,
          "everyone": true, "teams": [],
          "cache_enabled": false, "cache_ttl_s": 300, "cache_scope": "team" },
    ]);
    changed["limits"] = json!([
        { "scope": "gateway", "name": null, "requests_per_minute": 200, "tokens_per_minute": null, "concurrent": null },
        { "scope": "team", "name": "Design", "requests_per_minute": 5, "tokens_per_minute": null, "concurrent": null },
    ]);
    changed["budgets"] = json!([
        { "scope": "user", "name": "priya@example.com", "amount_micros": 1_000_000, "period": "daily", "action": "block" },
    ]);
    changed["settings"] = json!({ "log_retention_days": 90 });

    let report = portable::import(store, &file_of(changed), &actor(), false)
        .await
        .unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let created: Vec<String> = report
        .created
        .iter()
        .map(|i| format!("{} {}", i.kind, i.name))
        .collect();
    assert_eq!(
        created,
        [
            "provider extra",
            "team Design",
            "model extra/claude",
            "limit team Design",
            "budget user priya@example.com (daily)",
        ]
    );
    let updated: Vec<String> = report
        .updated
        .iter()
        .map(|i| format!("{} {}", i.kind, i.name))
        .collect();
    assert_eq!(
        updated,
        [
            "provider main",
            "model main/gpt-4o",
            "route chat",
            "limit gateway",
            "settings log retention"
        ]
    );
    let model = report
        .updated
        .iter()
        .find(|i| i.name == "main/gpt-4o")
        .unwrap();
    assert_eq!(model.changes, ["enabled", "input_price_micros", "grants"]);

    // Nothing was deleted: the other model, route, limits and budgets stay.
    let state = store.config_state().await.unwrap();
    assert_eq!(state.providers.len(), 3);
    assert_eq!(state.models.len(), 4);
    assert_eq!(state.routes.len(), 2);
    assert_eq!(
        state.limits.len(),
        5,
        "gateway, platform, lena, key, design"
    );
    assert_eq!(state.budgets.len(), 5);
    assert_eq!(state.log_retention_days, 90);
    // What the file says is now so.
    let chat = state.routes.iter().find(|r| r.name == "chat").unwrap();
    assert!(chat.everyone && !chat.cache.enabled);
    assert_eq!(chat.settings.retries, 1);
    let main = state.providers.iter().find(|p| p.name == "main").unwrap();
    assert_eq!(main.base_url, "https://eu.openai.example/v1");
    // The credential of an existing provider is left as it was.
    assert!(main.credential.is_some());
}

#[tokio::test]
async fn new_providers_have_no_credential_and_the_report_says_so() {
    let w = world().await;
    let store = &w.org.api.store;
    let mut file = expected_export();
    file["providers"] = json!([{ "name": "extra", "kind": "openai", "base_url": "https://x.example", "api_version": null }]);
    let report = portable::import(store, &file_of(file), &actor(), false)
        .await
        .unwrap();
    assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
    assert_eq!(report.warnings[0].at, "providers[0]");
    assert!(
        report.warnings[0].message.contains("no credential"),
        "{}",
        report.warnings[0].message
    );
    let added = store.provider_by_name("extra").await.unwrap().unwrap();
    assert!(added.credential.is_none());
}

#[tokio::test]
async fn a_dry_run_reports_and_writes_nothing() {
    let w = world().await;
    let store = &w.org.api.store;
    let before = audit_count(store).await;
    let mut file = expected_export();
    file["teams"] = json!([{ "name": "Design" }]);
    file["settings"] = json!({ "log_retention_days": 7 });
    let report = portable::import(store, &file_of(file), &actor(), true)
        .await
        .unwrap();
    assert!(report.errors.is_empty());
    assert_eq!(
        report
            .created
            .iter()
            .map(|i| i.name.as_str())
            .collect::<Vec<_>>(),
        ["Design"]
    );
    assert_eq!(report.updated.len(), 1);
    let state = store.config_state().await.unwrap();
    assert_eq!(state.log_retention_days, 45);
    assert_eq!(state.teams.len(), 3);
    assert_eq!(audit_count(store).await, before);
}

async fn audit_count(store: &Store) -> usize {
    store.list_audit(200, None).await.unwrap().len()
}

#[tokio::test]
async fn unknown_references_are_reported_exactly_and_nothing_is_written() {
    let w = world().await;
    let store = &w.org.api.store;
    let before_audit = audit_count(store).await;
    let state = store.config_state().await.unwrap();
    let ids = |s: &ultrafast_gateway::store::ConfigState| {
        (
            s.providers.len(),
            s.models.len(),
            s.teams.len(),
            s.routes.len(),
            s.limits.len(),
            s.budgets.len(),
            s.log_retention_days,
        )
    };
    let before = ids(&state);

    let file = json!({
        "format": "ultrafast-config", "version": 1,
        "providers": [{ "name": "fresh", "kind": "openai", "base_url": "https://f.example", "api_version": null }],
        "teams": [{ "name": "Brand new" }],
        "models": [
            { "provider": "fresh", "name": "ok", "enabled": true, "input_price_micros": null, "output_price_micros": null,
              "grants": { "everyone": false, "teams": ["Brand new", "No such team"], "users": ["nobody@example.com", "tomas@example.com"] } },
            { "provider": "ghost", "name": "m", "enabled": true, "input_price_micros": null, "output_price_micros": null,
              "grants": { "everyone": true, "teams": [], "users": [] } },
        ],
        "routes": [
            { "name": "r", "primaries": [ { "model": "fresh/ok", "weight": 1 }, { "model": "fresh/missing", "weight": 1 } ],
              "fallbacks": [ "ghost/m" ],
              "retries": 0, "first_token_timeout_ms": 1000, "total_timeout_ms": 2000,
              "breaker_failures": 1, "breaker_window_s": 5, "breaker_open_s": 5,
              "everyone": false, "teams": ["No such team"],
              "cache_enabled": false, "cache_ttl_s": 300, "cache_scope": "team" },
        ],
        "limits": [
            { "scope": "user", "name": "nobody@example.com", "requests_per_minute": 1, "tokens_per_minute": null, "concurrent": null },
        ],
        "budgets": [
            { "scope": "team", "name": "No such team", "amount_micros": 1, "period": "daily", "action": "block" },
        ],
        "settings": { "log_retention_days": 9999 },
    });
    let report = portable::import(store, &file_of(file.clone()), &actor(), false)
        .await
        .unwrap();
    let errors: Vec<(String, String)> = report
        .errors
        .iter()
        .map(|e| (e.at.clone(), e.message.clone()))
        .collect();
    let said = |at: &str, message: &str| errors.contains(&(at.to_string(), message.to_string()));
    assert!(
        said(
            "models[0].grants.teams[1]",
            "team 'No such team' does not exist"
        ),
        "{errors:?}"
    );
    assert!(
        said(
            "models[0].grants.users[0]",
            "user 'nobody@example.com' does not exist"
        ),
        "{errors:?}"
    );
    assert!(
        said(
            "models[1].provider",
            "provider 'ghost' is not in the file or in the gateway"
        ),
        "{errors:?}"
    );
    assert!(
        said(
            "routes[0].primaries[1].model",
            "model 'fresh/missing' is not in the file or in the gateway"
        ),
        "{errors:?}"
    );
    assert!(
        said(
            "routes[0].fallbacks[0]",
            "model 'ghost/m' is not in the file or in the gateway"
        ),
        "{errors:?}"
    );
    assert!(
        said("routes[0].teams[0]", "team 'No such team' does not exist"),
        "{errors:?}"
    );
    assert!(
        said("limits[0].name", "user 'nobody@example.com' does not exist"),
        "{errors:?}"
    );
    assert!(
        said("budgets[0].name", "team 'No such team' does not exist"),
        "{errors:?}"
    );
    assert!(
        said("settings.log_retention_days", "must be from 1 to 3650"),
        "{errors:?}"
    );
    // A team named in the file is known, though it is created by the import.
    assert!(
        !errors
            .iter()
            .any(|(at, _)| at == "models[0].grants.teams[0]"),
        "{errors:?}"
    );
    // Nothing of the valid parts was written either.
    assert!(report.created.is_empty() && report.updated.is_empty());
    assert_eq!(ids(&store.config_state().await.unwrap()), before);
    assert_eq!(audit_count(store).await, before_audit);

    // Through the API: 422, and the body is the report.
    let maya = w.admin().await;
    for query in ["", "?dry_run=true", "?dry_run=false"] {
        let (status, _, body) = call(
            &w.org.api.app,
            "POST",
            &format!("/api/config/import{query}"),
            Some(&maya),
            Some(file.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{query}: {body}");
        assert_eq!(
            body["errors"].as_array().unwrap().len(),
            report.errors.len()
        );
        assert!(body["created"].as_array().unwrap().is_empty());
    }
    assert_eq!(ids(&store.config_state().await.unwrap()), before);
}

#[tokio::test]
async fn values_are_checked_as_the_api_checks_them() {
    let w = world().await;
    let store = &w.org.api.store;
    let file = json!({
        "format": "ultrafast-config", "version": 1,
        "providers": [
            { "name": "Bad Name", "kind": "openai", "base_url": "https://a.example", "api_version": null },
            { "name": "k", "kind": "palm", "base_url": "https://a.example", "api_version": null },
            { "name": "u", "kind": "openai", "base_url": "ftp://a.example", "api_version": null },
            { "name": "v", "kind": "openai", "base_url": "https://a.example", "api_version": "2024-10-21" },
            { "name": "main", "kind": "anthropic", "base_url": "https://a.example", "api_version": null },
            { "name": "main", "kind": "openai", "base_url": "https://a.example", "api_version": null },
        ],
        "models": [
            { "provider": "main", "name": "has space", "enabled": true, "input_price_micros": -1, "output_price_micros": null,
              "grants": { "everyone": true, "teams": ["Platform"], "users": [] } },
        ],
        "teams": [{ "name": "" }, { "name": "Platform" }, { "name": "Platform" }],
        "routes": [
            { "name": "Bad Route", "primaries": [], "fallbacks": [],
              "retries": 9, "first_token_timeout_ms": 5, "total_timeout_ms": 5,
              "breaker_failures": 0, "breaker_window_s": 1, "breaker_open_s": 1,
              "everyone": true, "teams": ["Platform"],
              "cache_enabled": false, "cache_ttl_s": 0, "cache_scope": "everyone" },
        ],
        "limits": [
            { "scope": "key", "name": "ci", "requests_per_minute": 1, "tokens_per_minute": null, "concurrent": null },
            { "scope": "gateway", "name": null, "requests_per_minute": 0, "tokens_per_minute": null, "concurrent": null },
            { "scope": "team", "name": "Platform", "requests_per_minute": null, "tokens_per_minute": null, "concurrent": null },
            { "scope": "gateway", "name": "x", "requests_per_minute": 1, "tokens_per_minute": null, "concurrent": null },
        ],
        "budgets": [
            { "scope": "gateway", "name": null, "amount_micros": 0, "period": "hourly", "action": "stop" },
        ],
    });
    let report = portable::import(store, &file_of(file), &actor(), true)
        .await
        .unwrap();
    let at: Vec<&str> = report.errors.iter().map(|e| e.at.as_str()).collect();
    for expected in [
        "providers[0].name",
        "providers[1].kind",
        "providers[2].base_url",
        "providers[3].api_version",
        "providers[4].kind",
        "providers[5]",
        "models[0].name",
        "models[0].input_price_micros",
        "models[0].grants",
        "teams[0].name",
        "teams[2]",
        "routes[0].name",
        "routes[0].primaries",
        "routes[0].retries",
        "routes[0].first_token_timeout_ms",
        "routes[0].breaker_failures",
        "routes[0].breaker_window_s",
        "routes[0].everyone",
        "routes[0].cache_ttl_s",
        "routes[0].cache_scope",
        "limits[0].scope",
        "limits[1].requests_per_minute",
        "limits[2]",
        "limits[3].name",
        "budgets[0].amount_micros",
        "budgets[0].period",
        "budgets[0].action",
    ] {
        assert!(at.contains(&expected), "{expected} is not in {at:?}");
    }
}

#[tokio::test]
async fn the_file_must_be_the_one_this_gateway_reads() {
    let w = world().await;
    let maya = w.admin().await;
    let post = |body: Value| {
        call(
            &w.org.api.app,
            "POST",
            "/api/config/import",
            Some(&maya),
            Some(body),
        )
    };
    // Another format, another version.
    let (status, _, body) = post(json!({ "format": "something", "version": 1 })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["errors"][0]["at"], "format");
    let (status, _, body) = post(json!({ "format": "ultrafast-config", "version": 2 })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["errors"][0]["at"], "version");
    // A field that is not known, such as a credential, is refused, not dropped.
    let mut with_secret = expected_export();
    with_secret["providers"][0]["api_key"] = json!("sk-should-never-be-imported");
    let (status, _, body) = post(with_secret).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["errors"][0]["at"], "file");
    assert!(!body.to_string().contains("sk-should-never-be-imported"));
    // Not JSON at all.
    let (status, _, body) = common::send(
        &w.org.api.app,
        "POST",
        "/api/config/import",
        &[("cookie", &maya.cookie), ("x-csrf-token", &maya.csrf)],
        Some(b"{ not json".to_vec()),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["errors"][0]["at"], "file");
}

#[tokio::test]
async fn an_applied_import_is_audited_and_reaches_v1_at_once() {
    let sink = std::sync::Arc::new(common::MemorySink::default());
    let org = org_with_sink(Some(sink)).await;
    let maya = org.sign_in("maya").await;
    let file = json!({
        "format": "ultrafast-config", "version": 1,
        "providers": [{ "name": "fresh", "kind": "openai", "base_url": "https://f.example", "api_version": null }],
        "models": [{ "provider": "fresh", "name": "m", "enabled": true, "input_price_micros": null, "output_price_micros": null,
                     "grants": { "everyone": true, "teams": [], "users": [] } }],
    });
    assert!(org.api.state.snapshot.load().model("fresh", "m").is_none());
    let (status, _, body) = call(
        &org.api.app,
        "POST",
        "/api/config/import?dry_run=false",
        Some(&maya),
        Some(file),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["created"].as_array().unwrap().len(), 2);
    // The snapshot was refreshed: the model can be called by everyone.
    assert!(org
        .api
        .state
        .snapshot
        .load()
        .model("fresh", "m")
        .is_some_and(|m| m.enabled && m.everyone));
    let actions = org.audit_actions().await;
    assert!(
        actions.contains(&"config.import".to_string()),
        "{actions:?}"
    );
    assert!(
        actions.contains(&"provider.import".to_string()),
        "{actions:?}"
    );
    assert!(actions.contains(&"model.import".to_string()), "{actions:?}");
    let summary = org.last_summary("config.import").await;
    assert_eq!(
        summary,
        "Imported configuration: 2 created, 0 updated, 0 unchanged"
    );
    // No audit row says anything of a credential or holds the file.
    let rows = org.api.store.list_audit(50, None).await.unwrap();
    assert!(rows
        .iter()
        .all(|r| !r.summary.contains("https://f.example")));
}

#[tokio::test]
async fn only_an_admin_exports_or_imports_and_a_session_needs_the_csrf_token() {
    let w = world().await;
    for who in ["arjun", "lena"] {
        let me = w.org.sign_in(who).await;
        let (status, _, body) =
            call(&w.org.api.app, "GET", "/api/config/export", Some(&me), None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{who}");
        assert_eq!(error_code(&body), "forbidden");
        let (status, _, _) = call(
            &w.org.api.app,
            "POST",
            "/api/config/import?dry_run=false",
            Some(&me),
            Some(expected_export()),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{who}");
    }
    let (status, _, _) = call(&w.org.api.app, "GET", "/api/config/export", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // No CSRF token: refused, and nothing is written.
    let maya = w.admin().await;
    let (status, _, body) = common::send(
        &w.org.api.app,
        "POST",
        "/api/config/import?dry_run=false",
        &[("cookie", &maya.cookie)],
        Some(serde_json::to_vec(&json!({ "format": "ultrafast-config", "version": 1, "teams": [{ "name": "Sneaky" }] })).unwrap()),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error_code(&body), "csrf_failed");
    assert!(w
        .org
        .api
        .store
        .config_state()
        .await
        .unwrap()
        .teams
        .iter()
        .all(|(_, n)| n != "Sneaky"));
    // `dry_run` is true or false.
    let (status, _, _) = call(
        &w.org.api.app,
        "POST",
        "/api/config/import?dry_run=maybe",
        Some(&maya),
        Some(expected_export()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn an_export_is_audited() {
    let w = world().await;
    w.export().await;
    assert!(w
        .org
        .audit_actions()
        .await
        .contains(&"config.export".to_string()));
}

#[tokio::test]
async fn users_are_matched_by_email_and_never_created() {
    let w = world().await;
    let store = &w.org.api.store;
    seed_user(store, "late@example.com", Role::Member, ORG_PASSWORD).await;
    let mut file = expected_export();
    file["budgets"] = json!([{ "scope": "user", "name": "late@example.com", "amount_micros": 1, "period": "daily", "action": "alert" }]);
    let report = portable::import(store, &file_of(file), &actor(), false)
        .await
        .unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(store.config_state().await.unwrap().users.len(), 6);
}
