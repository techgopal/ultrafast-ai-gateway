mod common;

use axum::http::StatusCode;
use common::{error_code, org, Org, Signed};
use serde_json::{json, Value};

const API_KEY: &str = "sk-provider-0123456789";
const NEW_API_KEY: &str = "sk-replacement-9876543210";
const BASE_URL: &str = "https://api.openai.com/v1";

/// The audit actions of this area, oldest first. Signing in is audited
/// too, and is left out.
async fn audited(org: &Org) -> Vec<String> {
    let all = org.audit_actions().await;
    all.into_iter()
        .filter(|a| a.starts_with("provider."))
        .collect()
}

fn provider_path(id: i64) -> String {
    format!("/api/providers/{id}")
}

fn openai(name: &str) -> Value {
    json!({ "name": name, "kind": "openai", "base_url": BASE_URL, "api_key": API_KEY })
}

async fn create(org: &Org, who: &Signed, body: Value) -> (StatusCode, Value) {
    org.call(Some(who), "POST", "/api/providers", Some(body))
        .await
}

/// Creates a provider as the admin and returns its id.
async fn seed(org: &Org, maya: &Signed, name: &str) -> i64 {
    let (status, body) = create(org, maya, openai(name)).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["id"].as_i64().unwrap()
}

async fn patch(org: &Org, who: &Signed, id: i64, body: Value) -> (StatusCode, Value) {
    org.call(Some(who), "PATCH", &provider_path(id), Some(body))
        .await
}

/// The stored credential, decrypted with the cipher of the gateway.
async fn stored_key(org: &Org, id: i64) -> Option<String> {
    let row = org.api.store.provider_by_id(id).await.unwrap().unwrap();
    row.credential.map(|bytes| {
        let plain = org.api.state.cipher.decrypt(&bytes).unwrap();
        String::from_utf8(plain).unwrap()
    })
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn assert_invalid(status: StatusCode, body: &Value, field: &str) {
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{field}: {body}");
    assert_eq!(error_code(body), "validation_failed");
    assert!(
        body["error"]["fields"][field].is_string(),
        "{field}: {body}"
    );
    let text = body.to_string();
    assert!(!text.contains(API_KEY), "{text}");
}

#[tokio::test]
async fn providers_need_a_sign_in() {
    let org = org().await;
    for (method, path) in [
        ("GET", "/api/providers"),
        ("POST", "/api/providers"),
        ("PATCH", "/api/providers/1"),
        ("DELETE", "/api/providers/1"),
    ] {
        let (status, body) = org.call(None, method, path, Some(openai("p"))).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {path}");
        assert_eq!(error_code(&body), "unauthenticated");
    }
    assert!(org.api.store.list_providers().await.unwrap().is_empty());
}

#[tokio::test]
async fn everyone_lists_providers_without_credentials() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    seed(&org, &maya, "openai").await;
    let bare =
        json!({ "name": "local", "kind": "openai", "base_url": "http://localhost:11434/v1" });
    assert_eq!(create(&org, &maya, bare).await.0, StatusCode::CREATED);

    let lena = org.sign_in("lena").await;
    let (status, body) = org.call(Some(&lena), "GET", "/api/providers", None).await;
    assert_eq!(status, StatusCode::OK);
    let list = body["providers"].as_array().unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(
        list[0],
        json!({
            "id": list[0]["id"], "name": "local", "kind": "openai",
            "base_url": "http://localhost:11434/v1", "has_credential": false
        })
    );
    assert_eq!(list[1]["name"], "openai");
    assert_eq!(list[1]["has_credential"], true);
    assert_eq!(list[1].as_object().unwrap().len(), 5);

    let text = body.to_string();
    assert!(!text.contains(API_KEY), "{text}");
    assert!(
        !text.replace("has_credential", "").contains("credential"),
        "{text}"
    );
    assert!(!text.contains("api_key"), "{text}");
}

