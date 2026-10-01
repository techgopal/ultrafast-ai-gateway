mod common;

use axum::http::StatusCode;
use common::{error_code, org, Org, Signed};
use serde_json::{json, Value};

/// A provider with models, made through the store. Returns the model ids.
async fn seed_models(org: &Org, provider: &str, names: &[&str], enabled: bool) -> Vec<i64> {
    let p = org
        .api
        .store
        .insert_provider(provider, "openai", "https://x.example.com/v1", None)
        .await
        .unwrap();
    let mut ids = Vec::new();
    let mut tx = org.api.store.begin().await.unwrap();
    for n in names {
        let id = tx.insert_model(p, n).await.unwrap();
        tx.set_model_enabled(id, enabled).await.unwrap();
        ids.push(id);
    }
    tx.commit().await.unwrap();
    org.api.state.refresh().await.unwrap();
    ids
}

fn body(name: &str, primaries: &[(i64, i64)], fallbacks: &[i64], teams: &[i64]) -> Value {
    json!({
        "name": name,
        "primaries": primaries.iter().map(|(m, w)| json!({"model_id": m, "weight": w})).collect::<Vec<_>>(),
        "fallbacks": fallbacks,
        "retries": 2,
        "first_token_timeout_ms": 30000,
        "total_timeout_ms": 300000,
        "breaker_failures": 5,
        "breaker_window_s": 60,
        "breaker_open_s": 30,
        "everyone": teams.is_empty(),
        "team_ids": teams,
    })
}

async fn create(org: &Org, who: &Signed, b: Value) -> (StatusCode, Value) {
    org.call(Some(who), "POST", "/api/routes", Some(b)).await
}

async fn routes(org: &Org, who: &Signed) -> Vec<Value> {
    let (status, b) = org.call(Some(who), "GET", "/api/routes", None).await;
    assert_eq!(status, StatusCode::OK, "{b}");
    b["routes"].as_array().unwrap().clone()
}

