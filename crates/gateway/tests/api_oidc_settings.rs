//! `GET` and `PUT /api/settings/oidc`, and `POST /api/settings/oidc/test`.

mod common;

use axum::http::StatusCode;
use common::{error_code, org, org_with_public_url, Org};
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SECRET: &str = "client-secret-never-shown-4f2a9c";

fn full(issuer: &str) -> Value {
    json!({
        "enabled": false,
        "label": "Test IdP",
        "issuer": issuer,
        "client_id": "gateway",
        "client_secret": SECRET,
        "scopes": "offline_access",
        "groups_claim": "roles",
        "admin_group": "gateway-admins",
        "link_by_email": true,
        "auto_create": true,
        "allowed_domains": ["Example.com", "corp.example.org"],
    })
}

async fn put(org: &Org, who: &common::Signed, body: Value) -> (StatusCode, Value) {
    org.call(Some(who), "PUT", "/api/settings/oidc", Some(body))
        .await
}

#[tokio::test]
async fn only_an_admin_reaches_the_oidc_settings() {
    let org = org().await;
    for (method, path, body) in [
        ("GET", "/api/settings/oidc", None),
        (
            "PUT",
            "/api/settings/oidc",
            Some(full("https://idp.example.com")),
        ),
        ("POST", "/api/settings/oidc/test", Some(json!({}))),
    ] {
        let (status, _) = org.call(None, method, path, body.clone()).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {path}");
        for name in ["arjun", "lena"] {
            let who = org.sign_in(name).await;
            let (status, body) = org.call(Some(&who), method, path, body.clone()).await;
            assert_eq!(
                status,
                StatusCode::FORBIDDEN,
                "{name} {method} {path}: {body}"
            );
        }
    }
    assert!(org
        .api
        .store
        .oidc_settings()
        .await
        .unwrap()
        .issuer
        .is_empty());
}

#[tokio::test]
async fn the_defaults_are_off_and_unconfigured() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let (status, body) = org
        .call(Some(&maya), "GET", "/api/settings/oidc", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body,
        json!({
            "enabled": false,
            "label": "SSO",
            "issuer": "",
            "client_id": "",
            "client_secret_set": false,
            "client_secret_unreadable": false,
            "scopes": "",
            "groups_claim": "groups",
            "admin_group": "",
            "link_by_email": true,
            "auto_create": false,
            "allowed_domains": [],
            "redirect_uri": null,
            "public_url_set": false,
        })
    );
}

#[tokio::test]
async fn the_redirect_uri_follows_the_public_url() {
    let org = org_with_public_url("https://gateway.example.com/").await;
    let maya = org.sign_in("maya").await;
    let (_, body) = org
        .call(Some(&maya), "GET", "/api/settings/oidc", None)
        .await;
    assert_eq!(body["public_url_set"], true);
    assert_eq!(
        body["redirect_uri"],
        "https://gateway.example.com/api/auth/oidc/callback"
    );
}

#[tokio::test]
async fn a_saved_configuration_reads_back_without_its_secret() {
    let org = org_with_public_url("https://gateway.example.com").await;
    let maya = org.sign_in("maya").await;
    let mut body = full("https://idp.example.com/realms/main");
    body["enabled"] = json!(true);
    let (status, saved) = put(&org, &maya, body).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    let (_, read) = org
        .call(Some(&maya), "GET", "/api/settings/oidc", None)
        .await;
    assert_eq!(saved, read);
    assert_eq!(read["enabled"], true);
    assert_eq!(read["label"], "Test IdP");
    assert_eq!(read["issuer"], "https://idp.example.com/realms/main");
    assert_eq!(read["client_id"], "gateway");
    assert_eq!(read["client_secret_set"], true);
    assert_eq!(read["scopes"], "offline_access");
    assert_eq!(read["groups_claim"], "roles");
    assert_eq!(read["admin_group"], "gateway-admins");
    assert_eq!(read["auto_create"], true);
    // Lower case, in the order given.
    assert_eq!(
        read["allowed_domains"],
        json!(["example.com", "corp.example.org"])
    );
    assert_eq!(
        org.audit_actions().await.last().unwrap(),
        "settings.oidc_update"
    );
}

