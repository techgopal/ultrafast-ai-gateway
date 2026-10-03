//! `GET /api/logs` and `GET /api/logs/{id}`.

mod common;

use axum::http::StatusCode;
use common::{error_code, org, Org};
use serde_json::{json, Value};
use ultrafast_gateway::secrets::generate_key;
use ultrafast_gateway::store::NewLog;

fn log(at: &str, user: Option<i64>, team: Option<i64>, requested: &str) -> NewLog {
    NewLog {
        at: at.into(),
        key_id: None,
        user_id: user,
        team_id: team,
        requested: requested.into(),
        endpoint: "chat".into(),
        stream: false,
        status: 200,
        provider: Some("main".into()),
        model: Some("gpt-4o".into()),
        input_tokens: Some(10),
        output_tokens: Some(5),
        cost_micros: 70,
        priced: true,
        cached: false,
        duration_ms: 12,
        attempts: json!([{
            "provider": "main", "model": "gpt-4o", "outcome": "ok",
            "status": 200, "duration_ms": 11
        }])
        .to_string(),
    }
}

struct Seeded {
    org: Org,
    lena_key: i64,
}

/// Rows `r1`..`r8`, ids 1..=8 in this order:
/// r1 lena in Platform, r2 tomas in Research, r3 priya alone,
/// r4 tomas in Platform, r5 lena without team, r6 arjun in Research,
/// r7 nobody, r8 tomas in Research (model "other", status 500).
async fn seeded() -> Seeded {
    let org = org().await;
    let key = generate_key();
    let mut tx = org.api.store.begin().await.unwrap();
    let lena_key = tx
        .insert_key(
            "lena-key",
            &key.hash,
            &key.display,
            None,
            Some(org.lena),
            Some(org.platform),
        )
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let (m, a, l, t, p) = (org.maya, org.arjun, org.lena, org.tomas, org.priya);
    let _ = (m, a);
    let mut r1 = log("2026-01-01 10:00:00", Some(l), Some(org.platform), "r1");
    r1.key_id = Some(lena_key);
    let r2 = log("2026-01-02 10:00:00", Some(t), Some(org.research), "r2");
    let r3 = log("2026-01-03 10:00:00", Some(p), None, "r3");
    let r4 = log("2026-01-04 10:00:00", Some(t), Some(org.platform), "r4");
    let r5 = log("2026-01-05 10:00:00", Some(l), None, "r5");
    let r6 = log("2026-01-06 10:00:00", Some(a), Some(org.research), "r6");
    let mut r7 = log("2026-01-07 10:00:00", None, None, "r7");
    r7.key_id = Some(9999);
    let mut r8 = log("2026-01-08 10:00:00", Some(t), Some(org.research), "r8");
    r8.model = Some("other".into());
    r8.status = 500;
    org.api
        .store
        .insert_logs(&[r1, r2, r3, r4, r5, r6, r7, r8])
        .await
        .unwrap();
    Seeded { org, lena_key }
}

