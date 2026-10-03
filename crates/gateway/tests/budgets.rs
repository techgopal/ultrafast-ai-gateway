//! Budgets: the counters on their own, then on `/v1` at every scope, the
//! refusal in both shapes, accounting from the log writer, the flush, and
//! the rebuild after a restart.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{harness_with_sink, post_to, seed_team, seed_user, Harness};
use serde_json::{json, Value};
use time::macros::datetime;
use time::OffsetDateTime;
use tokio::sync::watch;
use ultrafast_gateway::app::AppState;
use ultrafast_gateway::budgets::{
    self, account, usd, Budget, BudgetAction, Budgets, MemoryBudgets, Period, FLUSH_INTERVAL,
};
use ultrafast_gateway::identity::{Role, TeamRole};
use ultrafast_gateway::limits::LimitScope;
use ultrafast_gateway::logs::writer::{spawn_accounted, WriterConfig};
use ultrafast_gateway::logs::{snapshot_prices, LogSink};
use ultrafast_gateway::secrets::generate_key;
use ultrafast_gateway::store::{NewLog, Store};
use ultrafast_gateway::telemetry::RequestRecord;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

const PASSWORD: &str = "correct horse battery";

fn budget(id: i64, amount: u64, period: Period, action: BudgetAction) -> Arc<Budget> {
    Arc::new(Budget {
        id,
        scope: LimitScope::Team,
        scope_id: 7,
        scope_label: "team 'Platform'".to_string(),
        amount_micros: amount,
        period,
        action,
    })
}

// ---- periods -------------------------------------------------------------

#[test]
fn periods_are_utc_calendar_periods_and_weeks_start_on_monday() {
    // 2999-01-06 is a Sunday; the week began on Monday 2998-12-31.
    let sunday = datetime!(2999-01-06 23:59:59 UTC);
    assert_eq!(Period::Daily.start_string(sunday), "2999-01-06");
    assert_eq!(Period::Weekly.start_string(sunday), "2998-12-31");
    assert_eq!(Period::Monthly.start_string(sunday), "2999-01-01");
    // On a Monday the week starts that day, at midnight.
    let monday = datetime!(2999-01-07 00:00:00 UTC);
    assert_eq!(Period::Weekly.start_string(monday), "2999-01-07");
    // The next period starts at the next midnight, Monday and month.
    assert_eq!(
        Period::Daily.next_start(sunday),
        datetime!(2999-01-07 00:00 UTC)
    );
    assert_eq!(
        Period::Weekly.next_start(sunday),
        datetime!(2999-01-07 00:00 UTC)
    );
    assert_eq!(
        Period::Monthly.next_start(datetime!(2999-12-31 12:00 UTC)),
        datetime!(3000-01-01 00:00 UTC)
    );
}

#[test]
fn dollars_have_at_least_two_decimals_and_no_noise() {
    assert_eq!(usd(50_000_000), "$50.00");
    assert_eq!(usd(1_500_000), "$1.50");
    assert_eq!(usd(1_234), "$0.001234");
    assert_eq!(usd(10_000), "$0.01");
    assert_eq!(usd(0), "$0.00");
}

// ---- the counters --------------------------------------------------------

#[test]
fn a_block_budget_refuses_from_the_amount_on_and_names_itself() {
    let b = MemoryBudgets::new();
    let monthly = budget(1, 50_000_000, Period::Monthly, BudgetAction::Block);
    let now = datetime!(2999-01-10 12:00:00 UTC);
    let who = [monthly.clone()];
    b.check(&who, now).unwrap();
    b.spend(&who, 49_999_999, now);
    b.check(&who, now).unwrap();
    b.spend(&who, 1, now);
    let refusal = b.check(&who, now).unwrap_err();
    assert_eq!(
        refusal.message(),
        "budget 'monthly $50.00' of team 'Platform' reached"
    );
    // Until the first of the next month, 2999-02-01 00:00.
    assert_eq!(refusal.retry_after_seconds(), 21 * 86_400 + 43_200);
    let near_end = datetime!(2999-01-31 23:59:30 UTC);
    assert_eq!(
        b.check(&who, near_end).unwrap_err().retry_after_seconds(),
        30
    );
}