#[tokio::test]
async fn create_view_update_delete() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let m = seed_models(&org, "openai", &["a", "b", "c"], true).await;

    let (status, v) = create(
        &org,
        &maya,
        body("fast", &[(m[0], 3), (m[1], 1)], &[m[2]], &[org.platform]),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{v}");
    let id = v["id"].as_i64().unwrap();
    assert_eq!(v["name"], "fast");
    assert_eq!(
        v["primaries"],
        json!([
            {"model_id": m[0], "model": "openai/a", "weight": 3, "enabled": true},
            {"model_id": m[1], "model": "openai/b", "weight": 1, "enabled": true},
        ])
    );
    assert_eq!(
        v["fallbacks"],
        json!([{"model_id": m[2], "model": "openai/c", "enabled": true}])
    );
    assert_eq!(v["retries"], 2);
    assert_eq!(v["first_token_timeout_ms"], 30000);
    assert_eq!(v["total_timeout_ms"], 300000);
    assert_eq!(v["breaker_failures"], 5);
    assert_eq!(v["breaker_window_s"], 60);
    assert_eq!(v["breaker_open_s"], 30);
    assert_eq!(v["team_ids"], json!([org.platform]));
    assert_eq!(v["broken"], false);
    assert!(v["created_at"].is_string());
    assert_eq!(org.last_summary("route.create").await, "Created route fast");

    let (status, got) = org
        .call(Some(&maya), "GET", &format!("/api/routes/{id}"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(got, v);
    assert_eq!(routes(&org, &maya).await, vec![v.clone()]);

    // A full replace: order of fallbacks, settings, teams.
    let mut b = body("quick", &[(m[1], 7)], &[m[2], m[0]], &[]);
    b["retries"] = json!(0);
    b["breaker_open_s"] = json!(5);
    let (status, u) = org
        .call(Some(&maya), "PUT", &format!("/api/routes/{id}"), Some(b))
        .await;
    assert_eq!(status, StatusCode::OK, "{u}");
    assert_eq!(u["id"], id);
    assert_eq!(u["name"], "quick");
    assert_eq!(u["primaries"].as_array().unwrap().len(), 1);
    assert_eq!(u["primaries"][0]["weight"], 7);
    assert_eq!(u["fallbacks"][0]["model"], "openai/c");
    assert_eq!(u["fallbacks"][1]["model"], "openai/a");
    assert_eq!(u["retries"], 0);
    assert_eq!(u["breaker_open_s"], 5);
    assert_eq!(u["team_ids"], json!([]));
    assert_eq!(
        org.last_summary("route.update").await,
        "Updated route quick"
    );

    // Keeping its own name is not a clash.
    let (status, _) = org
        .call(
            Some(&maya),
            "PUT",
            &format!("/api/routes/{id}"),
            Some(body("quick", &[(m[0], 1)], &[], &[])),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    // A second route cannot take the name.
    let (status, e) = create(&org, &maya, body("quick", &[(m[0], 1)], &[], &[])).await;
    assert_eq!(
        (status, error_code(&e)),
        (StatusCode::CONFLICT, "route_exists")
    );
    let (_, other) = create(&org, &maya, body("other", &[(m[0], 1)], &[], &[])).await;
    let oid = other["id"].as_i64().unwrap();
    let (status, e) = org
        .call(
            Some(&maya),
            "PUT",
            &format!("/api/routes/{oid}"),
            Some(body("quick", &[(m[0], 1)], &[], &[])),
        )
        .await;
    assert_eq!(
        (status, error_code(&e)),
        (StatusCode::CONFLICT, "route_exists")
    );

    let (status, _) = org
        .call(Some(&maya), "DELETE", &format!("/api/routes/{id}"), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        org.last_summary("route.delete").await,
        "Deleted route quick"
    );
    let (status, e) = org
        .call(Some(&maya), "GET", &format!("/api/routes/{id}"), None)
        .await;
    assert_eq!(
        (status, error_code(&e)),
        (StatusCode::NOT_FOUND, "not_found")
    );
    let (status, _) = org
        .call(Some(&maya), "DELETE", &format!("/api/routes/{id}"), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = org
        .call(
            Some(&maya),
            "PUT",
            &format!("/api/routes/{id}"),
            Some(body("z", &[(m[0], 1)], &[], &[])),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(routes(&org, &maya).await.len(), 1);
}

#[tokio::test]
async fn validation_errors_on_fields() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let m = seed_models(&org, "openai", &["a", "b"], true).await;
    let ok = || body("r", &[(m[0], 1)], &[], &[]);

    let with = |k: &str, v: Value| {
        let mut b = ok();
        b[k] = v;
        b
    };
    let cases: Vec<(&str, Value)> = vec![
        ("name", with("name", json!(""))),
        ("name", with("name", json!("x".repeat(65)))),
        ("name", with("name", json!("Upper"))),
        ("name", with("name", json!("-lead"))),
        ("name", with("name", json!("has space"))),
        ("primaries", with("primaries", json!([]))),
        ("primaries", body("r", &[(m[0], 0)], &[], &[])),
        ("primaries", body("r", &[(m[0], 1001)], &[], &[])),
        ("primaries", body("r", &[(m[0], 1), (m[0], 1)], &[], &[])),
        ("primaries", body("r", &[(9999, 1)], &[], &[])),
        ("fallbacks", body("r", &[(m[0], 1)], &[m[1], m[1]], &[])),
        ("fallbacks", body("r", &[(m[0], 1)], &[m[0]], &[])),
        ("fallbacks", body("r", &[(m[0], 1)], &[9999], &[])),
        ("retries", with("retries", json!(6))),
        ("retries", with("retries", json!(-1))),
        (
            "first_token_timeout_ms",
            with("first_token_timeout_ms", json!(999)),
        ),
        (
            "first_token_timeout_ms",
            with("first_token_timeout_ms", json!(300001)),
        ),
        ("total_timeout_ms", with("total_timeout_ms", json!(999))),
        ("total_timeout_ms", with("total_timeout_ms", json!(3600001))),
        ("total_timeout_ms", {
            let mut b = ok();
            b["first_token_timeout_ms"] = json!(20000);
            b["total_timeout_ms"] = json!(19999);
            b
        }),
        ("breaker_failures", with("breaker_failures", json!(0))),
        ("breaker_failures", with("breaker_failures", json!(101))),
        ("breaker_window_s", with("breaker_window_s", json!(4))),
        ("breaker_window_s", with("breaker_window_s", json!(3601))),
        ("breaker_open_s", with("breaker_open_s", json!(4))),
        ("breaker_open_s", with("breaker_open_s", json!(3601))),
        ("team_ids", body("r", &[(m[0], 1)], &[], &[9999])),
    ];
    for (field, b) in cases {
        let (status, e) = create(&org, &maya, b.clone()).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{field}: {b}");
        assert!(e["error"]["fields"][field].is_string(), "{field}: {e}");
    }
    // Boundaries are accepted.
    let mut b = body("edge", &[(m[0], 1000)], &[], &[]);
    b["retries"] = json!(5);
    b["first_token_timeout_ms"] = json!(1000);
    b["total_timeout_ms"] = json!(1000);
    b["breaker_failures"] = json!(100);
    b["breaker_window_s"] = json!(3600);
    b["breaker_open_s"] = json!(5);
    let (status, e) = create(&org, &maya, b).await;
    assert_eq!(status, StatusCode::CREATED, "{e}");
    // A unknown top-level field is a 400, not silently ignored.
    let (status, _) = create(&org, &maya, with("surprise", json!(1))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // Nothing from the refused calls was stored.
    assert_eq!(routes(&org, &maya).await.len(), 1);
}

#[tokio::test]
async fn name_with_slash_is_refused() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let m = seed_models(&org, "openai", &["a"], true).await;
    for name in ["openai/a", "a/b", "/"] {
        let (status, e) = create(&org, &maya, body(name, &[(m[0], 1)], &[], &[])).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{name}");
        assert!(e["error"]["fields"]["name"].is_string(), "{name}");
    }
    for name in ["a", "gpt-4.fast_1", "0x"] {
        let (status, e) = create(&org, &maya, body(name, &[(m[0], 1)], &[], &[])).await;
        assert_eq!(status, StatusCode::CREATED, "{name}: {e}");
    }
}

#[tokio::test]
async fn model_delete_removes_target_and_marks_broken() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let m = seed_models(&org, "openai", &["a", "b", "c"], true).await;
    let (_, v) = create(
        &org,
        &maya,
        body("r", &[(m[0], 1), (m[1], 1)], &[m[2]], &[]),
    )
    .await;
    let id = v["id"].as_i64().unwrap();
    assert_eq!(v["broken"], false);

    let del = |model: i64| {
        let org = &org;
        let maya = &maya;
        async move {
            let (s, _) = org
                .call(Some(maya), "DELETE", &format!("/api/models/{model}"), None)
                .await;
            assert_eq!(s, StatusCode::NO_CONTENT);
            let (_, v) = org
                .call(Some(maya), "GET", &format!("/api/routes/{id}"), None)
                .await;
            v
        }
    };
    let v = del(m[0]).await;
    assert_eq!(v["primaries"].as_array().unwrap().len(), 1);
    assert_eq!(v["broken"], false);
    let v = del(m[2]).await;
    assert_eq!(v["fallbacks"], json!([]));
    assert_eq!(v["broken"], false);
    let v = del(m[1]).await;
    assert_eq!(v["primaries"], json!([]));
    assert_eq!(v["broken"], true);
    // The route itself stays.
    assert_eq!(routes(&org, &maya).await.len(), 1);
}

#[tokio::test]
async fn broken_means_no_enabled_target() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let off = seed_models(&org, "off", &["a", "b"], false).await;
    let on = seed_models(&org, "on", &["a"], true).await;
    let (_, only_off) = create(
        &org,
        &maya,
        body("only-off", &[(off[0], 1)], &[off[1]], &[]),
    )
    .await;
    assert_eq!(only_off["broken"], true);
    assert_eq!(only_off["primaries"][0]["enabled"], false);
    // An enabled fallback is enough.
    let (_, fb) = create(&org, &maya, body("fb", &[(off[0], 1)], &[on[0]], &[])).await;
    assert_eq!(fb["broken"], false);
    // Enabling a model heals the route.
    let (s, _) = org
        .call(
            Some(&maya),
            "PATCH",
            &format!("/api/models/{}", off[0]),
            Some(json!({"enabled": true})),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let id = only_off["id"].as_i64().unwrap();
    let (_, v) = org
        .call(Some(&maya), "GET", &format!("/api/routes/{id}"), None)
        .await;
    assert_eq!(v["broken"], false);
}

#[tokio::test]
async fn non_admin_sees_only_usable_routes() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let m = seed_models(&org, "openai", &["a", "b"], true).await;
    let mut ids = Vec::new();
    for (name, teams) in [
        ("everyone", vec![]),
        ("platform", vec![org.platform]),
        ("research", vec![org.research]),
        ("both", vec![org.platform, org.research]),
    ] {
        let mut b = body(name, &[(m[0], 5)], &[m[1]], &teams);
        b["retries"] = json!(4);
        let (s, v) = create(&org, &maya, b).await;
        assert_eq!(s, StatusCode::CREATED, "{v}");
        ids.push(v["id"].as_i64().unwrap());
    }
    assert_eq!(routes(&org, &maya).await.len(), 4);

    let cases = [
        ("lena", vec!["everyone", "platform", "both"]),
        ("tomas", vec!["everyone", "research", "both"]),
        ("priya", vec!["everyone"]),
        ("arjun", vec!["everyone", "platform", "research", "both"]),
    ];
    for (who, want) in cases {
        let signed = org.sign_in(who).await;
        let seen = routes(&org, &signed).await;
        let mut got: Vec<&str> = seen.iter().map(|r| r["name"].as_str().unwrap()).collect();
        got.sort_unstable();
        let mut want = want;
        want.sort_unstable();
        assert_eq!(got, want, "{who}");
        for r in &seen {
            // One shape; settings and teams are empty for a non-admin.
            assert_eq!(r["team_ids"], json!([]), "{who}");
            assert_eq!(r["retries"], 0);
            assert_eq!(r["first_token_timeout_ms"], 0);
            assert_eq!(r["total_timeout_ms"], 0);
            assert_eq!(r["breaker_failures"], 0);
            assert_eq!(r["breaker_window_s"], 0);
            assert_eq!(r["breaker_open_s"], 0);
            assert_eq!(r["primaries"][0]["model"], "openai/a");
            assert_eq!(r["primaries"][0]["enabled"], true);
            assert_eq!(r["primaries"][0]["weight"], 0);
            assert_eq!(r["primaries"][0]["model_id"], 0);
            assert_eq!(r["fallbacks"][0]["model"], "openai/b");
            assert_eq!(r["broken"], false);
        }
    }

    // A single view is hidden the same way, and writes are the admin's.
    let priya = org.sign_in("priya").await;
    let (s, e) = org
        .call(
            Some(&priya),
            "GET",
            &format!("/api/routes/{}", ids[1]),
            None,
        )
        .await;
    assert_eq!((s, error_code(&e)), (StatusCode::NOT_FOUND, "not_found"));
    let (s, v) = org
        .call(
            Some(&priya),
            "GET",
            &format!("/api/routes/{}", ids[0]),
            None,
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["team_ids"], json!([]));
    for (meth, p, b) in [
        (
            "POST",
            "/api/routes".to_string(),
            Some(body("x", &[(m[0], 1)], &[], &[])),
        ),
        (
            "PUT",
            format!("/api/routes/{}", ids[0]),
            Some(body("x", &[(m[0], 1)], &[], &[])),
        ),
        ("DELETE", format!("/api/routes/{}", ids[0]), None),
    ] {
        let (s, e) = org.call(Some(&priya), meth, &p, b).await;
        assert_eq!(
            (s, error_code(&e)),
            (StatusCode::FORBIDDEN, "forbidden"),
            "{meth}"
        );
    }
    assert_eq!(routes(&org, &maya).await.len(), 4);
}

#[tokio::test]
async fn team_delete_closes_a_restricted_route_instead_of_opening_it() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let m = seed_models(&org, "openai", &["a"], true).await;
    let (_, v) = create(&org, &maya, body("r", &[(m[0], 1)], &[], &[org.research])).await;
    let id = v["id"].as_i64().unwrap();
    assert_eq!(v["everyone"], false);
    let (s, _) = org
        .call(
            Some(&maya),
            "DELETE",
            &format!("/api/teams/{}", org.research),
            None,
        )
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, v) = org
        .call(Some(&maya), "GET", &format!("/api/routes/{id}"), None)
        .await;
    assert_eq!(v["team_ids"], json!([]));
    assert_eq!(v["everyone"], false);
    for who in ["tomas", "lena", "priya"] {
        let signed = org.sign_in(who).await;
        assert!(routes(&org, &signed).await.is_empty(), "{who}");
        let (s, _) = org
            .call(Some(&signed), "GET", &format!("/api/routes/{id}"), None)
            .await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{who}");
    }
    assert_eq!(routes(&org, &maya).await.len(), 1);
}

