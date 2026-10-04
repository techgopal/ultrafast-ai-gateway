//! A call counts against the team limits and budgets of the key's team only;
//! a key without a team counts against all of its owner's teams.

mod common;

use common::{org, Org};
use ultrafast_gateway::budgets::{BudgetAction, Period};
use ultrafast_gateway::limits::{LimitScope, RateLimit};
use ultrafast_gateway::secrets::generate_key;
use ultrafast_gateway::snapshot::SnapKey;
use ultrafast_gateway::store::NewLog;

struct Keys {
    org: Org,
    /// Arjun's key of the Platform team.
    platform_key: i64,
    /// Arjun's key without a team.
    loose_key: i64,
}

async fn keys() -> Keys {
    let org = org().await;
    let mut tx = org.api.store.begin().await.unwrap();
    let mut ids = Vec::new();
    for (name, team) in [("platform-key", Some(org.platform)), ("loose-key", None)] {
        let k = generate_key();
        ids.push(
            tx.insert_key(name, &k.hash, &k.display, None, Some(org.arjun), team)
                .await
                .unwrap(),
        );
    }
    for team in [org.platform, org.research] {
        tx.upsert_limit(
            LimitScope::Team,
            Some(team),
            &RateLimit {
                requests_per_minute: Some(5),
                tokens_per_minute: None,
                concurrent: None,
            },
        )
        .await
        .unwrap();
        tx.upsert_budget(
            LimitScope::Team,
            Some(team),
            5_000_000,
            Period::Monthly,
            BudgetAction::Block,
        )
        .await
        .unwrap();
    }
    tx.commit().await.unwrap();
    org.api.state.refresh().await.unwrap();
    Keys {
        org,
        platform_key: ids[0],
        loose_key: ids[1],
    }
}

fn snap_key(k: &Keys, id: i64, team: Option<i64>) -> SnapKey {
    SnapKey {
        id,
        name: String::new(),
        user_id: Some(k.org.arjun),
        team_id: team,
        expires_at: None,
        allowed: None,
        tags: Default::default(),
    }
}

fn team_ids_of_limits(k: &Keys, key: &SnapKey) -> Vec<i64> {
    let mut ids: Vec<i64> = k
        .org
        .api
        .state
        .snapshot
        .load()
        .subjects(key)
        .teams
        .iter()
        .map(|s| s.id)
        .collect();
    ids.sort();
    ids
}

#[tokio::test]
async fn a_team_key_counts_for_its_team_only() {
    let k = keys().await;
    let key = snap_key(&k, k.platform_key, Some(k.org.platform));
    assert_eq!(team_ids_of_limits(&k, &key), vec![k.org.platform]);
    let budgets =
        k.org
            .api
            .state
            .snapshot
            .load()
            .budgets_of(Some(key.id), key.user_id, key.team_id);
    assert_eq!(budgets.len(), 1);
    assert_eq!(budgets[0].scope_id, k.org.platform);
}

#[tokio::test]
async fn a_key_without_a_team_counts_for_all_of_its_owners_teams() {
    let k = keys().await;
    let key = snap_key(&k, k.loose_key, None);
    let mut both = vec![k.org.platform, k.org.research];
    both.sort();
    assert_eq!(team_ids_of_limits(&k, &key), both);
    let budgets =
        k.org
            .api
            .state
            .snapshot
            .load()
            .budgets_of(Some(key.id), key.user_id, key.team_id);
    let mut ids: Vec<i64> = budgets.iter().map(|b| b.scope_id).collect();
    ids.sort();
    assert_eq!(ids, both);
}

fn log(user: i64, team: Option<i64>, cost: i64) -> NewLog {
    NewLog {
        tags: None,
        at: ultrafast_gateway::store::now(),
        key_id: None,
        user_id: Some(user),
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

#[tokio::test]
async fn the_rebuild_counts_the_same_way() {
    let k = keys().await;
    let store = &k.org.api.store;
    store
        .insert_logs(&[
            // Platform key of Arjun: Platform only.
            log(k.org.arjun, Some(k.org.platform), 1_000),
            // Arjun's key without a team: all of his teams.
            log(k.org.arjun, None, 10),
            // Tomas's team-less key counts for Research (his team).
            log(k.org.tomas, None, 100),
        ])
        .await
        .unwrap();
    let since = "2000-01-01 00:00:00";
    let platform = store
        .spend_since(LimitScope::Team, Some(k.org.platform), since)
        .await
        .unwrap();
    let research = store
        .spend_since(LimitScope::Team, Some(k.org.research), since)
        .await
        .unwrap();
    assert_eq!(platform, 1_010);
    assert_eq!(research, 110);
}
