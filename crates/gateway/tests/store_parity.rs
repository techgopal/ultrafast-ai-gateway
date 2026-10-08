//! What the store answers must not depend on the database behind it. These
//! run on SQLite, and on PostgreSQL when `UF_TEST_DATABASE_URL` is set.

mod common;

use std::sync::Arc;
use std::time::Duration;

use ultrafast_gateway::budgets::{BudgetAction, Period};
use ultrafast_gateway::limits::{LimitScope, RateLimit};
use ultrafast_gateway::store::{LogFilter, LogScope, NewLog, Store, UsageGroup};

fn limit(rpm: u64) -> RateLimit {
    RateLimit {
        requests_per_minute: Some(rpm),
        tokens_per_minute: None,
        concurrent: None,
    }
}

/// The unique indexes on `COALESCE(scope_id, 0)` decide what "the same
/// subject" is, and the upsert names them as its conflict target.
#[tokio::test]
async fn an_upsert_on_an_expression_index_updates_the_one_row_of_a_subject() {
    let store = Store::open_in_memory().await.unwrap();

    // The gateway has no scope id (NULL): still one row, however often it is set.
    let mut tx = store.begin().await.unwrap();
    let first = tx
        .upsert_limit(LimitScope::Gateway, None, &limit(10))
        .await
        .unwrap();
    let second = tx
        .upsert_limit(LimitScope::Gateway, None, &limit(20))
        .await
        .unwrap();
    // Another subject of another scope with the same id is another row.
    let team = tx
        .upsert_limit(LimitScope::Team, Some(7), &limit(1))
        .await
        .unwrap();
    let user = tx
        .upsert_limit(LimitScope::User, Some(7), &limit(2))
        .await
        .unwrap();
    let team_again = tx
        .upsert_limit(LimitScope::Team, Some(7), &limit(3))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(first, second, "one row for the gateway");
    assert_eq!(team, team_again);
    assert_ne!(team, user);
    let rows = store.list_limits().await.unwrap();
    assert_eq!(rows.len(), 3);
    let value = |scope| {
        rows.iter()
            .find(|r| r.scope == scope)
            .and_then(|r| r.limit.requests_per_minute)
    };
    assert_eq!(value(LimitScope::Gateway), Some(20));
    assert_eq!(value(LimitScope::Team), Some(3));
    assert_eq!(value(LimitScope::User), Some(2));

    // Budgets: the period is part of the subject.
    let mut tx = store.begin().await.unwrap();
    let day = tx
        .upsert_budget(
            LimitScope::Gateway,
            None,
            5,
            Period::Daily,
            BudgetAction::Block,
        )
        .await
        .unwrap();
    let day_again = tx
        .upsert_budget(
            LimitScope::Gateway,
            None,
            9,
            Period::Daily,
            BudgetAction::Alert,
        )
        .await
        .unwrap();
    let month = tx
        .upsert_budget(
            LimitScope::Gateway,
            None,
            5,
            Period::Monthly,
            BudgetAction::Block,
        )
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(day, day_again);
    assert_ne!(day, month);
    let budgets = store.list_budgets().await.unwrap();
    assert_eq!(budgets.len(), 2);
    let daily = budgets.iter().find(|b| b.period == Period::Daily).unwrap();
    assert_eq!(
        (daily.amount_micros, daily.action),
        (9, BudgetAction::Alert)
    );
}

async fn ids_with(store: &Store, tags: &[(&str, &str)]) -> Vec<String> {
    let filter = LogFilter {
        tags: tags
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        ..Default::default()
    };
    let mut at: Vec<String> = store
        .list_logs(&LogScope::All, &filter, 50)
        .await
        .unwrap()
        .into_iter()
        .map(|l| l.row.at)
        .collect();
    at.sort();
    at
}

fn log(at: &str, tags: Option<&str>, status: i64, cost: i64) -> NewLog {
    NewLog {
        tags: tags.map(str::to_string),
        at: at.into(),
        key_id: None,
        user_id: None,
        team_id: None,
        requested: "p/m".into(),
        endpoint: "chat".into(),
        stream: false,
        status,
        provider: Some("p".into()),
        model: Some("m".into()),
        input_tokens: Some(10),
        output_tokens: Some(5),
        cost_micros: cost,
        priced: true,
        cached: false,
        estimated: false,
        duration_ms: 1,
        attempts: "[]".into(),
    }
}

