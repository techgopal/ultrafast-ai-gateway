//! The budgets API: who sees which budget and what it has spent, who may
//! set one, what is refused.

mod common;

use axum::http::StatusCode;
use common::{error_code, org, Org, Signed};
use serde_json::{json, Value};
use ultrafast_gateway::budgets::Period;
use ultrafast_gateway::secrets::generate_key;
use ultrafast_gateway::store::NewLog;

/// An org with a key of lena (in Platform) and one of tomas (in Research).
struct World {
    org: Org,
    lena_key: i64,
    tomas_key: i64,
}

async fn world() -> World {
    let org = org().await;
    let mut ids = Vec::new();
    for (name, owner, team) in [
        ("lena-key", org.lena, org.platform),
        ("tomas-key", org.tomas, org.research),
    ] {
        let key = generate_key();
        let mut tx = org.api.store.begin().await.unwrap();
        ids.push(
            tx.insert_key(name, &key.hash, &key.display, None, Some(owner), Some(team))
                .await
                .unwrap(),
        );
        tx.commit().await.unwrap();
    }
    World {
        org,
        lena_key: ids[0],
        tomas_key: ids[1],
    }
}

async fn put(w: &World, who: &Signed, body: Value) -> (StatusCode, Value) {
    w.org
        .call(Some(who), "PUT", "/api/budgets", Some(body))
        .await
}

fn team_budget(team: i64, amount: i64) -> Value {
    json!({ "scope": "team", "scope_id": team, "amount_micros": amount, "period": "monthly", "action": "block" })
}

