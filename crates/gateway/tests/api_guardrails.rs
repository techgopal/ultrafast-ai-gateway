//! Guardrails over `/api`: definition and validation, who may, the secret of
//! an external one, attaching to routes and keys, the configuration file,
//! the snapshot and the test endpoint.

mod common;

use axum::http::StatusCode;
use common::{error_code, org, Org, Signed};
use serde_json::{json, Value};

const LIST: &str = "/api/guardrails";

fn path(id: i64) -> String {
    format!("{LIST}/{id}")
}

fn pii_email() -> Value {
    json!({ "id": "email", "matcher": { "pii": ["EMAIL"] }, "action": "redact", "directions": "both" })
}

fn rules_body(name: &str) -> Value {
    json!({ "name": name, "kind": "rules", "rules": [pii_email()] })
}

fn external_body(name: &str, url: &str) -> Value {
    json!({ "name": name, "kind": "external", "url": url })
}

async fn make(org: &Org, who: &Signed, body: Value) -> Value {
    let (status, v) = org.call(Some(who), "POST", LIST, Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{v}");
    v
}

async fn make_id(org: &Org, who: &Signed, name: &str) -> i64 {
    make(org, who, rules_body(name)).await["guardrail"]["id"]
        .as_i64()
        .unwrap()
}

fn assert_invalid(status: StatusCode, body: &Value, field: &str) {
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{field}: {body}");
    assert_eq!(error_code(body), "validation_failed");
    assert!(
        body["error"]["fields"][field].is_string(),
        "{field}: {body}"
    );
}

/// A model and a route over it, made through the store.
async fn seed_route(org: &Org, name: &str) -> i64 {
    let store = &org.api.store;
    let provider = store
        .insert_provider(
            &format!("p-{name}"),
            "openai",
            "https://x.example.com/v1",
            None,
        )
        .await
        .unwrap();
    let mut tx = store.begin().await.unwrap();
    let model = tx.insert_model(provider, "m").await.unwrap();
    tx.set_model_enabled(model, true).await.unwrap();
    tx.commit().await.unwrap();
    let (status, v) = org
        .call(
            Some(&org.sign_in("maya").await),
            "POST",
            "/api/routes",
            Some(route_body(name, model, None)),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{v}");
    v["id"].as_i64().unwrap()
}

fn route_body(name: &str, model: i64, guardrail_ids: Option<Vec<i64>>) -> Value {
    let mut b = json!({
        "name": name,
        "primaries": [{ "model_id": model, "weight": 1 }],
        "fallbacks": [],
        "retries": 2,
        "first_token_timeout_ms": 30000,
        "total_timeout_ms": 300000,
        "breaker_failures": 5,
        "breaker_window_s": 60,
        "breaker_open_s": 30,
        "everyone": true,
        "team_ids": [],
    });
    if let Some(ids) = guardrail_ids {
        b["guardrail_ids"] = json!(ids);
    }
    b
}

async fn model_of(org: &Org, route_id: i64) -> i64 {
    let (_, v) = org
        .call(
            Some(&org.sign_in("maya").await),
            "GET",
            &format!("/api/routes/{route_id}"),
            None,
        )
        .await;
    v["primaries"][0]["model_id"].as_i64().unwrap()
}

#[tokio::test]
async fn create_view_update_delete() {
    let org = org().await;
    let maya = org.sign_in("maya").await;

    let v = make(&org, &maya, rules_body("pii")).await;
    let g = &v["guardrail"];
    let id = g["id"].as_i64().unwrap();
    assert_eq!(g["name"], "pii");
    assert_eq!(g["description"], "");
    assert_eq!(g["kind"], "rules");
    assert_eq!(g["enabled"], true);
    assert_eq!(g["is_default"], false);
    assert_eq!(g["rules"], json!([pii_email()]));
    assert_eq!(g["routes"], json!([]));
    assert_eq!(g["key_count"], 0);
    assert!(g["url_host"].is_null() && g["fail_mode"].is_null());
    assert!(g["created_at"].is_string());
    assert!(v["secret"].is_null(), "a rules guardrail has no secret");

    let (status, list) = org.call(Some(&maya), "GET", LIST, None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list["guardrails"].as_array().unwrap().len(), 1);
    let (status, one) = org.call(Some(&maya), "GET", &path(id), None).await;
    assert_eq!(status, StatusCode::OK, "{one}");
    assert_eq!(one, *g);

    let keywords = json!([{
        "id": "secret-word",
        "matcher": { "keywords": { "words": ["Project Zed"], "whole_word": true } },
        "action": "block", "directions": "input",
    }]);
    let (status, after) = org
        .call(
            Some(&maya),
            "PATCH",
            &path(id),
            Some(json!({
                "name": "company", "description": "keeps secrets in", "enabled": false,
                "is_default": true, "rules": keywords,
            })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{after}");
    assert_eq!(after["name"], "company");
    assert_eq!(after["description"], "keeps secrets in");
    assert_eq!(after["enabled"], false);
    assert_eq!(after["is_default"], true);
    assert_eq!(after["rules"], keywords);

    // Only what is sent changes.
    let (_, after) = org
        .call(
            Some(&maya),
            "PATCH",
            &path(id),
            Some(json!({ "enabled": true })),
        )
        .await;
    assert_eq!(
        (after["name"].as_str(), after["is_default"].as_bool()),
        (Some("company"), Some(true))
    );
    assert_eq!(after["rules"], keywords);

    let (status, _) = org.call(Some(&maya), "DELETE", &path(id), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = org.call(Some(&maya), "GET", &path(id), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error_code(&body), "not_found");
    let (status, _) = org.call(Some(&maya), "DELETE", &path(id), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = org
        .call(
            Some(&maya),
            "PATCH",
            &path(id),
            Some(json!({ "enabled": true })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    assert_eq!(
        org.audit_actions().await,
        [
            "auth.login",
            "guardrail.create",
            "guardrail.update",
            "guardrail.update",
            "guardrail.delete"
        ]
    );
    assert_eq!(
        org.last_summary("guardrail.create").await,
        "Created guardrail pii (rules, 1 rule)"
    );
}

#[tokio::test]
async fn names_are_unique() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    make_id(&org, &maya, "pii").await;
    let other = make_id(&org, &maya, "other").await;
    let (status, body) = org
        .call(Some(&maya), "POST", LIST, Some(rules_body("pii")))
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(error_code(&body), "guardrail_exists");
    let (status, body) = org
        .call(
            Some(&maya),
            "PATCH",
            &path(other),
            Some(json!({ "name": "pii" })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(error_code(&body), "guardrail_exists");
}

#[tokio::test]
async fn bad_input_is_refused_with_the_field() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let rule = |matcher: Value| json!({ "id": "r", "matcher": matcher, "action": "block", "directions": "both" });
    let create = |body: Value| {
        let (org, maya) = (&org, &maya);
        async move { org.call(Some(maya), "POST", LIST, Some(body)).await }
    };

    // A regular expression that does not compile: named by its rule.
    let (status, body) = create(json!({
        "name": "g", "kind": "rules", "rules": [pii_email(), rule(json!({ "regex": "(unclosed" }))]
    }))
    .await;
    assert_invalid(status, &body, "rules[1]");
    let message = body["error"]["fields"]["rules[1]"].as_str().unwrap();
    assert!(message.contains("regular expression"), "{message}");
    // One that matches the empty string, and one too large to compile.
    let (status, body) =
        create(json!({ "name": "g", "kind": "rules", "rules": [rule(json!({ "regex": "a*" }))] }))
            .await;
    assert_invalid(status, &body, "rules[0]");
    let (status, body) = create(
        json!({ "name": "g", "kind": "rules", "rules": [rule(json!({ "regex": "(a{1000}){1000}" }))] }),
    )
    .await;
    assert_invalid(status, &body, "rules[0]");
    // An anchored one: a stream never sees the start or end of the whole text.
    for anchored in [
        "^secret",
        "secret$",
        r"\Asecret",
        r"secret\z",
        "(?m)^secret",
    ] {
        let (status, body) = create(
            json!({ "name": "g", "kind": "rules", "rules": [pii_email(), rule(json!({ "regex": anchored }))] }),
        )
        .await;
        assert_invalid(status, &body, "rules[1]");
        let message = body["error"]["fields"]["rules[1]"].as_str().unwrap();
        assert!(message.contains("anchor"), "{message}");
    }
    // Keywords: none, or an empty one.
    let (status, body) = create(json!({
        "name": "g", "kind": "rules",
        "rules": [rule(json!({ "keywords": { "words": [] } }))]
    }))
    .await;
    assert_invalid(status, &body, "rules[0]");
    let (status, body) = create(json!({
        "name": "g", "kind": "rules",
        "rules": [rule(json!({ "keywords": { "words": [""] } }))]
    }))
    .await;
    assert_invalid(status, &body, "rules[0]");
    // Rules as a set: a repeated id, too many, none at all.
    let (status, body) =
        create(json!({ "name": "g", "kind": "rules", "rules": [pii_email(), pii_email()] })).await;
    assert_invalid(status, &body, "rules");
    let many: Vec<Value> = (0..51)
        .map(|i| {
            json!({ "id": format!("r{i}"), "matcher": { "pii": ["EMAIL"] },
                    "action": "flag", "directions": "both" })
        })
        .collect();
    let (status, body) = create(json!({ "name": "g", "kind": "rules", "rules": many })).await;
    assert_invalid(status, &body, "rules");
    let (status, body) = create(json!({ "name": "g", "kind": "rules", "rules": [] })).await;
    assert_invalid(status, &body, "rules");
    let (status, body) = create(json!({ "name": "g", "kind": "rules" })).await;
    assert_invalid(status, &body, "rules");
    // Names, kinds, descriptions.
    let (status, body) =
        create(json!({ "name": " ", "kind": "rules", "rules": [pii_email()] })).await;
    assert_invalid(status, &body, "name");
    let (status, body) =
        create(json!({ "name": "g", "kind": "magic", "rules": [pii_email()] })).await;
    assert_invalid(status, &body, "kind");
    let (status, body) = create(json!({
        "name": "g", "kind": "rules", "rules": [pii_email()], "description": "x".repeat(501)
    }))
    .await;
    assert_invalid(status, &body, "description");
    // Fields of the other kind.
    let (status, body) = create(json!({
        "name": "g", "kind": "rules", "rules": [pii_email()], "url": "https://hook.example.com"
    }))
    .await;
    assert_invalid(status, &body, "url");
    let (status, body) = create(json!({
        "name": "g", "kind": "external", "url": "https://hook.example.com", "rules": [pii_email()]
    }))
    .await;
    assert_invalid(status, &body, "rules");
    // External: a URL is needed and must be valid; timeout and fail mode are checked.
    let (status, body) = create(json!({ "name": "g", "kind": "external" })).await;
    assert_invalid(status, &body, "url");
    let (status, body) = create(external_body("g", "not a url")).await;
    assert_invalid(status, &body, "url");
    let mut e = external_body("g", "https://hook.example.com");
    e["timeout_ms"] = json!(999);
    let (status, body) = create(e.clone()).await;
    assert_invalid(status, &body, "timeout_ms");
    e["timeout_ms"] = json!(10_001);
    let (status, body) = create(e.clone()).await;
    assert_invalid(status, &body, "timeout_ms");
    e["timeout_ms"] = json!(3000);
    e["fail_mode"] = json!("maybe");
    let (status, body) = create(e).await;
    assert_invalid(status, &body, "fail_mode");
    // A field the API does not know is a malformed request.
    let (status, body) = create(json!({
        "name": "g", "kind": "rules", "rules": [pii_email()], "colour": "red"
    }))
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    // A rule field the API does not know.
    let (status, _) = create(json!({
        "name": "g", "kind": "rules",
        "rules": [{ "id": "r", "matcher": { "pii": ["EMAIL"] }, "action": "block",
                    "directions": "both", "extra": 1 }]
    }))
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(org.api.store.list_guardrails().await.unwrap().is_empty());

    // Updates are checked the same way, and a bad one changes nothing.
    let id = make_id(&org, &maya, "ok").await;
    let (status, body) = org
        .call(
            Some(&maya),
            "PATCH",
            &path(id),
            Some(json!({ "rules": [rule(json!({ "regex": "(unclosed" }))] })),
        )
        .await;
    assert_invalid(status, &body, "rules[0]");
    let (status, body) = org
        .call(
            Some(&maya),
            "PATCH",
            &path(id),
            Some(json!({ "url": "https://hook.example.com" })),
        )
        .await;
    assert_invalid(status, &body, "url");
    let (status, body) = org
        .call(Some(&maya), "PATCH", &path(id), Some(json!({})))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let (_, now) = org.call(Some(&maya), "GET", &path(id), None).await;
    assert_eq!(now["rules"], json!([pii_email()]));
    let ext = make(
        &org,
        &maya,
        external_body("hook", "https://hook.example.com"),
    )
    .await["guardrail"]["id"]
        .as_i64()
        .unwrap();
    let (status, body) = org
        .call(
            Some(&maya),
            "PATCH",
            &path(ext),
            Some(json!({ "rules": [pii_email()] })),
        )
        .await;
    assert_invalid(status, &body, "rules");
    let (status, body) = org
        .call(
            Some(&maya),
            "PATCH",
            &path(ext),
            Some(json!({ "timeout_ms": 0 })),
        )
        .await;
    assert_invalid(status, &body, "timeout_ms");
}

#[tokio::test]
async fn only_an_admin_manages_guardrails() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let id = make_id(&org, &maya, "pii").await;
    for name in ["arjun", "lena", "priya"] {
        let who = org.sign_in(name).await;
        for (method, p, body) in [
            ("GET", LIST.to_string(), None),
            ("POST", LIST.to_string(), Some(rules_body("mine"))),
            ("GET", path(id), None),
            ("PATCH", path(id), Some(json!({ "enabled": false }))),
            ("DELETE", path(id), None),
            ("POST", format!("{}/rotate-secret", path(id)), None),
            (
                "POST",
                format!("{LIST}/test"),
                Some(json!({ "rules": [pii_email()], "direction": "input", "text": "a@b.co" })),
            ),
        ] {
            let (status, resp) = org.call(Some(&who), method, &p, body).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{name} {method} {p}: {resp}");
            assert_eq!(error_code(&resp), "forbidden");
        }
    }
    for (method, p) in [("GET", LIST.to_string()), ("DELETE", path(id))] {
        let (status, resp) = org.call(None, method, &p, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{resp}");
    }
    assert_eq!(org.api.store.list_guardrails().await.unwrap().len(), 1);
}

#[tokio::test]
async fn the_secret_of_an_external_guardrail_is_shown_once_and_the_url_never() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let url = "https://hooks.example.com:8443/check?token=hunter2";
    let v = make(&org, &maya, external_body("hook", url)).await;
    let secret = v["secret"].as_str().unwrap().to_string();
    assert!(
        secret.starts_with("whsec_") && secret.len() > 20,
        "{secret}"
    );
    let g = &v["guardrail"];
    let id = g["id"].as_i64().unwrap();
    assert_eq!(g["kind"], "external");
    assert_eq!(g["url_host"], "https://hooks.example.com:8443");
    assert_eq!(
        (g["timeout_ms"].as_i64(), g["fail_mode"].as_str()),
        (Some(3000), Some("open"))
    );
    assert_eq!(g["directions"], "both");
    assert_eq!(g["rules"], json!([]));

    let (_, one) = org.call(Some(&maya), "GET", &path(id), None).await;
    let (_, list) = org.call(Some(&maya), "GET", LIST, None).await;
    let (_, patched) = org
        .call(
            Some(&maya),
            "PATCH",
            &path(id),
            Some(json!({ "fail_mode": "closed", "timeout_ms": 1500, "directions": "output" })),
        )
        .await;
    assert_eq!(patched["fail_mode"], "closed");
    assert_eq!(patched["timeout_ms"], 1500);
    assert_eq!(patched["directions"], "output");
    for shown in [one.to_string(), list.to_string(), patched.to_string()] {
        for hidden in [&secret, "hunter2", "/check", "secret"] {
            assert!(!shown.contains(hidden), "{hidden} is shown: {shown}");
        }
    }

    // Stored encrypted: neither is in the row in the clear.
    let row = org.api.store.guardrail_by_id(id).await.unwrap().unwrap();
    let cipher = &org.api.state.cipher;
    let stored_url = row.url_enc.clone().unwrap();
    assert!(!String::from_utf8_lossy(&stored_url).contains("hooks.example.com"));
    assert_eq!(cipher.decrypt(&stored_url).unwrap(), url.as_bytes());
    assert_eq!(
        cipher.decrypt(row.secret_enc.as_deref().unwrap()).unwrap(),
        secret.as_bytes()
    );
    assert!(!format!("{row:?}").contains("hunter2"));

    // A new URL replaces the old; a rotation makes another secret, shown once.
    let (status, moved) = org
        .call(
            Some(&maya),
            "PATCH",
            &path(id),
            Some(json!({ "url": "https://other.example.com/x" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{moved}");
    assert_eq!(moved["url_host"], "https://other.example.com");
    let (status, rotated) = org
        .call(
            Some(&maya),
            "POST",
            &format!("{}/rotate-secret", path(id)),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{rotated}");
    let second = rotated["secret"].as_str().unwrap();
    assert!(second.starts_with("whsec_") && second != secret);
    let row = org.api.store.guardrail_by_id(id).await.unwrap().unwrap();
    assert_eq!(
        cipher.decrypt(row.secret_enc.as_deref().unwrap()).unwrap(),
        second.as_bytes()
    );

    // Nothing of either is in the audit log.
    let audit: String = org
        .api
        .store
        .list_audit(100, None)
        .await
        .unwrap()
        .iter()
        .map(|a| a.summary.clone())
        .collect();
    for hidden in [&secret, second, "hunter2", "/check"] {
        assert!(
            !audit.contains(hidden),
            "{hidden} in the audit log: {audit}"
        );
    }

    // A rules guardrail has no secret to rotate.
    let plain = make_id(&org, &maya, "plain").await;
    let (status, body) = org
        .call(
            Some(&maya),
            "POST",
            &format!("{}/rotate-secret", path(plain)),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let (status, _) = org
        .call(
            Some(&maya),
            "POST",
            &format!("{}/rotate-secret", path(9999)),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn guardrails_attach_to_routes_in_order() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let a = make_id(&org, &maya, "a").await;
    let b = make_id(&org, &maya, "b").await;
    let route = seed_route(&org, "chat").await;
    let model = model_of(&org, route).await;

    let (_, v) = org
        .call(Some(&maya), "GET", &format!("/api/routes/{route}"), None)
        .await;
    assert_eq!(v["guardrails"], json!([]));

    // Duplicates collapse; the order given is kept.
    let (status, v) = org
        .call(
            Some(&maya),
            "PUT",
            &format!("/api/routes/{route}"),
            Some(route_body("chat", model, Some(vec![b, a, b]))),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(
        v["guardrails"],
        json!([{ "id": b, "name": "b" }, { "id": a, "name": "a" }])
    );
    assert!(org
        .audit_actions()
        .await
        .contains(&"guardrail.attach".to_string()));
    assert_eq!(
        org.last_summary("guardrail.attach").await,
        "Set the guardrails of route chat to b, a"
    );

    // Left out, they stay; an empty list takes them off.
    let (_, v) = org
        .call(
            Some(&maya),
            "PUT",
            &format!("/api/routes/{route}"),
            Some(route_body("chat", model, None)),
        )
        .await;
    assert_eq!(v["guardrails"].as_array().unwrap().len(), 2);
    // The guardrail lists where it is attached.
    let (_, g) = org.call(Some(&maya), "GET", &path(a), None).await;
    assert_eq!(g["routes"], json!([{ "id": route, "name": "chat" }]));

    // An id that does not exist is refused and nothing changes.
    let (status, body) = org
        .call(
            Some(&maya),
            "PUT",
            &format!("/api/routes/{route}"),
            Some(route_body("chat", model, Some(vec![a, 9999]))),
        )
        .await;
    assert_invalid(status, &body, "guardrail_ids");
    let (_, v) = org
        .call(Some(&maya), "GET", &format!("/api/routes/{route}"), None)
        .await;
    assert_eq!(v["guardrails"].as_array().unwrap().len(), 2);
    let too_many: Vec<i64> = (0..21).collect();
    let (status, body) = org
        .call(
            Some(&maya),
            "PUT",
            &format!("/api/routes/{route}"),
            Some(route_body("chat", model, Some(too_many))),
        )
        .await;
    assert_invalid(status, &body, "guardrail_ids");

    // Deleting a guardrail takes it off; a new route can start with some.
    org.call(Some(&maya), "DELETE", &path(b), None).await;
    let (_, v) = org
        .call(Some(&maya), "GET", &format!("/api/routes/{route}"), None)
        .await;
    assert_eq!(v["guardrails"], json!([{ "id": a, "name": "a" }]));
    let (status, v) = org
        .call(
            Some(&maya),
            "POST",
            "/api/routes",
            Some(route_body("second", model + 1000, Some(vec![a]))),
        )
        .await;
    // (the model does not exist: still refused as a whole)
    assert_invalid(status, &v, "primaries");
    let (status, v) = org
        .call(
            Some(&maya),
            "POST",
            "/api/routes",
            Some(route_body("second", model, Some(vec![a]))),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{v}");
    assert_eq!(v["guardrails"], json!([{ "id": a, "name": "a" }]));

    // The list shows them to an admin; others do not see which are attached.
    let (_, list) = org.call(Some(&maya), "GET", "/api/routes", None).await;
    assert!(list["routes"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["guardrails"].as_array().unwrap().len() == 1));
    let lena = org.sign_in("lena").await;
    let (_, list) = org.call(Some(&lena), "GET", "/api/routes", None).await;
    assert!(list["routes"]
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["guardrails"] == json!([])));
}

#[tokio::test]
async fn guardrails_attach_to_keys_for_admins_and_lenders_see_them() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let arjun = org.sign_in("arjun").await;
    let lena = org.sign_in("lena").await;
    let a = make_id(&org, &maya, "a").await;
    let b = make_id(&org, &maya, "b").await;

    // Admin: on create, in order.
    let (status, v) = org
        .call(
            Some(&maya),
            "POST",
            "/api/keys",
            Some(
                json!({ "name": "k1", "owner_id": org.lena, "team_id": org.platform,
                         "guardrail_ids": [b, a, b] }),
            ),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{v}");
    let key = v["key"]["id"].as_i64().unwrap();
    let expected = json!([{ "id": b, "name": "b" }, { "id": a, "name": "a" }]);
    assert_eq!(v["key"]["guardrails"], expected);
    assert_eq!(
        org.last_summary("guardrail.attach").await,
        "Set the guardrails of key k1 to b, a"
    );

    // A lead of the team sees them on the key, and so does its owner.
    for who in [&arjun, &lena] {
        let (status, got) = org
            .call(Some(who), "GET", &format!("/api/keys/{key}"), None)
            .await;
        assert_eq!(status, StatusCode::OK, "{got}");
        assert_eq!(got["guardrails"], expected);
        let (_, list) = org.call(Some(who), "GET", "/api/keys", None).await;
        let listed = list["keys"]
            .as_array()
            .unwrap()
            .iter()
            .find(|k| k["id"] == key)
            .unwrap();
        assert_eq!(listed["guardrails"], expected);
    }
    let (_, g) = org.call(Some(&maya), "GET", &path(a), None).await;
    assert_eq!(g["key_count"], 1);

    // A lead who sends the field is refused, even empty; without it they make a key.
    for ids in [json!([a]), json!([])] {
        let (status, body) = org
            .call(
                Some(&arjun),
                "POST",
                "/api/keys",
                Some(
                    json!({ "name": "k2", "owner_id": org.lena, "team_id": org.platform,
                             "guardrail_ids": ids }),
                ),
            )
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert_eq!(error_code(&body), "forbidden");
    }
    let (status, own) = org
        .call(
            Some(&lena),
            "POST",
            "/api/keys",
            Some(json!({ "name": "mine", "guardrail_ids": [a] })),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{own}");
    let (status, own) = org
        .call(
            Some(&lena),
            "POST",
            "/api/keys",
            Some(json!({ "name": "mine" })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{own}");
    assert_eq!(own["key"]["guardrails"], json!([]));
    assert_eq!(org.api.store.list_keys().await.unwrap().len(), 2);

    // An id that does not exist is refused, and no key is made.
    let (status, body) = org
        .call(
            Some(&maya),
            "POST",
            "/api/keys",
            Some(json!({ "name": "k3", "guardrail_ids": [9999] })),
        )
        .await;
    assert_invalid(status, &body, "guardrail_ids");
    assert_eq!(org.api.store.list_keys().await.unwrap().len(), 2);

    // PATCH: the admin changes them; the tags stay when only guardrails are sent.
    let (status, _) = org
        .call(
            Some(&maya),
            "PATCH",
            &format!("/api/keys/{key}"),
            Some(json!({ "tags": { "team": "platform" } })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, got) = org
        .call(
            Some(&maya),
            "PATCH",
            &format!("/api/keys/{key}"),
            Some(json!({ "guardrail_ids": [a] })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{got}");
    assert_eq!(got["guardrails"], json!([{ "id": a, "name": "a" }]));
    assert_eq!(got["tags"], json!({ "team": "platform" }));
    let (_, got) = org
        .call(
            Some(&maya),
            "PATCH",
            &format!("/api/keys/{key}"),
            Some(json!({ "tags": {} })),
        )
        .await;
    assert_eq!(got["guardrails"], json!([{ "id": a, "name": "a" }]));
    let (_, got) = org
        .call(
            Some(&maya),
            "PATCH",
            &format!("/api/keys/{key}"),
            Some(json!({ "guardrail_ids": [] })),
        )
        .await;
    assert_eq!(got["guardrails"], json!([]));
    let (status, body) = org
        .call(
            Some(&maya),
            "PATCH",
            &format!("/api/keys/{key}"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let (status, body) = org
        .call(
            Some(&maya),
            "PATCH",
            &format!("/api/keys/{key}"),
            Some(json!({ "guardrail_ids": [9999] })),
        )
        .await;
    assert_invalid(status, &body, "guardrail_ids");
    for who in [&arjun, &lena] {
        let (status, body) = org
            .call(
                Some(who),
                "PATCH",
                &format!("/api/keys/{key}"),
                Some(json!({ "guardrail_ids": [b] })),
            )
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    }
    // A guardrail that is deleted leaves the key.
    org.call(
        Some(&maya),
        "PATCH",
        &format!("/api/keys/{key}"),
        Some(json!({ "guardrail_ids": [a, b] })),
    )
    .await;
    org.call(Some(&maya), "DELETE", &path(a), None).await;
    let (_, got) = org
        .call(Some(&maya), "GET", &format!("/api/keys/{key}"), None)
        .await;
    assert_eq!(got["guardrails"], json!([{ "id": b, "name": "b" }]));
}

#[tokio::test]
async fn the_snapshot_holds_what_is_enabled_and_its_fingerprint_follows_every_edit() {
    use ultrafast_gateway::snapshot::Snapshot;

    let org = org().await;
    let maya = org.sign_in("maya").await;
    let state = &org.api.state;
    let fingerprint = || state.snapshot.load().cache_fingerprint();
    let empty = fingerprint();

    let a = make_id(&org, &maya, "a").await;
    let after_create = fingerprint();
    assert_ne!(after_create, empty, "a new guardrail");
    // A load from scratch gives the same fingerprint as the refreshed one.
    let loaded = Snapshot::load(&org.api.store, &state.cipher).await.unwrap();
    assert_eq!(loaded.cache_fingerprint(), after_create);

    let patch = |body: Value| {
        let (org, maya) = (&org, &maya);
        async move {
            let (status, v) = org.call(Some(maya), "PATCH", &path(a), Some(body)).await;
            assert_eq!(status, StatusCode::OK, "{v}");
        }
    };
    let mut seen = vec![empty, after_create];
    for body in [
        json!({ "rules": [{ "id": "x", "matcher": { "regex": "foo" }, "action": "block", "directions": "input" }] }),
        json!({ "enabled": false }),
        json!({ "enabled": true }),
        json!({ "is_default": true }),
        json!({ "name": "renamed" }),
    ] {
        let before = fingerprint();
        patch(body.clone()).await;
        let after = fingerprint();
        assert_ne!(before, after, "{body}");
        if body["enabled"] != json!(true) {
            assert!(
                !seen.contains(&after),
                "{body} came back to an earlier state"
            );
        }
        seen.push(after);
    }
    // Saving what is already there changes nothing.
    let before = fingerprint();
    patch(json!({ "name": "renamed" })).await;
    assert_eq!(fingerprint(), before);

    // Attaching to a route and to a key counts too.
    let route = seed_route(&org, "chat").await;
    let model = model_of(&org, route).await;
    let before = fingerprint();
    org.call(
        Some(&maya),
        "PUT",
        &format!("/api/routes/{route}"),
        Some(route_body("chat", model, Some(vec![a]))),
    )
    .await;
    assert_ne!(fingerprint(), before, "attached to a route");
    let (_, k) = org
        .call(
            Some(&maya),
            "POST",
            "/api/keys",
            Some(json!({ "name": "k" })),
        )
        .await;
    let key = k["key"]["id"].as_i64().unwrap();
    let before = fingerprint();
    org.call(
        Some(&maya),
        "PATCH",
        &format!("/api/keys/{key}"),
        Some(json!({ "guardrail_ids": [a] })),
    )
    .await;
    assert_ne!(fingerprint(), before, "attached to a key");
    let before = fingerprint();
    org.call(Some(&maya), "DELETE", &path(a), None).await;
    assert_ne!(fingerprint(), before, "deleted");
}

#[tokio::test]
async fn the_test_endpoint_runs_the_engine_and_never_echoes_a_match() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let test = |body: Value| {
        let (org, maya) = (&org, &maya);
        async move {
            org.call(Some(maya), "POST", "/api/guardrails/test", Some(body))
                .await
        }
    };

    // Rules sent with the request.
    let (status, v) = test(json!({
        "rules": [pii_email()], "direction": "input", "text": "write to ann@example.com now"
    }))
    .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["redacted_text"], "write to [REDACTED:EMAIL] now");
    assert_eq!(v["outcome"]["redactions"], json!({ "EMAIL": 1 }));
    assert!(v["outcome"]["blocked_by"].is_null());
    assert_eq!(v["outcome"]["flags"], json!([]));
    assert!(!v["outcome"].to_string().contains("ann@"));

    // Block: the text is left alone.
    let (_, v) = test(json!({
        "rules": [{ "id": "no", "matcher": { "keywords": { "words": ["zed"] } },
                    "action": "block", "directions": "both" }],
        "direction": "output", "text": "the Zed project",
    }))
    .await;
    assert_eq!(v["outcome"]["blocked_by"]["name"], "Test rules");
    assert_eq!(v["redacted_text"], "the Zed project");

    // Flag.
    let (_, v) = test(json!({
        "rules": [{ "id": "mail", "matcher": { "pii": ["EMAIL"] }, "action": "flag", "directions": "both" }],
        "direction": "input", "text": "a@b.co",
    }))
    .await;
    assert_eq!(
        v["outcome"]["flags"],
        json!([{ "guardrail_id": 0, "guardrail_name": "Test rules", "rule_id": "mail" }])
    );
    assert_eq!(v["redacted_text"], "a@b.co");

    // A stored guardrail, by id; its rules apply only to their directions.
    let output_only = json!([{ "id": "o", "matcher": { "pii": ["EMAIL"] }, "action": "redact", "directions": "output" }]);
    let (_, created) = org
        .call(
            Some(&maya),
            "POST",
            LIST,
            Some(json!({ "name": "stored", "kind": "rules", "rules": output_only, "enabled": false })),
        )
        .await;
    let id = created["guardrail"]["id"].as_i64().unwrap();
    let (status, v) =
        test(json!({ "guardrail_id": id, "direction": "output", "text": "a@b.co" })).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["redacted_text"], "[REDACTED:EMAIL]");
    assert_eq!(v["outcome"]["blocked_by"], Value::Null);
    let (_, v) = test(json!({ "guardrail_id": id, "direction": "input", "text": "a@b.co" })).await;
    assert_eq!(v["redacted_text"], "a@b.co");
    assert_eq!(v["outcome"]["redactions"], json!({}));

    // Exactly one of rules and guardrail_id; known ids; valid rules and directions.
    let (status, v) = test(json!({ "direction": "input", "text": "x" })).await;
    assert_invalid(status, &v, "rules");
    let (status, v) = test(json!({
        "rules": [pii_email()], "guardrail_id": id, "direction": "input", "text": "x"
    }))
    .await;
    assert_invalid(status, &v, "guardrail_id");
    let (status, v) =
        test(json!({ "guardrail_id": 9999, "direction": "input", "text": "x" })).await;
    assert_invalid(status, &v, "guardrail_id");
    let (status, v) = test(json!({
        "rules": [{ "id": "r", "matcher": { "regex": "(" }, "action": "block", "directions": "both" }],
        "direction": "input", "text": "x"
    }))
    .await;
    assert_invalid(status, &v, "rules[0]");
    let (status, v) = test(json!({
        "rules": [{ "id": "r", "matcher": { "regex": "^x" }, "action": "block", "directions": "both" }],
        "direction": "input", "text": "x"
    }))
    .await;
    assert_invalid(status, &v, "rules[0]");
    let (status, _) =
        test(json!({ "rules": [pii_email()], "direction": "sideways", "text": "x" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // An external guardrail is never called by the test unless asked, and
    // the asking is not available yet.
    let (_, ext) = org
        .call(
            Some(&maya),
            "POST",
            LIST,
            Some(external_body("hook", "http://127.0.0.1:1/x")),
        )
        .await;
    let ext = ext["guardrail"]["id"].as_i64().unwrap();
    let (status, v) = test(json!({ "guardrail_id": ext, "direction": "input", "text": "x" })).await;
    assert_invalid(status, &v, "guardrail_id");
    let (status, v) = test(json!({
        "guardrail_id": ext, "direction": "input", "text": "x", "call_external": true
    }))
    .await;
    assert_invalid(status, &v, "call_external");
    // The test changes nothing and is not audited.
    assert!(!org.audit_actions().await.iter().any(|a| a.contains("test")));
}

#[tokio::test]
async fn external_guardrails_need_a_url_before_they_are_enabled() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let v = make(
        &org,
        &maya,
        json!({ "name": "hook", "kind": "external", "url": "https://hook.example.com", "enabled": false }),
    )
    .await;
    assert_eq!(v["guardrail"]["enabled"], false);
    // A row as an import leaves it: no URL, off.
    let mut tx = org.api.store.begin().await.unwrap();
    let cipher = &org.api.state.cipher;
    let id = tx
        .insert_guardrail(ultrafast_gateway::store::NewGuardrail {
            name: "imported",
            description: "",
            kind: "external",
            rules: "[]",
            url: Some((&cipher.encrypt(b""), "")),
            secret_enc: Some(&cipher.encrypt(b"whsec_x")),
            timeout_ms: 3000,
            fail_mode: "open",
            directions: "both",
            enabled: false,
            is_default: false,
        })
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let (_, g) = org.call(Some(&maya), "GET", &path(id), None).await;
    assert_eq!(g["url_host"], "");
    let (status, body) = org
        .call(
            Some(&maya),
            "PATCH",
            &path(id),
            Some(json!({ "enabled": true })),
        )
        .await;
    assert_invalid(status, &body, "enabled");
    let (status, body) = org
        .call(
            Some(&maya),
            "PATCH",
            &path(id),
            Some(json!({ "url": "https://hook.example.com/a", "enabled": true })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        (body["enabled"].as_bool(), body["url_host"].as_str()),
        (Some(true), Some("https://hook.example.com"))
    );
}

// ------------------------------------------------------------ configuration

async fn export(org: &Org, who: &Signed) -> Value {
    let (status, v) = org.call(Some(who), "GET", "/api/config/export", None).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    v
}

async fn import(org: &Org, who: &Signed, file: Value, dry_run: bool) -> (StatusCode, Value) {
    org.call(
        Some(who),
        "POST",
        &format!("/api/config/import?dry_run={dry_run}"),
        Some(file),
    )
    .await
}

#[tokio::test]
async fn the_export_holds_guardrails_without_their_url_or_secret_and_the_import_restores_them() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let url = "https://hooks.example.com/check?token=hunter2";
    let rules = make(
        &org,
        &maya,
        json!({ "name": "pii", "kind": "rules", "description": "mail", "rules": [pii_email()],
                "is_default": true }),
    )
    .await;
    let ext = make(
        &org,
        &maya,
        json!({ "name": "hook", "kind": "external", "url": url, "timeout_ms": 2500,
                "fail_mode": "closed", "directions": "input" }),
    )
    .await;
    let secret = ext["secret"].as_str().unwrap().to_string();
    let route = seed_route(&org, "chat").await;
    let model = model_of(&org, route).await;
    let ids = [
        ext["guardrail"]["id"].as_i64().unwrap(),
        rules["guardrail"]["id"].as_i64().unwrap(),
    ];
    org.call(
        Some(&maya),
        "PUT",
        &format!("/api/routes/{route}"),
        Some(route_body("chat", model, Some(ids.to_vec()))),
    )
    .await;

    let first = export(&org, &maya).await;
    let text = first.to_string();
    for hidden in ["hunter2", "hooks.example.com", &secret, "whsec_"] {
        assert!(!text.contains(hidden), "the export holds {hidden}");
    }
    assert_eq!(
        first["guardrails"],
        json!([
            { "name": "hook", "description": "", "kind": "external", "enabled": true,
              "is_default": false, "rules": [],
              "external": { "timeout_ms": 2500, "fail_mode": "closed", "directions": "input" } },
            { "name": "pii", "description": "mail", "kind": "rules", "enabled": true,
              "is_default": true, "rules": [pii_email()] },
        ])
    );
    let chat = first["routes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "chat")
        .unwrap();
    assert_eq!(chat["guardrails"], json!(["hook", "pii"]));

    // Into a fresh gateway (a dry run first writes nothing).
    let fresh = common::org().await;
    let fresh_maya = fresh.sign_in("maya").await;
    let (status, dry) = import(&fresh, &fresh_maya, first.clone(), true).await;
    assert_eq!(status, StatusCode::OK, "{dry}");
    let created: Vec<(&str, &str)> = dry["created"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| (i["kind"].as_str().unwrap(), i["name"].as_str().unwrap()))
        .collect();
    assert!(created.contains(&("guardrail", "pii")) && created.contains(&("guardrail", "hook")));
    assert!(fresh.api.store.list_guardrails().await.unwrap().is_empty());
    let warnings = dry["warnings"].to_string();
    assert!(
        warnings.contains("hook") && warnings.contains("URL"),
        "{warnings}"
    );

    let (status, done) = import(&fresh, &fresh_maya, first.clone(), false).await;
    assert_eq!(status, StatusCode::OK, "{done}");
    assert!(done["errors"].as_array().unwrap().is_empty(), "{done}");
    let (_, list) = fresh.call(Some(&fresh_maya), "GET", LIST, None).await;
    let by_name = |n: &str| {
        list["guardrails"]
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["name"] == n)
            .unwrap()
            .clone()
    };
    // The rules guardrail is as it was; the external one is off and has no URL.
    assert_eq!(by_name("pii")["rules"], json!([pii_email()]));
    assert_eq!(by_name("pii")["is_default"], true);
    assert_eq!(by_name("pii")["enabled"], true);
    let hook = by_name("hook");
    assert_eq!(hook["enabled"], false);
    assert_eq!(hook["url_host"], "");
    assert_eq!(
        (
            hook["timeout_ms"].as_i64(),
            hook["fail_mode"].as_str(),
            hook["directions"].as_str()
        ),
        (Some(2500), Some("closed"), Some("input"))
    );
    let (_, routes) = fresh
        .call(Some(&fresh_maya), "GET", "/api/routes", None)
        .await;
    let route = routes["routes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "chat")
        .unwrap();
    assert_eq!(
        route["guardrails"],
        json!([{ "id": hook["id"], "name": "hook" }, { "id": by_name("pii")["id"], "name": "pii" }])
    );
    // It can be given a URL and then enabled; it has a secret to show.
    let (status, rotated) = fresh
        .call(
            Some(&fresh_maya),
            "POST",
            &format!("{}/rotate-secret", path(hook["id"].as_i64().unwrap())),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{rotated}");

    // The export of the copy is the export of the original, except that the
    // external guardrail is off until it has a URL.
    let mut second = export(&fresh, &fresh_maya).await;
    assert_eq!(second["guardrails"][0]["enabled"], false);
    second["guardrails"][0]["enabled"] = json!(true);
    assert_eq!(second["guardrails"], first["guardrails"]);
    assert_eq!(second["routes"], first["routes"]);

    // A second import changes nothing, and audits only the summary.
    let (_, again) = import(&fresh, &fresh_maya, first.clone(), false).await;
    assert!(again["created"].as_array().unwrap().is_empty(), "{again}");
    assert!(again["updated"].as_array().unwrap().is_empty(), "{again}");
    let actions = fresh.audit_actions().await;
    assert_eq!(
        actions.iter().filter(|a| *a == "guardrail.import").count(),
        2
    );

    // Updating an existing guardrail from a file; an external one is never
    // switched on by a file while it has no URL, and its URL is not touched.
    let mut changed = first.clone();
    changed["guardrails"][1]["description"] = json!("changed");
    changed["guardrails"][1]["rules"] = json!([]);
    changed["guardrails"][1]["rules"] = json!([{ "id": "k", "matcher": { "keywords": { "words": ["zed"], "whole_word": false } },
                                                "action": "flag", "directions": "output" }]);
    changed["guardrails"][0]["external"]["timeout_ms"] = json!(4000);
    changed["guardrails"][0]["enabled"] = json!(true);
    let (_, report) = import(&fresh, &fresh_maya, changed, false).await;
    assert!(report["errors"].as_array().unwrap().is_empty(), "{report}");
    assert_eq!(report["updated"].as_array().unwrap().len(), 2, "{report}");
    let (_, hook) = fresh
        .call(
            Some(&fresh_maya),
            "GET",
            &path(hook["id"].as_i64().unwrap()),
            None,
        )
        .await;
    assert_eq!(
        (hook["timeout_ms"].as_i64(), hook["enabled"].as_bool()),
        (Some(4000), Some(false))
    );
}

#[tokio::test]
async fn an_import_is_checked_before_anything_is_written() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let file = |guardrails: Value| json!({ "format": "ultrafast-config", "version": 1, "guardrails": guardrails });
    let errors = |v: &Value| -> Vec<String> {
        v["errors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["at"].as_str().unwrap().to_string())
            .collect()
    };
    let bad_regex = json!({ "name": "g", "kind": "rules", "enabled": true, "is_default": false,
        "rules": [{ "id": "r", "matcher": { "regex": "(" }, "action": "block", "directions": "both" }] });
    let (status, v) = import(&org, &maya, file(json!([bad_regex])), false).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{v}");
    assert_eq!(errors(&v), ["guardrails[0].rules[0]"]);
    let ok = json!({ "name": "g", "kind": "rules", "enabled": true, "is_default": false,
        "rules": [pii_email()] });
    let (_, v) = import(&org, &maya, file(json!([ok, ok])), false).await;
    assert_eq!(errors(&v), ["guardrails[1]"]);
    let mut wrong = ok.clone();
    wrong["kind"] = json!("magic");
    let (_, v) = import(&org, &maya, file(json!([wrong])), false).await;
    assert_eq!(errors(&v), ["guardrails[0].kind"]);
    let mut no_rules = ok.clone();
    no_rules["rules"] = json!([]);
    let (_, v) = import(&org, &maya, file(json!([no_rules])), false).await;
    assert_eq!(errors(&v), ["guardrails[0].rules"]);
    let mut external_with_rules = ok.clone();
    external_with_rules["kind"] = json!("external");
    let (_, v) = import(&org, &maya, file(json!([external_with_rules])), false).await;
    assert_eq!(errors(&v), ["guardrails[0].rules"]);
    // A route that names a guardrail the file and the gateway lack.
    let mut with_route = file(json!([]));
    let store = &org.api.store;
    let provider = store
        .insert_provider("p", "openai", "https://x.example.com/v1", None)
        .await
        .unwrap();
    let mut tx = store.begin().await.unwrap();
    tx.insert_model(provider, "m").await.unwrap();
    tx.commit().await.unwrap();
    with_route["routes"] = json!([{
        "name": "r", "primaries": [{ "model": "p/m", "weight": 1 }], "fallbacks": [],
        "retries": 1, "first_token_timeout_ms": 20000, "total_timeout_ms": 120000,
        "breaker_failures": 4, "breaker_window_s": 30, "breaker_open_s": 15,
        "everyone": true, "teams": [], "cache_enabled": false, "cache_ttl_s": 300,
        "cache_scope": "team", "guardrails": ["missing"],
    }]);
    let (_, v) = import(&org, &maya, with_route.clone(), false).await;
    assert_eq!(errors(&v), ["routes[0].guardrails[0]"]);
    // Named in the file: fine.
    with_route["guardrails"] = json!([ok]);
    with_route["routes"][0]["guardrails"] = json!(["g"]);
    let (status, v) = import(&org, &maya, with_route.clone(), false).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    // A file that does not say leaves what is attached alone.
    with_route["routes"][0]
        .as_object_mut()
        .unwrap()
        .remove("guardrails");
    let (_, v) = import(&org, &maya, with_route, false).await;
    assert!(v["updated"].as_array().unwrap().is_empty(), "{v}");
    let (_, routes) = org.call(Some(&maya), "GET", "/api/routes", None).await;
    assert_eq!(
        routes["routes"][0]["guardrails"].as_array().unwrap().len(),
        1
    );
    // An existing guardrail of another kind is not changed into it.
    let mut other = ok.clone();
    other["kind"] = json!("external");
    other["rules"] = json!([]);
    other["external"] = json!({ "timeout_ms": 3000, "fail_mode": "open", "directions": "both" });
    let (_, v) = import(&org, &maya, file(json!([other])), false).await;
    assert_eq!(errors(&v), ["guardrails[0].kind"]);
}

#[tokio::test]
async fn the_snapshot_holds_the_enabled_guardrails_in_the_order_calls_use_them() {
    use ultrafast_gateway::guardrails::{check_texts, Direction};
    use ultrafast_gateway::snapshot::Snapshot;

    let org = org().await;
    let maya = org.sign_in("maya").await;
    let named = |name: &str, extra: Value| {
        let mut b = rules_body(name);
        for (k, v) in extra.as_object().unwrap() {
            b[k] = v.clone();
        }
        b
    };
    let a = make(&org, &maya, named("a", json!({ "is_default": true }))).await;
    let b = make(&org, &maya, named("b", json!({ "is_default": true }))).await;
    let off = make(
        &org,
        &maya,
        named("off", json!({ "enabled": false, "is_default": true })),
    )
    .await;
    let d = make(&org, &maya, named("d", json!({}))).await;
    let e = make(&org, &maya, named("e", json!({}))).await;
    let hook_url = "https://hooks.example.com/check?token=hunter2";
    let ext = make(&org, &maya, external_body("hook", hook_url)).await;
    let id = |v: &Value| v["guardrail"]["id"].as_i64().unwrap();
    let (a, b, off, d, e, ext) = (id(&a), id(&b), id(&off), id(&d), id(&e), id(&ext));

    let route = seed_route(&org, "chat").await;
    let model = model_of(&org, route).await;
    let (status, _) = org
        .call(
            Some(&maya),
            "PUT",
            &format!("/api/routes/{route}"),
            Some(route_body("chat", model, Some(vec![d, off, ext]))),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (_, k) = org
        .call(
            Some(&maya),
            "POST",
            "/api/keys",
            Some(json!({ "name": "k", "guardrail_ids": [e, d, a] })),
        )
        .await;
    let secret = k["secret"].as_str().unwrap().to_string();

    // A row that does not compile is left out; the load still succeeds.
    let mut tx = org.api.store.begin().await.unwrap();
    tx.insert_guardrail(ultrafast_gateway::store::NewGuardrail {
        name: "broken",
        description: "",
        kind: "rules",
        rules: "not json",
        url: None,
        secret_enc: None,
        timeout_ms: 3000,
        fail_mode: "open",
        directions: "both",
        enabled: true,
        is_default: true,
    })
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let snap = Snapshot::load(&org.api.store, &org.api.state.cipher)
        .await
        .unwrap();
    assert_eq!(
        snap.default_guardrails(),
        [a, b],
        "enabled defaults, by name"
    );
    assert!(snap.guardrail(off).is_none(), "a disabled one is not held");
    assert!(snap.guardrail(a).unwrap().rules.is_some());
    let route = snap.route("chat").unwrap();
    assert_eq!(route.guardrails, [d, off, ext], "as attached, in order");
    let key = snap
        .key(
            &ultrafast_gateway::secrets::hash_key(&secret),
            "2000-01-01 00:00:00",
        )
        .unwrap();
    assert_eq!(key.guardrails, [e, d, a]);

    // Defaults first, then the route's, then the key's; each once.
    let effective: Vec<i64> = snap
        .effective_guardrails(Some(route), Some(key))
        .iter()
        .map(|g| g.id)
        .collect();
    assert_eq!(effective, [a, b, d, ext, e]);
    let without_key: Vec<i64> = snap
        .effective_guardrails(Some(route), None)
        .iter()
        .map(|g| g.id)
        .collect();
    assert_eq!(without_key, [a, b, d, ext]);
    let neither: Vec<i64> = snap
        .effective_guardrails(None, None)
        .iter()
        .map(|g| g.id)
        .collect();
    assert_eq!(neither, [a, b]);

    // The compiled rules run, and the external one holds what it needs but
    // shows none of it.
    let set: Vec<_> = snap
        .effective_guardrails(None, None)
        .iter()
        .filter_map(|g| g.rules.clone())
        .collect();
    let mut texts = [String::from("mail ann@example.com")];
    let outcome = check_texts(&set, Direction::Input, &mut texts);
    assert_eq!(texts[0], "mail [REDACTED:EMAIL]");
    assert_eq!(outcome.redactions.get("EMAIL"), Some(&1));
    let external = snap.guardrail(ext).unwrap();
    let x = external.external.as_ref().unwrap();
    assert_eq!((x.url.as_str(), x.fail_open), (hook_url, true));
    assert!(x.secret.starts_with("whsec_"));
    let shown = format!("{external:?}");
    assert!(
        !shown.contains("hunter2") && !shown.contains(&x.secret),
        "{shown}"
    );
}