#[tokio::test]
async fn the_client_secret_is_write_only_and_stored_encrypted() {
    let org = org_with_public_url("https://gateway.example.com").await;
    let maya = org.sign_in("maya").await;
    let (status, saved) = put(&org, &maya, full("https://idp.example.com")).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert!(!saved.to_string().contains(SECRET));
    assert!(!saved.as_object().unwrap().contains_key("client_secret"));

    let (_, read) = org
        .call(Some(&maya), "GET", "/api/settings/oidc", None)
        .await;
    assert!(!read.to_string().contains(SECRET));

    // Not in the audit log, in any field.
    for row in org.api.store.list_audit(200, None).await.unwrap() {
        assert!(!format!("{row:?}").contains(SECRET), "{row:?}");
    }
    // Not in the configuration export.
    let (status, export) = org
        .call(Some(&maya), "GET", "/api/config/export", None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!export.to_string().contains(SECRET));
    assert!(!export.to_string().contains("oidc"));

    // Stored as the hex of an encryption under the master key.
    let settings = org.api.store.oidc_settings().await.unwrap();
    let stored = settings.client_secret_enc.clone().expect("a stored secret");
    assert!(!stored.contains(SECRET));
    let plain = org
        .api
        .state
        .cipher
        .decrypt(&hex::decode(&stored).unwrap())
        .unwrap();
    assert_eq!(plain, SECRET.as_bytes());
    assert!(!format!("{settings:?}").contains(&stored));

    // A request without the secret keeps it; a new one replaces it.
    let mut body = full("https://idp.example.com");
    body.as_object_mut().unwrap().remove("client_secret");
    body["label"] = json!("Renamed");
    let (status, read) = put(&org, &maya, body).await;
    assert_eq!(status, StatusCode::OK, "{read}");
    assert_eq!(read["client_secret_set"], true);
    assert_eq!(
        org.api
            .store
            .oidc_settings()
            .await
            .unwrap()
            .client_secret_enc,
        Some(stored.clone())
    );
    let mut body = full("https://idp.example.com");
    body["client_secret"] = json!("another-secret-value");
    put(&org, &maya, body).await;
    let after = org.api.store.oidc_settings().await.unwrap();
    assert_ne!(after.client_secret_enc, Some(stored));
}

