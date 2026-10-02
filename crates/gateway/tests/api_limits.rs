//! The limits API: who sees which limit, who may set one, what is refused.

mod common;

use axum::http::StatusCode;
use common::{error_code, org, Org, Signed};
use serde_json::{json, Value};
use ultrafast_gateway::secrets::generate_key;

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
        .call(Some(who), "PUT", "/api/limits", Some(body))
        .await
}

/// The labels of the limits a caller sees, sorted.
async fn labels(w: &World, who: &Signed) -> Vec<String> {
    let (status, body) = w.org.call(Some(who), "GET", "/api/limits", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let mut labels: Vec<String> = body["limits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["label"].as_str().unwrap().to_string())
        .collect();
    labels.sort();
    labels
}

#[tokio::test]
async fn an_admin_sets_a_limit_and_sees_it() {
    let w = world().await;
    let maya = w.org.sign_in("maya").await;
    let (status, body) = put(
        &w,
        &maya,
        json!({ "scope": "team", "scope_id": w.org.platform, "requests_per_minute": 60, "concurrent": 4 }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["scope"], "team");
    assert_eq!(body["scope_id"], w.org.platform);
    assert_eq!(body["label"], "team 'Platform'");
    assert_eq!(body["requests_per_minute"], 60);
    assert_eq!(body["tokens_per_minute"], Value::Null);
    assert_eq!(body["concurrent"], 4);
    let id = body["id"].as_i64().unwrap();

    let (_, list) = w.org.call(Some(&maya), "GET", "/api/limits", None).await;
    assert_eq!(list["limits"].as_array().unwrap().len(), 1);
    assert_eq!(list["limits"][0]["id"], id);
    assert_eq!(
        w.org.last_summary("limit.set").await,
        "Set the limits of team 'Platform': 60 requests per minute, 4 concurrent requests"
    );
}

#[tokio::test]
async fn putting_again_replaces_the_limit() {
    let w = world().await;
    let maya = w.org.sign_in("maya").await;
    let body = |tokens: u64| json!({ "scope": "gateway", "tokens_per_minute": tokens });
    let (_, first) = put(&w, &maya, body(1000)).await;
    assert_eq!(first["scope"], "gateway");
    assert_eq!(first["scope_id"], Value::Null);
    assert_eq!(first["label"], "gateway");
    let (_, second) = put(
        &w,
        &maya,
        json!({ "scope": "gateway", "requests_per_minute": 5 }),
    )
    .await;
    assert_eq!(second["id"], first["id"]);
    // What is not sent is no limit any more.
    assert_eq!(second["tokens_per_minute"], Value::Null);
    assert_eq!(second["requests_per_minute"], 5);
    let (_, list) = w.org.call(Some(&maya), "GET", "/api/limits", None).await;
    assert_eq!(list["limits"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn labels_of_each_scope() {
    let w = world().await;
    let maya = w.org.sign_in("maya").await;
    for body in [
        json!({ "scope": "gateway", "concurrent": 1 }),
        json!({ "scope": "team", "scope_id": w.org.research, "concurrent": 1 }),
        json!({ "scope": "user", "scope_id": w.org.lena, "concurrent": 1 }),
        json!({ "scope": "key", "scope_id": w.lena_key, "concurrent": 1 }),
    ] {
        assert_eq!(put(&w, &maya, body).await.0, StatusCode::OK);
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
async fn who_sees_which_limits() {
    let w = world().await;
    let maya = w.org.sign_in("maya").await;
    for body in [
        json!({ "scope": "gateway", "concurrent": 1 }),
        json!({ "scope": "team", "scope_id": w.org.platform, "concurrent": 1 }),
        json!({ "scope": "team", "scope_id": w.org.research, "concurrent": 1 }),
        json!({ "scope": "team", "scope_id": w.org.growth, "concurrent": 1 }),
        json!({ "scope": "user", "scope_id": w.org.lena, "concurrent": 1 }),
        json!({ "scope": "user", "scope_id": w.org.tomas, "concurrent": 1 }),
        json!({ "scope": "key", "scope_id": w.lena_key, "concurrent": 1 }),
        json!({ "scope": "key", "scope_id": w.tomas_key, "concurrent": 1 }),
    ] {
        assert_eq!(put(&w, &maya, body).await.0, StatusCode::OK);
    }
    assert_eq!(labels(&w, &maya).await.len(), 8);

    // lena: gateway, her team, herself, her key.
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
    // arjun leads Platform and is in Research; he owns no key and has no
    // limit of his own, but the keys of both his teams throttle him.
    let arjun = w.org.sign_in("arjun").await;
    assert_eq!(
        labels(&w, &arjun).await,
        [
            "gateway",
            "key 'lena-key'",
            "key 'tomas-key'",
            "team 'Platform'",
            "team 'Research'"
        ]
    );
    // priya is in no team.
    let priya = w.org.sign_in("priya").await;
    assert_eq!(labels(&w, &priya).await, ["gateway"]);
}

/// A key of a team is listed to the members of that team, and to its owner
/// even when the owner has left the team.
#[tokio::test]
async fn a_team_key_limit_is_listed_to_the_teams_members() {
    let w = world().await;
    let maya = w.org.sign_in("maya").await;
    // lena owns a key of Research, a team she is not in.
    let key = generate_key();
    let mut tx = w.org.api.store.begin().await.unwrap();
    let id = tx
        .insert_key(
            "lena-research",
            &key.hash,
            &key.display,
            None,
            Some(w.org.lena),
            Some(w.org.research),
        )
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let (status, _) = put(
        &w,
        &maya,
        json!({ "scope": "key", "scope_id": id, "concurrent": 1 }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let seen = |who: &'static str| {
        let w = &w;
        async move {
            let who = w.org.sign_in(who).await;
            labels(w, &who)
                .await
                .contains(&"key 'lena-research'".to_string())
        }
    };
    assert!(seen("lena").await, "the owner");
    assert!(seen("tomas").await, "a member of the key's team");
    assert!(seen("arjun").await, "a member of the key's team");
    assert!(!seen("priya").await, "in no team");
}

#[tokio::test]
async fn only_an_admin_writes() {
    let w = world().await;
    let maya = w.org.sign_in("maya").await;
    let (_, made) = put(&w, &maya, json!({ "scope": "gateway", "concurrent": 3 })).await;
    let id = made["id"].as_i64().unwrap();
    for name in ["arjun", "lena"] {
        let who = w.org.sign_in(name).await;
        let (status, body) = put(&w, &who, json!({ "scope": "gateway", "concurrent": 9 })).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{name}");
        assert_eq!(error_code(&body), "forbidden");
        let (status, body) = w
            .org
            .call(Some(&who), "DELETE", &format!("/api/limits/{id}"), None)
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{name}");
        assert_eq!(error_code(&body), "forbidden");
    }
    let (status, _) = w.org.call(None, "GET", "/api/limits", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // Untouched.
    let (_, list) = w.org.call(Some(&maya), "GET", "/api/limits", None).await;
    assert_eq!(list["limits"][0]["concurrent"], 3);
}

#[tokio::test]
async fn validation() {
    let w = world().await;
    let maya = w.org.sign_in("maya").await;
    let cases = [
        (json!({ "scope": "galaxy", "concurrent": 1 }), "scope"),
        (
            json!({ "scope": "gateway", "scope_id": 1, "concurrent": 1 }),
            "scope_id",
        ),
        (json!({ "scope": "team", "concurrent": 1 }), "scope_id"),
        (
            json!({ "scope": "team", "scope_id": 9999, "concurrent": 1 }),
            "scope_id",
        ),
        (
            json!({ "scope": "user", "scope_id": 9999, "concurrent": 1 }),
            "scope_id",
        ),
        (
            json!({ "scope": "key", "scope_id": 9999, "concurrent": 1 }),
            "scope_id",
        ),
        (json!({ "scope": "gateway" }), "requests_per_minute"),
        (
            json!({ "scope": "gateway", "requests_per_minute": 0 }),
            "requests_per_minute",
        ),
        (
            json!({ "scope": "gateway", "requests_per_minute": -1 }),
            "requests_per_minute",
        ),
        (
            json!({ "scope": "gateway", "tokens_per_minute": 0 }),
            "tokens_per_minute",
        ),
        (json!({ "scope": "gateway", "concurrent": 0 }), "concurrent"),
        (
            json!({ "scope": "gateway", "concurrent": 1000001 }),
            "concurrent",
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
    // Not JSON of the expected shape at all.
    let (status, _) = put(
        &w,
        &maya,
        json!({ "scope": "gateway", "concurrent": 1, "extra": 1 }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = put(
        &w,
        &maya,
        json!({ "scope": "gateway", "concurrent": "many" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(labels(&w, &maya).await.is_empty());
}

#[tokio::test]
async fn delete_removes_the_limit_and_audits() {
    let w = world().await;
    let maya = w.org.sign_in("maya").await;
    let (_, made) = put(
        &w,
        &maya,
        json!({ "scope": "user", "scope_id": w.org.lena, "requests_per_minute": 5 }),
    )
    .await;
    let path = format!("/api/limits/{}", made["id"]);
    let (status, _) = w.org.call(Some(&maya), "DELETE", &path, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(labels(&w, &maya).await.is_empty());
    assert_eq!(
        w.org.last_summary("limit.delete").await,
        "Removed the limits of user 'lena@example.com'"
    );
    let (status, body) = w.org.call(Some(&maya), "DELETE", &path, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error_code(&body), "not_found");
    let (status, _) = w
        .org
        .call(Some(&maya), "DELETE", "/api/limits/abc", None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn writes_refresh_the_snapshot() {
    let w = world().await;
    let maya = w.org.sign_in("maya").await;
    let state = &w.org.api.state;
    let key = generate_key();
    let mut tx = w.org.api.store.begin().await.unwrap();
    let key_id = tx
        .insert_key(
            "fresh",
            &key.hash,
            &key.display,
            None,
            Some(w.org.lena),
            Some(w.org.platform),
        )
        .await
        .unwrap();
    tx.commit().await.unwrap();
    state.refresh().await.unwrap();
    let hash = key.hash.clone();
    let subjects_of = || {
        let snapshot = state.snapshot.load_full();
        let key = snapshot.key(&hash, "2000-01-01 00:00:00").unwrap().clone();
        snapshot.subjects(&key)
    };
    assert!(subjects_of().key.is_none());

    let before = state.refresh_count();
    let (_, made) = put(
        &w,
        &maya,
        json!({ "scope": "key", "scope_id": key_id, "concurrent": 2 }),
    )
    .await;
    assert!(state.refresh_count() > before);
    let subject = subjects_of().key.expect("the limit is in the snapshot");
    assert_eq!(subject.label, "key 'fresh'");
    assert_eq!(subject.limit.concurrent, Some(2));

    let before = state.refresh_count();
    let (status, _) = w
        .org
        .call(
            Some(&maya),
            "DELETE",
            &format!("/api/limits/{}", made["id"]),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(state.refresh_count() > before);
    assert!(subjects_of().key.is_none());
}

#[tokio::test]
async fn a_deleted_team_or_user_takes_its_limits_with_it() {
    let w = world().await;
    let maya = w.org.sign_in("maya").await;
    for body in [
        json!({ "scope": "team", "scope_id": w.org.growth, "concurrent": 1 }),
        json!({ "scope": "user", "scope_id": w.org.priya, "concurrent": 1 }),
        json!({ "scope": "gateway", "concurrent": 1 }),
    ] {
        assert_eq!(put(&w, &maya, body).await.0, StatusCode::OK);
    }
    let (s, _) = w
        .org
        .call(
            Some(&maya),
            "DELETE",
            &format!("/api/teams/{}", w.org.growth),
            None,
        )
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _) = w
        .org
        .call(
            Some(&maya),
            "DELETE",
            &format!("/api/users/{}", w.org.priya),
            None,
        )
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    assert_eq!(labels(&w, &maya).await, ["gateway"]);
}

#[tokio::test]
async fn the_database_refuses_a_second_gateway_limit() {
    let w = world().await;
    let mut tx = w.org.api.store.begin().await.unwrap();
    use ultrafast_gateway::limits::{LimitScope, RateLimit};
    let limit = RateLimit {
        requests_per_minute: Some(1),
        tokens_per_minute: None,
        concurrent: None,
    };
    let a = tx
        .upsert_limit(LimitScope::Gateway, None, &limit)
        .await
        .unwrap();
    let b = tx
        .upsert_limit(LimitScope::Gateway, None, &limit)
        .await
        .unwrap();
    assert_eq!(a, b, "an upsert, not a second row (NULL is not distinct)");
}