#[tokio::test]
async fn only_admins_change_providers() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let id = seed(&org, &maya, "openai").await;
    let audit_before = audited(&org).await;

    for name in ["arjun", "lena"] {
        let who = org.sign_in(name).await;
        let (status, body) = create(&org, &who, openai("other")).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(error_code(&body), "forbidden");
        // Refused before the input is looked at.
        let (status, _) = create(
            &org,
            &who,
            json!({
                "name": "Not Valid", "kind": "gemini", "base_url": "x", "api_key": ""
            }),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = patch(&org, &who, id, json!({ "api_key": null })).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = patch(&org, &who, 9999, json!({ "api_key": null })).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = org
            .call(Some(&who), "DELETE", &provider_path(id), None)
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _) = org
            .call(Some(&who), "DELETE", &provider_path(9999), None)
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    assert_eq!(org.api.store.list_providers().await.unwrap().len(), 1);
    assert_eq!(stored_key(&org, id).await.as_deref(), Some(API_KEY));
    assert_eq!(audited(&org).await, audit_before);
}

#[tokio::test]
async fn creating_a_provider_encrypts_the_key() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let (status, body) = create(&org, &maya, openai("openai")).await;
    assert_eq!(status, StatusCode::CREATED);
    let id = body["id"].as_i64().unwrap();
    assert_eq!(
        body,
        json!({
            "id": id, "name": "openai", "kind": "openai",
            "base_url": BASE_URL, "has_credential": true
        })
    );

    let row = org.api.store.provider_by_id(id).await.unwrap().unwrap();
    let stored = row.credential.expect("a stored credential");
    assert!(!contains(&stored, API_KEY.as_bytes()));
    assert_eq!(stored_key(&org, id).await.as_deref(), Some(API_KEY));

    let bare =
        json!({ "name": "claude", "kind": "anthropic", "base_url": "https://api.anthropic.com" });
    let (status, body) = create(&org, &maya, bare).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["has_credential"], false);
    assert_eq!(body["kind"], "anthropic");
    let id = body["id"].as_i64().unwrap();
    assert_eq!(stored_key(&org, id).await, None);
}

