//! OAuth toward an MCP server: the gateway gets a token for the server itself,
//! keeps it, renews it once when the server refuses it, and never passes an
//! agent's key on.

mod common;
#[path = "support/mock_mcp.rs"]
mod mock_mcp;

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::http::StatusCode;
use common::{harness_with_state, post_to, Harness};
use mock_mcp::{tool, MockMcp};
use serde_json::{json, Value};
use ultrafast_gateway::mcp::encrypt_secret;
use ultrafast_gateway::mcp::oauth::{Config, Grant, TokenService};
use ultrafast_gateway::store::{DiscoveredTool, Grants, NewMcpServer, NewOauth};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

const CLIENT_SECRET: &str = "client-secret-5e1f";

/// Answers `tok-1`, `tok-2`, ... (or always `fixed`), after `delay`.
struct Tokens {
    n: AtomicUsize,
    fixed: Option<&'static str>,
    expires_in: Option<u64>,
    delay: Duration,
    status: u16,
}

impl Respond for Tokens {
    fn respond(&self, _: &Request) -> ResponseTemplate {
        if self.status != 200 {
            return ResponseTemplate::new(self.status);
        }
        let n = self.n.fetch_add(1, Ordering::SeqCst) + 1;
        let token = self.fixed.map_or_else(|| format!("tok-{n}"), String::from);
        let mut body = json!({"access_token": token, "token_type": "Bearer"});
        if let Some(e) = self.expires_in {
            body["expires_in"] = e.into();
        }
        ResponseTemplate::new(200)
            .set_delay(self.delay)
            .set_body_json(body)
    }
}

async fn token_endpoint(t: Tokens) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(t)
        .mount(&server)
        .await;
    server
}

fn tokens() -> Tokens {
    Tokens {
        n: AtomicUsize::new(0),
        fixed: None,
        expires_in: Some(3600),
        delay: Duration::ZERO,
        status: 200,
    }
}

/// The form a request to the token endpoint carried.
fn form_of(request: &Request) -> HashMap<String, String> {
    let text = String::from_utf8(request.body.clone()).unwrap();
    text.split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(k, v)| (decode(k), decode(v)))
        .collect()
}

fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            out.push(u8::from_str_radix(&text[i + 1..i + 3], 16).unwrap());
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap()
}

fn config(token_url: &str, resource: &str, grant: Grant) -> Config {
    Config {
        server_id: 1,
        token_url: token_url.to_string(),
        client_id: "gateway".into(),
        secret: CLIENT_SECRET.into(),
        scopes: Some("mcp:read mcp:write".into()),
        grant,
        resource: resource.to_string(),
    }
}

async fn requests(server: &MockServer) -> Vec<Request> {
    server.received_requests().await.unwrap()
}

// ---------------------------------------------------------- the token service

#[tokio::test]
async fn a_client_credentials_token_is_asked_for_the_server_once_and_kept() {
    let idp = token_endpoint(tokens()).await;
    let h = harness_with_state("openai", |_| {}).await;
    let cfg = config(
        &format!("{}/token", idp.uri()),
        "https://mcp.example/mcp",
        Grant::ClientCredentials,
    );
    for _ in 0..3 {
        assert_eq!(h.state.oauth.token(&cfg).await.unwrap(), "tok-1");
    }
    let sent = requests(&idp).await;
    assert_eq!(sent.len(), 1);
    let form = form_of(&sent[0]);
    assert_eq!(form["grant_type"], "client_credentials");
    assert_eq!(form["client_id"], "gateway");
    assert_eq!(form["client_secret"], CLIENT_SECRET);
    assert_eq!(form["scope"], "mcp:read mcp:write");
    // The token is for the server's URL (RFC 8707).
    assert_eq!(form["resource"], "https://mcp.example/mcp");
    assert_eq!(
        sent[0]
            .headers
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap(),
        "application/x-www-form-urlencoded"
    );
}