#[tokio::test]
async fn validation_names_the_field() {
    let org = org_with_public_url("https://gateway.example.com").await;
    let maya = org.sign_in("maya").await;
    let long = "x".repeat(3000);
    let table: Vec<(&str, Value, &str)> = vec![
        (
            "issuer is not a URL",
            json!({ "issuer": "not a url" }),
            "issuer",
        ),
        (
            "issuer over plain http",
            json!({ "issuer": "http://idp.example.com" }),
            "issuer",
        ),
        (
            "issuer on another scheme",
            json!({ "issuer": "ftp://idp.example.com" }),
            "issuer",
        ),
        (
            "issuer with credentials",
            json!({ "issuer": "https://u:p@idp.example.com" }),
            "issuer",
        ),
        (
            "issuer with a query",
            json!({ "issuer": "https://idp.example.com/?a=1" }),
            "issuer",
        ),
        (
            "issuer with a fragment",
            json!({ "issuer": "https://idp.example.com/#a" }),
            "issuer",
        ),
        (
            "issuer too long",
            json!({ "issuer": format!("https://{long}.example.com") }),
            "issuer",
        ),
        ("empty label", json!({ "label": "  " }), "label"),
        ("long label", json!({ "label": "x".repeat(41) }), "label"),
        ("long client id", json!({ "client_id": long }), "client_id"),
        (
            "empty secret",
            json!({ "client_secret": "" }),
            "client_secret",
        ),
        (
            "long secret",
            json!({ "client_secret": "x".repeat(4097) }),
            "client_secret",
        ),
        ("control in scopes", json!({ "scopes": "a\nb" }), "scopes"),
        ("long scopes", json!({ "scopes": long }), "scopes"),
        (
            "empty groups claim",
            json!({ "groups_claim": "" }),
            "groups_claim",
        ),
        (
            "long admin group",
            json!({ "admin_group": long }),
            "admin_group",
        ),
        (
            "domain with an at sign",
            json!({ "allowed_domains": ["a@example.com"] }),
            "allowed_domains",
        ),
        (
            "domain with a space",
            json!({ "allowed_domains": ["exa mple.com"] }),
            "allowed_domains",
        ),
        (
            "empty domain",
            json!({ "allowed_domains": [""] }),
            "allowed_domains",
        ),
        (
            "auto create without a domain",
            json!({ "auto_create": true, "allowed_domains": [] }),
            "allowed_domains",
        ),
        (
            "enabled without an issuer",
            json!({ "enabled": true, "issuer": "" }),
            "issuer",
        ),
        (
            "enabled without a client id",
            json!({ "enabled": true, "client_id": "" }),
            "client_id",
        ),
    ];
    for (what, change, field) in table {
        let mut body = full("https://idp.example.com");
        for (k, v) in change.as_object().unwrap() {
            body[k] = v.clone();
        }
        let (status, answer) = put(&org, &maya, body).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{what}: {answer}");
        assert_eq!(error_code(&answer), "validation_failed", "{what}");
        assert!(
            answer["error"]["fields"][field].is_string(),
            "{what}: {answer}"
        );
    }
    // Enabled without any secret.
    let mut body = full("https://idp.example.com");
    body["enabled"] = json!(true);
    body.as_object_mut().unwrap().remove("client_secret");
    let (status, answer) = put(&org, &maya, body).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
    assert!(
        answer["error"]["fields"]["client_secret"].is_string(),
        "{answer}"
    );
    // Nothing was written by any refusal.
    assert!(org
        .api
        .store
        .oidc_settings()
        .await
        .unwrap()
        .issuer
        .is_empty());
    assert!(!org
        .audit_actions()
        .await
        .contains(&"settings.oidc_update".to_string()));

    // Unknown fields are refused.
    let mut body = full("https://idp.example.com");
    body["extra"] = json!(1);
    let (status, _) = put(&org, &maya, body).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn http_is_allowed_for_the_loopback_only() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    for issuer in [
        "http://localhost:9000",
        "http://127.0.0.1:9000/realms/x",
        "http://[::1]:9000",
        "https://idp.example.com",
    ] {
        let (status, body) = put(&org, &maya, full(issuer)).await;
        assert_eq!(status, StatusCode::OK, "{issuer}: {body}");
    }
    for issuer in [
        "http://idp.example.com",
        "http://localhost.example.com",
        "http://127.0.0.2:9000",
        "http://10.0.0.1",
    ] {
        let (status, _) = put(&org, &maya, full(issuer)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{issuer}");
    }
}

#[tokio::test]
async fn enabling_needs_the_public_url() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let mut body = full("https://idp.example.com");
    body["enabled"] = json!(true);
    let (status, answer) = put(&org, &maya, body.clone()).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{answer}");
    assert_eq!(error_code(&answer), "validation_failed");
    assert!(answer["error"]["fields"]["enabled"].is_string(), "{answer}");
    assert!(!org.api.store.oidc_settings().await.unwrap().enabled);
    // The same settings, off, are saved.
    body["enabled"] = json!(false);
    let (status, _) = put(&org, &maya, body).await;
    assert_eq!(status, StatusCode::OK);
}

fn discovery(issuer: &str) -> Value {
    json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{issuer}/authorize"),
        "token_endpoint": format!("{issuer}/token"),
        "jwks_uri": format!("{issuer}/jwks"),
    })
}

