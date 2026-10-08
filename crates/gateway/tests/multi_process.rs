//! Several gateway processes on one database: two `AppState`s over one
//! PostgreSQL schema, each with its own in-memory counters, snapshot and
//! alert state. Skipped without `UF_TEST_DATABASE_URL` (SQLite is one
//! process per database).

mod common;

use std::sync::Arc;

use axum::http::StatusCode;
use common::{call, org, Org};
use serde_json::json;
use time::OffsetDateTime;
use ultrafast_gateway::app::{router, AppState};
use ultrafast_gateway::budgets::{self, BudgetAction, Period};
use ultrafast_gateway::limits::LimitScope;
use ultrafast_gateway::secrets::Cipher;
use ultrafast_gateway::store::Dialect;

/// Process A is the organization's own gateway; B is another one on the
/// same database. `None` where the tests do not run.
async fn two() -> Option<(Org, Arc<AppState>)> {
    let org = org().await;
    if org.api.store.dialect() != Dialect::Postgres {
        eprintln!("SKIPPED on SQLite: one gateway per database");
        return None;
    }
    let cipher = Cipher::from_hex(&Cipher::generate_master_hex()).unwrap();
    let mut b = AppState::new(org.api.store.clone(), cipher).await.unwrap();
    b.cookie_secure = false;
    Some((org, Arc::new(b)))
}

async fn gateway_budget(org: &Org, b: &Arc<AppState>, amount: u64, action: BudgetAction) -> i64 {
    let mut tx = org.api.store.begin().await.unwrap();
    let id = tx
        .upsert_budget(LimitScope::Gateway, None, amount, Period::Monthly, action)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    org.api.state.refresh().await.unwrap();
    b.refresh().await.unwrap();
    id
}

fn spend(state: &AppState, micros: u64) {
    let budgets = state.snapshot.load().all_budgets();
    state
        .budgets
        .spend(&budgets, micros, OffsetDateTime::now_utc());
}

fn spent(state: &AppState) -> u64 {
    let budgets = state.snapshot.load().all_budgets();
    state.budgets.spent(&budgets[0], OffsetDateTime::now_utc())
}

fn refused(state: &AppState) -> bool {
    let budgets = state.snapshot.load().all_budgets();
    state
        .budgets
        .check(&budgets, OffsetDateTime::now_utc())
        .is_err()
}

async fn stored(org: &Org, id: i64) -> Option<(u64, bool)> {
    let start = Period::Monthly.start_string(OffsetDateTime::now_utc());
    org.api.store.budget_usage(id, &start).await.unwrap()
}

#[tokio::test]
async fn spend_is_the_sum_of_both_processes_after_they_flush() {
    let Some((org, b)) = two().await else { return };
    let a = org.api.state.clone();
    let id = gateway_budget(&org, &b, 100_000_000, BudgetAction::Block).await;
    spend(&a, 3_000_000);
    spend(&b, 4_000_000);
    budgets::flush(&a).await;
    budgets::flush(&b).await;
    assert_eq!(stored(&org, id).await, Some((7_000_000, false)));
    // B read the total back when it flushed; A learns B's share on its next flush.
    assert_eq!(spent(&b), 7_000_000);
    budgets::flush(&a).await;
    assert_eq!(spent(&a), 7_000_000);
    // More on both sides, flushed in the other order, converges the same way.
    spend(&a, 1_000_000);
    spend(&b, 2_000_000);
    budgets::flush(&b).await;
    budgets::flush(&a).await;
    budgets::flush(&b).await;
    assert_eq!(stored(&org, id).await, Some((10_000_000, false)));
    assert_eq!((spent(&a), spent(&b)), (10_000_000, 10_000_000));
    // Nothing is counted twice when nobody spends.
    for _ in 0..3 {
        budgets::flush(&a).await;
        budgets::flush(&b).await;
    }
    assert_eq!(stored(&org, id).await, Some((10_000_000, false)));
    assert_eq!((spent(&a), spent(&b)), (10_000_000, 10_000_000));
}

#[tokio::test]
async fn a_process_that_never_spent_refuses_once_the_others_spent_the_budget() {
    let Some((org, b)) = two().await else { return };
    let a = org.api.state.clone();
    gateway_budget(&org, &b, 10_000_000, BudgetAction::Block).await;
    spend(&a, 10_000_000);
    assert!(refused(&a));
    assert!(!refused(&b));
    budgets::flush(&a).await;
    budgets::flush(&b).await;
    assert!(refused(&b), "B sees what A spent after its flush");
}