#[tokio::test]
async fn a_token_exchange_sends_the_stored_token_as_the_subject_token() {
    let idp = token_endpoint(tokens()).await;
    let h = harness_with_state("openai", |_| {}).await;
    let cfg = config(
        &format!("{}/token", idp.uri()),
        "https://mcp.example/mcp",
        Grant::TokenExchange,
    );
    h.state.oauth.token(&cfg).await.unwrap();
    let form = form_of(&requests(&idp).await[0]);
    assert_eq!(
        form["grant_type"],
        "urn:ietf:params:oauth:grant-type:token-exchange"
    );
    assert_eq!(form["subject_token"], CLIENT_SECRET);
    assert_eq!(
        form["subject_token_type"],
        "urn:ietf:params:oauth:token-type:access_token"
    );
    assert_eq!(form["resource"], "https://mcp.example/mcp");
    assert!(!form.contains_key("client_secret"));
}

#[tokio::test]
async fn a_token_that_ends_within_a_minute_is_not_kept() {
    let idp = token_endpoint(Tokens {
        expires_in: Some(45),
        ..tokens()
    })
    .await;
    let h = harness_with_state("openai", |_| {}).await;
    let cfg = config(
        &format!("{}/token", idp.uri()),
        "https://mcp.example/mcp",
        Grant::ClientCredentials,
    );
    assert_eq!(h.state.oauth.token(&cfg).await.unwrap(), "tok-1");
    assert_eq!(h.state.oauth.token(&cfg).await.unwrap(), "tok-2");
    assert_eq!(requests(&idp).await.len(), 2);
}

#[tokio::test]
async fn many_calls_at_once_make_one_request_to_the_token_endpoint() {
    let idp = token_endpoint(Tokens {
        delay: Duration::from_millis(250),
        ..tokens()
    })
    .await;
    let h = harness_with_state("openai", |_| {}).await;
    let cfg = Arc::new(config(
        &format!("{}/token", idp.uri()),
        "https://mcp.example/mcp",
        Grant::ClientCredentials,
    ));
    let calls: Vec<_> = (0..25)
        .map(|_| {
            let (state, cfg) = (h.state.clone(), cfg.clone());
            tokio::spawn(async move { state.oauth.token(&cfg).await.unwrap() })
        })
        .collect();
    for call in calls {
        assert_eq!(call.await.unwrap(), "tok-1");
    }
    assert_eq!(requests(&idp).await.len(), 1);
}

#[tokio::test]
async fn many_calls_refused_at_once_renew_the_token_once() {
    use ultrafast_gateway::mcp::upstream::UpstreamError;
    let idp = token_endpoint(Tokens {
        delay: Duration::from_millis(150),
        ..tokens()
    })
    .await;
    let h = harness_with_state("openai", |_| {}).await;
    let cfg = Arc::new(config(
        &format!("{}/token", idp.uri()),
        "https://mcp.example/mcp",
        Grant::ClientCredentials,
    ));
    // The refusals arrive one after another, some after the renewal is done.
    let calls: Vec<_> = (0..5u64)
        .map(|i| {
            let (state, cfg) = (h.state.clone(), cfg.clone());
            tokio::spawn(async move {
                state
                    .oauth
                    .with_bearer(&cfg, |token| async move {
                        let wait = if token == "tok-1" { 100 + i * 150 } else { 0 };
                        tokio::time::sleep(Duration::from_millis(wait)).await;
                        if token == "tok-1" {
                            Err(UpstreamError::Auth)
                        } else {
                            Ok(token)
                        }
                    })
                    .await
            })
        })
        .collect();
    for call in calls {
        assert_eq!(call.await.unwrap().unwrap(), "tok-2");
    }
    // The first token, and one renewal for all five refusals.
    assert_eq!(requests(&idp).await.len(), 2);
}

