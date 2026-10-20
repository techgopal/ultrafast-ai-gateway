//! Registering an MCP server that signs in with OAuth, through the admin API.

mod common;

use axum::http::StatusCode;
use common::{org_tweaked, Org, Signed};
use serde_json::{json, Value};
use ufaigw_gateway::portable::{self, Actor, ConfigFile};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SECRET: &str = "oauth-client-secret-77ab";

async fn world() -> (Org, Signed) {
    let org = org_tweaked(None, |s| s.mcp.allow_private_upstreams = true).await;
    let maya = org.sign_in("maya").await;
    (org, maya)
}

async fn create(org: &Org, who: &Signed, body: Value) -> (StatusCode, Value) {
    org.call(Some(who), "POST", "/api/mcp/servers", Some(body))
        .await
}

async fn patch(org: &Org, who: &Signed, id: i64, body: Value) -> (StatusCode, Value) {
    org.call(
        Some(who),
        "PATCH",
        &format!("/api/mcp/servers/{id}"),
        Some(body),
    )
    .await
}

fn oauth(extra: Value) -> Value {
    let mut body = json!({
        "name": "api", "url": "https://mcp.example.com/mcp", "auth_kind": "oauth",
        "oauth_token_url": "https://idp.example.com/token", "oauth_client_id": "gateway",
        "oauth_scopes": "mcp:read", "secret": SECRET,
    });
    for (k, v) in extra.as_object().unwrap() {
        body[k] = v.clone();
    }
    body
}

fn actor() -> Actor<'static> {
    Actor {
        user_id: None,
        email: "cli",
        cipher: None,
    }
}

#[tokio::test]
async fn an_oauth_server_is_created_without_ever_showing_its_secret() {
    let (org, maya) = world().await;
    let (status, body) = create(&org, &maya, oauth(json!({}))).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["auth_kind"], "oauth");
    assert_eq!(body["oauth_token_url"], "https://idp.example.com/token");
    assert_eq!(body["oauth_client_id"], "gateway");
    assert_eq!(body["oauth_scopes"], "mcp:read");
    assert_eq!(body["oauth_grant"], "client_credentials");
    assert_eq!(body["has_secret"], true);
    assert_eq!(body["usable"], true);
    assert!(!body.to_string().contains(SECRET));
    assert!(!org.last_summary("mcp.create").await.contains(SECRET));
    let id = body["id"].as_i64().unwrap();
    let row = org.api.store.server_by_id(id).await.unwrap().unwrap();
    assert!(row.is_oauth() && row.auth_kind == "none" && row.auth_secret_enc.is_none());
    assert!(!row.oauth_client_secret_enc.unwrap().contains(SECRET));
    // It is in the snapshot with its configuration, and the key is not part of it.
    let snap = org.api.state.snapshot.load();
    let server = snap.mcp_server("api").unwrap();
    assert!(server.usable && server.oauth.is_some() && server.secret.is_none());
    // The list view says the same.
    let (_, list) = org.call(Some(&maya), "GET", "/api/mcp/servers", None).await;
    assert_eq!(list["servers"][0]["auth_kind"], "oauth");
}