/// The same rows, filtered by tag and summed by day, give the same numbers
/// on every database (the tag is read out of a JSON text column).
#[tokio::test]
async fn tag_filters_and_usage_by_day_give_the_same_numbers() {
    let store = Store::open_in_memory().await.unwrap();
    store
        .insert_logs(&[
            log(
                "2999-01-01 10:00:00",
                Some(r#"{"team":"a","env":"prod"}"#),
                200,
                100,
            ),
            log("2999-01-01 23:59:59", Some(r#"{"team":"a"}"#), 500, 50),
            log("2999-01-02 00:00:00", Some(r#"{"team":"b"}"#), 200, 7),
            log("2999-01-02 12:00:00", None, 200, 1),
            // A tag name with characters that are not word characters.
            log(
                "2999-01-03 00:00:01",
                Some(r#"{"cost-center":"x.y","team":"a"}"#),
                200,
                3,
            ),
        ])
        .await
        .unwrap();

    assert_eq!(
        ids_with(&store, &[("team", "a")]).await,
        [
            "2999-01-01 10:00:00",
            "2999-01-01 23:59:59",
            "2999-01-03 00:00:01"
        ]
    );
    assert_eq!(
        ids_with(&store, &[("team", "a"), ("env", "prod")]).await,
        ["2999-01-01 10:00:00"]
    );
    assert_eq!(
        ids_with(&store, &[("cost-center", "x.y")]).await,
        ["2999-01-03 00:00:01"]
    );
    assert!(ids_with(&store, &[("team", "c")]).await.is_empty());
    assert!(ids_with(&store, &[("team", "")]).await.is_empty());

    let days = store
        .usage(&LogScope::All, "2999-01-01", "2999-01-03", UsageGroup::Day)
        .await
        .unwrap();
    let shape: Vec<_> = days
        .iter()
        .map(|d| (d.group.as_str(), d.requests, d.errors, d.cost_micros))
        .collect();
    assert_eq!(
        shape,
        [
            ("2999-01-01", 2, 1, 150),
            ("2999-01-02", 2, 0, 8),
            ("2999-01-03", 1, 0, 3)
        ]
    );
    // The last day counts whole, and a day before the range does not count.
    let one = store
        .usage(&LogScope::All, "2999-01-02", "2999-01-02", UsageGroup::Day)
        .await
        .unwrap();
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].requests, 2);

    let by_tag = store
        .usage(
            &LogScope::All,
            "2999-01-01",
            "2999-01-03",
            UsageGroup::Tag("team".into()),
        )
        .await
        .unwrap();
    let shape: Vec<_> = by_tag
        .iter()
        .map(|d| (d.group.as_str(), d.label.as_str(), d.requests))
        .collect();
    // Most requests first; equal counts by value, the rows without the tag first.
    assert_eq!(shape, [("a", "a", 3), ("", "(none)", 1), ("b", "b", 1)]);
}

/// The `to` day is inclusive to its last second, and the next day's first
/// second is out, on every database.
#[tokio::test]
async fn usage_counts_the_to_day_to_its_last_second() {
    let store = Store::open_in_memory().await.unwrap();
    store
        .insert_logs(&[
            log("2999-01-01 23:59:59", None, 200, 1),
            log("2999-01-02 00:00:00", None, 200, 10),
            log("2999-01-02 23:59:59", None, 200, 100),
            log("2999-01-03 00:00:00", None, 200, 1000),
        ])
        .await
        .unwrap();
    let rows = store
        .usage(&LogScope::All, "2999-01-02", "2999-01-02", UsageGroup::Day)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].requests, rows[0].cost_micros), (2, 110));
}

/// Tags that tie on requests order bytewise on every database (an uppercase
/// letter before a lowercase one), whatever the server's collation is.
#[tokio::test]
async fn tag_groups_that_tie_order_bytewise() {
    let store = Store::open_in_memory().await.unwrap();
    store
        .insert_logs(&[
            log("2999-01-01 10:00:00", Some(r#"{"team":"a"}"#), 200, 1),
            log("2999-01-01 10:00:01", Some(r#"{"team":"B"}"#), 200, 1),
            log("2999-01-01 10:00:02", Some(r#"{"team":"b"}"#), 200, 1),
            log("2999-01-01 10:00:03", Some(r#"{"team":"A"}"#), 200, 1),
        ])
        .await
        .unwrap();
    let rows = store
        .usage(
            &LogScope::All,
            "2999-01-01",
            "2999-01-01",
            UsageGroup::Tag("team".into()),
        )
        .await
        .unwrap();
    let order: Vec<&str> = rows.iter().map(|r| r.group.as_str()).collect();
    assert_eq!(order, ["A", "B", "a", "b"]);
}

/// A read-then-write transaction started with `begin_immediate` runs alone:
/// eight of them each add one to what they read, and none is lost or fails.
#[tokio::test]
async fn transactions_that_read_then_write_are_serialized() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(common::concurrent_store(dir.path()).await);
    let mut tx = store.begin().await.unwrap();
    let id = tx
        .upsert_limit(LimitScope::Gateway, None, &limit(1))
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let mut tasks = Vec::new();
    for _ in 0..8 {
        let store = store.clone();
        tasks.push(tokio::spawn(async move {
            let mut tx = store.begin_immediate().await?;
            let now = tx
                .limit_by_id(id)
                .await?
                .and_then(|r| r.limit.requests_per_minute)
                .unwrap();
            // Long enough for the others to start and wait, or to interleave.
            tokio::time::sleep(Duration::from_millis(30)).await;
            tx.upsert_limit(LimitScope::Gateway, None, &limit(now + 1))
                .await?;
            tx.commit().await
        }));
    }
    for t in tasks {
        t.await.unwrap().expect("a transaction failed");
    }
    let rows = store.list_limits().await.unwrap();
    assert_eq!(
        rows[0].limit.requests_per_minute,
        Some(1 + 8),
        "an update was lost"
    );
}

/// A limit or budget names its team or user without a foreign key, and a log
/// row names them too: deleting the team or user must take the first two with
/// it and leave the rows of the logs without an owner, whatever the database
/// (triggers in both schemas). Rows of others stay.
#[tokio::test]
async fn deleting_a_team_or_user_removes_its_limits_and_budgets_and_detaches_its_logs() {
    use ultrafast_gateway::identity::{Role, TeamRole};
    let store = Store::open_in_memory().await.unwrap();
    let gone_user =
        common::seed_user(&store, "gone@example.com", Role::Member, "pw-pw-pw-pw").await;
    let kept_user =
        common::seed_user(&store, "kept@example.com", Role::Member, "pw-pw-pw-pw").await;
    let gone_team = common::seed_team(&store, "Gone", &[(gone_user, TeamRole::Member)]).await;
    let kept_team = common::seed_team(&store, "Kept", &[(kept_user, TeamRole::Member)]).await;

    let mut tx = store.begin().await.unwrap();
    for (scope, id) in [
        (LimitScope::Gateway, None),
        (LimitScope::User, Some(gone_user)),
        (LimitScope::User, Some(kept_user)),
        (LimitScope::Team, Some(gone_team)),
        (LimitScope::Team, Some(kept_team)),
    ] {
        tx.upsert_limit(scope, id, &limit(5)).await.unwrap();
        if scope != LimitScope::Gateway {
            tx.upsert_budget(scope, id, 9, Period::Daily, BudgetAction::Block)
                .await
                .unwrap();
        }
    }
    tx.commit().await.unwrap();
    let mut mine = log("2999-01-01 00:00:00", None, 200, 1);
    mine.user_id = Some(gone_user);
    mine.team_id = Some(gone_team);
    let mut theirs = log("2999-01-01 00:00:01", None, 200, 1);
    theirs.user_id = Some(kept_user);
    theirs.team_id = Some(kept_team);
    store.insert_logs(&[mine, theirs]).await.unwrap();

    let mut tx = store.begin().await.unwrap();
    assert!(tx.delete_user(gone_user).await.unwrap());
    assert!(tx.delete_team(gone_team).await.unwrap());
    tx.commit().await.unwrap();

    let subjects = |rows: Vec<(LimitScope, Option<i64>)>| {
        let mut rows = rows;
        rows.sort_by_key(|(s, id)| (s.as_str().to_string(), *id));
        rows
    };
    let limits = subjects(
        store
            .list_limits()
            .await
            .unwrap()
            .into_iter()
            .map(|r| (r.scope, r.scope_id))
            .collect(),
    );
    assert_eq!(
        limits,
        subjects(vec![
            (LimitScope::Gateway, None),
            (LimitScope::User, Some(kept_user)),
            (LimitScope::Team, Some(kept_team)),
        ])
    );
    let budgets = subjects(
        store
            .list_budgets()
            .await
            .unwrap()
            .into_iter()
            .map(|r| (r.scope, r.scope_id))
            .collect(),
    );
    assert_eq!(
        budgets,
        subjects(vec![
            (LimitScope::User, Some(kept_user)),
            (LimitScope::Team, Some(kept_team)),
        ])
    );
    let logs = store
        .list_logs(&LogScope::All, &LogFilter::default(), 10)
        .await
        .unwrap();
    assert_eq!(logs.len(), 2);
    for l in &logs {
        if l.row.at.ends_with(":00") {
            assert_eq!((l.row.user_id, l.row.team_id), (None, None), "detached");
        } else {
            assert_eq!(
                (l.row.user_id, l.row.team_id),
                (Some(kept_user), Some(kept_team))
            );
        }
    }
}