#[test]
fn the_wait_runs_to_the_end_of_the_period_and_is_at_least_a_second() {
    let b = MemoryBudgets::new();
    let weekly = [budget(1, 10, Period::Weekly, BudgetAction::Block)];
    // Midnight Monday: the whole week is left.
    let monday = datetime!(2999-01-07 00:00:00 UTC);
    b.spend(&weekly, 10, monday);
    assert_eq!(
        b.check(&weekly, monday).unwrap_err().retry_after_seconds(),
        7 * 86_400
    );
    // Half a second before the reset is told as one second.
    let late = datetime!(2999-01-13 23:59:59.5 UTC);
    b.spend(&weekly, 10, late);
    assert_eq!(b.check(&weekly, late).unwrap_err().retry_after_seconds(), 1);
    // A month, from the 10th at noon to the 1st: 21 days and 12 hours.
    let monthly = [budget(2, 10, Period::Monthly, BudgetAction::Block)];
    let mid = datetime!(2999-01-10 12:00:00 UTC);
    b.spend(&monthly, 10, mid);
    assert_eq!(
        b.check(&monthly, mid).unwrap_err().retry_after_seconds(),
        21 * 86_400 + 43_200
    );
}

#[test]
fn seed_merges_with_a_counter_that_a_spend_created_first() {
    let b = MemoryBudgets::new();
    let who = [budget(1, 5_000_000, Period::Monthly, BudgetAction::Block)];
    let now = datetime!(2999-01-10 12:00:00 UTC);
    // A priced record lands between the refresh and the read of the logs.
    b.spend(&who, 10, now);
    b.seed(&who[0], "2999-01-01", 4_000_000, false);
    assert!(
        b.spent(&who[0], now) >= 4_000_000,
        "{}",
        b.spent(&who[0], now)
    );
    // A counter that is ahead of the logs is not lowered.
    b.spend(&who, 2_000_000, now);
    let ahead = b.spent(&who[0], now);
    b.seed(&who[0], "2999-01-01", 4_000_000, false);
    assert_eq!(b.spent(&who[0], now), ahead);
    // Merging keeps an alert that was already raised.
    let alert = [budget(2, 100, Period::Daily, BudgetAction::Alert)];
    let day = datetime!(2999-01-10 08:00:00 UTC);
    b.spend(&alert, 150, day);
    assert_eq!(b.drain().alerts.len(), 1);
    b.seed(&alert[0], "2999-01-10", 150, false);
    assert!(b.drain().alerts.is_empty(), "alerted once already");
}

