mod common;

use axum::http::StatusCode;
use common::{error_code, org, Org, Signed};
use serde_json::{json, Value};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const API_KEY: &str = "sk-provider-0123456789";

/// Adds a provider whose credential is `API_KEY` and makes the gateway
/// see it. Returns its id.
async fn seed_provider(org: &Org, name: &str, kind: &str, base_url: &str) -> i64 {
    let credential = org.api.state.cipher.encrypt(API_KEY.as_bytes());
    let id = org
        .api
        .store
        .insert_provider(name, kind, base_url, Some(&credential))
        .await
        .unwrap();
    org.api.state.refresh().await.unwrap();
    id
}

async fn openai_lists(ids: &[&str]) -> MockServer {
    let server = MockServer::start().await;
    let data: Vec<Value> = ids
        .iter()
        .map(|id| json!({ "id": id, "object": "model" }))
        .collect();
    Mock::given(method("GET"))
        .and(path("/models"))
        .and(header(
            "authorization",
            format!("Bearer {API_KEY}").as_str(),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": data })))
        .mount(&server)
        .await;
    server
}

async fn sync(org: &Org, who: &Signed, provider: i64) -> (StatusCode, Value) {
    org.call(
        Some(who),
        "POST",
        &format!("/api/providers/{provider}/sync"),
        None,
    )
    .await
}

async fn models(org: &Org, who: &Signed) -> Vec<Value> {
    let (status, body) = org.call(Some(who), "GET", "/api/models", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["models"].as_array().unwrap().clone()
}

fn names(models: &[Value]) -> Vec<&str> {
    models.iter().map(|m| m["name"].as_str().unwrap()).collect()
}

async fn create(org: &Org, who: &Signed, provider: i64, name: &str) -> (StatusCode, Value) {
    org.call(
        Some(who),
        "POST",
        "/api/models",
        Some(json!({ "provider_id": provider, "name": name })),
    )
    .await
}

async fn put_grants(org: &Org, who: &Signed, id: i64, body: Value) -> (StatusCode, Value) {
    org.call(
        Some(who),
        "PUT",
        &format!("/api/models/{id}/grants"),
        Some(body),
    )
    .await
}

async fn enable(org: &Org, who: &Signed, id: i64, enabled: bool) -> (StatusCode, Value) {
    org.call(
        Some(who),
        "PATCH",
        &format!("/api/models/{id}"),
        Some(json!({ "enabled": enabled })),
    )
    .await
}

fn grants(everyone: bool, teams: &[i64], users: &[i64]) -> Value {
    json!({ "everyone": everyone, "team_ids": teams, "user_ids": users })
}

#[tokio::test]
async fn sync_adds_new_models_disabled() {
    let org = org().await;
    let maya = org.sign_in("maya").await;

    let openai = openai_lists(&["gpt-4o", "gpt-4o-mini"]).await;
    let a = seed_provider(&org, "openai", "openai", &openai.uri()).await;

    let anthropic = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("x-api-key", API_KEY))
        .and(header("anthropic-version", "2023-06-01"))
        .and(query_param("after_id", "claude-b"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "id": "claude-c", "type": "model" }],
            "has_more": false, "last_id": "claude-c",
        })))
        .mount(&anthropic)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("x-api-key", API_KEY))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "id": "claude-a" }, { "id": "claude-b" }],
            "has_more": true, "last_id": "claude-b",
        })))
        .mount(&anthropic)
        .await;
    let b = seed_provider(&org, "anthropic", "anthropic", &anthropic.uri()).await;

    let (status, body) = sync(&org, &maya, a).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body,
        json!({ "added": ["gpt-4o", "gpt-4o-mini"], "existing": 0 })
    );
    let (status, body) = sync(&org, &maya, b).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body,
        json!({ "added": ["claude-a", "claude-b", "claude-c"], "existing": 0 })
    );

    let all = models(&org, &maya).await;
    assert_eq!(all.len(), 5);
    for m in &all {
        assert_eq!(m["enabled"], false, "{m}");
        assert_eq!(m["grants"], grants(false, &[], &[]));
    }
    assert_eq!(all[0]["provider_name"], "anthropic");

    // Nothing is new the second time, and nothing is deleted.
    let (status, body) = sync(&org, &maya, a).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "added": [], "existing": 2 }));
    assert_eq!(models(&org, &maya).await.len(), 5);

    assert_eq!(
        org.last_summary("provider.sync").await,
        "Synced 0 new models from openai"
    );
    let summaries: Vec<String> = org
        .api
        .store
        .list_audit(50, None)
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.action == "provider.sync")
        .map(|r| r.summary)
        .collect();
    assert!(summaries.contains(&"Synced 3 new models from anthropic".to_string()));
    assert!(summaries.contains(&"Synced 2 new models from openai".to_string()));
    assert!(!summaries.concat().contains(API_KEY));
}

