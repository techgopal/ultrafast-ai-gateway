//! A database written by v2.0.0-beta.2 (migrations 0001 to 0013) is opened by
//! this build: the later migrations are applied, nothing it holds changes, and
//! every table is read back through the public store API.
//!
//! The database is built here from copies of the beta.2 migration files in
//! `fixtures/migrations-beta2/` (a test pins them to the shipped ones), with
//! one or more rows in every table those migrations made.

use std::path::Path;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::Row;
use ultrafast_gateway::budgets::{BudgetAction, Period};
use ultrafast_gateway::identity::{Role, UserStatus};
use ultrafast_gateway::limits::LimitScope;
use ultrafast_gateway::secrets::hash_key;
use ultrafast_gateway::store::{LogFilter, LogScope, Store, UsageGroup};

const SESSION: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const TOKEN: &str = "uf-at-00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

const ROWS: &[&str] = &[
    "INSERT INTO providers (id, name, kind, base_url, credential, api_version, created_at) VALUES
        (1, 'openai', 'openai', 'https://api.openai.com/v1', x'656e63', NULL, '2026-09-01 10:00:00'),
        (2, 'azure', 'azure', 'https://x.openai.azure.com', NULL, '2024-02-01', '2026-09-01 10:00:01')",
    "INSERT INTO users (id, email, name, role, status, password_hash, auth_provider, external_id, created_at, last_active_at) VALUES
        (1, 'maya@example.com', 'Maya', 'admin', 'active', '$argon2id$x', 'password', NULL, '2026-09-01 09:00:00', '2026-09-30 12:00:00'),
        (2, 'arjun@example.com', 'Arjun', 'member', 'active', NULL, 'oidc', 'sub-2', '2026-09-01 09:00:01', NULL),
        (3, 'lena@example.com', 'Lena', 'member', 'invited', NULL, 'password', NULL, '2026-09-01 09:00:02', NULL)",
    "INSERT INTO invites (id, user_id, token_hash, expires_at, used_at, created_at) VALUES
        (1, 3, 'invhash', '2999-01-01 00:00:00', NULL, '2026-09-01 09:00:03')",
    "INSERT INTO teams (id, name, created_at) VALUES (10, 'Platform', '2026-09-01 09:30:00')",
    "INSERT INTO team_members (team_id, user_id, role) VALUES (10, 2, 'lead'), (10, 1, 'member')",
    // Key 3 was made by Maya (admin) for Arjun, key 4 by Arjun (a lead) for Lena: a team key.
    "INSERT INTO virtual_keys (id, name, key_hash, display, expires_at, revoked_at, created_at, user_id, team_id, allowed, tags, created_by, team_only) VALUES
        (1, 'ci', 'keyhash1', 'uf-sk-…aaaa', NULL, NULL, '2026-09-02 08:00:00', 1, 10, '[\"gpt-4o\",\"chat\"]', '{\"env\":\"ci\"}', 1, 0),
        (2, 'old', 'keyhash2', 'uf-sk-…bbbb', '2999-01-01 00:00:00', '2026-09-20 00:00:00', '2026-09-02 08:00:01', NULL, NULL, NULL, NULL, NULL, 0),
        (3, 'for-arjun', 'keyhash3', 'uf-sk-…cccc', NULL, NULL, '2026-09-02 08:00:02', 2, 10, NULL, NULL, 1, 0),
        (4, 'for-lena', 'keyhash4', 'uf-sk-…dddd', NULL, NULL, '2026-09-02 08:00:03', 3, 10, NULL, NULL, 2, 1)",
    "INSERT INTO sessions (id, user_id, id_hash, csrf_token, expires_at, created_at) VALUES
        (1, 1, '__SESSION_HASH__', 'csrf-1', '2999-01-01 00:00:00', '2026-09-30 12:00:00'),
        (2, 2, 'expiredhash', 'csrf-2', '2000-01-01 00:00:00', '2026-01-01 00:00:00')",
    "INSERT INTO access_tokens (id, user_id, name, token_hash, display, expires_at, revoked_at, last_used_at, created_at) VALUES
        (1, 1, 'laptop', '__TOKEN_HASH__', 'uf-at-…eeff', NULL, NULL, '2026-09-29 11:00:00', '2026-09-03 08:00:00'),
        (2, 1, 'gone', 'tokenhash2', 'uf-at-…0000', NULL, '2026-09-10 00:00:00', NULL, '2026-09-03 08:00:01')",
    "INSERT INTO audit_log (id, at, actor_user_id, actor_email, action, target_type, target_id, summary) VALUES
        (1, '2026-09-02 08:00:00', 1, 'maya@example.com', 'key.create', 'key', 1, 'created key ci'),
        (2, '2026-09-02 08:00:02', 1, 'maya@example.com', 'key.create', 'key', 3, 'created key for-arjun'),
        (3, '2026-09-02 08:00:03', 2, 'arjun@example.com', 'key.create', 'key', 4, 'created key for-lena'),
        (4, '2026-09-03 09:00:00', NULL, 'cli', 'user.create', 'user', NULL, 'from the command line')",
    "INSERT INTO models (id, provider_id, name, enabled, created_at, input_price_micros, output_price_micros) VALUES
        (1, 1, 'gpt-4o', 1, '2026-09-01 10:01:00', 2500000, 10000000),
        (2, 1, 'gpt-4o-mini', 0, '2026-09-01 10:01:01', NULL, NULL),
        (3, 2, 'dep-1', 1, '2026-09-01 10:01:02', 1, 2)",
    "INSERT INTO model_grants (id, model_id, team_id, user_id) VALUES
        (1, 1, NULL, NULL), (2, 3, 10, NULL), (3, 3, NULL, 1)",
    "INSERT INTO routes (id, name, retries, first_token_timeout_ms, total_timeout_ms, breaker_failures, breaker_window_s, breaker_open_s, created_at, cache_enabled, cache_ttl_s, cache_scope) VALUES
        (1, 'chat', 3, 20000, 200000, 4, 50, 25, '2026-09-04 08:00:00', 1, 120, 'key'),
        (2, 'internal', 2, 30000, 300000, 5, 60, 30, '2026-09-04 08:00:01', 0, 300, 'team')",
    "INSERT INTO route_targets (id, route_id, model_id, tier, weight, position) VALUES
        (1, 1, 1, 'primary', 3, 0), (2, 1, 3, 'primary', 1, 1), (3, 1, 2, 'fallback', 1, 2), (4, 2, 3, 'primary', 1, 0)",
    "INSERT INTO route_grants (route_id, team_id) VALUES (2, 10)",
    // 0005 ran on an empty routes table, so `everyone` is the default 1; restrict route 2 as the console does.
    "UPDATE routes SET everyone = 0 WHERE id = 2",
    "INSERT INTO request_logs (id, at, key_id, user_id, team_id, requested, endpoint, stream, status, provider, model, input_tokens, output_tokens, cost_micros, priced, cached, duration_ms, attempts, estimated, tags) VALUES
        (1, '2026-09-30 10:00:00', 1, 1, 10, 'chat', 'chat/completions', 0, 200, 'openai', 'gpt-4o', 100, 50, 750, 1, 0, 800, '[]', 0, '{\"env\":\"ci\"}'),
        (2, '2026-09-30 10:05:00', 3, 2, 10, 'chat', 'chat/completions', 1, 502, NULL, NULL, NULL, NULL, 0, 0, 0, 120, '[]', 1, NULL),
        (3, '2026-10-01 10:05:00', NULL, NULL, NULL, 'gpt-4o', 'embeddings', 0, 200, 'openai', 'gpt-4o', 10, NULL, 25, 1, 1, 30, '[]', 0, NULL)",
    "INSERT INTO rate_limits (id, scope, scope_id, requests_per_minute, tokens_per_minute, concurrent, created_at) VALUES
        (1, 'gateway', NULL, 1000, NULL, 50, '2026-09-05 08:00:00'),
        (2, 'team', 10, NULL, 90000, NULL, '2026-09-05 08:00:01'),
        (3, 'key', 1, 60, 10000, 2, '2026-09-05 08:00:02')",
    "INSERT INTO budgets (id, scope, scope_id, amount_micros, period, action, created_at) VALUES
        (1, 'gateway', NULL, 500000000, 'monthly', 'alert', '2026-09-05 09:00:00'),
        (2, 'user', 2, 20000000, 'daily', 'block', '2026-09-05 09:00:01')",
    "INSERT INTO budget_usage (budget_id, period_start, spent_micros, alerted) VALUES
        (1, '2026-09-01', 123456, 1), (2, '2026-09-30', 7, 0)",
    "UPDATE settings SET value = '45' WHERE key = 'log_retention_days'",
];

