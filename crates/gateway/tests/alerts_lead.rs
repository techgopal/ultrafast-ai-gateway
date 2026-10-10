//! A team lead reads the alert rules and events about their teams' budgets,
//! and nothing else. A member reads none.

mod common;

use axum::http::StatusCode;
use common::{org, Org};
use serde_json::{json, Value};
use ultrafast_gateway::budgets::{BudgetAction, Period};
use ultrafast_gateway::limits::LimitScope;
use ultrafast_gateway::secrets::generate_key;
use ultrafast_gateway::store::NewAlertEvent;

struct World {
    org: Org,
    /// Rule names that arjun (leads Platform) may see.
    visible: Vec<&'static str>,
}

async fn world() -> World {
    let org = org().await;
    let mut tx = org.api.store.begin().await.unwrap();
    let mut keys = Vec::new();
    for (name, owner, team) in [
        ("lena-key", org.lena, org.platform),
        ("tomas-key", org.tomas, org.research),
    ] {
        let key = generate_key();
        keys.push(
            tx.insert_key(name, &key.hash, &key.display, None, Some(owner), Some(team))
                .await
                .unwrap(),
        );
    }
    let budget = |scope, id| (scope, id, 1_000_000u64);
    let mut ids = Vec::new();
    for (scope, id, amount) in [
        budget(LimitScope::Team, Some(org.platform)),
        budget(LimitScope::Team, Some(org.research)),
        budget(LimitScope::Key, Some(keys[0])),
        budget(LimitScope::Key, Some(keys[1])),
        budget(LimitScope::User, Some(org.arjun)),
    ] {
        ids.push(
            tx.upsert_budget(scope, id, amount, Period::Monthly, BudgetAction::Block)
                .await
                .unwrap(),
        );
    }
    let mut rules = Vec::new();
    for (name, kind, params) in [
        (
            "platform-team",
            "budget",
            json!({ "budget_id": ids[0], "percent": 80 }),
        ),
        (
            "research-team",
            "budget",
            json!({ "budget_id": ids[1], "percent": 80 }),
        ),
        (
            "lena-key",
            "budget",
            json!({ "budget_id": ids[2], "percent": 80 }),
        ),
        (
            "tomas-key",
            "budget",
            json!({ "budget_id": ids[3], "percent": 80 }),
        ),
        (
            "arjun-user",
            "budget",
            json!({ "budget_id": ids[4], "percent": 80 }),
        ),
        (
            "every-budget",
            "budget",
            json!({ "budget_id": null, "percent": 80 }),
        ),
        (
            "errors",
            "error_rate",
            json!({ "scope": "gateway", "percent": 10 }),
        ),
    ] {
        let id = tx
            .insert_alert_rule(name, kind, &params.to_string(), true)
            .await
            .unwrap();
        rules.push((id, name, kind));
    }
    for (id, name, kind) in &rules {
        tx.insert_alert_event(NewAlertEvent {
            rule_id: Some(*id),
            rule_name: name,
            kind,
            subject: "s",
            state: "firing",
            summary: "x",
            details: "{}",
            at: "2999-01-01 00:00:00",
        })
        .await
        .unwrap();
    }
    tx.insert_alert_event(NewAlertEvent {
        rule_id: None,
        rule_name: "gone",
        kind: "budget",
        subject: "s",
        state: "firing",
        summary: "x",
        details: "{}",
        at: "2999-01-01 00:00:00",
    })
    .await
    .unwrap();
    tx.commit().await.unwrap();
    World {
        org,
        visible: vec!["lena-key", "platform-team"],
    }
}

fn names(body: &Value, list: &str, field: &str) -> Vec<String> {
    let mut v: Vec<String> = body[list]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r[field].as_str().unwrap().to_string())
        .collect();
    v.sort();
    v
}

#[tokio::test]
async fn a_lead_sees_only_the_rules_on_their_teams_budgets() {
    let w = world().await;
    let arjun = w.org.sign_in("arjun").await;
    let (status, body) = w
        .org
        .call(Some(&arjun), "GET", "/api/alerts/rules", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(names(&body, "rules", "name"), ["lena-key", "platform-team"]);
    for rule in body["rules"].as_array().unwrap() {
        assert_eq!(rule["channels"], json!([]), "channels stay the admin's");
    }
    let maya = w.org.sign_in("maya").await;
    let (_, body) = w
        .org
        .call(Some(&maya), "GET", "/api/alerts/rules", None)
        .await;
    assert_eq!(body["rules"].as_array().unwrap().len(), 7);
}

#[tokio::test]
async fn a_lead_sees_only_the_events_of_those_rules_and_pages_through_them() {
    let w = world().await;
    let arjun = w.org.sign_in("arjun").await;
    let (status, body) = w
        .org
        .call(Some(&arjun), "GET", "/api/alerts/events", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        names(&body, "events", "rule_name"),
        w.visible.iter().map(|s| s.to_string()).collect::<Vec<_>>()
    );
    // Paging by one still reaches both, newest first, past the hidden rows.
    let (_, first) = w
        .org
        .call(Some(&arjun), "GET", "/api/alerts/events?limit=1", None)
        .await;
    assert_eq!(first["events"][0]["rule_name"], "lena-key");
    let before = first["events"][0]["id"].as_i64().unwrap();
    let (_, second) = w
        .org
        .call(
            Some(&arjun),
            "GET",
            &format!("/api/alerts/events?limit=1&before_id={before}"),
            None,
        )
        .await;
    assert_eq!(second["events"][0]["rule_name"], "platform-team");
    // Asking for a hidden rule's events gives nothing.
    let research = w
        .org
        .api
        .store
        .list_alert_rules()
        .await
        .unwrap()
        .into_iter()
        .find(|r| r.name == "research-team")
        .unwrap();
    let (status, body) = w
        .org
        .call(
            Some(&arjun),
            "GET",
            &format!("/api/alerts/events?rule_id={}", research.id),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["events"], json!([]));
}

#[tokio::test]
async fn a_member_is_refused() {
    let w = world().await;
    let lena = w.org.sign_in("lena").await;
    for path in ["/api/alerts/rules", "/api/alerts/events"] {
        let (status, body) = w.org.call(Some(&lena), "GET", path, None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path} {body}");
    }
}