#[tokio::test]
async fn sync_keeps_names_verbatim() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let five = [
        "gpt-4o-2024-08-06",
        "models/gemini-2.0-flash",
        "meta-llama/Llama-3.3-70B",
        "llama3.2:3b",
        "claude-3-5-sonnet@20241022",
    ];
    let mut listed = five.to_vec();
    // Names that are not valid are left out, not mangled.
    listed.extend(["has space", "tab\there", ""]);
    let server = openai_lists(&listed).await;
    let id = seed_provider(&org, "openai", "openai", &server.uri()).await;

    let (status, body) = sync(&org, &maya, id).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let mut want = five.to_vec();
    want.sort_unstable();
    let mut added: Vec<&str> = body["added"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    added.sort_unstable();
    assert_eq!(added, want);

    let all = models(&org, &maya).await;
    let mut got = names(&all);
    got.sort_unstable();
    assert_eq!(got, want);

    // Each round-trips through grants and the single-model views.
    for m in &all {
        let model_id = m["id"].as_i64().unwrap();
        let (status, view) = put_grants(&org, &maya, model_id, grants(true, &[], &[])).await;
        assert_eq!(status, StatusCode::OK, "{view}");
        assert_eq!(view["name"], m["name"]);
    }
}

#[tokio::test]
async fn sync_upstream_error_is_502_without_body() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    for upstream in [500, 401, 404, 302] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/models"))
            .respond_with(
                ResponseTemplate::new(upstream).set_body_string(
                    "upstream-detail: the key sk-live-LEAK was refused for org-77",
                ),
            )
            .mount(&server)
            .await;
        let id = seed_provider(&org, &format!("p{upstream}"), "openai", &server.uri()).await;
        let (status, body) = sync(&org, &maya, id).await;
        assert_eq!(status, StatusCode::BAD_GATEWAY, "{upstream}: {body}");
        assert_eq!(error_code(&body), "sync_failed");
        assert_eq!(
            body["error"]["message"],
            "The provider did not return its models."
        );
        let text = body.to_string();
        assert!(!text.contains("LEAK") && !text.contains("org-77"), "{text}");
        assert!(!text.contains(API_KEY));
    }
    // Not JSON, wrong shape, too large, unreachable: all the same answer.
    let bad = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>nope</html>"))
        .mount(&bad)
        .await;
    let id = seed_provider(&org, "html", "openai", &bad.uri()).await;
    assert_eq!(sync(&org, &maya, id).await.0, StatusCode::BAD_GATEWAY);

    let huge = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_string("x".repeat(4 * 1024 * 1024 + 1)))
        .mount(&huge)
        .await;
    let id = seed_provider(&org, "huge", "openai", &huge.uri()).await;
    assert_eq!(sync(&org, &maya, id).await.0, StatusCode::BAD_GATEWAY);

    let id = seed_provider(&org, "down", "openai", "http://127.0.0.1:1").await;
    let (status, body) = sync(&org, &maya, id).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(error_code(&body), "sync_failed");
    assert!(models(&org, &maya).await.is_empty());

    let (status, body) = sync(&org, &maya, 9999).await;
    assert_eq!(
        (status, error_code(&body)),
        (StatusCode::NOT_FOUND, "not_found")
    );
}