/// Applies the beta.2 migrations to a new file and fills it.
async fn beta2_database(path: &Path) {
    let copies = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/migrations-beta2");
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(path)
                .create_if_missing(true)
                .foreign_keys(true),
        )
        .await
        .unwrap();
    sqlx::migrate::Migrator::new(copies)
        .await
        .unwrap()
        .run(&pool)
        .await
        .unwrap();
    for sql in ROWS {
        let sql = sql
            .replace("__SESSION_HASH__", &hash_key(SESSION))
            .replace("__TOKEN_HASH__", &hash_key(TOKEN));
        sqlx::query(sqlx::AssertSqlSafe(sql.clone()))
            .execute(&pool)
            .await
            .unwrap_or_else(|e| panic!("{e}: {sql}"));
    }
    pool.close().await;
}

async fn raw_migration_rows(path: &Path) -> Vec<(i64, String, Vec<u8>, i64)> {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(SqliteConnectOptions::new().filename(path))
        .await
        .unwrap();
    let rows = sqlx::query(
        "SELECT version, installed_on, checksum, execution_time FROM _sqlx_migrations ORDER BY version",
    )
    .fetch_all(&pool)
    .await
    .unwrap()
    .iter()
    .map(|r| (r.get(0), r.get::<String, _>(1), r.get(2), r.get(3)))
    .collect();
    pool.close().await;
    rows
}