async fn ids(org: &Org, who: &str, query: &str) -> Vec<i64> {
    let signed = org.sign_in(who).await;
    let (status, body) = org
        .call(Some(&signed), "GET", &format!("/api/logs{query}"), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{who} {query}: {body}");
    body["logs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["id"].as_i64().unwrap())
        .collect()
}

#[tokio::test]
async fn scope_matrix() {
    let s = seeded().await;
    let o = &s.org;
    assert_eq!(ids(o, "maya", "").await, vec![8, 7, 6, 5, 4, 3, 2, 1]);
    // arjun leads Platform: its team rows, its members' rows, his own.
    assert_eq!(ids(o, "arjun", "").await, vec![6, 5, 4, 1]);
    // lena is a plain member: only her own user id.
    assert_eq!(ids(o, "lena", "").await, vec![5, 1]);
    assert_eq!(ids(o, "tomas", "").await, vec![8, 4, 2]);
    assert_eq!(ids(o, "priya", "").await, vec![3]);
}

#[tokio::test]
async fn the_row_has_names_and_every_field() {
    let s = seeded().await;
    let maya = s.org.sign_in("maya").await;
    let (_, body) = s.org.call(Some(&maya), "GET", "/api/logs", None).await;
    let rows = body["logs"].as_array().unwrap();
    let r1 = rows.iter().find(|r| r["id"] == 1).unwrap();
    assert_eq!(
        *r1,
        json!({
            "id": 1, "at": "2026-01-01 10:00:00",
            "key_id": s.lena_key, "key_name": "lena-key",
            "user_id": s.org.lena, "user_email": "lena@example.com",
            "team_id": s.org.platform, "team_name": "Platform",
            "requested": "r1", "endpoint": "chat", "stream": false, "status": 200,
            "provider": "main", "model": "gpt-4o",
            "input_tokens": 10, "output_tokens": 5,
            "cost_micros": 70, "priced": true, "cached": false, "duration_ms": 12,
        })
    );
    // A key that is gone and a row without user or team: ids stay, names are null.
    let r7 = rows.iter().find(|r| r["id"] == 7).unwrap();
    assert_eq!(r7["key_id"], 9999);
    for field in ["key_name", "user_id", "user_email", "team_id", "team_name"] {
        assert_eq!(r7[field], Value::Null, "{field}");
    }
}

#[tokio::test]
async fn filters() {
    let s = seeded().await;
    let o = &s.org;
    let q = |s: String| async move { s };
    let _ = q;
    assert_eq!(
        ids(o, "maya", &format!("?user_id={}", o.tomas)).await,
        vec![8, 4, 2]
    );
    assert_eq!(
        ids(o, "maya", &format!("?team_id={}", o.research)).await,
        vec![8, 6, 2]
    );
    assert_eq!(
        ids(o, "maya", &format!("?key_id={}", s.lena_key)).await,
        vec![1]
    );
    assert_eq!(ids(o, "maya", "?model=other").await, vec![8]);
    assert_eq!(ids(o, "maya", "?status=500").await, vec![8]);
    // Errors only: status 400 and above, combinable with the others.
    assert_eq!(ids(o, "maya", "?errors=true").await, vec![8]);
    assert_eq!(ids(o, "maya", "?errors=true&status=500").await, vec![8]);
    assert_eq!(
        ids(o, "maya", &format!("?errors=true&user_id={}", o.lena)).await,
        Vec::<i64>::new()
    );
    assert_eq!(ids(o, "maya", "?errors=false").await.len(), 8);
    // Both bounds are inclusive; a date is a whole UTC day.
    assert_eq!(
        ids(o, "maya", "?from=2026-01-03&to=2026-01-05").await,
        vec![5, 4, 3]
    );
    assert_eq!(
        ids(
            o,
            "maya",
            "?from=2026-01-03T10:00:01Z&to=2026-01-05T12:00:00%2B02:00"
        )
        .await,
        vec![5, 4]
    );
    // Filters intersect.
    assert_eq!(
        ids(o, "maya", &format!("?user_id={}&model=other", o.tomas)).await,
        vec![8]
    );
    assert_eq!(
        ids(o, "maya", &format!("?user_id={}&status=404", o.tomas)).await,
        Vec::<i64>::new()
    );
}

#[tokio::test]
async fn filters_cannot_widen_the_scope() {
    let s = seeded().await;
    let o = &s.org;
    // lena asks for tomas's rows, for Research, for a key that is not hers.
    for q in [
        format!("?user_id={}", o.tomas),
        format!("?team_id={}", o.research),
        "?key_id=9999".to_string(),
        format!("?team_id={}&user_id={}", o.platform, o.tomas),
    ] {
        assert_eq!(ids(o, "lena", &q).await, Vec::<i64>::new(), "{q}");
    }
    // Her own team filter still only shows her rows.
    assert_eq!(
        ids(o, "lena", &format!("?team_id={}", o.platform)).await,
        vec![1]
    );
    // arjun leads Platform, he is only a member of Research (and r2 is tomas's).
    assert_eq!(
        ids(o, "arjun", &format!("?team_id={}", o.research)).await,
        vec![6]
    );
    assert_eq!(
        ids(o, "arjun", &format!("?user_id={}", o.tomas)).await,
        vec![4]
    );
    assert_eq!(ids(o, "arjun", "?key_id=9999").await, Vec::<i64>::new());
    // A cursor does not widen it either.
    assert_eq!(ids(o, "lena", "?before=100").await, vec![5, 1]);
}

#[tokio::test]
async fn cursor_and_limit() {
    let s = seeded().await;
    let o = &s.org;
    assert_eq!(ids(o, "maya", "?limit=3").await, vec![8, 7, 6]);
    assert_eq!(ids(o, "maya", "?limit=3&before=6").await, vec![5, 4, 3]);
    assert_eq!(ids(o, "maya", "?limit=3&before=3").await, vec![2, 1]);
    assert_eq!(ids(o, "maya", "?before=1").await, Vec::<i64>::new());
    assert_eq!(ids(o, "maya", "?limit=200").await.len(), 8);
    assert_eq!(ids(o, "maya", "?limit=1").await, vec![8]);
}

#[tokio::test]
async fn default_limit_is_50() {
    let o = org().await;
    let rows: Vec<NewLog> = (0..60)
        .map(|i| log("2026-02-01 00:00:00", None, None, &format!("x{i}")))
        .collect();
    o.api.store.insert_logs(&rows).await.unwrap();
    assert_eq!(ids(&o, "maya", "").await.len(), 50);
}

#[tokio::test]
async fn bad_parameters_are_422_with_fields() {
    let s = seeded().await;
    let maya = s.org.sign_in("maya").await;
    for (query, field) in [
        ("limit=0", "limit"),
        ("limit=201", "limit"),
        ("limit=x", "limit"),
        ("before=0", "before"),
        ("before=-1", "before"),
        ("before=abc", "before"),
        ("from=yesterday", "from"),
        ("from=2026-13-01", "from"),
        ("to=2026-01-01%2010:00:00", "to"),
        ("to=", "to"),
        ("key_id=0", "key_id"),
        ("user_id=a", "user_id"),
        ("team_id=-3", "team_id"),
        ("status=99", "status"),
        ("status=600", "status"),
        ("status=ok", "status"),
        ("errors=maybe", "errors"),
        ("errors=", "errors"),
    ] {
        let (status, body) = s
            .org
            .call(Some(&maya), "GET", &format!("/api/logs?{query}"), None)
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{query}: {body}");
        assert_eq!(error_code(&body), "validation_failed", "{query}");
        assert!(
            body["error"]["fields"][field].is_string(),
            "{query}: {body}"
        );
    }
    // Several at once are all named.
    let (_, body) = s
        .org
        .call(Some(&maya), "GET", "/api/logs?limit=0&from=x", None)
        .await;
    assert!(body["error"]["fields"]["limit"].is_string());
    assert!(body["error"]["fields"]["from"].is_string());
}

#[tokio::test]
async fn view_has_attempts_and_respects_scope() {
    let s = seeded().await;
    let o = &s.org;
    let maya = o.sign_in("maya").await;
    let (status, body) = o.call(Some(&maya), "GET", "/api/logs/1", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["id"], 1);
    assert_eq!(body["key_name"], "lena-key");
    assert_eq!(
        body["attempts"],
        json!([{
            "provider": "main", "model": "gpt-4o", "outcome": "ok",
            "status": 200, "duration_ms": 11
        }])
    );

    // (who, id, visible)
    for (who, id, visible) in [
        ("arjun", 1, true),
        ("arjun", 4, true),
        ("arjun", 5, true),
        ("arjun", 6, true),
        ("arjun", 2, false),
        ("arjun", 3, false),
        ("arjun", 7, false),
        ("lena", 1, true),
        ("lena", 5, true),
        ("lena", 4, false),
        ("lena", 2, false),
        ("tomas", 4, true),
        ("tomas", 1, false),
        ("priya", 3, true),
        ("priya", 1, false),
    ] {
        let signed = o.sign_in(who).await;
        let (status, body) = o
            .call(Some(&signed), "GET", &format!("/api/logs/{id}"), None)
            .await;
        if visible {
            assert_eq!(status, StatusCode::OK, "{who} {id}: {body}");
        } else {
            assert_eq!(status, StatusCode::NOT_FOUND, "{who} {id}: {body}");
            assert_eq!(error_code(&body), "not_found");
        }
    }
    // A log that does not exist and a bad id answer the same.
    for path in ["/api/logs/999", "/api/logs/0", "/api/logs/abc"] {
        let (status, body) = o.call(Some(&maya), "GET", path, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
        assert_eq!(error_code(&body), "not_found");
    }
}

#[tokio::test]
async fn needs_a_session() {
    let s = seeded().await;
    for path in ["/api/logs", "/api/logs/1"] {
        let (status, body) = s.org.call(None, "GET", path, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{path}");
        assert_eq!(error_code(&body), "unauthenticated");
    }
}

#[tokio::test]
async fn a_lead_loses_rows_when_removed_from_the_team() {
    let s = seeded().await;
    let o = &s.org;
    assert_eq!(ids(o, "arjun", "").await, vec![6, 5, 4, 1]);
    let maya = o.sign_in("maya").await;
    let (status, _) = o
        .call(
            Some(&maya),
            "DELETE",
            &format!("/api/teams/{}/members/{}", o.platform, o.arjun),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    // He now sees only his own rows.
    assert_eq!(ids(o, "arjun", "").await, vec![6]);
}