#[tokio::test]
async fn create_update_grants_delete() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let p = seed_provider(&org, "openai", "openai", "https://api.openai.com/v1").await;

    let before = org.api.state.refresh_count();
    let (status, view) = create(&org, &maya, p, "gpt-4o").await;
    assert_eq!(status, StatusCode::CREATED, "{view}");
    assert_eq!(view["name"], "gpt-4o");
    assert_eq!(view["provider_id"], p);
    assert_eq!(view["provider_name"], "openai");
    assert_eq!(view["enabled"], false);
    assert_eq!(view["grants"], grants(false, &[], &[]));
    assert!(view["created_at"].is_string());
    let id = view["id"].as_i64().unwrap();

    let (status, body) = create(&org, &maya, p, "gpt-4o").await;
    assert_eq!(
        (status, error_code(&body)),
        (StatusCode::CONFLICT, "model_exists")
    );

    let (status, view) = enable(&org, &maya, id, true).await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["enabled"], true);

    let (status, view) = put_grants(
        &org,
        &maya,
        id,
        grants(false, &[org.platform, org.research], &[org.priya]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(
        view["grants"],
        grants(false, &[org.platform, org.research], &[org.priya])
    );
    let (_, view) = put_grants(&org, &maya, id, grants(true, &[], &[])).await;
    assert_eq!(view["grants"], grants(true, &[], &[]));
    // The put replaces, it does not add.
    let (_, view) = put_grants(&org, &maya, id, grants(false, &[org.growth], &[])).await;
    assert_eq!(view["grants"], grants(false, &[org.growth], &[]));
    assert_eq!(models(&org, &maya).await[0]["grants"], view["grants"]);

    let (status, _) = org
        .call(Some(&maya), "DELETE", &format!("/api/models/{id}"), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(models(&org, &maya).await.is_empty());
    let (status, body) = org
        .call(Some(&maya), "DELETE", &format!("/api/models/{id}"), None)
        .await;
    assert_eq!(
        (status, error_code(&body)),
        (StatusCode::NOT_FOUND, "not_found")
    );
    let (status, _) = enable(&org, &maya, id, true).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = put_grants(&org, &maya, id, grants(true, &[], &[])).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Every write is audited and published.
    let actions: Vec<String> = org
        .audit_actions()
        .await
        .into_iter()
        .filter(|a| a.starts_with("model."))
        .collect();
    assert_eq!(
        actions,
        [
            "model.create",
            "model.update",
            "model.grants",
            "model.grants",
            "model.grants",
            "model.delete"
        ]
    );
    assert_eq!(org.last_summary("model.update").await, "Enabled gpt-4o");
    assert!(org.api.state.refresh_count() >= before + 6);
    let (_, view) = create(&org, &maya, p, "gpt-4o").await;
    let id = view["id"].as_i64().unwrap();
    let (_, view) = enable(&org, &maya, id, false).await;
    assert_eq!(view["enabled"], false);
    assert_eq!(org.last_summary("model.update").await, "Disabled gpt-4o");
}

#[tokio::test]
async fn grants_validation() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let p = seed_provider(&org, "openai", "openai", "https://api.openai.com/v1").await;
    let (_, view) = create(&org, &maya, p, "gpt-4o").await;
    let id = view["id"].as_i64().unwrap();

    let (status, body) = put_grants(&org, &maya, id, grants(false, &[9999], &[])).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(error_code(&body), "validation_failed");
    assert!(body["error"]["fields"]["team_ids"].is_string());
    assert!(body["error"]["fields"]["user_ids"].is_null());

    let (status, body) = put_grants(&org, &maya, id, grants(false, &[org.platform], &[9999])).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["error"]["fields"]["user_ids"].is_string());
    assert!(body["error"]["fields"]["team_ids"].is_null());

    let (status, body) = put_grants(&org, &maya, id, grants(false, &[0, -1], &[0])).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["error"]["fields"]["team_ids"].is_string());
    assert!(body["error"]["fields"]["user_ids"].is_string());

    // Everyone cannot be combined with a team or a user.
    let (status, body) = put_grants(&org, &maya, id, grants(true, &[org.platform], &[])).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["error"]["fields"]["everyone"].is_string());

    // Nothing was stored by a refused call; repeats collapse.
    assert_eq!(
        models(&org, &maya).await[0]["grants"],
        grants(false, &[], &[])
    );
    let (status, view) = put_grants(
        &org,
        &maya,
        id,
        grants(false, &[org.platform, org.platform], &[org.lena, org.lena]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["grants"], grants(false, &[org.platform], &[org.lena]));

    // Body shape.
    let (status, body) = org
        .call(
            Some(&maya),
            "PUT",
            &format!("/api/models/{id}/grants"),
            Some(json!({ "everyone": true })),
        )
        .await;
    assert_eq!(
        (status, error_code(&body)),
        (StatusCode::BAD_REQUEST, "bad_request")
    );

    // Names.
    let bad = [
        "".to_string(),
        "has space".into(),
        " lead".into(),
        "tab\t".into(),
        "nl\n".into(),
        "nul\u{0}".into(),
        "x".repeat(201),
    ];
    for name in &bad {
        let (status, body) = create(&org, &maya, p, name).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{name:?}: {body}");
        assert!(body["error"]["fields"]["name"].is_string());
    }
    let (status, _) = create(&org, &maya, p, &"x".repeat(200)).await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, body) = create(&org, &maya, 9999, "m").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["error"]["fields"]["provider_id"].is_string());
}