#[tokio::test]
async fn a_spend_between_flushes_is_kept_on_top_of_the_total() {
    let Some((org, b)) = two().await else { return };
    let a = org.api.state.clone();
    let id = gateway_budget(&org, &b, 100_000_000, BudgetAction::Block).await;
    spend(&a, 1_000_000);
    budgets::flush(&a).await;
    spend(&b, 2_000_000);
    budgets::flush(&b).await;
    spend(&a, 500_000);
    // A's counter: the database total it read (1.0M) plus its own 0.5M.
    assert_eq!(spent(&a), 1_500_000);
    budgets::flush(&a).await;
    assert_eq!(stored(&org, id).await, Some((3_500_000, false)));
    assert_eq!(spent(&a), 3_500_000);
}

#[tokio::test]
async fn an_alert_budget_alerts_once_for_both_processes() {
    let Some((org, b)) = two().await else { return };
    let a = org.api.state.clone();
    let id = gateway_budget(&org, &b, 10_000_000, BudgetAction::Alert).await;
    // Neither reaches the amount alone.
    spend(&a, 6_000_000);
    spend(&b, 6_000_000);
    for _ in 0..2 {
        budgets::flush(&a).await;
        budgets::flush(&b).await;
    }
    assert_eq!(stored(&org, id).await, Some((12_000_000, true)));
    let alerts = org
        .audit_actions()
        .await
        .into_iter()
        .filter(|a| a == "budget.alert")
        .count();
    assert_eq!(alerts, 1, "one audit row for the period");
    // Both reached it at once and both raised it: still one.
    let id2 = {
        let mut tx = org.api.store.begin().await.unwrap();
        let id = tx
            .upsert_budget(
                LimitScope::Gateway,
                None,
                1_000_000,
                Period::Daily,
                BudgetAction::Alert,
            )
            .await
            .unwrap();
        tx.commit().await.unwrap();
        id
    };
    org.api.state.refresh().await.unwrap();
    b.refresh().await.unwrap();
    spend(&a, 2_000_000);
    spend(&b, 2_000_000);
    budgets::flush(&a).await;
    budgets::flush(&b).await;
    let daily = Period::Daily.start_string(OffsetDateTime::now_utc());
    assert_eq!(
        org.api.store.budget_usage(id2, &daily).await.unwrap(),
        Some((4_000_000, true))
    );
    let summaries = org.audit_actions().await;
    assert_eq!(
        summaries.iter().filter(|a| *a == "budget.alert").count(),
        2,
        "one per budget and period"
    );
}

#[tokio::test]
async fn a_route_change_through_one_process_reaches_the_other_on_refresh() {
    let Some((org, b)) = two().await else { return };
    let maya = org.sign_in("maya").await;
    let p = org
        .api
        .store
        .insert_provider("openai", "openai", "https://x.example.com/v1", None)
        .await
        .unwrap();
    let mut tx = org.api.store.begin().await.unwrap();
    let model = tx.insert_model(p, "a").await.unwrap();
    tx.set_model_enabled(model, true).await.unwrap();
    tx.commit().await.unwrap();
    org.api.state.refresh().await.unwrap();
    let body = json!({
        "name": "fast",
        "primaries": [{ "model_id": model, "weight": 1 }],
        "fallbacks": [],
        "retries": 2,
        "first_token_timeout_ms": 30000,
        "total_timeout_ms": 300000,
        "breaker_failures": 5,
        "breaker_window_s": 60,
        "breaker_open_s": 30,
        "everyone": true,
        "team_ids": [],
    });
    let (status, made) = org
        .call(Some(&maya), "POST", "/api/routes", Some(body))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{made}");
    assert!(
        b.snapshot.load().route("fast").is_none(),
        "B has not looked yet"
    );
    b.refresh().await.unwrap();
    assert!(b.snapshot.load().route("fast").is_some());
}

#[tokio::test]
async fn a_session_made_on_one_process_signs_in_on_the_other() {
    let Some((org, b)) = two().await else { return };
    let maya = org.sign_in("maya").await;
    let other = router(b.clone());
    let (status, _, body) = call(&other, "GET", "/api/auth/me", Some(&maya), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["user"]["email"], common::email_of("maya"));
}