#[tokio::test]
async fn provider_input_is_validated() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let with = |field: &str, value: Value| {
        let mut body = openai("openai");
        body[field] = value;
        body
    };
    let long = "a".repeat(41);
    for name in [
        "",
        "Open AI",
        "a/b",
        "-x",
        long.as_str(),
        "_x",
        "OPENAI",
        " openai",
    ] {
        let (status, body) = create(&org, &maya, with("name", json!(name))).await;
        assert_invalid(status, &body, "name");
    }
    for kind in ["gemini", "", "OpenAI"] {
        let (status, body) = create(&org, &maya, with("kind", json!(kind))).await;
        assert_invalid(status, &body, "kind");
    }
    for url in [
        "https://user:secret-in-url@api.openai.com/v1",
        "ftp://api.openai.com",
        "https://api.openai.com/v1?key=secret-in-url",
        "",
    ] {
        let (status, body) = create(&org, &maya, with("base_url", json!(url))).await;
        assert_invalid(status, &body, "base_url");
        assert!(!body.to_string().contains("secret-in-url"), "{body}");
    }
    for key in ["  ", "", "\n\t"] {
        let (status, body) = create(&org, &maya, with("api_key", json!(key))).await;
        assert_invalid(status, &body, "api_key");
        assert_eq!(body["error"]["fields"]["api_key"], "must not be empty");
    }

    // Every field that failed is named.
    let all = json!({ "name": "A B", "kind": "gemini", "base_url": "x", "api_key": " " });
    let (status, body) = create(&org, &maya, all).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["fields"].as_object().unwrap().len(), 4);

    for body in [
        json!({ "name": "openai", "kind": "openai" }),
        json!({ "name": "openai", "kind": "openai", "base_url": BASE_URL, "credential": "x" }),
        json!({ "name": "openai", "kind": "openai", "base_url": BASE_URL, "api_key": 5 }),
    ] {
        let (status, answer) = create(&org, &maya, body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(error_code(&answer), "bad_request");
    }

    assert!(org.api.store.list_providers().await.unwrap().is_empty());
    assert!(audited(&org).await.is_empty());

    let id = seed(&org, &maya, "openai").await;
    let (status, body) = patch(&org, &maya, id, json!({ "api_key": "  " })).await;
    assert_invalid(status, &body, "api_key");
    assert_eq!(body["error"]["fields"]["api_key"], "must not be empty");
    let bad_url = json!({ "base_url": "https://u:p@host" });
    let (status, body) = patch(&org, &maya, id, bad_url).await;
    assert_invalid(status, &body, "base_url");
    let (status, _) = patch(&org, &maya, id, json!({})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = patch(&org, &maya, id, json!({ "name": "renamed" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    assert_eq!(stored_key(&org, id).await.as_deref(), Some(API_KEY));
    assert_eq!(audited(&org).await, ["provider.create"]);
}

#[tokio::test]
async fn duplicate_provider_name_is_409() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let id = seed(&org, &maya, "openai").await;
    let mut again = openai("openai");
    again["api_key"] = json!(NEW_API_KEY);
    let (status, body) = create(&org, &maya, again).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&body), "provider_exists");
    assert!(!body.to_string().contains(NEW_API_KEY));
    assert_eq!(org.api.store.list_providers().await.unwrap().len(), 1);
    assert_eq!(stored_key(&org, id).await.as_deref(), Some(API_KEY));
    assert_eq!(audited(&org).await, ["provider.create"]);
}

#[tokio::test]
async fn patch_replaces_and_removes_the_key() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let id = seed(&org, &maya, "openai").await;

    let (status, body) = patch(&org, &maya, id, json!({ "api_key": NEW_API_KEY })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["has_credential"], true);
    assert_eq!(body["base_url"], BASE_URL);
    assert!(!body.to_string().contains(NEW_API_KEY));
    assert_eq!(stored_key(&org, id).await.as_deref(), Some(NEW_API_KEY));
    assert!(org
        .last_summary("provider.update")
        .await
        .contains("credential replaced"));

    let new_url = "https://proxy.example.com/v1";
    let (status, body) = patch(&org, &maya, id, json!({ "base_url": new_url })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["base_url"], new_url);
    assert_eq!(body["has_credential"], true);
    assert_eq!(stored_key(&org, id).await.as_deref(), Some(NEW_API_KEY));
    assert!(!org
        .last_summary("provider.update")
        .await
        .contains("credential"));

    let (status, body) = patch(&org, &maya, id, json!({ "api_key": null })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["has_credential"], false);
    assert_eq!(body["base_url"], new_url);
    assert_eq!(stored_key(&org, id).await, None);
    assert!(org
        .last_summary("provider.update")
        .await
        .contains("credential removed"));

    let (status, body) = patch(&org, &maya, id, json!({ "api_key": API_KEY })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["has_credential"], true);
    assert_eq!(stored_key(&org, id).await.as_deref(), Some(API_KEY));
    assert!(org
        .last_summary("provider.update")
        .await
        .contains("credential set"));
    assert_eq!(audited(&org).await.len(), 5);

    // Nothing to change, so nothing to record.
    let (status, _) = patch(&org, &maya, id, json!({ "base_url": new_url })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(audited(&org).await.len(), 5);

    let (status, _) = patch(&org, &maya, 9999, json!({ "api_key": null })).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn provider_audit_never_shows_the_key() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let id = seed(&org, &maya, "openai").await;
    patch(&org, &maya, id, json!({ "api_key": NEW_API_KEY })).await;
    patch(&org, &maya, id, json!({ "api_key": null })).await;
    patch(
        &org,
        &maya,
        id,
        json!({ "api_key": NEW_API_KEY, "base_url": "http://h" }),
    )
    .await;
    org.call(Some(&maya), "DELETE", &provider_path(id), None)
        .await;

    assert_eq!(
        audited(&org).await,
        [
            "provider.create",
            "provider.update",
            "provider.update",
            "provider.update",
            "provider.delete"
        ]
    );
    let (status, body) = org.call(Some(&maya), "GET", "/api/audit", None).await;
    assert_eq!(status, StatusCode::OK);
    let text = body.to_string();
    for secret in [API_KEY, NEW_API_KEY, "sk-"] {
        assert!(!text.contains(secret), "{text}");
    }
    for row in org.api.store.list_audit(200, None).await.unwrap() {
        assert!(!row.summary.contains("sk-"), "{}", row.summary);
        if !row.action.starts_with("provider.") {
            continue;
        }
        assert!(row.summary.contains("openai"), "{}", row.summary);
        assert_eq!(row.target_type, "provider");
        assert_eq!(row.target_id, Some(id));
        // Neither the value nor anything that tells its length.
        assert!(
            !row.summary.contains(&API_KEY.len().to_string()),
            "{}",
            row.summary
        );
        assert!(
            !row.summary.contains(&NEW_API_KEY.len().to_string()),
            "{}",
            row.summary
        );
    }
    assert!(org
        .last_summary("provider.create")
        .await
        .contains("credential set"));
}

#[tokio::test]
async fn deleting_a_provider() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let id = seed(&org, &maya, "openai").await;
    let keep = seed(&org, &maya, "other").await;

    let (status, body) = org
        .call(Some(&maya), "DELETE", &provider_path(id), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null);
    let (status, body) = org
        .call(Some(&maya), "DELETE", &provider_path(id), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(error_code(&body), "not_found");
    let (status, _) = org
        .call(Some(&maya), "DELETE", "/api/providers/abc", None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (_, body) = org.call(Some(&maya), "GET", "/api/providers", None).await;
    let list = body["providers"].as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"], keep);
    let deletes = audited(&org).await;
    assert_eq!(
        deletes.iter().filter(|a| *a == "provider.delete").count(),
        1
    );

    // The name is free again.
    assert_eq!(
        create(&org, &maya, openai("openai")).await.0,
        StatusCode::CREATED
    );
}

#[tokio::test]
async fn a_refusal_is_the_same_for_every_provider_id() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let id = seed(&org, &maya, "openai").await;
    let paths = [
        provider_path(id),
        provider_path(9999),
        "/api/providers/abc".to_string(),
    ];

    for name in ["arjun", "lena"] {
        let who = org.sign_in(name).await;
        for method in ["PATCH", "DELETE"] {
            let body = (method == "PATCH").then(|| json!({ "api_key": null }));
            let mut answers = Vec::new();
            for path in &paths {
                let answer = common::raw(&org, &who, method, path, body.clone()).await;
                assert_eq!(answer.0, StatusCode::FORBIDDEN, "{name} {method} {path}");
                answers.push(answer);
            }
            for answer in &answers[1..] {
                assert_eq!(
                    common::compared(answer),
                    common::compared(&answers[0]),
                    "{name} {method}"
                );
            }
        }
    }
    assert_eq!(stored_key(&org, id).await.as_deref(), Some(API_KEY));

    for path in &paths[1..] {
        let (status, _) = org
            .call(Some(&maya), "PATCH", path, Some(json!({ "api_key": null })))
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = org.call(Some(&maya), "DELETE", path, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
    let (status, _) = patch(&org, &maya, id, json!({ "api_key": null })).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = org.call(Some(&maya), "DELETE", &paths[0], None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn a_key_is_stored_without_surrounding_whitespace() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let mut body = openai("openai");
    body["api_key"] = json!("  sk-abc\n");
    let (status, body) = create(&org, &maya, body).await;
    assert_eq!(status, StatusCode::CREATED);
    let id = body["id"].as_i64().unwrap();
    assert_eq!(stored_key(&org, id).await.as_deref(), Some("sk-abc"));

    let (status, _) = patch(&org, &maya, id, json!({ "api_key": "\tsk-def \n" })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stored_key(&org, id).await.as_deref(), Some("sk-def"));
}