#[tokio::test]
async fn everyone_cannot_be_combined_with_teams() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let m = seed_models(&org, "openai", &["a"], true).await;
    let mut b = body("r", &[(m[0], 1)], &[], &[org.platform]);
    b["everyone"] = json!(true);
    let (s, e) = create(&org, &maya, b).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        e["error"]["fields"]["everyone"],
        "must not be combined with teams or users"
    );
    // everyone is required.
    let mut b = body("r", &[(m[0], 1)], &[], &[]);
    b.as_object_mut().unwrap().remove("everyone");
    let (s, _) = create(&org, &maya, b).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    // Closed to everybody but admins is allowed.
    let mut b = body("closed", &[(m[0], 1)], &[], &[]);
    b["everyone"] = json!(false);
    let (s, v) = create(&org, &maya, b).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let lena = org.sign_in("lena").await;
    assert!(routes(&org, &lena).await.is_empty());
}

#[tokio::test]
async fn refused_put_leaves_targets_and_grants_unchanged() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let m = seed_models(&org, "openai", &["a", "b", "c"], true).await;
    let (_, v) = create(
        &org,
        &maya,
        body("keep", &[(m[0], 3)], &[m[1], m[2]], &[org.platform]),
    )
    .await;
    let id = v["id"].as_i64().unwrap();
    let (_, _) = create(&org, &maya, body("taken", &[(m[0], 1)], &[], &[])).await;
    let put = |b: Value| {
        let (org, maya) = (&org, &maya);
        async move {
            org.call(Some(maya), "PUT", &format!("/api/routes/{id}"), Some(b))
                .await
        }
    };
    let (s, _) = put(body("taken", &[(m[2], 9)], &[], &[org.research])).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _) = put(body("new", &[(m[2], 9)], &[], &[9999])).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    let (s, _) = put(body("new", &[(9999, 9)], &[], &[org.research])).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    let (_, after) = org
        .call(Some(&maya), "GET", &format!("/api/routes/{id}"), None)
        .await;
    assert_eq!(after, v);
}

