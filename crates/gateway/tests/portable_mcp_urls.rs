//! An import checks the URLs of MCP servers as the admin API does. This file
//! has one test because it sets a process-wide environment variable.

mod common;

use common::org_tweaked;
use serde_json::json;
use ultrafast_gateway::portable::{self, Actor, ConfigFile};

fn actor() -> Actor<'static> {
    Actor {
        user_id: None,
        email: "cli",
        cipher: None,
    }
}

async fn dry_run(org: &common::Org, file: serde_json::Value) -> portable::ImportReport {
    let parsed: ConfigFile = serde_json::from_value(file).unwrap();
    portable::import(&org.api.store, &parsed, &actor(), true)
        .await
        .unwrap()
}

#[tokio::test]
async fn an_import_refuses_private_urls_and_http_token_urls_unless_private_upstreams_are_allowed() {
    let org = org_tweaked(None, |s| s.mcp.allow_private_upstreams = true).await;
    let maya = org.sign_in("maya").await;
    let (status, _) = org
        .call(
            Some(&maya),
            "POST",
            "/api/mcp/servers",
            Some(json!({
                "name": "api", "url": "https://mcp.example.com/mcp", "auth_kind": "oauth",
                "oauth_token_url": "https://idp.example.com/token", "oauth_client_id": "gateway",
                "secret": "oauth-client-secret-77ab",
            })),
        )
        .await;
    assert_eq!(status, axum::http::StatusCode::CREATED);
    let (_, base) = org
        .call(Some(&maya), "GET", "/api/config/export", None)
        .await;

    let mut private = base.clone();
    private["mcp_servers"][0]["url"] = "https://127.0.0.1/mcp".into();
    let report = dry_run(&org, private.clone()).await;
    assert!(
        report.errors.iter().any(|e| e.at == "mcp_servers[0].url"),
        "{:?}",
        report.errors
    );

    let mut http = base.clone();
    http["mcp_servers"][0]["oauth"]["token_url"] = "http://idp.example.com/token".into();
    let report = dry_run(&org, http.clone()).await;
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.at == "mcp_servers[0].oauth.token_url"),
        "{:?}",
        report.errors
    );

    let mut private_token = base.clone();
    private_token["mcp_servers"][0]["oauth"]["token_url"] = "https://10.0.0.1/token".into();
    let report = dry_run(&org, private_token).await;
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.at == "mcp_servers[0].oauth.token_url"),
        "{:?}",
        report.errors
    );

    assert!(dry_run(&org, base).await.is_clean());

    std::env::set_var("UF_MCP_ALLOW_PRIVATE_UPSTREAMS", "on");
    assert!(dry_run(&org, private).await.is_clean());
    assert!(dry_run(&org, http).await.is_clean());
}