#[test]
fn alert_fires_once_under_concurrent_spend() {
    let b = Arc::new(MemoryBudgets::new());
    let who = [budget(1, 100, Period::Daily, BudgetAction::Alert)];
    let now = datetime!(2999-01-10 08:00:00 UTC);
    let threads: Vec<_> = (0..32)
        .map(|_| {
            let (b, who) = (b.clone(), who.clone());
            std::thread::spawn(move || {
                for _ in 0..50 {
                    b.spend(&who, 1, now);
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    assert_eq!(b.spent(&who[0], now), 1_600);
    assert_eq!(b.drain().alerts.len(), 1);
}

#[test]
fn spend_that_arrives_during_a_flush_is_kept_and_written_next() {
    let b = MemoryBudgets::new();
    let who = [budget(1, 1_000_000, Period::Daily, BudgetAction::Block)];
    let now = datetime!(2999-01-10 08:00:00 UTC);
    b.spend(&who, 5, now);
    let drained = b.drain();
    // The write is running; more spend arrives, then the write fails.
    b.spend(&who, 7, now);
    b.requeue(drained);
    let next = b.drain();
    assert_eq!(next.usage.len(), 1);
    assert_eq!(next.usage[0].spent_micros, 12);
    // And when the write succeeds, the spend of the meantime is still dirty.
    b.spend(&who, 1, now);
    let drained = b.drain();
    b.spend(&who, 2, now);
    drop(drained);
    assert_eq!(b.drain().usage[0].spent_micros, 15);
}

#[test]
fn the_period_rolls_over_and_the_counter_starts_again() {
    let b = MemoryBudgets::new();
    let daily = [budget(1, 100, Period::Daily, BudgetAction::Block)];
    let day = datetime!(2999-01-10 23:59:59 UTC);
    b.spend(&daily, 100, day);
    assert!(b.check(&daily, day).is_err());
    let next = datetime!(2999-01-11 00:00:00 UTC);
    b.check(&daily, next).unwrap();
    assert_eq!(b.spent(&daily[0], next), 0);
    b.spend(&daily, 40, next);
    assert_eq!(b.spent(&daily[0], next), 40);
    // The old day's spend is not added to the new one.
    b.check(&daily, next).unwrap();
}

#[test]
fn the_strictest_budget_of_several_refuses() {
    let b = MemoryBudgets::new();
    let big = budget(1, 1_000, Period::Monthly, BudgetAction::Block);
    let small = budget(2, 10, Period::Daily, BudgetAction::Block);
    let who = [big, small];
    let now = datetime!(2999-01-10 12:00:00 UTC);
    b.spend(&who, 10, now);
    let refusal = b.check(&who, now).unwrap_err();
    assert!(
        refusal.message().starts_with("budget 'daily $0.00001"),
        "{}",
        refusal.message()
    );
}

#[test]
fn an_alert_budget_allows_and_alerts_once_per_period() {
    let b = MemoryBudgets::new();
    let alert = [budget(3, 100, Period::Daily, BudgetAction::Alert)];
    let day = datetime!(2999-01-10 08:00:00 UTC);
    b.spend(&alert, 60, day);
    assert!(b.drain().alerts.is_empty());
    b.spend(&alert, 60, day);
    b.check(&alert, day).unwrap();
    let alerts = b.drain().alerts;
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].budget_id, 3);
    assert_eq!(alerts[0].period_start, "2999-01-10");
    assert_eq!(
        alerts[0].summary,
        "Budget 'team 'Platform' daily' reached $0.00012 of $0.0001"
    );
    // More spend in the same period: still one.
    b.spend(&alert, 500, day);
    assert!(b.drain().alerts.is_empty());
    // A new period alerts again.
    let next = datetime!(2999-01-11 08:00:00 UTC);
    b.spend(&alert, 150, next);
    let alerts = b.drain().alerts;
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].period_start, "2999-01-11");
}

#[test]
fn a_block_budget_raises_no_alert() {
    let b = MemoryBudgets::new();
    let who = [budget(1, 10, Period::Daily, BudgetAction::Block)];
    let now = datetime!(2999-01-10 08:00:00 UTC);
    b.spend(&who, 100, now);
    assert!(b.drain().alerts.is_empty());
}

#[test]
fn drain_hands_out_what_changed_once_and_requeue_gives_it_back() {
    let b = MemoryBudgets::new();
    let who = [budget(1, 1_000, Period::Daily, BudgetAction::Block)];
    let now = datetime!(2999-01-10 08:00:00 UTC);
    b.spend(&who, 5, now);
    let drained = b.drain();
    assert_eq!(drained.usage.len(), 1);
    assert_eq!(drained.usage[0].budget_id, 1);
    assert_eq!(drained.usage[0].spent_micros, 5);
    assert_eq!(drained.usage[0].period_start, "2999-01-10");
    assert!(b.drain().usage.is_empty(), "nothing changed since");
    b.requeue(drained);
    assert_eq!(b.drain().usage.len(), 1, "a failed write is tried again");
}

#[test]
fn a_forgotten_budget_has_no_counter_and_seed_keeps_the_current_one() {
    let b = MemoryBudgets::new();
    let who = [budget(1, 100, Period::Daily, BudgetAction::Block)];
    let now = datetime!(2999-01-10 08:00:00 UTC);
    b.spend(&who, 30, now);
    // A counter of the current period is not replaced by an older read.
    b.seed(&who[0], "2999-01-10", 5, false);
    assert_eq!(b.spent(&who[0], now), 30);
    b.forget(1);
    assert_eq!(b.spent(&who[0], now), 0);
    b.seed(&who[0], "2999-01-10", 70, false);
    assert_eq!(b.spent(&who[0], now), 70);
    // Seeding a spend over the amount of an alert budget alerts at the next flush.
    let alert = budget(2, 10, Period::Daily, BudgetAction::Alert);
    b.seed(&alert, "2999-01-10", 20, false);
    assert_eq!(b.drain().alerts.len(), 1);
    let done = budget(3, 10, Period::Daily, BudgetAction::Alert);
    b.seed(&done, "2999-01-10", 20, true);
    assert!(
        b.drain().alerts.is_empty(),
        "already alerted before the restart"
    );
}

// ---- on /v1 --------------------------------------------------------------

struct World {
    h: Harness,
    user: i64,
    team: i64,
    key_id: i64,
    key: String,
}

async fn world() -> World {
    let (sink, _rx) = LogSink::channel(10);
    let h = harness_with_sink("openai", Arc::new(sink)).await;
    let user = seed_user(&h.store, "lena@example.com", Role::Member, PASSWORD).await;
    let team = seed_team(&h.store, "Platform", &[(user, TeamRole::Member)]).await;
    let key = generate_key();
    let mut tx = h.store.begin().await.unwrap();
    let key_id = tx
        .insert_key("ci", &key.hash, &key.display, None, Some(user), Some(team))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    h.state.refresh().await.unwrap();
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "c1", "model": "gpt-4o",
            "choices": [{ "message": { "role": "assistant", "content": "hello" }, "finish_reason": "stop" }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 2 }
        })))
        .mount(&h.upstream)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "m1", "type": "message", "role": "assistant", "model": "claude-sonnet-5",
            "content": [{ "type": "text", "text": "hello" }],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 1, "output_tokens": 2 }
        })))
        .mount(&h.upstream)
        .await;
    World {
        h,
        user,
        team,
        key_id,
        key: key.full,
    }
}

