//! Guardrails attached to teams and users: they cover the keys of the team
//! and of the user, also keys made afterwards, in the order defaults, team,
//! user, route, key; only an admin sets them; the configuration file holds
//! the teams'.

mod common;

use axum::http::StatusCode;
use common::{error_code, org, Org, Signed};
use serde_json::{json, Value};
use ultrafast_gateway::snapshot::Snapshot;

fn pii_email() -> Value {
    json!({ "id": "email", "matcher": { "pii": ["EMAIL"] }, "action": "redact", "directions": "both" })
}

async fn make_id(org: &Org, who: &Signed, name: &str, is_default: bool) -> i64 {
    let body = json!({ "name": name, "kind": "rules", "rules": [pii_email()],
                       "is_default": is_default });
    let (status, v) = org
        .call(Some(who), "POST", "/api/guardrails", Some(body))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{v}");
    v["guardrail"]["id"].as_i64().unwrap()
}

fn team_path(id: i64) -> String {
    format!("/api/teams/{id}/guardrails")
}

fn user_path(id: i64) -> String {
    format!("/api/users/{id}/guardrails")
}

async fn put(org: &Org, who: &Signed, path: &str, ids: Value) -> (StatusCode, Value) {
    org.call(
        Some(who),
        "PUT",
        path,
        Some(json!({ "guardrail_ids": ids })),
    )
    .await
}

async fn new_key(org: &Org, who: &Signed, name: &str, owner: i64, team: Option<i64>) -> String {
    let (status, v) = org
        .call(
            Some(who),
            "POST",
            "/api/keys",
            Some(json!({ "name": name, "owner_id": owner, "team_id": team })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{v}");
    v["secret"].as_str().unwrap().to_string()
}

async fn effective(org: &Org, secret: &str, route: Option<&str>) -> Vec<i64> {
    let snap = Snapshot::load(&org.api.store, &org.api.state.cipher)
        .await
        .unwrap();
    let key = snap
        .key(
            &ultrafast_gateway::secrets::hash_key(secret),
            "2000-01-01 00:00:00",
        )
        .unwrap();
    let route = route.map(|n| snap.route(n).unwrap());
    snap.effective_guardrails(route, Some(key))
        .iter()
        .map(|g| g.id)
        .collect()
}

#[tokio::test]
async fn a_team_guardrail_covers_a_key_made_afterwards() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let g = make_id(&org, &maya, "g", false).await;
    let (status, v) = put(&org, &maya, &team_path(org.platform), json!([g])).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["guardrail_ids"], json!([g]));

    let in_team = new_key(&org, &maya, "a", org.lena, Some(org.platform)).await;
    let other_team = new_key(&org, &maya, "b", org.tomas, Some(org.research)).await;
    assert_eq!(effective(&org, &in_team, None).await, [g]);
    assert!(effective(&org, &other_team, None).await.is_empty());

    let (_, detail) = org
        .call(
            Some(&maya),
            "GET",
            &format!("/api/teams/{}", org.platform),
            None,
        )
        .await;
    assert_eq!(detail["guardrail_ids"], json!([g]));
    assert_eq!(
        org.last_summary("guardrail.attach").await,
        "Set the guardrails of team Platform to g"
    );

    // Taking it off takes it off the key too.
    put(&org, &maya, &team_path(org.platform), json!([])).await;
    assert!(effective(&org, &in_team, None).await.is_empty());
}

#[tokio::test]
async fn a_user_guardrail_covers_the_keys_of_that_user() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let g = make_id(&org, &maya, "g", false).await;
    let (status, v) = put(&org, &maya, &user_path(org.lena), json!([g])).await;
    assert_eq!(status, StatusCode::OK, "{v}");

    let lena_key = new_key(&org, &maya, "a", org.lena, None).await;
    let tomas_key = new_key(&org, &maya, "b", org.tomas, None).await;
    assert_eq!(effective(&org, &lena_key, None).await, [g]);
    assert!(effective(&org, &tomas_key, None).await.is_empty());

    let (_, detail) = org
        .call(
            Some(&maya),
            "GET",
            &format!("/api/users/{}", org.lena),
            None,
        )
        .await;
    assert_eq!(detail["guardrail_ids"], json!([g]));
    let (_, list) = org.call(Some(&maya), "GET", "/api/users", None).await;
    let listed = list["users"]
        .as_array()
        .unwrap()
        .iter()
        .find(|u| u["id"] == org.lena)
        .unwrap();
    assert_eq!(listed["guardrail_ids"], json!([g]));
}

