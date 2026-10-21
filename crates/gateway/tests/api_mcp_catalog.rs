//! Registering an MCP server from the catalog, and the private-address guard.

mod common;
#[path = "support/mock_mcp.rs"]
mod mock_mcp;

use std::sync::Arc;
use std::time::Duration;

use axum::http::StatusCode;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use common::{error_code, org_tweaked, Org, Signed};
use ed25519_dalek::{Signer, SigningKey};
use mock_mcp::{tool, MockMcp};
use serde_json::{json, Value};
use ufaigw_gateway::mcp::catalog::{Catalog, Config};
use ufaigw_gateway::mcp::local::{Config as LocalConfig, Supervisor};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TOKEN: &str = "catalog-secret-5b1d";

fn key() -> SigningKey {
    SigningKey::from_bytes(&[9u8; 32])
}

/// A signed catalog of `entries`, served by a mock web server.
async fn publish(entries: Value) -> MockServer {
    let body = json!({"schema": 1, "updated": "2999-01-01", "entries": entries}).to_string();
    let sig = STANDARD.encode(key().sign(body.as_bytes()).to_bytes());
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1.json"))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1.json.sig"))
        .respond_with(ResponseTemplate::new(200).set_body_string(sig))
        .mount(&server)
        .await;
    server
}

fn entry(id: &str, url: &str) -> Value {
    json!({
        "id": id, "name": "Mock", "description": "The mock server.",
        "homepage": "https://x.example", "transport": "http", "url": url,
        "env": [{"name": "MOCK_TOKEN", "secret": true, "help": "A token."}],
        "auth": "bearer", "tags": ["test"]
    })
}

/// A gateway that fetches `pages` as its catalog.
async fn org_with(pages: &MockServer, allow_private: bool) -> (Org, Signed) {
    let url = format!("{}/v1.json", pages.uri());
    let org = org_tweaked(None, move |s| {
        s.mcp.allow_private_upstreams = allow_private;
        s.catalog = Arc::new(Catalog::new(
            s.store.clone(),
            reqwest::Client::new(),
            Config {
                off: false,
                url,
                refresh: Duration::from_secs(86_400),
            },
            key().verifying_key(),
        ));
    })
    .await;
    let maya = org.sign_in("maya").await;
    let (status, body) = org
        .call(Some(&maya), "POST", "/api/mcp/catalog/refresh", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["source"], "remote", "{body}");
    (org, maya)
}

async fn add(org: &Org, who: &Signed, body: Value) -> (StatusCode, Value) {
    org.call(
        Some(who),
        "POST",
        "/api/mcp/servers/from-catalog",
        Some(body),
    )
    .await
}