impl World {
    fn scope_id(&self, scope: LimitScope) -> Option<i64> {
        match scope {
            LimitScope::Gateway => None,
            LimitScope::Team => Some(self.team),
            LimitScope::User => Some(self.user),
            LimitScope::Key => Some(self.key_id),
        }
    }

    async fn budget(
        &self,
        scope: LimitScope,
        amount: u64,
        period: Period,
        action: BudgetAction,
    ) -> i64 {
        let mut tx = self.h.store.begin().await.unwrap();
        let id = tx
            .upsert_budget(scope, self.scope_id(scope), amount, period, action)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        self.h.state.refresh().await.unwrap();
        id
    }

    fn record(&self) -> RequestRecord {
        RequestRecord {
            key_id: self.key_id,
            user_id: Some(self.user),
            team_id: Some(self.team),
            requested: "p/m".into(),
            endpoint: "chat",
            stream: false,
            status: 200,
            usage: None,
            attempts: Vec::new(),
            cached: false,
            estimated: false,
            started_at: ultrafast_gateway::store::now(),
            duration_ms: 1,
        }
    }

    /// What the log writer does when it prices a record.
    fn spend(&self, micros: u64) {
        account(
            &self.h.state,
            &self.record(),
            micros,
            OffsetDateTime::now_utc(),
        );
    }

    async fn chat(&self) -> (axum::http::StatusCode, axum::http::HeaderMap, String) {
        let bearer = format!("Bearer {}", self.key);
        post_to(
            &self.h.app,
            "/v1/chat/completions",
            &[("authorization", &bearer)],
            r#"{"model":"p/gpt-4o","max_tokens":10,"messages":[{"role":"user","content":"hi"}]}"#,
        )
        .await
    }

    async fn messages(&self) -> (axum::http::StatusCode, axum::http::HeaderMap, String) {
        post_to(
            &self.h.app,
            "/v1/messages",
            &[("x-api-key", &self.key)],
            r#"{"model":"p/claude-sonnet-5","max_tokens":10,"messages":[{"role":"user","content":"hi"}]}"#,
        )
        .await
    }
}

const SCOPES: [(LimitScope, &str); 4] = [
    (LimitScope::Gateway, "gateway"),
    (LimitScope::Team, "team 'Platform'"),
    (LimitScope::User, "user 'lena@example.com'"),
    (LimitScope::Key, "key 'ci'"),
];

#[tokio::test]
async fn a_block_budget_refuses_at_every_scope() {
    for (scope, label) in SCOPES {
        let w = world().await;
        w.budget(scope, 5_000_000, Period::Monthly, BudgetAction::Block)
            .await;
        assert_eq!(w.chat().await.0, 200, "{label}: nothing spent yet");
        w.spend(4_999_999);
        assert_eq!(w.chat().await.0, 200, "{label}: just under");
        w.spend(1);
        let (status, headers, body) = w.chat().await;
        assert_eq!(status, 429, "{label}");
        let v: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            v["error"]["message"],
            format!("budget 'monthly $5.00' of {label} reached")
        );
        assert_eq!(v["error"]["type"], "rate_limit_error");
        assert_eq!(v["error"]["code"], "budget_exceeded");
        let wait: u64 = headers["retry-after"].to_str().unwrap().parse().unwrap();
        assert!((1..=31 * 86_400).contains(&wait), "{label}: {wait}");
    }
}