#[tokio::test]
async fn the_order_is_defaults_team_user_route_key_each_once() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let d = make_id(&org, &maya, "d", true).await;
    let t1 = make_id(&org, &maya, "t1", false).await;
    let t2 = make_id(&org, &maya, "t2", false).await;
    let u1 = make_id(&org, &maya, "u1", false).await;
    let r1 = make_id(&org, &maya, "r1", false).await;
    let k1 = make_id(&org, &maya, "k1", false).await;

    put(&org, &maya, &team_path(org.platform), json!([t2, t1, d])).await;
    put(&org, &maya, &user_path(org.lena), json!([u1, t1])).await;

    // A route with one of them, then a key repeating several.
    let store = &org.api.store;
    let provider = store
        .insert_provider("p", "openai", "https://x.example.com/v1", None)
        .await
        .unwrap();
    let mut tx = store.begin().await.unwrap();
    let model = tx.insert_model(provider, "m").await.unwrap();
    tx.set_model_enabled(model, true).await.unwrap();
    tx.commit().await.unwrap();
    let (status, v) = org
        .call(
            Some(&maya),
            "POST",
            "/api/routes",
            Some(json!({
                "name": "chat", "primaries": [{ "model_id": model, "weight": 1 }], "fallbacks": [],
                "retries": 2, "first_token_timeout_ms": 30000, "total_timeout_ms": 300000,
                "breaker_failures": 5, "breaker_window_s": 60, "breaker_open_s": 30,
                "everyone": true, "team_ids": [], "guardrail_ids": [r1, u1]
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{v}");
    let (status, key) = org
        .call(
            Some(&maya),
            "POST",
            "/api/keys",
            Some(
                json!({ "name": "k", "owner_id": org.lena, "team_id": org.platform,
                         "guardrail_ids": [k1, r1, t2] }),
            ),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{key}");
    let secret = key["secret"].as_str().unwrap();
    // defaults, team (t2 t1; d already seen), user (u1; t1 seen), route (r1), key (k1).
    assert_eq!(
        effective(&org, secret, Some("chat")).await,
        [d, t2, t1, u1, r1, k1]
    );
    // Without a route: the route's slot is empty (r1 is also the key's own).
    assert_eq!(effective(&org, secret, None).await, [d, t2, t1, u1, k1, r1]);
}

#[tokio::test]
async fn only_an_admin_attaches_and_the_ids_are_checked() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let arjun = org.sign_in("arjun").await;
    let lena = org.sign_in("lena").await;
    let g = make_id(&org, &maya, "g", false).await;

    for who in [&arjun, &lena] {
        let (status, v) = put(&org, who, &team_path(org.platform), json!([g])).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{v}");
        assert_eq!(error_code(&v), "forbidden");
        let (status, v) = put(&org, who, &user_path(org.lena), json!([g])).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{v}");
    }
    // A lead may read the ids of their own team.
    put(&org, &maya, &team_path(org.platform), json!([g])).await;
    let (status, v) = org
        .call(
            Some(&arjun),
            "GET",
            &format!("/api/teams/{}", org.platform),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(v["guardrail_ids"], json!([g]));

    for path in [team_path(org.platform), user_path(org.lena)] {
        let (status, v) = put(&org, &maya, &path, json!([g + 1000])).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
        assert!(v["error"]["fields"]["guardrail_ids"].is_string(), "{v}");
        let too_many: Vec<i64> = (1..=100).collect();
        let (status, v) = put(&org, &maya, &path, json!(too_many)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    }
    let (status, _) = put(&org, &maya, &team_path(9999), json!([])).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = put(&org, &maya, &user_path(9999), json!([])).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn deleting_a_guardrail_or_a_team_detaches_it() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let g = make_id(&org, &maya, "g", false).await;
    put(&org, &maya, &team_path(org.growth), json!([g])).await;
    let (status, _) = org
        .call(Some(&maya), "DELETE", &format!("/api/guardrails/{g}"), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, detail) = org
        .call(
            Some(&maya),
            "GET",
            &format!("/api/teams/{}", org.growth),
            None,
        )
        .await;
    assert_eq!(detail["guardrail_ids"], json!([]));
}

async fn export(org: &Org, who: &Signed) -> Value {
    let (status, v) = org.call(Some(who), "GET", "/api/config/export", None).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    v
}

async fn import(org: &Org, who: &Signed, file: &Value, dry_run: bool) -> (StatusCode, Value) {
    org.call(
        Some(who),
        "POST",
        &format!("/api/config/import?dry_run={dry_run}"),
        Some(file.clone()),
    )
    .await
}

#[tokio::test]
async fn the_export_holds_team_guardrails_and_the_import_restores_them() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let a = make_id(&org, &maya, "a", false).await;
    let b = make_id(&org, &maya, "b", false).await;
    put(&org, &maya, &team_path(org.platform), json!([b, a])).await;

    let first = export(&org, &maya).await;
    let team = |file: &Value, name: &str| -> Value {
        file["teams"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == name)
            .unwrap()
            .clone()
    };
    assert_eq!(team(&first, "Platform")["guardrails"], json!(["b", "a"]));
    // A team without any has no key: the file stays as before.
    assert!(team(&first, "Growth").get("guardrails").is_none());
    assert_eq!(first["version"], 1);

    // Into a fresh gateway, whose teams exist already.
    let fresh = common::org().await;
    let fresh_maya = fresh.sign_in("maya").await;
    let (status, dry) = import(&fresh, &fresh_maya, &first, true).await;
    assert_eq!(status, StatusCode::OK, "{dry}");
    let updated = dry["updated"].to_string();
    assert!(updated.contains("Platform"), "{dry}");
    let (_, detail) = fresh
        .call(
            Some(&fresh_maya),
            "GET",
            &format!("/api/teams/{}", fresh.platform),
            None,
        )
        .await;
    assert_eq!(
        detail["guardrail_ids"],
        json!([]),
        "a dry run writes nothing"
    );

    let (status, done) = import(&fresh, &fresh_maya, &first, false).await;
    assert_eq!(status, StatusCode::OK, "{done}");
    assert!(done["errors"].as_array().unwrap().is_empty(), "{done}");
    let second = export(&fresh, &fresh_maya).await;
    assert_eq!(second["teams"], first["teams"]);

    // Again: nothing changes.
    let (_, again) = import(&fresh, &fresh_maya, &first, false).await;
    assert!(again["updated"].as_array().unwrap().is_empty(), "{again}");
    assert!(again["created"].as_array().unwrap().is_empty(), "{again}");

    // A file that leaves it out does not change what is attached; [] takes it off.
    let mut quiet = first.clone();
    quiet["teams"][0]
        .as_object_mut()
        .unwrap()
        .remove("guardrails");
    let (_, report) = import(&fresh, &fresh_maya, &quiet, false).await;
    assert!(report["updated"].as_array().unwrap().is_empty(), "{report}");
    let mut cleared = first.clone();
    for t in cleared["teams"].as_array_mut().unwrap() {
        if t["name"] == "Platform" {
            t["guardrails"] = json!([]);
        }
    }
    import(&fresh, &fresh_maya, &cleared, false).await;
    assert!(team(&export(&fresh, &fresh_maya).await, "Platform")
        .get("guardrails")
        .is_none());

    // A name that does not exist is named by its place; nothing is written.
    let mut bad = first.clone();
    let at = bad["teams"]
        .as_array()
        .unwrap()
        .iter()
        .position(|t| t["name"] == "Platform")
        .unwrap();
    bad["teams"][at]["guardrails"] = json!(["a", "missing"]);
    let (status, report) = import(&fresh, &fresh_maya, &bad, true).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{report}");
    assert!(
        report["errors"]
            .to_string()
            .contains(&format!("teams[{at}].guardrails[1]")),
        "{report}"
    );
}

#[tokio::test]
async fn a_new_team_of_a_file_gets_its_guardrails() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    make_id(&org, &maya, "a", false).await;
    let mut file = export(&org, &maya).await;
    file["teams"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "name": "Fresh", "guardrails": ["a"] }));
    let (status, done) = import(&org, &maya, &file, false).await;
    assert_eq!(status, StatusCode::OK, "{done}");
    assert!(done["errors"].as_array().unwrap().is_empty(), "{done}");
    let again = export(&org, &maya).await;
    let fresh = again["teams"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "Fresh")
        .unwrap();
    assert_eq!(fresh["guardrails"], json!(["a"]));
}