#[tokio::test]
async fn non_admin_list_shows_only_callable() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let p = seed_provider(&org, "openai", "openai", "https://api.openai.com/v1").await;

    // lena: Platform member. tomas: Research. priya: no team.
    let mut ids = Vec::new();
    for name in [
        "everyone",
        "platform",
        "research",
        "priya-only",
        "disabled",
        "nobody",
    ] {
        let (_, view) = create(&org, &maya, p, name).await;
        let id = view["id"].as_i64().unwrap();
        if name != "disabled" {
            enable(&org, &maya, id, true).await;
        }
        ids.push(id);
    }
    put_grants(&org, &maya, ids[0], grants(true, &[], &[])).await;
    put_grants(&org, &maya, ids[1], grants(false, &[org.platform], &[])).await;
    put_grants(&org, &maya, ids[2], grants(false, &[org.research], &[])).await;
    put_grants(&org, &maya, ids[3], grants(false, &[], &[org.priya])).await;
    put_grants(&org, &maya, ids[4], grants(true, &[], &[])).await;

    assert_eq!(models(&org, &maya).await.len(), 6);

    let cases = [
        ("lena", vec!["everyone", "platform"]),
        ("tomas", vec!["everyone", "research"]),
        ("priya", vec!["everyone", "priya-only"]),
        ("arjun", vec!["everyone", "platform", "research"]),
    ];
    for (who, want) in cases {
        let signed = org.sign_in(who).await;
        let seen = models(&org, &signed).await;
        let mut got = names(&seen);
        got.sort_unstable();
        let mut want = want;
        want.sort_unstable();
        assert_eq!(got, want, "{who}");
        for m in &seen {
            // Always the one shape, empty for a non-admin.
            assert_eq!(m["grants"], grants(false, &[], &[]), "{who}: {m}");
        }
    }

    // Writes are the admin's.
    let lena = org.sign_in("lena").await;
    let arjun = org.sign_in("arjun").await;
    for who in [&lena, &arjun] {
        for (m, p, body) in [
            (
                "POST",
                "/api/models".to_string(),
                Some(json!({"provider_id": 1, "name": "x"})),
            ),
            (
                "PATCH",
                format!("/api/models/{}", ids[0]),
                Some(json!({"enabled": false})),
            ),
            (
                "PUT",
                format!("/api/models/{}/grants", ids[0]),
                Some(grants(true, &[], &[])),
            ),
            ("DELETE", format!("/api/models/{}", ids[0]), None),
            ("POST", "/api/providers/1/sync".to_string(), None),
        ] {
            let (status, body) = org.call(Some(who), m, &p, body).await;
            assert_eq!(
                (status, error_code(&body)),
                (StatusCode::FORBIDDEN, "forbidden"),
                "{m} {p}"
            );
        }
    }
    assert_eq!(models(&org, &maya).await.len(), 6);
}

#[tokio::test]
async fn provider_delete_cascades() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let a = seed_provider(&org, "alpha", "openai", "https://a.example.com/v1").await;
    let b = seed_provider(&org, "beta", "openai", "https://b.example.com/v1").await;
    let mut kept = 0;
    for (p, name) in [(a, "m1"), (a, "m2"), (a, "m3"), (b, "m1")] {
        let (_, view) = create(&org, &maya, p, name).await;
        let id = view["id"].as_i64().unwrap();
        put_grants(&org, &maya, id, grants(false, &[org.platform], &[org.lena])).await;
        if p == b {
            kept = id;
        }
    }

    let (status, _) = org
        .call(Some(&maya), "DELETE", &format!("/api/providers/{a}"), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let left = models(&org, &maya).await;
    assert_eq!(left.len(), 1);
    assert_eq!(left[0]["id"], kept);
    assert_eq!(left[0]["provider_name"], "beta");

    let rows: i64 = sqlx_count(&org, "model_grants").await;
    assert_eq!(rows, 2, "only the grants of the model that stayed");
    assert_eq!(
        org.last_summary("provider.delete").await,
        "Deleted provider alpha and its 3 models"
    );
}

async fn sqlx_count(org: &Org, table: &str) -> i64 {
    // The store keeps its SQL to itself; count through the API-visible
    // grants of every model instead of the table.
    assert_eq!(table, "model_grants");
    let all = org.api.store.list_model_grants().await.unwrap();
    all.len() as i64
}

#[tokio::test]
async fn sync_adds_at_most_ten_thousand_names() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let listed: Vec<String> = (0..10_050).map(|i| format!("m{i}")).collect();
    let refs: Vec<&str> = listed.iter().map(String::as_str).collect();
    let server = openai_lists(&refs).await;
    let id = seed_provider(&org, "big", "openai", &server.uri()).await;
    let (status, body) = sync(&org, &maya, id).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["added"].as_array().unwrap().len(), 10_000);
    assert_eq!(body["existing"], 0);
    assert_eq!(body["added"][0], "m0");
    assert_eq!(models(&org, &maya).await.len(), 10_000);
}