#[tokio::test]
async fn an_oauth_server_is_checked_field_by_field() {
    let (org, maya) = world().await;
    for (extra, field) in [
        (json!({"oauth_token_url": null}), "oauth_token_url"),
        (json!({"oauth_token_url": "ftp://x"}), "oauth_token_url"),
        (json!({"oauth_client_id": ""}), "oauth_client_id"),
        (json!({"oauth_grant": "password"}), "oauth_grant"),
        (json!({"oauth_scopes": "a\nb"}), "oauth_scopes"),
        (json!({"secret": null}), "secret"),
        (json!({"auth_header": "x-key"}), "auth_header"),
    ] {
        let (status, body) = create(&org, &maya, oauth(extra.clone())).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{extra} {body}");
        assert!(
            body["error"]["fields"][field].is_string(),
            "{extra}: {body}"
        );
    }
    // oauth fields without oauth.
    let (status, body) = create(
        &org,
        &maya,
        json!({"name": "x", "url": "https://mcp.example.com/mcp", "oauth_client_id": "a"}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
}

#[tokio::test]
async fn the_token_url_must_be_https_and_not_private_unless_private_addresses_are_allowed() {
    let org = org_tweaked(None, |_| {}).await;
    let maya = org.sign_in("maya").await;
    for url in [
        "http://idp.example.com/token",
        "https://127.0.0.1/token",
        "https://10.0.0.1/token",
    ] {
        let (status, body) = create(&org, &maya, oauth(json!({"oauth_token_url": url}))).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{url}: {body}");
        assert!(body["error"]["fields"]["oauth_token_url"].is_string());
    }
    let (status, _) = create(&org, &maya, oauth(json!({}))).await;
    assert_eq!(status, StatusCode::CREATED);
}

#[tokio::test]
async fn the_token_url_is_checked_against_the_authorization_servers_the_server_names() {
    let mcp = MockServer::start().await;
    let idp = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/.well-known/oauth-protected-resource/mcp"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "resource": format!("{}/mcp", mcp.uri()),
            "authorization_servers": [idp.uri()],
        })))
        .mount(&mcp)
        .await;
    let (org, maya) = world().await;
    let url = format!("{}/mcp", mcp.uri());
    // On a named authorization server: accepted.
    let (status, body) = create(
        &org,
        &maya,
        oauth(
            json!({"name": "named", "url": url, "oauth_token_url": format!("{}/token", idp.uri())}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    // On another host: refused, with the reason.
    let (status, body) = create(
        &org,
        &maya,
        oauth(json!({"name": "other", "url": url, "oauth_token_url": "https://evil.example.com/token"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["error"]["fields"]["oauth_token_url"]
        .as_str()
        .unwrap()
        .contains("authorization server"));
    // A server that says nothing is not held to anything.
    let (status, _) = create(&org, &maya, oauth(json!({"name": "silent"}))).await;
    assert_eq!(status, StatusCode::CREATED);
    // Moving the token URL of an existing server is checked the same way.
    let id = body_id(&org, &maya, "named").await;
    let (status, body) = patch(
        &org,
        &maya,
        id,
        json!({"oauth_token_url": "https://evil.example.com/token", "secret": SECRET}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
}

async fn body_id(org: &Org, who: &Signed, name: &str) -> i64 {
    let (_, list) = org.call(Some(who), "GET", "/api/mcp/servers", None).await;
    list["servers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == name)
        .unwrap()["id"]
        .as_i64()
        .unwrap()
}

#[tokio::test]
async fn editing_an_oauth_server_keeps_its_secret_until_the_token_host_moves() {
    let (org, maya) = world().await;
    let (_, made) = create(&org, &maya, oauth(json!({}))).await;
    let id = made["id"].as_i64().unwrap();
    let before = org.api.store.server_by_id(id).await.unwrap().unwrap();
    // Scopes, grant and description change; the secret stays.
    let (status, view) = patch(
        &org,
        &maya,
        id,
        json!({"oauth_scopes": "mcp:read mcp:write", "oauth_grant": "token_exchange", "description": "d"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["oauth_grant"], "token_exchange");
    assert_eq!(view["oauth_scopes"], "mcp:read mcp:write");
    let after = org.api.store.server_by_id(id).await.unwrap().unwrap();
    assert_eq!(
        after.oauth_client_secret_enc,
        before.oauth_client_secret_enc
    );
    let summary = org.last_summary("mcp.update").await;
    assert!(
        summary.contains("scopes changed") && summary.contains("grant changed"),
        "{summary}"
    );
    assert!(!summary.contains(SECRET));
    // Empty scopes remove them.
    let (_, view) = patch(&org, &maya, id, json!({"oauth_scopes": ""})).await;
    assert!(view["oauth_scopes"].is_null());
    // A token endpoint on another host needs the secret again.
    let (status, body) = patch(
        &org,
        &maya,
        id,
        json!({"oauth_token_url": "https://other-idp.example.com/token"}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["error"]["fields"]["secret"].is_string());
    let (status, _) = patch(
        &org,
        &maya,
        id,
        json!({"oauth_token_url": "https://other-idp.example.com/token", "secret": "new-secret"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // A cached token goes with a change.
    org.api
        .store
        .put_mcp_token(id, "enc", "2999-01-01 00:00:00")
        .await
        .unwrap();
    let (status, _) = patch(&org, &maya, id, json!({"oauth_client_id": "other"})).await;
    assert_eq!(status, StatusCode::OK);
    assert!(org.api.store.mcp_token(id).await.unwrap().is_none());
}

#[tokio::test]
async fn a_server_changes_to_oauth_and_back() {
    let (org, maya) = world().await;
    let (_, made) = create(
        &org,
        &maya,
        json!({"name": "web", "url": "https://mcp.example.com/mcp"}),
    )
    .await;
    let id = made["id"].as_i64().unwrap();
    // To oauth: everything is needed.
    let (status, body) = patch(&org, &maya, id, json!({"auth_kind": "oauth"})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    for field in ["oauth_token_url", "oauth_client_id", "secret"] {
        assert!(
            body["error"]["fields"][field].is_string(),
            "{field}: {body}"
        );
    }
    let (status, view) = patch(
        &org,
        &maya,
        id,
        json!({"auth_kind": "oauth", "oauth_token_url": "https://idp.example.com/token",
               "oauth_client_id": "gateway", "secret": SECRET}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["auth_kind"], "oauth");
    // Back to a bearer secret: oauth is gone and the new secret is needed.
    let (status, _) = patch(&org, &maya, id, json!({"auth_kind": "bearer"})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, view) = patch(
        &org,
        &maya,
        id,
        json!({"auth_kind": "bearer", "secret": "tok"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["auth_kind"], "bearer");
    assert!(view["oauth_token_url"].is_null() && view["oauth_grant"].is_null());
    let row = org.api.store.server_by_id(id).await.unwrap().unwrap();
    assert!(!row.is_oauth() && row.oauth_client_secret_enc.is_none());
    // And to none.
    let (_, view) = patch(&org, &maya, id, json!({"auth_kind": "none"})).await;
    assert_eq!(view["auth_kind"], "none");
}

#[tokio::test]
async fn an_oauth_server_round_trips_through_the_configuration_without_its_secret() {
    let (org, maya) = world().await;
    let (_, made) = create(&org, &maya, oauth(json!({"oauth_grant": "token_exchange"}))).await;
    let id = made["id"].as_i64().unwrap();
    let (status, file) = org
        .call(Some(&maya), "GET", "/api/config/export", None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!file.to_string().contains(SECRET));
    let entry = &file["mcp_servers"][0];
    assert_eq!(entry["auth_kind"], "oauth");
    assert_eq!(entry["oauth"]["grant"], "token_exchange");
    assert_eq!(entry["oauth"]["client_id"], "gateway");
    // An import of it changes nothing, and an edit elsewhere keeps oauth and its secret.
    let mut edited = file.clone();
    edited["mcp_servers"][0]["description"] = "changed".into();
    let parsed: ConfigFile = serde_json::from_value(edited).unwrap();
    let report = portable::import(&org.api.store, &parsed, &actor(), false)
        .await
        .unwrap();
    assert!(report.is_clean(), "{:?}", report.errors);
    let row = org.api.store.server_by_id(id).await.unwrap().unwrap();
    assert!(
        row.is_oauth() && row.description == "changed" && row.oauth_client_secret_enc.is_some()
    );
    // A token endpoint on another host in the file drops the secret, with a warning.
    let mut moved = file.clone();
    moved["mcp_servers"][0]["oauth"]["token_url"] = "https://elsewhere.example.com/token".into();
    let parsed: ConfigFile = serde_json::from_value(moved).unwrap();
    let report = portable::import(&org.api.store, &parsed, &actor(), false)
        .await
        .unwrap();
    assert!(report.warnings.iter().any(|w| w.message.contains("secret")));
    assert!(org
        .api
        .store
        .server_by_id(id)
        .await
        .unwrap()
        .unwrap()
        .oauth_client_secret_enc
        .is_none());
    // A file cannot name oauth without saying how, nor say how without oauth.
    let mut bad = file.clone();
    bad["mcp_servers"][0]["oauth"] = Value::Null;
    let parsed: ConfigFile = serde_json::from_value(bad).unwrap();
    let report = portable::import(&org.api.store, &parsed, &actor(), true)
        .await
        .unwrap();
    assert!(!report.errors.is_empty());
    // A server created from a file has no secret and cannot be called.
    let mut fresh = file;
    fresh["mcp_servers"][0]["name"] = "fresh".into();
    fresh["mcp_servers"][0]["oauth"]["token_url"] = "https://idp.example.com/token".into();
    let parsed: ConfigFile = serde_json::from_value(fresh).unwrap();
    portable::import(&org.api.store, &parsed, &actor(), false)
        .await
        .unwrap();
    let (_, list) = org.call(Some(&maya), "GET", "/api/mcp/servers", None).await;
    let fresh = list["servers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "fresh")
        .unwrap();
    assert_eq!(fresh["usable"], false);
    assert_eq!(fresh["auth_kind"], "oauth");
}