/// A call that a budget refuses before it is dispatched gives back its
/// request-per-minute slot and its token estimate at every scope: the
/// next refusals are still budget refusals, never rate-limit ones.
#[tokio::test]
async fn a_budget_refusal_gives_back_the_request_slot_and_the_tokens() {
    use ultrafast_gateway::limits::RateLimit;
    let w = world().await;
    let mut tx = w.h.store.begin().await.unwrap();
    for scope in [
        LimitScope::Gateway,
        LimitScope::Team,
        LimitScope::User,
        LimitScope::Key,
    ] {
        tx.upsert_limit(
            scope,
            w.scope_id(scope),
            &RateLimit {
                requests_per_minute: Some(1),
                tokens_per_minute: Some(1_000),
                concurrent: None,
            },
        )
        .await
        .unwrap();
    }
    tx.commit().await.unwrap();
    w.budget(
        LimitScope::Gateway,
        5_000_000,
        Period::Monthly,
        BudgetAction::Block,
    )
    .await;
    w.spend(5_000_000);
    for round in 0..4 {
        let (status, _, body) = w.chat().await;
        assert_eq!(status, 429, "{round}: {body}");
        assert!(body.contains("budget_exceeded"), "{round}: {body}");
        assert!(!body.contains("rate limit"), "{round}: {body}");
    }
    // Once the budget is gone, the slot is there for one call.
    let budgets = w.h.store.list_budgets().await.unwrap();
    let mut tx = w.h.store.begin().await.unwrap();
    for b in budgets {
        tx.delete_budget(b.id).await.unwrap();
    }
    tx.commit().await.unwrap();
    w.h.state.refresh().await.unwrap();
    assert_eq!(w.chat().await.0, 200);
    let (status, _, body) = w.chat().await;
    assert_eq!(status, 429);
    assert!(body.contains("rate limit"), "{body}");
}

#[tokio::test]
async fn the_refusal_has_the_anthropic_shape_on_messages() {
    let w = world().await;
    w.budget(
        LimitScope::Team,
        1_000_000,
        Period::Daily,
        BudgetAction::Block,
    )
    .await;
    assert_eq!(w.messages().await.0, 200);
    w.spend(1_000_000);
    let (status, headers, body) = w.messages().await;
    assert_eq!(status, 429);
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["type"], "error");
    assert_eq!(v["error"]["type"], "rate_limit_error");
    assert_eq!(
        v["error"]["message"],
        "budget 'daily $1.00' of team 'Platform' reached"
    );
    assert!(headers.contains_key("retry-after"));
}

#[tokio::test]
async fn a_refused_call_reaches_no_provider() {
    let w = world().await;
    w.budget(LimitScope::Gateway, 10, Period::Daily, BudgetAction::Block)
        .await;
    w.spend(10);
    let before = w.h.upstream.received_requests().await.unwrap().len();
    assert_eq!(w.chat().await.0, 429);
    assert_eq!(
        w.h.upstream.received_requests().await.unwrap().len(),
        before
    );
}

#[tokio::test]
async fn an_alert_budget_lets_calls_through_and_writes_one_audit_row() {
    let w = world().await;
    let id = w
        .budget(
            LimitScope::Team,
            1_000_000,
            Period::Daily,
            BudgetAction::Alert,
        )
        .await;
    w.spend(1_200_000);
    assert_eq!(w.chat().await.0, 200);
    w.spend(1_000_000);
    budgets::flush(&w.h.state).await;
    budgets::flush(&w.h.state).await;
    let audit = w.h.store.list_audit(50, None).await.unwrap();
    let alerts: Vec<_> = audit
        .iter()
        .filter(|a| a.action == "budget.alert")
        .collect();
    assert_eq!(alerts.len(), 1, "{audit:?}");
    assert_eq!(alerts[0].actor_email, "system");
    assert_eq!(alerts[0].target_id, Some(id));
    assert_eq!(
        alerts[0].summary,
        "Budget 'team 'Platform' daily' reached $1.20 of $1.00"
    );
}

#[tokio::test]
async fn an_alert_is_not_written_again_after_a_restart() {
    let w = world().await;
    w.budget(
        LimitScope::Team,
        1_000_000,
        Period::Daily,
        BudgetAction::Alert,
    )
    .await;
    w.spend(2_000_000);
    budgets::flush(&w.h.state).await;
    // A new process over the same store: the alert is already in the audit
    // log and is not raised again.
    let state = restarted(&w.h).await;
    budgets::rebuild(&state, OffsetDateTime::now_utc())
        .await
        .unwrap();
    budgets::flush(&state).await;
    let audit = w.h.store.list_audit(50, None).await.unwrap();
    assert_eq!(
        audit.iter().filter(|a| a.action == "budget.alert").count(),
        1
    );
}