async fn idp(discovery_body: impl FnOnce(&str) -> Value, keys: usize) -> MockServer {
    let server = MockServer::start().await;
    let issuer = server.uri();
    Mock::given(method("GET"))
        .and(path("/.well-known/openid-configuration"))
        .respond_with(ResponseTemplate::new(200).set_body_json(discovery_body(&issuer)))
        .mount(&server)
        .await;
    let keys: Vec<Value> = (0..keys)
        .map(|i| json!({ "kty": "RSA", "kid": i.to_string() }))
        .collect();
    Mock::given(method("GET"))
        .and(path("/jwks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "keys": keys })))
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn the_test_reads_discovery_and_the_keys() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let server = idp(discovery, 2).await;
    let issuer = server.uri();
    let (status, body) = org
        .call(
            Some(&maya),
            "POST",
            "/api/settings/oidc/test",
            Some(json!({ "issuer": issuer })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body,
        json!({
            "ok": true,
            "issuer": issuer,
            "authorization_endpoint": format!("{issuer}/authorize"),
            "token_endpoint": format!("{issuer}/token"),
            "jwks_keys": 2,
            "error": null,
        })
    );
    // Without an issuer in the request, the saved one is tested.
    let (status, _) = put(&org, &maya, full(&issuer)).await;
    assert_eq!(status, StatusCode::OK);
    let (_, body) = org
        .call(
            Some(&maya),
            "POST",
            "/api/settings/oidc/test",
            Some(json!({})),
        )
        .await;
    assert_eq!(body["ok"], true, "{body}");
    // Testing writes nothing.
    assert_eq!(
        org.audit_actions()
            .await
            .iter()
            .filter(|a| a.starts_with("settings."))
            .count(),
        1
    );
}

#[tokio::test]
async fn the_test_reports_what_is_wrong_without_failing() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    // The discovery document names another issuer.
    let mismatched = idp(|_| discovery("https://other.example.com"), 1).await;
    // No keys at all.
    let keyless = idp(discovery, 0).await;
    // Not found.
    let missing = MockServer::start().await;
    // Not JSON.
    let garbage = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>hi</html>"))
        .mount(&garbage)
        .await;
    // Moved: redirects are not followed.
    let moved = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", "http://127.0.0.1:1/x"))
        .mount(&moved)
        .await;
    for (what, issuer) in [
        ("mismatch", mismatched.uri()),
        ("no keys", keyless.uri()),
        ("not found", missing.uri()),
        ("not json", garbage.uri()),
        ("redirect", moved.uri()),
        ("refused", "http://127.0.0.1:1".to_string()),
    ] {
        let (status, body) = org
            .call(
                Some(&maya),
                "POST",
                "/api/settings/oidc/test",
                Some(json!({ "issuer": issuer })),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{what}: {body}");
        assert_eq!(body["ok"], false, "{what}: {body}");
        let error = body["error"]
            .as_str()
            .unwrap_or_else(|| panic!("{what}: {body}"));
        assert!(!error.is_empty());
        assert!(
            !error.contains(&issuer),
            "{what}: the message repeats the URL"
        );
        // What a provider claims to be is shown only once it is the issuer
        // that was entered.
        assert_eq!(
            body["issuer"].is_null(),
            what != "no keys",
            "{what}: {body}"
        );
    }
}

#[tokio::test]
async fn the_test_refuses_a_missing_or_unusable_issuer() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    for body in [
        json!({}),
        json!({ "issuer": "" }),
        json!({ "issuer": "http://idp.example.com" }),
        json!({ "issuer": "nonsense" }),
    ] {
        let (status, answer) = org
            .call(
                Some(&maya),
                "POST",
                "/api/settings/oidc/test",
                Some(body.clone()),
            )
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}: {answer}");
        assert!(answer["error"]["fields"]["issuer"].is_string(), "{answer}");
    }
}

#[tokio::test]
async fn a_secret_that_cannot_be_read_is_told_and_logged() {
    let org = org_with_public_url("https://gateway.example.com").await;
    let maya = org.sign_in("maya").await;
    let (status, saved) = put(&org, &maya, full("https://idp.example.com")).await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["client_secret_set"], true);
    assert_eq!(saved["client_secret_unreadable"], false);

    // The master key changed since: the stored value is not ours.
    let mut settings = org.api.store.oidc_settings().await.unwrap();
    let mut tx = org.api.store.begin().await.unwrap();
    settings.client_secret_enc = Some("00ff00ff00ff00ff00ff00ff00ff00ff00ff".to_string());
    tx.set_oidc_settings(&settings).await.unwrap();
    tx.commit().await.unwrap();
    let (_, read) = org
        .call(Some(&maya), "GET", "/api/settings/oidc", None)
        .await;
    assert_eq!(read["client_secret_set"], true);
    assert_eq!(read["client_secret_unreadable"], true);
    assert!(!read.to_string().contains("00ff00ff"));

    // A new secret makes it readable again.
    let mut body = full("https://idp.example.com");
    body["client_secret"] = json!("another-secret-value");
    let (_, saved) = put(&org, &maya, body).await;
    assert_eq!(saved["client_secret_unreadable"], false);
}
