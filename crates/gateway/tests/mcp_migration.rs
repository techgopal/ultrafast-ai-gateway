//! Migration 0022: MCP servers that run programs, and the alert kind `mcp_down`.
//! A database that holds alert rules, their channels, states and events is
//! opened by this build; nothing it holds is lost, and the new kind works.

use std::path::Path;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use ufaigw_gateway::store::Store;

/// The shipped SQLite migrations before 0022, in a directory of their own.
fn before_0022(into: &Path) {
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations/sqlite");
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().into_string().unwrap();
        if name.as_str() < "0022" {
            std::fs::copy(entry.path(), into.join(&name)).unwrap();
        }
    }
}

const ROWS: &[&str] = &[
    "INSERT INTO alert_channels (id, name, kind, url_enc, url_host, secret_enc, enabled, created_at)
        VALUES (1, 'ops', 'webhook', x'01', 'hooks.example', x'02', 1, '2026-10-01 09:00:00'),
               (2, 'chat', 'slack', x'03', 'slack.example', x'04', 1, '2026-10-01 09:00:01')",
    "INSERT INTO alert_rules (id, name, kind, params, enabled, created_at) VALUES
        (1, 'breaker', 'circuit_open', '{}', 1, '2026-10-01 09:01:00'),
        (2, 'budget', 'budget', '{\"budget_id\":null,\"percent\":80}', 0, '2026-10-01 09:01:01')",
    "INSERT INTO alert_rule_channels (rule_id, channel_id) VALUES (1, 1), (1, 2), (2, 1)",
    "INSERT INTO alert_state (rule_id, subject, firing, since, owner, seen_at) VALUES
        (1, 'target:openai/gpt-4o', 1, '2026-10-02 10:00:00', 'proc-a', '2026-10-02 10:00:30')",
    "INSERT INTO alert_events (id, rule_id, rule_name, kind, subject, state, summary, details, at) VALUES
        (1, 1, 'breaker', 'circuit_open', 'target:openai/gpt-4o', 'firing', 's', '{}', '2026-10-02 10:00:00'),
        (2, NULL, 'gone', 'budget', 'budget:1:p', 'resolved', 's', '{}', '2026-10-02 10:01:00')",
    "INSERT INTO mcp_servers (id, name, url, auth_kind, enabled, description, created_at, updated_at)
        VALUES (1, 'web', 'https://mcp.example.com/mcp', 'none', 1, '', '2026-10-01 09:02:00', '2026-10-01 09:02:00')",
];

#[tokio::test]
async fn alert_rules_survive_the_rebuild_and_the_new_kind_works() {
    let dir = tempfile::tempdir().unwrap();
    let copies = dir.path().join("before");
    std::fs::create_dir(&copies).unwrap();
    before_0022(&copies);
    let path = dir.path().join("gateway.db");
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&path)
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
        sqlx::query(sqlx::AssertSqlSafe((*sql).to_string()))
            .execute(&pool)
            .await
            .unwrap_or_else(|e| panic!("{e}: {sql}"));
    }
    pool.close().await;

    let store = Store::open(&path).await.unwrap();

    // The servers read as HTTP servers with no program.
    let web = store.server_by_id(1).await.unwrap().unwrap();
    assert_eq!(web.transport, "http");
    assert!(web.command.is_none() && web.args.is_empty() && web.env_enc.is_empty());
    assert!(web.catalog_id.is_none());

    // The rules, and what hangs off them, are as they were.
    let rules = store.list_alert_rules().await.unwrap();
    let names: Vec<_> = rules
        .iter()
        .map(|r| (r.id, r.name.as_str(), r.enabled))
        .collect();
    assert_eq!(names, [(1, "breaker", true), (2, "budget", false)]);
    assert_eq!(store.enabled_channel_ids_of_rule(1).await.unwrap().len(), 2);
    let states = store.alert_states().await.unwrap();
    assert_eq!(states.len(), 1);
    assert_eq!(states[0].subject, "target:openai/gpt-4o");
    assert_eq!(states[0].owner.as_deref(), Some("proc-a"));
    let events = store.alert_events(10).await.unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events.iter().find(|e| e.id == 1).unwrap().rule_id, Some(1));
    assert_eq!(events.iter().find(|e| e.id == 2).unwrap().rule_id, None);

    // The new kind is accepted; an unknown one is not.
    let mut tx = store.begin().await.unwrap();
    let id = tx
        .insert_alert_rule("mcp", "mcp_down", "{}", true)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(id > 2);
    let mut tx = store.begin().await.unwrap();
    assert!(tx
        .insert_alert_rule("bad", "nonsense", "{}", true)
        .await
        .is_err());
    drop(tx);

    // The foreign keys still do their work: deleting a rule takes its states
    // and channel links, and keeps its events.
    let mut tx = store.begin().await.unwrap();
    assert!(tx.delete_alert_rule(1).await.unwrap());
    tx.commit().await.unwrap();
    assert!(store.alert_states().await.unwrap().is_empty());
    let kept = store.alert_events(10).await.unwrap();
    assert_eq!(kept.len(), 2);
    assert!(kept.iter().all(|e| e.rule_id.is_none()));
}