/// A new process over the same store and master key.
async fn restarted(h: &Harness) -> Arc<AppState> {
    let state = AppState::new(h.store.clone(), h.state.cipher.clone());
    Arc::new(state.await.unwrap())
}

// ---- from the log writer -------------------------------------------------

#[tokio::test]
async fn the_writer_accounts_the_cost_of_a_priced_call() {
    let (sink, rx) = LogSink::channel(100);
    let stats = sink.stats();
    let h = harness_with_sink("openai", Arc::new(sink)).await;
    let user = seed_user(&h.store, "lena@example.com", Role::Member, PASSWORD).await;
    let team = seed_team(&h.store, "Platform", &[(user, TeamRole::Member)]).await;
    let key = generate_key();
    let model_id = h
        .store
        .list_models()
        .await
        .unwrap()
        .into_iter()
        .find(|m| m.name == "m")
        .unwrap()
        .id;
    let mut tx = h.store.begin().await.unwrap();
    tx.insert_key("ci", &key.hash, &key.display, None, Some(user), Some(team))
        .await
        .unwrap();
    tx.set_model_input_price(model_id, Some(2_000_000))
        .await
        .unwrap();
    tx.set_model_output_price(model_id, Some(4_000_000))
        .await
        .unwrap();
    tx.upsert_budget(
        LimitScope::Team,
        Some(team),
        10_000,
        Period::Daily,
        BudgetAction::Block,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    h.state.refresh().await.unwrap();
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "c1", "model": "x",
            "choices": [{ "message": { "role": "assistant", "content": "hi" }, "finish_reason": "stop" }],
            "usage": { "prompt_tokens": 1000, "completion_tokens": 500 }
        })))
        .mount(&h.upstream)
        .await;
    let (stop, stopped) = watch::channel(false);
    let writer = spawn_accounted(
        h.store.clone(),
        rx,
        snapshot_prices(h.state.clone()),
        stats,
        WriterConfig {
            max_batch: 500,
            max_wait: Duration::from_millis(20),
            retry_delay: Duration::from_millis(10),
        },
        stopped,
        budgets::accountant(h.state.clone()),
    );
    let bearer = format!("Bearer {}", key.full);
    let call = || async {
        post_to(
            &h.app,
            "/v1/chat/completions",
            &[("authorization", &bearer)],
            r#"{"model":"p/m","messages":[{"role":"user","content":"hi"}]}"#,
        )
        .await
        .0
    };
    // Each call costs 1000 x 2 + 500 x 4 = 4 000 micro-dollars; the budget
    // is 10 000, so the third call is the first to find it spent.
    assert_eq!(call().await, 200);
    assert_eq!(call().await, 200);
    let mut third = axum::http::StatusCode::OK;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(30)).await;
        third = call().await;
        if third == 429 {
            break;
        }
    }
    assert_eq!(third, 429u16, "spend reached the budget through the writer");
    stop.send(true).unwrap();
    writer.await.unwrap();
}

// ---- flush ---------------------------------------------------------------

/// `(spent, alerted)` as the cache holds it for the period of now.
async fn usage_row(store: &Store, id: i64, period: Period) -> Option<(u64, bool)> {
    let start = period.start_string(OffsetDateTime::now_utc());
    store.budget_usage(id, &start).await.unwrap()
}

#[test]
fn the_flush_interval_is_five_seconds() {
    assert_eq!(FLUSH_INTERVAL, Duration::from_secs(5));
}

#[tokio::test]
async fn spend_is_written_on_the_interval_and_not_before() {
    let w = world().await;
    let id = w
        .budget(
            LimitScope::Team,
            9_000_000,
            Period::Daily,
            BudgetAction::Block,
        )
        .await;
    let (stop, stopped) = watch::channel(false);
    let task = budgets::spawn_flush(w.h.state.clone(), Duration::from_millis(700), stopped);
    w.spend(1_234);
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(
        usage_row(&w.h.store, id, Period::Daily).await,
        None,
        "not before the interval"
    );
    let mut row = None;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        row = usage_row(&w.h.store, id, Period::Daily).await;
        if row.is_some() {
            break;
        }
    }
    assert_eq!(row.expect("written within the interval"), (1_234, false));
    stop.send(true).unwrap();
    task.await.unwrap();
}