#[tokio::test]
async fn a_second_process_on_the_same_database_uses_the_token_the_first_got() {
    let idp = token_endpoint(tokens()).await;
    let h = harness_with_state("openai", |_| {}).await;
    // The server must exist for its token to be kept.
    let mut tx = h.store.begin().await.unwrap();
    let id = tx
        .insert_server(NewMcpServer {
            name: "mock",
            url: "https://mcp.example/mcp",
            ..Default::default()
        })
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let mut cfg = config(
        &format!("{}/token", idp.uri()),
        "https://mcp.example/mcp",
        Grant::ClientCredentials,
    );
    cfg.server_id = id;
    assert_eq!(h.state.oauth.token(&cfg).await.unwrap(), "tok-1");
    let other = TokenService::new(
        reqwest::Client::new(),
        h.store.clone(),
        h.state.cipher.clone(),
    );
    assert_eq!(other.token(&cfg).await.unwrap(), "tok-1");
    assert_eq!(requests(&idp).await.len(), 1);
    // The token is stored encrypted.
    let (stored, _) = h.store.mcp_token(id).await.unwrap().unwrap();
    assert!(!stored.contains("tok-1"));
    // A configuration it was not made for does not get it.
    let mut changed = cfg.clone();
    changed.secret = "another-secret".into();
    assert_eq!(other.token(&changed).await.unwrap(), "tok-2");
    // Forgotten, it is asked for again.
    other.forget(id).await;
    assert!(h.store.mcp_token(id).await.unwrap().is_none());
}

#[tokio::test]
async fn a_token_endpoint_that_refuses_or_fails_is_an_auth_error_and_nothing_is_kept() {
    let idp = token_endpoint(Tokens {
        status: 401,
        ..tokens()
    })
    .await;
    let h = harness_with_state("openai", |_| {}).await;
    let cfg = config(
        &format!("{}/token", idp.uri()),
        "https://mcp.example/mcp",
        Grant::ClientCredentials,
    );
    let error = h.state.oauth.token(&cfg).await.unwrap_err();
    assert_eq!(error.to_string(), "the MCP server refused the credentials");
    let gone = config(
        "http://127.0.0.1:1/token",
        "https://mcp.example/mcp",
        Grant::ClientCredentials,
    );
    assert!(h.state.oauth.token(&gone).await.is_err());
}

// ------------------------------------------------------------ through the gateway

struct World {
    h: Harness,
    mock: MockMcp,
    idp: MockServer,
    server_id: i64,
}

fn discovered(name: &str) -> DiscoveredTool {
    DiscoveredTool {
        name: name.into(),
        title: None,
        description: "d".into(),
        input_schema: r#"{"type":"object"}"#.into(),
        output_schema: None,
        annotations: None,
    }
}