#[tokio::test]
async fn provider_delete_removes_targets_and_breaks_the_route() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let gone = seed_models(&org, "gone", &["a"], true).await;
    let kept = seed_models(&org, "kept", &["b", "c", "d"], true).await;
    let (_, v) = create(
        &org,
        &maya,
        body("r", &[(gone[0], 1)], &[kept[0], kept[1], kept[2]], &[]),
    )
    .await;
    let id = v["id"].as_i64().unwrap();
    // Fallback order survives deleting a model in the middle.
    let (s, _) = org
        .call(
            Some(&maya),
            "DELETE",
            &format!("/api/models/{}", kept[1]),
            None,
        )
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, v) = org
        .call(Some(&maya), "GET", &format!("/api/routes/{id}"), None)
        .await;
    assert_eq!(v["fallbacks"][0]["model"], "kept/b");
    assert_eq!(v["fallbacks"][1]["model"], "kept/d");

    let p = org.api.store.list_providers().await.unwrap();
    let gone_id = p.iter().find(|p| p.name == "gone").unwrap().id;
    let (s, _) = org
        .call(
            Some(&maya),
            "DELETE",
            &format!("/api/providers/{gone_id}"),
            None,
        )
        .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, v) = org
        .call(Some(&maya), "GET", &format!("/api/routes/{id}"), None)
        .await;
    assert_eq!(v["primaries"], json!([]));
    assert_eq!(v["fallbacks"].as_array().unwrap().len(), 2);
    assert_eq!(v["broken"], false);
}