#[tokio::test]
async fn shutdown_flushes_what_the_interval_has_not() {
    let w = world().await;
    let id = w
        .budget(
            LimitScope::Key,
            9_000_000,
            Period::Daily,
            BudgetAction::Block,
        )
        .await;
    let (stop, stopped) = watch::channel(false);
    let task = budgets::spawn_flush(w.h.state.clone(), Duration::from_secs(3600), stopped);
    w.spend(77);
    assert_eq!(usage_row(&w.h.store, id, Period::Daily).await, None);
    stop.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("the task ends")
        .unwrap();
    assert_eq!(
        usage_row(&w.h.store, id, Period::Daily).await.unwrap().0,
        77
    );
}

// ---- restart -------------------------------------------------------------

fn log(at: &str, key_id: i64, user_id: i64, team_id: i64, cost: i64) -> NewLog {
    NewLog {
        at: at.to_string(),
        key_id: Some(key_id),
        user_id: Some(user_id),
        team_id: Some(team_id),
        requested: "p/m".into(),
        endpoint: "chat".into(),
        stream: false,
        status: 200,
        provider: Some("p".into()),
        model: Some("m".into()),
        input_tokens: Some(1),
        output_tokens: Some(1),
        cost_micros: cost,
        priced: true,
        cached: false,
        estimated: false,
        duration_ms: 1,
        attempts: "[]".into(),
    }
}

#[tokio::test]
async fn a_block_budget_already_spent_keeps_blocking_after_a_restart() {
    // Review focus 4: spend logged, a new AppState over the same store.
    for (scope, label) in SCOPES {
        let w = world().await;
        w.budget(scope, 5_000_000, Period::Monthly, BudgetAction::Block)
            .await;
        let now = store_now();
        w.h.store
            .insert_logs(&[
                log(&now, w.key_id, w.user, w.team, 3_000_000),
                log(&now, w.key_id, w.user, w.team, 2_000_000),
            ])
            .await
            .unwrap();
        // Rows older than the period do not count.
        w.h.store
            .insert_logs(&[log(
                "2000-01-01 00:00:00",
                w.key_id,
                w.user,
                w.team,
                9_000_000,
            )])
            .await
            .unwrap();

        let state = restarted(&w.h).await;
        budgets::rebuild(&state, OffsetDateTime::now_utc())
            .await
            .unwrap();
        let app = ultrafast_gateway::app::router(state);
        let bearer = format!("Bearer {}", w.key);
        let (status, _, body) = post_to(
            &app,
            "/v1/chat/completions",
            &[("authorization", &bearer)],
            r#"{"model":"p/gpt-4o","messages":[{"role":"user","content":"hi"}]}"#,
        )
        .await;
        assert_eq!(status, 429, "{label}: {body}");
        assert!(body.contains("reached"), "{label}: {body}");
    }
}

fn store_now() -> String {
    ultrafast_gateway::store::now()
}

#[tokio::test]
async fn a_restart_counts_only_the_current_period_and_the_right_scope() {
    let w = world().await;
    let other_team = seed_team(&w.h.store, "Research", &[]).await;
    let id = w
        .budget(
            LimitScope::Team,
            5_000_000,
            Period::Daily,
            BudgetAction::Block,
        )
        .await;
    let now = OffsetDateTime::now_utc();
    let at = store_now();
    w.h.store
        .insert_logs(&[
            log(&at, w.key_id, w.user, w.team, 1_000),
            // Before the period began.
            log("2000-01-01 00:00:00", w.key_id, w.user, w.team, 9_000_000),
            // Another team's key and user: not this budget's.
            log(&at, 999, 998, other_team, 5_000_000),
        ])
        .await
        .unwrap();
    let state = restarted(&w.h).await;
    budgets::rebuild(&state, now).await.unwrap();
    let b = state
        .snapshot
        .load()
        .budgets_of(w.key_id, Some(w.user), Some(w.team));
    assert_eq!(b.len(), 1);
    assert_eq!(b[0].id, id);
    assert_eq!(state.budgets.spent(&b[0], now), 1_000);
}

