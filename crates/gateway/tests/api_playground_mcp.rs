//! The console playground's MCP calls: the signed-in user calls a tool they
//! may call through the real pipeline of `/mcp/{server}`, so it is logged.

mod common;
#[path = "support/mock_mcp.rs"]
mod mock_mcp;

use std::sync::Arc;

use axum::http::StatusCode;
use common::{org_tweaked, MemorySink, Org, Signed};
use mock_mcp::{tool, MockMcp};
use serde_json::{json, Value};
use ufaigw_gateway::mcp::encrypt_secret;
use ufaigw_gateway::store::{DiscoveredTool, Grants, NewMcpServer};

const SECRET: &str = "upstream-secret-3a9c";

struct World {
    org: Org,
    sink: Arc<MemorySink>,
    mock: MockMcp,
    server: i64,
    echo: i64,
    secret: i64,
}

fn discovered(name: &str) -> DiscoveredTool {
    DiscoveredTool {
        name: name.into(),
        title: None,
        description: format!("does {name}"),
        input_schema: r#"{"type":"object","properties":{"q":{"type":"string"}}}"#.into(),
        output_schema: None,
        annotations: None,
    }
}

/// Server "mock" with `echo` (on, granted to the team Platform) and `secret`
/// (on, granted to nobody).
async fn world() -> World {
    let mock = MockMcp::builder()
        .tools(vec![tool("echo", "e"), tool("secret", "s")])
        .require_header("authorization", &format!("Bearer {SECRET}"))
        .start()
        .await;
    let sink = Arc::new(MemorySink::default());
    let org = org_tweaked(Some(sink.clone()), |_| {}).await;
    let enc = encrypt_secret(&org.api.state.cipher, SECRET);
    let mut tx = org.api.store.begin().await.unwrap();
    let server = tx
        .insert_server(NewMcpServer {
            name: "mock",
            url: &mock.url,
            auth_kind: "bearer",
            auth_secret_enc: Some(&enc),
            ..Default::default()
        })
        .await
        .unwrap();
    tx.upsert_tools(server, vec![discovered("echo"), discovered("secret")])
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let tools = org.api.store.list_tools(server).await.unwrap();
    let id_of = |n: &str| tools.iter().find(|t| t.name == n).unwrap().id;
    let (echo, secret) = (id_of("echo"), id_of("secret"));
    let mut tx = org.api.store.begin().await.unwrap();
    tx.set_tool_enabled(echo, true).await.unwrap();
    tx.set_tool_enabled(secret, true).await.unwrap();
    tx.replace_tool_grants(
        echo,
        &Grants {
            team_ids: vec![org.platform],
            ..Grants::default()
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    org.api.state.refresh().await.unwrap();
    World {
        org,
        sink,
        mock,
        server,
        echo,
        secret,
    }
}

async fn rpc(
    w: &World,
    who: &Signed,
    server: &str,
    method: &str,
    params: Value,
) -> (StatusCode, Value) {
    w.org
        .call(
            Some(who),
            "POST",
            &format!("/api/playground/mcp/{server}"),
            Some(json!({"jsonrpc": "2.0", "id": 7, "method": method, "params": params})),
        )
        .await
}

#[tokio::test]
async fn a_user_calls_a_tool_they_may_call_and_it_is_logged_without_a_key() {
    let w = world().await;
    let lena = w.org.sign_in("lena").await; // a member of Platform
    let (status, list) = rpc(&w, &lena, "mock", "tools/list", json!({})).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["echo"]);

    let (status, reply) = rpc(
        &w,
        &lena,
        "mock",
        "tools/call",
        json!({"name": "echo", "arguments": {"q": "hi"}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["result"]["content"][0]["text"], "{\"q\":\"hi\"}");
    // The upstream got the gateway's secret, never the user's session.
    let seen = w.mock.received_method("tools/call");
    assert_eq!(
        seen[0].header_values("authorization"),
        [format!("Bearer {SECRET}")]
    );
    // Logged like an agent's call: under the user, under no key.
    let rec = w.sink.wait_for(1).await.remove(0);
    assert_eq!(rec.endpoint, "mcp");
    assert_eq!(rec.requested, "mcp:mock/echo");
    assert_eq!(rec.key_id, None);
    assert_eq!(rec.user_id, Some(lena.user_id));
}

#[tokio::test]
async fn a_tool_the_user_may_not_call_is_unknown_and_a_stranger_gets_nothing() {
    let w = world().await;
    let lena = w.org.sign_in("lena").await;
    let (_, reply) = rpc(
        &w,
        &lena,
        "mock",
        "tools/call",
        json!({"name": "secret", "arguments": {}}),
    )
    .await;
    assert_eq!(reply["error"]["code"], -32602);
    assert_eq!(reply["error"]["message"], "Unknown tool");
    assert!(w.mock.received_method("tools/call").is_empty());
    // Someone in no team that grants anything sees no server at all.
    let priya = w.org.sign_in("priya").await;
    let (status, _) = rpc(&w, &priya, "mock", "tools/list", json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = rpc(&w, &lena, "nope", "tools/list", json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_playground_takes_a_session_only_and_the_limits_of_the_user_apply() {
    let w = world().await;
    let lena = w.org.sign_in("lena").await;
    // No session.
    let (status, _, _) = common::call(
        &w.org.api.app,
        "POST",
        "/api/playground/mcp/mock",
        None,
        Some(json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // A limit of the user's own: one request a minute.
    let mut tx = w.org.api.store.begin().await.unwrap();
    tx.upsert_limit(
        ufaigw_gateway::limits::LimitScope::User,
        Some(lena.user_id),
        &ufaigw_gateway::limits::RateLimit {
            requests_per_minute: Some(1),
            tokens_per_minute: None,
            concurrent: None,
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    w.org.api.state.refresh().await.unwrap();
    let (status, _) = rpc(&w, &lena, "mock", "tools/list", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    let (status, reply) = rpc(&w, &lena, "mock", "tools/list", json!({})).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{reply}");
    let _ = (w.server, w.echo, w.secret);
}