/// The labels of the budgets a caller sees, sorted.
async fn labels(w: &World, who: &Signed) -> Vec<String> {
    let (status, body) = w.org.call(Some(who), "GET", "/api/budgets", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let mut labels: Vec<String> = body["budgets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["label"].as_str().unwrap().to_string())
        .collect();
    labels.sort();
    labels
}

#[tokio::test]
async fn an_admin_sets_a_budget_and_sees_it() {
    let w = world().await;
    let maya = w.org.sign_in("maya").await;
    let (status, body) = put(&w, &maya, team_budget(w.org.platform, 50_000_000)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["scope"], "team");
    assert_eq!(body["scope_id"], w.org.platform);
    assert_eq!(body["label"], "team 'Platform'");
    assert_eq!(body["amount_micros"], 50_000_000);
    assert_eq!(body["period"], "monthly");
    assert_eq!(body["action"], "block");
    assert_eq!(body["spent_micros"], 0);
    let start = Period::Monthly.start_string(time::OffsetDateTime::now_utc());
    assert_eq!(body["period_start"], start);
    let (_, list) = w.org.call(Some(&maya), "GET", "/api/budgets", None).await;
    assert_eq!(list["budgets"].as_array().unwrap().len(), 1);
    assert_eq!(list["budgets"][0]["id"], body["id"]);
    assert_eq!(
        w.org.last_summary("budget.set").await,
        "Set the monthly budget of team 'Platform' to $50.00, action block"
    );
}

#[tokio::test]
async fn putting_again_changes_the_budget_of_that_subject_and_period() {
    let w = world().await;
    let maya = w.org.sign_in("maya").await;
    let (_, first) = put(&w, &maya, team_budget(w.org.platform, 5_000_000)).await;
    let (_, second) = put(
        &w,
        &maya,
        json!({ "scope": "team", "scope_id": w.org.platform, "amount_micros": 9_000_000, "period": "monthly", "action": "alert" }),
    )
    .await;
    assert_eq!(second["id"], first["id"]);
    assert_eq!(second["amount_micros"], 9_000_000);
    assert_eq!(second["action"], "alert");
    // Another period is another budget.
    let (_, daily) = put(
        &w,
        &maya,
        json!({ "scope": "team", "scope_id": w.org.platform, "amount_micros": 1_000_000, "period": "daily", "action": "block" }),
    )
    .await;
    assert_ne!(daily["id"], first["id"]);
    let (_, list) = w.org.call(Some(&maya), "GET", "/api/budgets", None).await;
    assert_eq!(list["budgets"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn a_gateway_budget_has_no_scope_id_and_stays_single() {
    let w = world().await;
    let maya = w.org.sign_in("maya").await;
    let body = |amount: i64| json!({ "scope": "gateway", "amount_micros": amount, "period": "weekly", "action": "alert" });
    let (_, a) = put(&w, &maya, body(100)).await;
    let (_, b) = put(&w, &maya, body(200)).await;
    assert_eq!(a["label"], "gateway");
    assert_eq!(a["scope_id"], Value::Null);
    assert_eq!(a["id"], b["id"]);
}

#[tokio::test]
async fn labels_of_each_scope() {
    let w = world().await;
    let maya = w.org.sign_in("maya").await;
    for (scope, id) in [
        ("team", Some(w.org.research)),
        ("user", Some(w.org.lena)),
        ("key", Some(w.lena_key)),
        ("gateway", None),
    ] {
        let mut body =
            json!({ "scope": scope, "amount_micros": 5, "period": "daily", "action": "block" });
        if let Some(id) = id {
            body["scope_id"] = id.into();
        }
        assert_eq!(put(&w, &maya, body).await.0, StatusCode::OK, "{scope}");
    }
    assert_eq!(
        labels(&w, &maya).await,
        [
            "gateway",
            "key 'lena-key'",
            "team 'Research'",
            "user 'lena@example.com'"
        ]
    );
}

#[tokio::test]
async fn who_sees_which_budgets() {
    let w = world().await;
    let maya = w.org.sign_in("maya").await;
    let mut bodies = vec![
        json!({ "scope": "gateway", "amount_micros": 5, "period": "daily", "action": "block" }),
    ];
    for (scope, id) in [
        ("team", w.org.platform),
        ("team", w.org.research),
        ("team", w.org.growth),
        ("user", w.org.lena),
        ("user", w.org.tomas),
        ("key", w.lena_key),
        ("key", w.tomas_key),
    ] {
        bodies.push(json!({ "scope": scope, "scope_id": id, "amount_micros": 5, "period": "daily", "action": "block" }));
    }
    for body in bodies {
        assert_eq!(put(&w, &maya, body).await.0, StatusCode::OK);
    }
    assert_eq!(labels(&w, &maya).await.len(), 8);
    let lena = w.org.sign_in("lena").await;
    assert_eq!(
        labels(&w, &lena).await,
        [
            "gateway",
            "key 'lena-key'",
            "team 'Platform'",
            "user 'lena@example.com'"
        ]
    );
    // arjun leads Platform and is in Research; he owns no key, and no one
    // else's key or user is his to see.
    let arjun = w.org.sign_in("arjun").await;
    assert_eq!(
        labels(&w, &arjun).await,
        ["gateway", "team 'Platform'", "team 'Research'"]
    );
    let priya = w.org.sign_in("priya").await;
    assert_eq!(labels(&w, &priya).await, ["gateway"]);
}

/// The spent figure of each budget, by label, as a caller sees it.
async fn spent(w: &World, who: &Signed) -> Vec<(String, Value)> {
    let (_, body) = w.org.call(Some(who), "GET", "/api/budgets", None).await;
    let mut rows: Vec<(String, Value)> = body["budgets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| {
            (
                b["label"].as_str().unwrap().to_string(),
                b["spent_micros"].clone(),
            )
        })
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows
}

/// A member sees what was spent by their own user and keys only; a lead also
/// by the teams they lead; the gateway's and the other teams' budgets show
/// the amount, period and action but no spend; an admin sees all.
#[tokio::test]
async fn spent_is_shown_only_where_the_caller_may_see_it() {
    let w = world().await;
    let maya = w.org.sign_in("maya").await;
    for (scope, id) in [
        ("gateway", None),
        ("team", Some(w.org.platform)),
        ("team", Some(w.org.research)),
        ("user", Some(w.org.lena)),
        ("key", Some(w.lena_key)),
    ] {
        let mut body = json!({ "scope": scope, "amount_micros": 5_000_000, "period": "monthly", "action": "alert" });
        if let Some(id) = id {
            body["scope_id"] = json!(id);
        }
        assert_eq!(put(&w, &maya, body).await.0, StatusCode::OK);
    }
    // Everything lena's key spent is counted by every budget.
    let record = ultrafast_gateway::telemetry::RequestRecord {
        tags: Default::default(),
        key_id: Some(w.lena_key),
        user_id: Some(w.org.lena),
        team_id: Some(w.org.platform),
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
    };
    ultrafast_gateway::budgets::account(
        &w.org.api.state,
        &record,
        1_000_000,
        time::OffsetDateTime::now_utc(),
    );
    let n = Value::Null;
    let m = |v: u64| json!(v);
    assert_eq!(
        spent(&w, &maya).await,
        [
            ("gateway".to_string(), m(1_000_000)),
            ("key 'lena-key'".to_string(), m(1_000_000)),
            ("team 'Platform'".to_string(), m(1_000_000)),
            ("team 'Research'".to_string(), m(0)),
            ("user 'lena@example.com'".to_string(), m(1_000_000)),
        ]
    );
    // lena, a member of Platform: her user and key with the spend, the
    // gateway and her team without.
    let lena = w.org.sign_in("lena").await;
    assert_eq!(
        spent(&w, &lena).await,
        [
            ("gateway".to_string(), n.clone()),
            ("key 'lena-key'".to_string(), m(1_000_000)),
            ("team 'Platform'".to_string(), n.clone()),
            ("user 'lena@example.com'".to_string(), m(1_000_000)),
        ]
    );
    // arjun leads Platform and is a member of Research.
    let arjun = w.org.sign_in("arjun").await;
    assert_eq!(
        spent(&w, &arjun).await,
        [
            ("gateway".to_string(), n.clone()),
            ("team 'Platform'".to_string(), m(1_000_000)),
            ("team 'Research'".to_string(), n),
        ]
    );
}

#[tokio::test]
async fn only_an_admin_writes() {
    let w = world().await;
    let maya = w.org.sign_in("maya").await;
    let (_, made) = put(&w, &maya, team_budget(w.org.platform, 100)).await;
    let id = made["id"].as_i64().unwrap();
    for name in ["arjun", "lena"] {
        let who = w.org.sign_in(name).await;
        let (status, body) = put(&w, &who, team_budget(w.org.platform, 999)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{name}");
        assert_eq!(error_code(&body), "forbidden");
        let (status, body) = w
            .org
            .call(Some(&who), "DELETE", &format!("/api/budgets/{id}"), None)
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{name}");
        assert_eq!(error_code(&body), "forbidden");
    }
    let (status, _) = w.org.call(None, "GET", "/api/budgets", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (_, list) = w.org.call(Some(&maya), "GET", "/api/budgets", None).await;
    assert_eq!(list["budgets"][0]["amount_micros"], 100);
}

#[tokio::test]
async fn validation() {
    let w = world().await;
    let maya = w.org.sign_in("maya").await;
    let ok = |scope: &str, id: Option<i64>| {
        let mut v =
            json!({ "scope": scope, "amount_micros": 100, "period": "daily", "action": "block" });
        if let Some(id) = id {
            v["scope_id"] = id.into();
        }
        v
    };
    let with = |mut v: Value, field: &str, value: Value| {
        v[field] = value;
        v
    };
    let cases = [
        (ok("galaxy", None), "scope"),
        (ok("gateway", Some(1)), "scope_id"),
        (ok("team", None), "scope_id"),
        (ok("team", Some(9999)), "scope_id"),
        (ok("user", Some(9999)), "scope_id"),
        (ok("key", Some(9999)), "scope_id"),
        (
            with(ok("gateway", None), "amount_micros", json!(0)),
            "amount_micros",
        ),
        (
            with(ok("gateway", None), "amount_micros", json!(-5)),
            "amount_micros",
        ),
        (
            with(
                ok("gateway", None),
                "amount_micros",
                json!(1_000_000_000_000_001_i64),
            ),
            "amount_micros",
        ),
        (
            with(ok("gateway", None), "period", json!("yearly")),
            "period",
        ),
        (
            with(ok("gateway", None), "action", json!("pause")),
            "action",
        ),
    ];
    for (body, field) in cases {
        let (status, answer) = put(&w, &maya, body.clone()).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}: {answer}");
        assert!(
            answer["error"]["fields"][field].is_string(),
            "{body}: {answer}"
        );
    }
    for body in [
        with(ok("gateway", None), "extra", json!(1)),
        with(ok("gateway", None), "amount_micros", json!("a lot")),
        json!({ "scope": "gateway", "period": "daily", "action": "block" }),
    ] {
        assert_eq!(put(&w, &maya, body).await.0, StatusCode::BAD_REQUEST);
    }
    assert!(labels(&w, &maya).await.is_empty());
}

#[tokio::test]
async fn delete_removes_the_budget_its_counter_and_audits() {
    let w = world().await;
    let maya = w.org.sign_in("maya").await;
    let (_, made) = put(&w, &maya, team_budget(w.org.platform, 100)).await;
    let id = made["id"].as_i64().unwrap();
    let path = format!("/api/budgets/{id}");
    let (status, _) = w.org.call(Some(&maya), "DELETE", &path, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(labels(&w, &maya).await.is_empty());
    assert_eq!(
        w.org.last_summary("budget.delete").await,
        "Removed the monthly budget of team 'Platform'"
    );
    let (status, body) = w.org.call(Some(&maya), "DELETE", &path, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error_code(&body), "not_found");
    let (status, _) = w
        .org
        .call(Some(&maya), "DELETE", "/api/budgets/abc", None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

fn log(at: &str, key_id: i64, user_id: i64, team_id: i64, cost: i64) -> NewLog {
    NewLog {
        tags: None,
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
async fn a_new_budget_counts_what_the_period_already_spent_and_the_list_shows_it() {
    let w = world().await;
    let maya = w.org.sign_in("maya").await;
    let now = ultrafast_gateway::store::now();
    w.org
        .api
        .store
        .insert_logs(&[
            log(&now, w.lena_key, w.org.lena, w.org.platform, 1_500_000),
            log(&now, w.tomas_key, w.org.tomas, w.org.research, 700_000),
            log(
                "2000-01-01 00:00:00",
                w.lena_key,
                w.org.lena,
                w.org.platform,
                9_000_000,
            ),
        ])
        .await
        .unwrap();
    let (_, made) = put(&w, &maya, team_budget(w.org.platform, 5_000_000)).await;
    assert_eq!(made["spent_micros"], 1_500_000);
    let (_, list) = w.org.call(Some(&maya), "GET", "/api/budgets", None).await;
    assert_eq!(list["budgets"][0]["spent_micros"], 1_500_000);
    // Changing the amount keeps the counter.
    let (_, again) = put(&w, &maya, team_budget(w.org.platform, 8_000_000)).await;
    assert_eq!(again["spent_micros"], 1_500_000);
}

#[tokio::test]
async fn writes_refresh_the_snapshot() {
    let w = world().await;
    let maya = w.org.sign_in("maya").await;
    let state = &w.org.api.state;
    let budgets = || {
        state
            .snapshot
            .load()
            .budgets_of(Some(w.lena_key), Some(w.org.lena), Some(w.org.platform))
    };
    assert!(budgets().is_empty());
    let before = state.refresh_count();
    let (_, made) = put(&w, &maya, team_budget(w.org.platform, 5)).await;
    assert!(state.refresh_count() > before);
    assert_eq!(budgets().len(), 1);
    assert_eq!(budgets()[0].scope_label, "team 'Platform'");
    let before = state.refresh_count();
    let (status, _) = w
        .org
        .call(
            Some(&maya),
            "DELETE",
            &format!("/api/budgets/{}", made["id"]),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(state.refresh_count() > before);
    assert!(budgets().is_empty());
}

#[tokio::test]
async fn a_deleted_team_or_user_takes_its_budgets_with_it() {
    let w = world().await;
    let maya = w.org.sign_in("maya").await;
    for body in [
        team_budget(w.org.growth, 5),
        json!({ "scope": "user", "scope_id": w.org.priya, "amount_micros": 5, "period": "daily", "action": "block" }),
        json!({ "scope": "gateway", "amount_micros": 5, "period": "daily", "action": "block" }),
    ] {
        assert_eq!(put(&w, &maya, body).await.0, StatusCode::OK);
    }
    for path in [
        format!("/api/teams/{}", w.org.growth),
        format!("/api/users/{}", w.org.priya),
    ] {
        let (s, _) = w.org.call(Some(&maya), "DELETE", &path, None).await;
        assert_eq!(s, StatusCode::NO_CONTENT);
    }
    assert_eq!(labels(&w, &maya).await, ["gateway"]);
}
