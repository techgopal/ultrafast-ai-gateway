//! A user or team id is given out again after a delete (rowid reuse). What
//! the old one left in the logs must not pass to the new one.

mod common;

use common::{email_of, org, seed_team, seed_user, Org, ORG_PASSWORD};
use ultrafast_gateway::budgets::{self, BudgetAction, Period};
use ultrafast_gateway::identity::{Role, TeamRole};
use ultrafast_gateway::limits::LimitScope;
use ultrafast_gateway::store::{LogFilter, LogScope, NewLog, UsageGroup};

fn log(at: &str, user: Option<i64>, team: Option<i64>, cost: i64) -> NewLog {
    NewLog {
        tags: None,
        at: at.into(),
        key_id: None,
        user_id: user,
        team_id: team,
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

struct Reused {
    org: Org,
    user: i64,
    team: i64,
}

/// A user and a team that are the newest of their tables, each with logs,
/// deleted and created again: the ids are the same.
async fn reused() -> Reused {
    let org = org().await;
    let store = org.api.store.clone();
    let user = seed_user(&store, "zed@example.com", Role::Member, ORG_PASSWORD).await;
    let team = seed_team(&store, "Ops", &[(user, TeamRole::Member)]).await;
    let now = ultrafast_gateway::store::now();
    store
        .insert_logs(&[
            log(&now, Some(user), Some(team), 4_000_000),
            log(&now, Some(user), None, 1_000_000),
            log(&now, None, Some(team), 2_000_000),
        ])
        .await
        .unwrap();
    let mut tx = store.begin().await.unwrap();
    assert!(tx.delete_user(user).await.unwrap());
    assert!(tx.delete_team(team).await.unwrap());
    tx.commit().await.unwrap();
    let user2 = seed_user(&store, "zed2@example.com", Role::Member, ORG_PASSWORD).await;
    let team2 = seed_team(&store, "Ops again", &[(user2, TeamRole::Lead)]).await;
    assert_eq!(user2, user, "the id is given out again");
    assert_eq!(team2, team, "the id is given out again");
    Reused { org, user, team }
}

#[tokio::test]
async fn a_reused_id_inherits_no_logs() {
    let r = reused().await;
    let store = &r.org.api.store;
    let by_user = LogFilter {
        user_id: Some(r.user),
        ..Default::default()
    };
    assert!(store
        .list_logs(&LogScope::All, &by_user, 50)
        .await
        .unwrap()
        .is_empty());
    let by_team = LogFilter {
        team_id: Some(r.team),
        ..Default::default()
    };
    assert!(store
        .list_logs(&LogScope::All, &by_team, 50)
        .await
        .unwrap()
        .is_empty());
    // The new user and the new lead see nothing of the old rows.
    for scope in [
        LogScope::Own { user_id: r.user },
        LogScope::Teams {
            team_ids: vec![r.team],
            own_user_id: r.user,
        },
    ] {
        assert!(store
            .list_logs(&scope, &LogFilter::default(), 50)
            .await
            .unwrap()
            .is_empty());
        for group in [UsageGroup::User, UsageGroup::Team, UsageGroup::Day] {
            assert!(store
                .usage(&scope, "2000-01-01", "2999-01-01", group)
                .await
                .unwrap()
                .is_empty());
        }
    }
    // The rows are still there, owned by no one.
    let all = store
        .list_logs(&LogScope::All, &LogFilter::default(), 50)
        .await
        .unwrap();
    assert_eq!(all.len(), 3);
    assert!(all
        .iter()
        .all(|l| l.row.user_id.is_none() && l.row.team_id.is_none()));
    let _ = email_of("maya");
}

#[tokio::test]
async fn a_budget_on_a_reused_id_starts_from_zero() {
    let r = reused().await;
    let store = &r.org.api.store;
    for (scope, id) in [
        (LimitScope::User, Some(r.user)),
        (LimitScope::Team, Some(r.team)),
    ] {
        let mut tx = store.begin().await.unwrap();
        let budget = tx
            .upsert_budget(scope, id, 5_000_000, Period::Monthly, BudgetAction::Block)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        assert!(budget > 0);
    }
    // The live seed and the rebuild after a restart both read the logs.
    r.org.api.state.refresh().await.unwrap();
    let now = time::OffsetDateTime::now_utc();
    budgets::rebuild(&r.org.api.state, now).await.unwrap();
    let snapshot = r.org.api.state.snapshot.load();
    let found = snapshot.all_budgets();
    assert_eq!(found.len(), 2);
    for b in found {
        assert_eq!(r.org.api.state.budgets.spent(&b, now), 0, "{:?}", b.scope);
    }
    assert_eq!(
        store
            .spend_since(LimitScope::User, Some(r.user), "2000-01-01 00:00:00")
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        store
            .spend_since(LimitScope::Team, Some(r.team), "2000-01-01 00:00:00")
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn deleting_a_user_or_team_keeps_other_rows_attributed() {
    let org = org().await;
    let store = &org.api.store;
    let now = ultrafast_gateway::store::now();
    store
        .insert_logs(&[log(&now, Some(org.lena), Some(org.platform), 1)])
        .await
        .unwrap();
    let z = seed_user(store, "zed@example.com", Role::Member, ORG_PASSWORD).await;
    let mut tx = store.begin().await.unwrap();
    assert!(tx.delete_user(z).await.unwrap());
    tx.commit().await.unwrap();
    let all = store
        .list_logs(&LogScope::All, &LogFilter::default(), 50)
        .await
        .unwrap();
    assert_eq!(all[0].row.user_id, Some(org.lena));
    assert_eq!(all[0].row.team_id, Some(org.platform));
}