#[tokio::test]
async fn beta2_database_opens_migrates_and_reads_back() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gateway.db");
    beta2_database(&path).await;
    let before = raw_migration_rows(&path).await;
    assert_eq!(before.len(), 13);

    let s = Store::open(&path).await.unwrap();

    // 0014 to 0020 were applied after the thirteen, which were not rewritten.
    let after = raw_migration_rows(&path).await;
    assert_eq!(after.len(), 20);
    assert_eq!(&after[..13], &before[..]);

    // providers
    let providers = s.list_providers().await.unwrap();
    assert_eq!(providers.len(), 2);
    let openai = s.provider_by_name("openai").await.unwrap().unwrap();
    assert_eq!(openai.base_url, "https://api.openai.com/v1");
    assert_eq!(openai.credential.as_deref(), Some(&b"enc"[..]));
    assert_eq!(openai.api_version, None);
    let azure = s.provider_by_id(2).await.unwrap().unwrap();
    assert_eq!(azure.credential, None);
    assert_eq!(azure.api_version.as_deref(), Some("2024-02-01"));

    // users, invites
    assert_eq!(s.count_users().await.unwrap(), 3);
    assert_eq!(s.count_active_admins().await.unwrap(), 1);
    let maya = s.user_by_email("maya@example.com").await.unwrap().unwrap();
    assert_eq!((maya.role, maya.status), (Role::Admin, UserStatus::Active));
    assert_eq!(maya.password_hash.as_deref(), Some("$argon2id$x"));
    assert_eq!(maya.last_active_at.as_deref(), Some("2026-09-30 12:00:00"));
    let arjun = s.user_by_external("oidc", "sub-2").await.unwrap().unwrap();
    assert_eq!(arjun.email, "arjun@example.com");
    assert_eq!(
        s.user_by_id(3).await.unwrap().unwrap().status,
        UserStatus::Invited
    );
    let invite = s.invite_by_hash("invhash").await.unwrap().unwrap();
    assert_eq!((invite.id, invite.user_id), (1, 3));

    // teams
    let team = s.team_by_id(10).await.unwrap().unwrap();
    assert_eq!(team.name, "Platform");
    assert_eq!(s.members_of(10).await.unwrap().len(), 2);
    assert_eq!(s.team_summary(10).await.unwrap().unwrap().member_count, 2);

    // keys, with what 0012 and 0013 worked out from the audit log
    let keys = s.list_keys().await.unwrap();
    assert_eq!(keys.len(), 4);
    let ci = s.active_key_by_hash("keyhash1").await.unwrap().unwrap();
    assert_eq!(
        ci.allowed.as_deref(),
        Some(&["gpt-4o".to_string(), "chat".to_string()][..])
    );
    assert_eq!(ci.tags.get("env").map(String::as_str), Some("ci"));
    assert_eq!(ci.owner_email.as_deref(), Some("maya@example.com"));
    assert!(
        s.active_key_by_hash("keyhash2").await.unwrap().is_none(),
        "revoked"
    );
    assert_eq!(
        s.key_by_id(2).await.unwrap().unwrap().revoked_at.as_deref(),
        Some("2026-09-20 00:00:00")
    );
    assert!(
        !s.key_by_id(3).await.unwrap().unwrap().team_only,
        "an admin made it"
    );
    assert!(
        s.key_by_id(4).await.unwrap().unwrap().team_only,
        "a lead made it for Lena"
    );
    // Not the revoked one, nor the one whose owner has not accepted the invite.
    assert_eq!(s.live_keys().await.unwrap().len(), 2);

    // sessions and tokens
    let session = s.live_session(SESSION).await.unwrap().unwrap();
    assert_eq!(
        (session.user_id, session.csrf_token.as_str()),
        (1, "csrf-1")
    );
    assert_eq!(s.delete_expired_sessions().await.unwrap(), 1);
    let token = s.live_token(TOKEN).await.unwrap().unwrap();
    assert_eq!(
        (token.name.as_str(), token.last_used_at.as_deref()),
        ("laptop", Some("2026-09-29 11:00:00"))
    );
    assert_eq!(s.list_tokens_of(1).await.unwrap().len(), 2);
    assert!(s
        .token_by_id(2)
        .await
        .unwrap()
        .unwrap()
        .revoked_at
        .is_some());

    // audit
    let audit = s.list_audit(10, None).await.unwrap();
    assert_eq!(audit.len(), 4);
    assert_eq!(audit[0].action, "user.create");
    assert_eq!(audit[0].target_id, None);

    // catalog
    let models = s.list_models().await.unwrap();
    assert_eq!(models.len(), 3);
    let gpt = s.model_by_id(1).await.unwrap().unwrap();
    assert!(gpt.enabled);
    assert_eq!(
        (gpt.input_price_micros, gpt.output_price_micros),
        (Some(2_500_000), Some(10_000_000))
    );
    assert_eq!(
        s.model_by_id(2).await.unwrap().unwrap().input_price_micros,
        None
    );
    let grants = s.grants_of(1).await.unwrap();
    assert!(grants.everyone);
    let grants = s.grants_of(3).await.unwrap();
    assert_eq!((grants.team_ids, grants.user_ids), (vec![10], vec![1]));

    // routes
    let routes = s.list_routes().await.unwrap();
    assert_eq!(routes.len(), 2);
    let chat = s.route_by_id(1).await.unwrap().unwrap();
    assert_eq!(
        (chat.settings.retries, chat.settings.breaker_open_s),
        (3, 25)
    );
    assert!(chat.everyone);
    assert!(chat.cache.enabled);
    assert_eq!(chat.cache.ttl_s, 120);
    let internal = s.route_by_id(2).await.unwrap().unwrap();
    assert!(!internal.everyone);
    assert_eq!(s.route_team_ids(2).await.unwrap(), vec![10]);
    let targets = s.route_targets_of(1).await.unwrap();
    assert_eq!(targets.len(), 3);
    assert_eq!(
        (
            targets[0].model_name.as_str(),
            targets[0].weight,
            targets[0].primary
        ),
        ("gpt-4o", 3, true)
    );
    assert!(!targets[2].primary);
    assert_eq!(s.list_route_grants().await.unwrap(), vec![(2, 10)]);

    // request logs
    let logs = s
        .list_logs(&LogScope::All, &LogFilter::default(), 10)
        .await
        .unwrap();
    assert_eq!(
        logs.iter().map(|l| l.row.id).collect::<Vec<_>>(),
        vec![3, 2, 1]
    );
    assert!(logs[1].row.estimated && logs[1].row.stream);
    assert_eq!(logs[2].row.tags.as_deref(), Some("{\"env\":\"ci\"}"));
    assert_eq!(logs[2].key_name.as_deref(), Some("ci"));
    let tagged = LogFilter {
        tags: vec![("env".into(), "ci".into())],
        ..LogFilter::default()
    };
    assert_eq!(
        s.list_logs(&LogScope::All, &tagged, 10)
            .await
            .unwrap()
            .len(),
        1
    );
    let by_day = s
        .usage(&LogScope::All, "2026-09-30", "2026-10-01", UsageGroup::Day)
        .await
        .unwrap();
    assert_eq!(
        by_day
            .iter()
            .map(|d| (d.group.as_str(), d.requests, d.cost_micros))
            .collect::<Vec<_>>(),
        vec![("2026-09-30", 2, 750), ("2026-10-01", 1, 25)]
    );
    assert_eq!(
        s.spend_since(LimitScope::Gateway, None, "2026-09-30 00:00:00")
            .await
            .unwrap(),
        775
    );

    // limits, budgets
    let limits = s.list_limits().await.unwrap();
    assert_eq!(limits.len(), 3);
    assert_eq!(limits[0].scope, LimitScope::Gateway);
    assert_eq!(limits[0].limit.requests_per_minute, Some(1000));
    assert_eq!(limits[1].name.as_deref(), Some("Platform"));
    assert_eq!(limits[2].limit.concurrent, Some(2));
    let budgets = s.list_budgets().await.unwrap();
    assert_eq!(budgets.len(), 2);
    assert_eq!(
        (
            budgets[0].period,
            budgets[0].action,
            budgets[0].amount_micros
        ),
        (Period::Monthly, BudgetAction::Alert, 500_000_000)
    );
    assert_eq!(budgets[1].name.as_deref(), Some("arjun@example.com"));
    assert_eq!(
        s.budget_usage(1, "2026-09-01").await.unwrap(),
        Some((123_456, true))
    );
    assert_eq!(
        s.budget_usage(2, "2026-09-30").await.unwrap(),
        Some((7, false))
    );

    // settings and what 0014 and 0015 added
    assert_eq!(s.log_retention_days().await.unwrap(), 45);
    assert!(s.list_alert_channels().await.unwrap().is_empty());
    assert!(s.list_alert_rules().await.unwrap().is_empty());
    assert!(!s.oidc_settings().await.unwrap().enabled);
    // and what 0017 added
    assert!(s.list_guardrails().await.unwrap().is_empty());
    // and what 0018 added
    assert!(s.list_prompt_templates().await.unwrap().is_empty());
    // and what 0019 added
    assert!(s.team_guardrail_refs().await.unwrap().is_empty());
    assert!(s.user_guardrail_refs().await.unwrap().is_empty());

    // and what 0020 added: the invites that existed are invites
    assert!(s.invite_by_hash("none").await.unwrap().is_none());

    // the snapshot reads everything at once
    let rows = s.snapshot_rows().await.unwrap();
    assert_eq!(
        (rows.keys.len(), rows.routes.len(), rows.users.len()),
        (2, 2, 3)
    );
}

#[tokio::test]
async fn the_upgraded_database_still_writes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gateway.db");
    beta2_database(&path).await;
    let s = Store::open(&path).await.unwrap();
    let mut tx = s.begin().await.unwrap();
    let id = tx.insert_team("Data").await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(id, 11);
    s.insert_key("new", "freshhash", "uf-sk-…zzzz", None)
        .await
        .unwrap();
    assert_eq!(s.list_keys().await.unwrap().len(), 5);
}