/// Server "mock" that wants `Bearer tok-1`, with a token endpoint.
async fn world(idp_tokens: Tokens) -> World {
    let mock = MockMcp::builder()
        .tools(vec![tool("echo", "e")])
        .require_header("authorization", "Bearer tok-1")
        .start()
        .await;
    let idp = token_endpoint(idp_tokens).await;
    let h = harness_with_state("openai", |s| s.mcp.allow_private_upstreams = true).await;
    let secret = encrypt_secret(&h.state.cipher, CLIENT_SECRET);
    let token_url = format!("{}/token", idp.uri());
    let mut tx = h.store.begin().await.unwrap();
    let server_id = tx
        .insert_server(NewMcpServer {
            name: "mock",
            url: &mock.url,
            oauth: Some(NewOauth {
                token_url: &token_url,
                client_id: "gateway",
                client_secret_enc: Some(&secret),
                scopes: None,
                grant: "client_credentials",
            }),
            ..Default::default()
        })
        .await
        .unwrap();
    tx.upsert_tools(server_id, vec![discovered("echo")])
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let echo = h.store.list_tools(server_id).await.unwrap()[0].id;
    let mut tx = h.store.begin().await.unwrap();
    tx.set_tool_enabled(echo, true).await.unwrap();
    tx.replace_tool_grants(
        echo,
        &Grants {
            everyone: true,
            ..Grants::default()
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    h.state.refresh().await.unwrap();
    World {
        h,
        mock,
        idp,
        server_id,
    }
}

async fn call(w: &World) -> Value {
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                      "params": {"name": "echo", "arguments": {"q": "hi"}}});
    let bearer = format!("Bearer {}", w.h.key);
    let (status, _, text) = post_to(
        &w.h.app,
        "/mcp/mock",
        &[("authorization", bearer.as_str())],
        &body.to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{text}");
    serde_json::from_str(&text).unwrap()
}

#[tokio::test]
async fn a_call_carries_the_token_of_the_gateway_and_never_the_agents_key() {
    let w = world(tokens()).await;
    let reply = call(&w).await;
    assert_eq!(reply["result"]["isError"], false, "{reply}");
    call(&w).await;
    assert_eq!(requests(&w.idp).await.len(), 1);
    let calls = w.mock.received_method("tools/call");
    assert_eq!(calls.len(), 2);
    assert!(calls
        .iter()
        .all(|c| c.header_values("authorization") == ["Bearer tok-1"]));
    // Nothing the upstream or the token endpoint saw holds the virtual key.
    let key = &w.h.key;
    for r in w.mock.received() {
        assert!(r.headers.iter().all(|(_, v)| !v.contains(key.as_str())));
        assert!(!r.body.to_string().contains(key.as_str()));
    }
    for r in requests(&w.idp).await {
        assert!(!String::from_utf8_lossy(&r.body).contains(key.as_str()));
        assert!(r
            .headers
            .iter()
            .all(|(_, v)| !v.to_str().unwrap_or("").contains(key.as_str())));
    }
    // And the token does not come back to the agent.
    assert!(!reply.to_string().contains("tok-1"));
}

#[tokio::test]
async fn a_refused_token_is_renewed_once_and_the_call_goes_through() {
    let w = world(tokens()).await;
    call(&w).await;
    // The server now wants the next token.
    w.mock.set_required("authorization", "Bearer tok-2");
    let reply = call(&w).await;
    assert_eq!(reply["result"]["isError"], false, "{reply}");
    assert_eq!(requests(&w.idp).await.len(), 2);
    let tried: Vec<_> = w
        .mock
        .received_method("tools/call")
        .iter()
        .map(|c| c.header_values("authorization").join(""))
        .collect();
    assert_eq!(tried, ["Bearer tok-1", "Bearer tok-1", "Bearer tok-2"]);
}

#[tokio::test]
async fn a_server_that_keeps_refusing_gets_one_retry_then_the_call_fails_and_the_token_is_dropped()
{
    let w = world(Tokens {
        fixed: Some("tok-1"),
        ..tokens()
    })
    .await;
    // The server wants a token the endpoint never gives.
    w.mock.set_required("authorization", "Bearer never");
    let reply = call(&w).await;
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    assert_eq!(
        reply["result"]["content"][0]["text"],
        "The MCP server did not accept the gateway's credentials."
    );
    // One token to begin with, one renewal, and no more: no loop.
    assert_eq!(requests(&w.idp).await.len(), 2);
    assert_eq!(w.mock.received_method("tools/call").len(), 2);
    assert!(w.h.store.mcp_token(w.server_id).await.unwrap().is_none());
    // A later call starts from a new token.
    w.mock.set_required("authorization", "Bearer tok-1");
    assert_eq!(call(&w).await["result"]["isError"], false);
    assert_eq!(requests(&w.idp).await.len(), 3);
}

#[tokio::test]
async fn a_token_endpoint_that_is_down_fails_the_call_without_reaching_the_server() {
    let w = world(Tokens {
        status: 500,
        ..tokens()
    })
    .await;
    let reply = call(&w).await;
    assert_eq!(reply["result"]["isError"], true);
    assert!(w.mock.received_method("tools/call").is_empty());
}

#[tokio::test]
async fn sync_and_the_health_probe_use_the_token_too() {
    let w = world(tokens()).await;
    let mut admin_state = ultrafast_gateway::mcp::sync::Actor {
        user_id: None,
        email: "test".into(),
    };
    admin_state.email = "test".into();
    let done = ultrafast_gateway::mcp::sync::sync_server(&w.h.state, w.server_id, &admin_state)
        .await
        .unwrap();
    assert!(done.delta.added.is_empty() && done.delta.removed.is_empty());
    ultrafast_gateway::mcp::health::probe_once(&w.h.state).await;
    let probe = w.h.state.mcp_health.get(w.server_id).unwrap();
    assert_eq!(probe.status, ultrafast_gateway::mcp::health::Status::Ok);
    let lists = w.mock.received_method("tools/list");
    assert!(lists.len() >= 2);
    assert!(lists
        .iter()
        .all(|r| r.header_values("authorization") == ["Bearer tok-1"]));
    assert_eq!(requests(&w.idp).await.len(), 1);
}