#[tokio::test]
async fn the_cached_usage_covers_logs_that_retention_deleted() {
    let w = world().await;
    let id = w
        .budget(
            LimitScope::Gateway,
            5_000_000,
            Period::Monthly,
            BudgetAction::Block,
        )
        .await;
    w.spend(4_000_000);
    budgets::flush(&w.h.state).await;
    assert_eq!(
        usage_row(&w.h.store, id, Period::Monthly).await.unwrap().0,
        4_000_000
    );
    // No logs at all, but the flushed counter of this period is kept.
    let state = restarted(&w.h).await;
    let now = OffsetDateTime::now_utc();
    budgets::rebuild(&state, now).await.unwrap();
    let b = state
        .snapshot
        .load()
        .budgets_of(w.key_id, Some(w.user), Some(w.team));
    assert_eq!(state.budgets.spent(&b[0], now), 4_000_000);
}

#[tokio::test]
async fn a_late_alert_does_not_lower_the_cached_spend() {
    let w = world().await;
    let id = w
        .budget(
            LimitScope::Team,
            1_000_000,
            Period::Daily,
            BudgetAction::Alert,
        )
        .await;
    // The alert is raised at 1.0M; 0.5M more arrives before the flush.
    w.spend(1_000_000);
    w.spend(500_000);
    budgets::flush(&w.h.state).await;
    assert_eq!(
        usage_row(&w.h.store, id, Period::Daily).await,
        Some((1_500_000, true))
    );
    // A write of an older, lower value never lowers the row.
    let start = Period::Daily.start_string(OffsetDateTime::now_utc());
    w.h.store
        .write_budget_usage(&[ultrafast_gateway::store::UsageRow {
            budget_id: id,
            period_start: start,
            spent_micros: 10,
        }])
        .await
        .unwrap();
    assert_eq!(
        usage_row(&w.h.store, id, Period::Daily).await,
        Some((1_500_000, true))
    );
}

#[tokio::test]
async fn a_call_that_crosses_midnight_is_counted_in_the_period_it_started_in() {
    let w = world().await;
    let id = w
        .budget(
            LimitScope::Gateway,
            5_000_000,
            Period::Daily,
            BudgetAction::Block,
        )
        .await;
    let mut record = w.record();
    record.started_at = "2999-01-10 23:59:59".into();
    // The writer finishes the record after midnight: it uses started_at.
    budgets::accountant(w.h.state.clone())(&record, 1_000);
    let b =
        w.h.state
            .snapshot
            .load()
            .budgets_of(w.key_id, Some(w.user), Some(w.team));
    assert_eq!(b[0].id, id);
    assert_eq!(
        w.h.state
            .budgets
            .spent(&b[0], datetime!(2999-01-10 12:00 UTC)),
        1_000
    );
    assert_eq!(
        w.h.state
            .budgets
            .spent(&b[0], datetime!(2999-01-11 00:00 UTC)),
        0
    );
}

#[tokio::test]
async fn a_spend_between_the_refresh_and_the_read_of_the_logs_is_merged() {
    let w = world().await;
    let id = w
        .budget(
            LimitScope::Gateway,
            5_000_000,
            Period::Monthly,
            BudgetAction::Block,
        )
        .await;
    w.h.store
        .insert_logs(&[log(&store_now(), w.key_id, w.user, w.team, 4_000_000)])
        .await
        .unwrap();
    w.spend(10);
    let b =
        w.h.state
            .snapshot
            .load()
            .budgets_of(w.key_id, Some(w.user), Some(w.team));
    assert_eq!(b[0].id, id);
    let now = OffsetDateTime::now_utc();
    budgets::seed_from_logs(&w.h.state, &b[0], now)
        .await
        .unwrap();
    assert!(w.h.state.budgets.spent(&b[0], now) >= 4_000_000);
}

#[tokio::test]
async fn spend_during_flushes_is_all_written_in_the_end() {
    let w = world().await;
    let id = w
        .budget(
            LimitScope::Key,
            900_000_000,
            Period::Daily,
            BudgetAction::Block,
        )
        .await;
    let state = w.h.state.clone();
    let flusher = tokio::spawn({
        let state = state.clone();
        async move {
            for _ in 0..20 {
                budgets::flush(&state).await;
                tokio::task::yield_now().await;
            }
        }
    });
    for _ in 0..200 {
        w.spend(3);
        tokio::task::yield_now().await;
    }
    flusher.await.unwrap();
    budgets::flush(&state).await;
    assert_eq!(
        usage_row(&w.h.store, id, Period::Daily).await.unwrap().0,
        600
    );
}