#[tokio::test]
async fn a_catalog_registration_pre_fills_exactly_the_entrys_fields() {
    let mock = MockMcp::builder()
        .tools(vec![tool("echo", "e"), tool("other", "o")])
        .require_header("authorization", &format!("Bearer {TOKEN}"))
        .start()
        .await;
    let pages = publish(json!([entry("mock", &mock.url)])).await;
    let (org, maya) = org_with(&pages, true).await;

    let (status, view) = org.call(Some(&maya), "GET", "/api/mcp/catalog", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["entries"][0]["id"], "mock");
    assert_eq!(view["fetch_enabled"], true);

    let (status, body) = add(
        &org,
        &maya,
        json!({"catalog_id": "mock", "secrets": {"MOCK_TOKEN": TOKEN}}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["name"], "mock");
    assert_eq!(body["url"], mock.url.as_str());
    assert_eq!(body["auth_kind"], "bearer");
    assert_eq!(body["description"], "The mock server.");
    assert_eq!(body["has_secret"], true);
    assert_eq!(body["enabled"], true);
    // Synced once, so the tools are there (switched off).
    assert_eq!(body["tool_count"], 2, "{body}");
    assert!(!body.to_string().contains(TOKEN));
    assert_eq!(
        mock.received_method("tools/list")[0].header_values("authorization"),
        [format!("Bearer {TOKEN}")]
    );
    // The audit row names the catalog entry and not the secret.
    let summary = org.last_summary("mcp.create").await;
    assert!(summary.contains("catalog entry mock"), "{summary}");
    assert!(!summary.contains(TOKEN));

    // A name of its own.
    let (status, body) = add(
        &org,
        &maya,
        json!({"catalog_id": "mock", "name": "second", "secrets": {"MOCK_TOKEN": TOKEN}}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["name"], "second");
}

#[tokio::test]
async fn a_catalog_registration_that_cannot_work_is_refused_with_the_field() {
    let mock = MockMcp::builder()
        .tools(vec![tool("echo", "e")])
        .start()
        .await;
    let mut stdio = entry("local", "unused");
    stdio["transport"] = "stdio".into();
    stdio["url"] = Value::Null;
    stdio["command"] = "npx".into();
    stdio["auth"] = "none".into();
    stdio["env"] = json!([]);
    let mut oauth = entry("oauthy", &mock.url);
    oauth["auth"] = "oauth".into();
    oauth["env"] = json!([]);
    let pages = publish(json!([entry("mock", &mock.url), stdio, oauth])).await;
    let (org, maya) = org_with(&pages, true).await;
    let secrets = json!({"MOCK_TOKEN": TOKEN});

    let (status, body) = add(&org, &maya, json!({"catalog_id": "nope"})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["error"]["fields"]["catalog_id"].is_string(), "{body}");
    let (status, body) = add(&org, &maya, json!({"catalog_id": "mock"})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        body["error"]["fields"]["secrets.MOCK_TOKEN"].is_string(),
        "{body}"
    );
    let (status, body) = add(
        &org,
        &maya,
        json!({"catalog_id": "mock", "secrets": {"MOCK_TOKEN": TOKEN, "EXTRA": "x"}}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        body["error"]["fields"]["secrets.EXTRA"].is_string(),
        "{body}"
    );
    let (status, _) = add(&org, &maya, json!({"catalog_id": "local"})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = add(&org, &maya, json!({"catalog_id": "oauthy"})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, body) = add(
        &org,
        &maya,
        json!({"catalog_id": "mock", "name": "Bad_Name", "secrets": secrets}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["error"]["fields"]["name"].is_string(), "{body}");
    // Nothing was made by any of those.
    let (_, list) = org.call(Some(&maya), "GET", "/api/mcp/servers", None).await;
    assert_eq!(list["servers"].as_array().unwrap().len(), 0);

    let (status, _) = add(
        &org,
        &maya,
        json!({"catalog_id": "mock", "secrets": secrets}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, body) = add(
        &org,
        &maya,
        json!({"catalog_id": "mock", "secrets": secrets}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&body), "mcp_server_exists");
}

#[tokio::test]
async fn a_private_address_is_refused_by_default_on_every_path() {
    let pages = publish(json!([entry("meta", "http://169.254.169.254/mcp")])).await;
    let (org, maya) = org_with(&pages, false).await;

    let (status, body) = add(
        &org,
        &maya,
        json!({"catalog_id": "meta", "secrets": {"MOCK_TOKEN": TOKEN}}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["error"]["fields"]["catalog_id"]
        .as_str()
        .unwrap()
        .contains("UF_MCP_ALLOW_PRIVATE_UPSTREAMS"));

    for url in [
        "http://127.0.0.1:9/mcp",
        "http://localhost/mcp",
        "http://10.0.0.5/mcp",
        "http://[::1]/mcp",
    ] {
        let (status, body) = org
            .call(
                Some(&maya),
                "POST",
                "/api/mcp/servers",
                Some(json!({"name": "private", "url": url})),
            )
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{url} {body}");
        assert!(body["error"]["fields"]["url"].is_string(), "{url}");
    }

    // Moving a public server to a private address is refused too.
    let (status, made) = org
        .call(
            Some(&maya),
            "POST",
            "/api/mcp/servers",
            Some(json!({"name": "public", "url": "https://mcp.example.com/mcp"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = made["id"].as_i64().unwrap();
    let (status, body) = org
        .call(
            Some(&maya),
            "PATCH",
            &format!("/api/mcp/servers/{id}"),
            Some(json!({"url": "http://192.168.1.1/mcp"})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["error"]["fields"]["url"].is_string());
    // Other edits of it stay possible.
    let (status, _) = org
        .call(
            Some(&maya),
            "PATCH",
            &format!("/api/mcp/servers/{id}"),
            Some(json!({"description": "still fine"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn the_switch_allows_a_private_address() {
    let pages = publish(json!([])).await;
    let (org, maya) = org_with(&pages, true).await;
    let (status, body) = org
        .call(
            Some(&maya),
            "POST",
            "/api/mcp/servers",
            Some(json!({"name": "lan", "url": "http://10.0.0.5/mcp"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

#[tokio::test]
async fn only_admins_see_the_catalog() {
    let pages = publish(json!([])).await;
    let (org, _maya) = org_with(&pages, false).await;
    let lena = org.sign_in("lena").await;
    let (status, _) = org.call(Some(&lena), "GET", "/api/mcp/catalog", None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = org
        .call(
            Some(&lena),
            "POST",
            "/api/mcp/servers/from-catalog",
            Some(json!({"catalog_id": "x"})),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_stdio_entry_becomes_a_local_server_with_its_env_when_local_servers_are_on() {
    let mock = env!("CARGO_BIN_EXE_mcp-stdio-mock");
    let entry = json!({
        "id": "fs", "name": "Files", "description": "Files on the host.",
        "homepage": "https://x.example", "transport": "stdio", "command": mock,
        "args": ["--stderr", "5"],
        "env": [{"name": "API_TOKEN", "secret": true, "help": ""}],
        "auth": "none", "tags": []
    });
    let pages = publish(json!([entry])).await;
    let url = format!("{}/v1.json", pages.uri());
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("mcp");
    let org = org_tweaked(None, move |s| {
        s.local = Arc::new(Supervisor::new(
            root,
            LocalConfig {
                restart_min: Duration::from_millis(50),
                ..LocalConfig::default()
            },
        ));
        s.catalog = Arc::new(Catalog::new(
            s.store.clone(),
            reqwest::Client::new(),
            Config {
                off: false,
                url,
                refresh: Duration::from_secs(86_400),
            },
            key().verifying_key(),
        ));
    })
    .await;
    let maya = org.sign_in("maya").await;
    let (_, view) = org
        .call(Some(&maya), "POST", "/api/mcp/catalog/refresh", None)
        .await;
    assert_eq!(view["local_enabled"], true);
    let (status, body) = add(
        &org,
        &maya,
        json!({"catalog_id": "fs", "secrets": {"API_TOKEN": "tok-from-the-form"}}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["transport"], "stdio");
    assert_eq!(body["command"], mock);
    assert_eq!(body["args"], json!(["--stderr", "5"]));
    assert_eq!(body["env_names"], json!(["API_TOKEN"]));
    assert_eq!(body["catalog_id"], "fs");
    // The program came up and was synced once.
    assert_eq!(body["tool_count"], 6, "{body}");
    assert!(!body.to_string().contains("tok-from-the-form"));
    let (status, body) = add(&org, &maya, json!({"catalog_id": "fs", "name": "fs2"})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        body["error"]["fields"]["secrets.API_TOKEN"].is_string(),
        "{body}"
    );
}
