//! `POST /mcp/{server}`: a stateless MCP endpoint over the registry, and the
//! call pipeline behind `tools/call`, against a mock upstream MCP server.

mod common;
#[path = "support/mock_mcp.rs"]
mod mock_mcp;

use std::time::{Duration, Instant};

use axum::http::{HeaderMap, StatusCode};
use common::{harness_with_state, post_to, Harness};
use mock_mcp::{tool, CallMode, Failure, MockMcp};
use serde_json::{json, Value};
use time::OffsetDateTime;
use tokio::sync::watch;
use ultrafast_gateway::app::AppState;
use ultrafast_gateway::budgets::{account, BudgetAction, Period};
use ultrafast_gateway::limits::{LimitScope, RateLimit};
use ultrafast_gateway::mcp::encrypt_secret;
use ultrafast_gateway::mcp::health::{probe_once, Status};
use ultrafast_gateway::otel::{Exporter, OtelConfig};
use ultrafast_gateway::store::{DiscoveredTool, Grants, NewGuardrail, NewMcpServer};
use ultrafast_gateway::telemetry::{AttemptOutcome, RequestRecord};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SECRET: &str = "upstream-secret-7c1e";
const METRICS_TOKEN: &str = "scrape-token-0123456789";

struct World {
    h: Harness,
    mock: MockMcp,
    server_id: i64,
    echo_id: i64,
}

fn discovered(name: &str) -> DiscoveredTool {
    DiscoveredTool {
        name: name.into(),
        title: Some(format!("{name} title")),
        description: format!("does {name}"),
        input_schema: r#"{"type":"object","properties":{"q":{"type":"string"}}}"#.into(),
        output_schema: Some(r#"{"type":"object"}"#.into()),
        annotations: Some(r#"{"readOnlyHint":true}"#.into()),
    }
}

/// A gateway with server "mock" (bearer secret) offering `echo` (on, granted
/// to everyone) and `secret` (discovered, left off).
async fn world_with(
    mode: CallMode,
    delay: Duration,
    failure: Failure,
    tweak: impl FnOnce(&mut AppState) + 'static,
) -> World {
    let mock = MockMcp::builder()
        .tools(vec![tool("echo", "echoes"), tool("secret", "hidden")])
        .call_mode(mode)
        .call_delay(delay)
        .failure(failure)
        .start()
        .await;
    let h = harness_with_state("openai", tweak).await;
    let enc = encrypt_secret(&h.state.cipher, SECRET);
    let mut tx = h.store.begin().await.unwrap();
    let server_id = tx
        .insert_server(NewMcpServer {
            name: "mock",
            url: &mock.url,
            auth_kind: "bearer",
            auth_header: None,
            auth_secret_enc: Some(&enc),
            enabled: true,
            description: "d",
            ..Default::default()
        })
        .await
        .unwrap();
    tx.upsert_tools(server_id, vec![discovered("echo"), discovered("secret")])
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let tools = h.store.list_tools(server_id).await.unwrap();
    let echo_id = tools.iter().find(|t| t.name == "echo").unwrap().id;
    let mut tx = h.store.begin().await.unwrap();
    assert!(tx.set_tool_enabled(echo_id, true).await.unwrap());
    tx.replace_tool_grants(
        echo_id,
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
        server_id,
        echo_id,
    }
}

async fn world() -> World {
    world_with(CallMode::Echo, Duration::ZERO, Failure::None, |_| {}).await
}

async fn post_raw(
    w: &World,
    server: &str,
    key: Option<&str>,
    extra: &[(&str, &str)],
    body: &str,
) -> (StatusCode, HeaderMap, Value) {
    let bearer = key.map(|k| format!("Bearer {k}"));
    let mut headers: Vec<(&str, &str)> = extra.to_vec();
    if let Some(b) = &bearer {
        headers.push(("authorization", b));
    }
    let (status, headers, text) =
        post_to(&w.h.app, &format!("/mcp/{server}"), &headers, body).await;
    let value = serde_json::from_str(&text).unwrap_or(Value::Null);
    (status, headers, value)
}

async fn rpc(w: &World, method: &str, params: Value) -> (StatusCode, HeaderMap, Value) {
    let body = json!({"jsonrpc": "2.0", "id": 7, "method": method, "params": params});
    post_raw(w, "mock", Some(&w.h.key), &[], &body.to_string()).await
}

async fn call(w: &World, tool: &str, args: Value) -> (StatusCode, HeaderMap, Value) {
    rpc(w, "tools/call", json!({"name": tool, "arguments": args})).await
}

fn text_of(reply: &Value) -> String {
    reply["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

fn upstream_calls(w: &World) -> usize {
    w.mock.received_method("tools/call").len()
}

async fn guardrail(w: &World, name: &str, rule: Value) {
    let rules = Value::Array(vec![rule]).to_string();
    let mut tx = w.h.store.begin().await.unwrap();
    tx.insert_guardrail(NewGuardrail {
        name,
        description: "",
        kind: "rules",
        rules: &rules,
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
    w.h.state.refresh().await.unwrap();
}

fn word_rule(word: &str, action: &str, directions: &str) -> Value {
    json!({ "id": "w", "matcher": { "keywords": { "words": [word] } },
            "action": action, "directions": directions })
}

async fn limit_requests(w: &World, per_minute: u64) {
    let mut tx = w.h.store.begin().await.unwrap();
    tx.upsert_limit(
        LimitScope::Gateway,
        None,
        &RateLimit {
            requests_per_minute: Some(per_minute),
            tokens_per_minute: None,
            concurrent: None,
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    w.h.state.refresh().await.unwrap();
}

#[tokio::test]
async fn the_list_shows_only_the_tools_the_key_may_call() {
    let w = world().await;
    let (status, headers, reply) = rpc(&w, "tools/list", json!({})).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    let tools = reply["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1, "{reply}");
    let t = &tools[0];
    assert_eq!(t["name"], "echo");
    assert_eq!(t["title"], "echo title");
    assert_eq!(t["description"], "does echo");
    assert_eq!(t["inputSchema"]["type"], "object");
    assert_eq!(t["outputSchema"]["type"], "object");
    assert_eq!(t["annotations"]["readOnlyHint"], true);
    assert_eq!(t["_meta"]["ultrafast"]["server"], "mock");
    assert_eq!(reply["id"], 7);
    assert!(headers.get("mcp-session-id").is_none());
    // The registry's list is answered from the snapshot: no upstream call.
    assert!(w.mock.received().is_empty());
}

#[tokio::test]
async fn a_hidden_or_unknown_tool_is_the_same_invalid_params_error() {
    let w = world().await;
    for name in ["secret", "nope"] {
        let (status, _, reply) = call(&w, name, json!({})).await;
        assert_eq!(status, StatusCode::OK, "{name}");
        assert_eq!(reply["error"]["code"], -32602, "{name}: {reply}");
        assert_eq!(reply["error"]["message"], "Unknown tool", "{name}");
        assert_eq!(reply["id"], 7);
    }
    assert_eq!(upstream_calls(&w), 0);
}

#[tokio::test]
async fn a_call_reaches_the_upstream_with_its_secret_and_without_a_session() {
    let w = world().await;
    let traceparent = "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01";
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "echo", "arguments": {"q": "hi"}}});
    let (status, headers, reply) = post_raw(
        &w,
        "mock",
        Some(&w.h.key),
        &[("traceparent", traceparent)],
        &body.to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["result"]["isError"], false);
    assert!(text_of(&reply).contains("hi"), "{reply}");
    assert!(headers.get("mcp-session-id").is_none());
    let sent = w.mock.received_method("tools/call");
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0].header_values("authorization"),
        [format!("Bearer {SECRET}")]
    );
    assert_eq!(sent[0].header_values("mcp-method"), ["tools/call"]);
    assert_eq!(sent[0].header_values("mcp-name"), ["echo"]);
    assert_eq!(sent[0].header_values("traceparent"), [traceparent]);
    assert!(sent[0].header_values("mcp-session-id").is_empty());
    // The agent's key never goes upstream.
    assert!(!format!("{:?}", sent[0].headers).contains(&w.h.key));
    // Repeat calls need no handshake and no session.
    for i in 0..3 {
        let (status, headers, reply) = call(&w, "echo", json!({"q": i})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(reply["result"]["isError"], false, "{reply}");
        assert!(headers.get("mcp-session-id").is_none());
    }
    // The result carries the upstream's content only, no secret.
    assert!(!reply.to_string().contains(SECRET));
}

#[tokio::test]
async fn a_rich_result_keeps_its_content_blocks_and_structured_content() {
    let w = world_with(CallMode::Rich, Duration::ZERO, Failure::None, |_| {}).await;
    let (_, _, reply) = call(&w, "echo", json!({"q": "x"})).await;
    let r = &reply["result"];
    assert_eq!(r["content"].as_array().unwrap().len(), 3, "{reply}");
    assert_eq!(r["content"][1]["type"], "image");
    assert_eq!(r["structuredContent"]["echo"]["q"], "x");
}

#[tokio::test]
async fn a_tool_that_reports_an_error_is_passed_on_as_one() {
    let w = world_with(CallMode::IsError, Duration::ZERO, Failure::None, |_| {}).await;
    let (status, _, reply) = call(&w, "echo", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reply["result"]["isError"], true);
    assert_eq!(text_of(&reply), "tool failed");
    let rec = w.h.sink.wait_for(1).await.remove(0);
    assert_eq!(rec.status, 200);
}

#[tokio::test]
async fn upstream_failures_are_tool_errors_that_say_nothing_of_the_upstream() {
    for (mode, status) in [
        (CallMode::RpcError, 502),
        (CallMode::Http401, 502),
        (CallMode::Http500, 502),
    ] {
        let w = world_with(mode.clone(), Duration::ZERO, Failure::None, |_| {}).await;
        let (http, _, reply) = call(&w, "echo", json!({})).await;
        assert_eq!(http, StatusCode::OK, "{mode:?}");
        assert_eq!(reply["result"]["isError"], true, "{mode:?}: {reply}");
        let text = text_of(&reply);
        assert!(
            !text.contains("exploded") && !text.contains("401"),
            "{text}"
        );
        let rec = w.h.sink.wait_for(1).await.remove(0);
        assert_eq!(rec.status, status, "{mode:?}");
        assert_eq!(rec.attempts[0].outcome, AttemptOutcome::Fatal);
    }
}

#[tokio::test]
async fn a_removed_grant_refuses_the_next_call_after_the_refresh() {
    let w = world().await;
    assert_eq!(
        call(&w, "echo", json!({})).await.2["result"]["isError"],
        false
    );
    // A second tool stays callable, so the server is still the key's.
    let tools = w.h.store.list_tools(w.server_id).await.unwrap();
    let other = tools.iter().find(|t| t.name == "secret").unwrap().id;
    let mut tx = w.h.store.begin().await.unwrap();
    assert!(tx.set_tool_enabled(other, true).await.unwrap());
    tx.replace_tool_grants(
        other,
        &Grants {
            everyone: true,
            ..Grants::default()
        },
    )
    .await
    .unwrap();
    tx.replace_tool_grants(w.echo_id, &Grants::default())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    // Until the refresh the endpoint still reads the old snapshot.
    assert_eq!(
        call(&w, "echo", json!({})).await.2["result"]["isError"],
        false
    );
    w.h.state.refresh().await.unwrap();
    let (_, _, reply) = call(&w, "echo", json!({})).await;
    assert_eq!(reply["error"]["code"], -32602, "{reply}");
    let (_, _, reply) = rpc(&w, "tools/list", json!({})).await;
    assert_eq!(reply["result"]["tools"][0]["name"], "secret");
    let mut tx = w.h.store.begin().await.unwrap();
    tx.replace_tool_grants(other, &Grants::default())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    w.h.state.refresh().await.unwrap();
    // A key with nothing to call on the server gets the unknown-server answer.
    let (status, _, reply) = rpc(&w, "tools/list", json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{reply}");
    let (_, _, unknown) = post_raw(&w, "nope", Some(&w.h.key), &[], "{}").await;
    assert_eq!(reply, unknown, "no oracle for which servers exist");
    assert_eq!(upstream_calls(&w), 2);
}

#[tokio::test]
async fn a_disabled_or_unknown_server_is_404() {
    let w = world().await;
    let (status, _, reply) = post_raw(&w, "nope", Some(&w.h.key), &[], "{}").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(reply["error"]["message"].is_string(), "{reply}");
    let enc = encrypt_secret(&w.h.state.cipher, SECRET);
    let mut tx = w.h.store.begin().await.unwrap();
    tx.update_server(
        w.server_id,
        NewMcpServer {
            name: "mock",
            url: &w.mock.url,
            auth_kind: "bearer",
            auth_header: None,
            auth_secret_enc: Some(&enc),
            enabled: false,
            description: "d",
            ..Default::default()
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    w.h.state.refresh().await.unwrap();
    let (status, _, _) = call(&w, "echo", json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = rpc(&w, "tools/list", json!({})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn authentication_and_methods() {
    let w = world().await;
    let body = json!({"jsonrpc":"2.0","id":1,"method":"ping"}).to_string();
    let (status, headers, _) = post_raw(&w, "mock", None, &[], &body).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(headers.contains_key("www-authenticate"));
    let (status, _, _) = post_raw(&w, "mock", Some("uf-wrong"), &[], &body).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    for verb in ["GET", "DELETE", "PUT"] {
        let req = axum::http::Request::builder()
            .method(verb)
            .uri("/mcp/mock")
            .header("authorization", format!("Bearer {}", w.h.key))
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = tower::ServiceExt::oneshot(w.h.app.clone(), req)
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED, "{verb}");
        assert_eq!(resp.headers()["allow"], "POST", "{verb}");
    }
    // Other paths under /mcp are JSON 404s, not the console.
    let (status, _, text) = post_to(&w.h.app, "/mcp/", &[], "{}").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(!text.contains("<html"), "{text}");
}

#[tokio::test]
async fn initialize_ping_and_the_methods_the_gateway_does_not_proxy() {
    let w = world().await;
    let (status, headers, reply) = rpc(
        &w,
        "initialize",
        json!({"protocolVersion": "2025-06-18", "capabilities": {},
               "clientInfo": {"name": "t", "version": "1"}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reply["result"]["protocolVersion"], "2025-06-18");
    assert!(reply["result"]["capabilities"]["tools"].is_object());
    assert!(reply["result"]["capabilities"]["resources"].is_object());
    assert!(reply["result"]["capabilities"]["prompts"].is_object());
    assert_eq!(reply["result"]["serverInfo"]["name"], "ultrafast-gateway");
    assert!(headers.get("mcp-session-id").is_none());
    let (_, _, reply) = rpc(&w, "ping", json!({})).await;
    assert_eq!(reply["result"], json!({}));
    for m in [
        "completion/complete",
        "resources/subscribe",
        "logging/setLevel",
    ] {
        let (status, _, reply) = rpc(&w, m, json!({})).await;
        assert_eq!(status, StatusCode::OK, "{m}");
        assert_eq!(reply["error"]["code"], -32601, "{m}: {reply}");
    }
    // A notification is accepted and answered with nothing.
    let note = json!({"jsonrpc":"2.0","method":"notifications/initialized"}).to_string();
    let (status, _, _) = post_raw(&w, "mock", Some(&w.h.key), &[], &note).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    // Not JSON, and not an object.
    let (status, _, reply) = post_raw(&w, "mock", Some(&w.h.key), &[], "<<").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(reply["error"]["code"], -32700);
    let (status, _, reply) = post_raw(&w, "mock", Some(&w.h.key), &[], "[]").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(reply["error"]["code"], -32600);
    // Arguments that are not an object.
    let (_, _, reply) = call(&w, "echo", json!([1])).await;
    assert_eq!(reply["error"]["code"], -32602, "{reply}");
}

#[tokio::test]
async fn a_request_body_over_the_cap_is_413() {
    let w = world_with(CallMode::Echo, Duration::ZERO, Failure::None, |s| {
        s.mcp.limits.max_args_bytes = 2048;
    })
    .await;
    let (status, _, reply) = call(&w, "echo", json!({"q": "x".repeat(4096)})).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{reply}");
    assert_eq!(upstream_calls(&w), 0);
    let (status, _, _) = call(&w, "echo", json!({"q": "small"})).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn the_log_row_of_a_call_names_the_server_and_the_tool() {
    let w = world().await;
    let (_, _, reply) = call(&w, "echo", json!({"q": "hi"})).await;
    assert_eq!(reply["result"]["isError"], false);
    let rec = w.h.sink.wait_for(1).await.remove(0);
    assert_eq!(rec.endpoint, "mcp");
    assert_eq!(rec.requested, "mcp:mock/echo");
    assert_eq!(rec.status, 200);
    assert!(rec.key_id.is_some());
    assert!(rec.usage.is_none());
    assert_eq!(rec.attempts.len(), 1);
    assert_eq!(rec.attempts[0].provider, "mcp:mock");
    assert_eq!(rec.attempts[0].model, "echo");
    assert_eq!(rec.attempts[0].outcome, AttemptOutcome::Ok);
    let prices: ultrafast_gateway::logs::PriceLookup = std::sync::Arc::new(|_, _| None);
    let row = ultrafast_gateway::logs::row_of(&rec, &prices);
    assert_eq!(row.endpoint, "mcp");
    assert_eq!(row.provider.as_deref(), Some("mcp:mock"));
    assert_eq!(row.model.as_deref(), Some("echo"));
    assert_eq!(row.cost_micros, 0);
    assert!(!row.priced);
    assert!(!row.stream);
    assert!(!row.cached);
    // The arguments are not in the record.
    assert!(!format!("{rec:?}").contains("\"hi\""));
    // A call that names an unknown tool is recorded too, as the caller's mistake.
    call(&w, "nope", json!({})).await;
    let recs = w.h.sink.wait_for(2).await;
    assert_eq!(recs[1].status, 400);
    assert!(recs[1].attempts.is_empty());
}

#[tokio::test]
async fn a_rate_limited_call_is_429_and_reaches_no_upstream() {
    let w = world().await;
    limit_requests(&w, 1).await;
    assert_eq!(call(&w, "echo", json!({})).await.0, StatusCode::OK);
    let (status, headers, reply) = call(&w, "echo", json!({})).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{reply}");
    assert!(headers.contains_key("retry-after"));
    assert_eq!(reply["error"]["code"], -32000);
    assert_eq!(upstream_calls(&w), 1);
}

#[tokio::test]
async fn a_rate_limited_list_is_429() {
    let w = world().await;
    limit_requests(&w, 1).await;
    assert_eq!(rpc(&w, "tools/list", json!({})).await.0, StatusCode::OK);
    let (status, headers, reply) = rpc(&w, "tools/list", json!({})).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{reply}");
    assert!(headers.contains_key("retry-after"));
    assert_eq!(reply["error"]["code"], -32000);
}

#[tokio::test]
async fn a_reply_on_a_stream_that_stays_open_is_returned_at_once() {
    let w = world_with(CallMode::SseOpen, Duration::ZERO, Failure::None, |s| {
        s.mcp.limits.call_timeout = Duration::from_secs(20)
    })
    .await;
    let started = std::time::Instant::now();
    let (http, _, reply) = call(&w, "echo", json!({"q": "x"})).await;
    assert_eq!(http, StatusCode::OK);
    assert_eq!(reply["result"]["isError"], false, "{reply}");
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[tokio::test]
async fn an_upstream_invalid_params_error_is_a_tool_error_with_fixed_text() {
    let w = world_with(
        CallMode::InvalidParams,
        Duration::ZERO,
        Failure::None,
        |_| {},
    )
    .await;
    let (http, _, reply) = call(&w, "echo", json!({})).await;
    assert_eq!(http, StatusCode::OK);
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    assert_eq!(text_of(&reply), "The server rejected the arguments.");
    let rec = w.h.sink.wait_for(1).await.remove(0);
    assert_eq!(rec.status, 400);
}

#[tokio::test]
async fn a_call_refused_after_the_limit_gives_its_slot_back() {
    let w = world().await;
    guardrail(&w, "g", word_rule("swordfish", "block", "input")).await;
    limit_requests(&w, 1).await;
    for _ in 0..3 {
        let (status, _, reply) = call(&w, "echo", json!({"q": "swordfish"})).await;
        assert_eq!(status, StatusCode::OK, "{reply}");
        assert_eq!(reply["result"]["isError"], true);
    }
    // The one request of the minute is still there.
    let (status, _, reply) = call(&w, "echo", json!({"q": "fine"})).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["result"]["isError"], false, "{reply}");
}

fn spent_record() -> RequestRecord {
    RequestRecord {
        tags: Default::default(),
        key_id: None,
        user_id: None,
        team_id: None,
        requested: "x".into(),
        endpoint: "chat",
        stream: false,
        status: 200,
        usage: None,
        attempts: Vec::new(),
        cached: false,
        estimated: false,
        started_at: ultrafast_gateway::store::now(),
        duration_ms: 1,
        trace_parent: None,
        provider_kinds: Vec::new(),
        started_unix_ms: 0,
        guardrails: None,
        prompt: None,
    }
}

#[tokio::test]
async fn a_spent_budget_is_402_before_the_upstream_and_gives_the_slot_back() {
    let w = world().await;
    limit_requests(&w, 1).await;
    let mut tx = w.h.store.begin().await.unwrap();
    tx.upsert_budget(
        LimitScope::Gateway,
        None,
        10,
        Period::Daily,
        BudgetAction::Block,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    w.h.state.refresh().await.unwrap();
    account(&w.h.state, &spent_record(), 10, OffsetDateTime::now_utc());
    for _ in 0..3 {
        let (status, headers, reply) = call(&w, "echo", json!({})).await;
        assert_eq!(status, StatusCode::PAYMENT_REQUIRED, "{reply}");
        assert!(headers.contains_key("retry-after"));
        assert!(reply["error"]["message"]
            .as_str()
            .unwrap()
            .contains("budget"));
    }
    assert_eq!(upstream_calls(&w), 0);
    let budgets = w.h.store.list_budgets().await.unwrap();
    let mut tx = w.h.store.begin().await.unwrap();
    for b in budgets {
        tx.delete_budget(b.id).await.unwrap();
    }
    tx.commit().await.unwrap();
    w.h.state.refresh().await.unwrap();
    assert_eq!(call(&w, "echo", json!({})).await.0, StatusCode::OK);
    assert_eq!(upstream_calls(&w), 1);
}

#[tokio::test]
async fn a_blocked_input_is_a_tool_error_with_no_upstream_call() {
    let w = world().await;
    guardrail(&w, "g", word_rule("swordfish", "block", "input")).await;
    let (status, _, reply) = call(&w, "echo", json!({"q": "the swordfish"})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    assert_eq!(text_of(&reply), "Blocked by guardrail 'g'.");
    assert!(reply.get("error").is_none());
    assert_eq!(upstream_calls(&w), 0);
    let rec = w.h.sink.wait_for(1).await.remove(0);
    assert_eq!(rec.status, 400);
    let found = rec.guardrails.expect("the guardrail column is set");
    assert_eq!(found.input.unwrap().blocked_by.unwrap().name, "g");
    assert!(rec.attempts.is_empty());
}

#[tokio::test]
async fn a_redaction_of_the_input_is_what_the_upstream_gets() {
    let w = world().await;
    guardrail(
        &w,
        "pii",
        json!({ "id": "e", "matcher": { "pii": ["EMAIL"] }, "action": "redact", "directions": "input" }),
    )
    .await;
    let (_, _, reply) = call(&w, "echo", json!({"q": "mail ada@example.com now"})).await;
    assert_eq!(reply["result"]["isError"], false, "{reply}");
    let sent = &w.mock.received_method("tools/call")[0].body;
    let args = sent["params"]["arguments"].to_string();
    assert!(!args.contains("ada@example.com"), "{args}");
    assert!(args.contains("mail"), "{args}");
}

#[tokio::test]
async fn a_blocked_output_is_a_tool_error_and_the_text_does_not_reach_the_agent() {
    let w = world().await;
    guardrail(&w, "g", word_rule("xyzzy", "block", "output")).await;
    let (_, _, reply) = call(&w, "echo", json!({"q": "xyzzy"})).await;
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    assert_eq!(text_of(&reply), "Blocked by guardrail 'g'.");
    assert!(!reply.to_string().contains("xyzzy"));
    assert_eq!(upstream_calls(&w), 1);
    let rec = w.h.sink.wait_for(1).await.remove(0);
    assert!(rec.guardrails.unwrap().output.is_some());
}

#[tokio::test]
async fn an_oversize_result_is_a_tool_error_and_a_failed_attempt() {
    let w = world_with(
        CallMode::Oversize(20_000),
        Duration::ZERO,
        Failure::None,
        |s| {
            s.mcp.limits.max_result_bytes = 4096;
        },
    )
    .await;
    let (status, _, reply) = call(&w, "echo", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    assert!(reply.to_string().len() < 1000);
    let rec = w.h.sink.wait_for(1).await.remove(0);
    assert_eq!(rec.status, 502);
    assert_eq!(rec.attempts[0].outcome, AttemptOutcome::Fatal);
}

#[tokio::test]
async fn a_call_past_the_timeout_is_a_tool_error_and_a_failed_attempt() {
    let w = world_with(CallMode::Echo, Duration::from_secs(5), Failure::None, |s| {
        s.mcp.limits.call_timeout = Duration::from_millis(300)
    })
    .await;
    let started = Instant::now();
    let (status, _, reply) = call(&w, "echo", json!({})).await;
    assert!(started.elapsed() < Duration::from_secs(4));
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    assert!(text_of(&reply).contains("in time"), "{reply}");
    let rec = w.h.sink.wait_for(1).await.remove(0);
    assert_eq!(rec.status, 504);
    assert_eq!(rec.attempts[0].outcome, AttemptOutcome::Fatal);
}

#[tokio::test]
async fn a_server_at_its_concurrency_cap_refuses_the_next_call() {
    let w = world_with(
        CallMode::Echo,
        Duration::from_millis(800),
        Failure::None,
        |s| {
            s.mcp.limits.calls_per_server = 1;
        },
    )
    .await;
    let (a, b) = tokio::join!(call(&w, "echo", json!({})), async {
        tokio::time::sleep(Duration::from_millis(200)).await;
        call(&w, "echo", json!({})).await
    });
    assert_eq!(a.0, StatusCode::OK);
    assert_eq!(b.0, StatusCode::SERVICE_UNAVAILABLE, "{}", b.2);
    assert_eq!(upstream_calls(&w), 1);
}

#[tokio::test]
async fn a_redirect_from_the_upstream_is_not_followed_and_the_secret_stays_home() {
    let target = MockMcp::builder().start().await;
    let w = world_with(
        CallMode::Echo,
        Duration::ZERO,
        Failure::Redirect(target.url.clone()),
        |_| {},
    )
    .await;
    let (status, _, reply) = call(&w, "echo", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    assert!(target.received().is_empty());
    assert!(!reply.to_string().contains(SECRET));
}

struct Observed {
    w: World,
    collector: MockServer,
    stop: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
}

async fn observed() -> Observed {
    let collector = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/traces"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .mount(&collector)
        .await;
    let (stop, stopped) = watch::channel(false);
    let endpoint = collector.uri();
    let slot = std::sync::Arc::new(std::sync::Mutex::new(None));
    let slot2 = slot.clone();
    let w = world_with(
        CallMode::Echo,
        Duration::ZERO,
        Failure::None,
        move |state| {
            let (exporter, task) = Exporter::spawn(
                OtelConfig {
                    endpoint,
                    headers: vec![],
                    service_name: "ultrafast-test".into(),
                    sample_ratio: 1.0,
                },
                state.http.clone(),
                state.metrics.clone(),
                stopped,
            );
            state.otel = Some(exporter);
            state.metrics_token = Some(METRICS_TOKEN.into());
            *slot2.lock().unwrap() = Some(task);
        },
    )
    .await;
    let task = slot.lock().unwrap().take().unwrap();
    Observed {
        w,
        collector,
        stop,
        task,
    }
}

#[tokio::test]
async fn a_call_is_counted_timed_and_exported_as_an_mcp_call_span() {
    let o = observed().await;
    let (_, _, reply) = call(&o.w, "echo", json!({"q": "hi"})).await;
    assert_eq!(reply["result"]["isError"], false);
    call(&o.w, "secret", json!({})).await;
    let req = axum::http::Request::builder()
        .uri("/metrics")
        .header("authorization", format!("Bearer {METRICS_TOKEN}"))
        .body(axum::body::Body::empty())
        .unwrap();
    let resp = tower::ServiceExt::oneshot(o.w.h.app.clone(), req)
        .await
        .unwrap();
    let text = String::from_utf8(
        axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(
        text.contains(r#"uf_mcp_calls_total{server="mock",tool="echo",status="ok"} 1"#),
        "{text}"
    );
    assert!(text.contains("# TYPE uf_mcp_call_duration_seconds histogram"));
    assert!(text.contains(r#"uf_mcp_call_duration_seconds_count{server="mock"} 1"#));
    // A name a caller made up is no label.
    assert!(!text.contains("tool=\"secret\""));
    o.stop.send(true).unwrap();
    o.task.await.unwrap();
    let reqs = o.collector.received_requests().await.unwrap();
    let spans: Vec<Value> = reqs
        .iter()
        .flat_map(|r| {
            let v: Value = serde_json::from_slice(&r.body).unwrap();
            v["resourceSpans"][0]["scopeSpans"][0]["spans"]
                .as_array()
                .unwrap()
                .clone()
        })
        .collect();
    let span = spans
        .iter()
        .find(|s| s["name"] == "mcp.call")
        .unwrap_or_else(|| panic!("no mcp.call span in {spans:?}"));
    let attr = |k: &str| {
        span["attributes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["key"] == k)
            .map(|a| a["value"]["stringValue"].clone())
    };
    assert_eq!(attr("mcp.server"), Some(json!("mock")));
    assert_eq!(attr("mcp.tool"), Some(json!("echo")));
    assert!(!format!("{spans:?}").contains(SECRET));
}

#[test]
fn the_mcp_endpoint_is_not_in_the_admin_spec() {
    let spec = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../openapi/admin.json"
    ))
    .unwrap();
    let v: Value = serde_json::from_str(&spec).unwrap();
    for p in v["paths"].as_object().unwrap().keys() {
        assert!(!p.starts_with("/mcp"), "{p}");
    }
}

#[tokio::test]
async fn a_keys_allowlist_cuts_what_it_may_list_and_call() {
    let w = world().await;
    let key = ultrafast_gateway::secrets::generate_key();
    let id =
        w.h.store
            .insert_key("limited", &key.hash, &key.display, None)
            .await
            .unwrap();
    let send = |body: Value| {
        let k = key.full.clone();
        let w = &w;
        async move { post_raw(w, "mock", Some(&k), &[], &body.to_string()).await }
    };
    let list = json!({"jsonrpc":"2.0","id":1,"method":"tools/list"});
    // An allowlist that names models only leaves no tool.
    let mut tx = w.h.store.begin().await.unwrap();
    tx.set_key_allowed(id, Some(&["p/gpt-4o".to_string()]))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    w.h.state.refresh().await.unwrap();
    assert_eq!(send(list.clone()).await.0, StatusCode::NOT_FOUND);
    let mut tx = w.h.store.begin().await.unwrap();
    tx.set_key_allowed(id, Some(&["mcp:mock/echo".to_string()]))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    w.h.state.refresh().await.unwrap();
    let (status, _, reply) = send(list).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["result"]["tools"][0]["name"], "echo");
}

#[tokio::test]
async fn resources_and_prompts_are_passed_to_the_upstream_with_its_secret() {
    let w = world().await;
    let (status, _, reply) = rpc(&w, "resources/list", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reply["result"]["resources"][0]["uri"], "file:///a");
    let (_, _, reply) = rpc(&w, "resources/read", json!({"uri": "file:///a"})).await;
    assert_eq!(reply["result"]["contents"][0]["uri"], "file:///a");
    let (_, _, reply) = rpc(&w, "prompts/list", json!({})).await;
    assert_eq!(reply["result"]["prompts"][0]["name"], "p");
    let (_, _, reply) = rpc(&w, "prompts/get", json!({"name": "p"})).await;
    assert_eq!(reply["result"]["messages"][0]["role"], "user");
    let seen = w.mock.received_method("resources/list");
    assert_eq!(
        seen[0].header_values("authorization"),
        [format!("Bearer {SECRET}")]
    );
    // The agent's own key never goes upstream.
    assert!(w.mock.received().iter().all(|r| !r
        .header_values("authorization")
        .iter()
        .any(|v| v.contains(&w.h.key))));
    // A request that is not an object is refused before the upstream.
    let (_, _, reply) = rpc(&w, "resources/read", json!([1])).await;
    assert_eq!(reply["error"]["code"], -32602);
}

#[tokio::test]
async fn passed_methods_need_a_key_and_a_server_it_may_use() {
    let w = world().await;
    let body = json!({"jsonrpc":"2.0","id":1,"method":"resources/list"}).to_string();
    let (status, _, _) = post_raw(&w, "mock", None, &[], &body).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _, _) = post_raw(&w, "nope", Some(&w.h.key), &[], &body).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(w.mock.received_method("resources/list").is_empty());
}

#[tokio::test]
async fn a_passed_result_over_the_cap_is_refused_and_an_unsupported_method_is_not_found() {
    let w2 = world_with(CallMode::Echo, Duration::ZERO, Failure::None, |s| {
        s.mcp.limits.max_result_bytes = 2_000;
    })
    .await;
    let mock2 = MockMcp::builder()
        .tools(vec![tool("echo", "echoes")])
        .resource_bytes(10_000)
        .start()
        .await;
    let mut tx = w2.h.store.begin().await.unwrap();
    tx.update_server(
        w2.server_id,
        NewMcpServer {
            name: "mock",
            url: &mock2.url,
            auth_kind: "none",
            auth_header: None,
            auth_secret_enc: None,
            enabled: true,
            description: "d",
            ..Default::default()
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    w2.h.state.refresh().await.unwrap();
    let (status, _, reply) = rpc(&w2, "resources/read", json!({"uri": "file:///a"})).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(reply["error"]["code"], -32000);
    assert!(!reply.to_string().contains("rrrr"));

    let w3 = world_with(CallMode::Echo, Duration::ZERO, Failure::None, |_| {}).await;
    let none = MockMcp::builder()
        .tools(vec![tool("echo", "echoes")])
        .without_resources()
        .start()
        .await;
    let mut tx = w3.h.store.begin().await.unwrap();
    tx.update_server(
        w3.server_id,
        NewMcpServer {
            name: "mock",
            url: &none.url,
            auth_kind: "none",
            auth_header: None,
            auth_secret_enc: None,
            enabled: true,
            description: "d",
            ..Default::default()
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    w3.h.state.refresh().await.unwrap();
    let (status, _, reply) = rpc(&w3, "prompts/list", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reply["error"]["code"], -32601);
}

#[tokio::test]
async fn a_tool_call_through_the_gateway_to_a_stateful_upstream_works() {
    let mock = MockMcp::builder()
        .tools(vec![tool("echo", "echoes")])
        .require_session()
        .start()
        .await;
    let w = world().await;
    let mut tx = w.h.store.begin().await.unwrap();
    tx.update_server(
        w.server_id,
        NewMcpServer {
            name: "mock",
            url: &mock.url,
            auth_kind: "none",
            auth_header: None,
            auth_secret_enc: None,
            enabled: true,
            description: "d",
            ..Default::default()
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    w.h.state.refresh().await.unwrap();
    let (_, headers, reply) = call(&w, "echo", json!({"a": 1})).await;
    assert_eq!(reply["result"]["isError"], false, "{reply}");
    // Toward the agent the endpoint stays stateless.
    assert!(headers.get("mcp-session-id").is_none());
    assert_eq!(mock.received_method("initialize").len(), 1);
}

// ---------------------------------------------------------------- health

async fn probe(w: &World) -> ultrafast_gateway::mcp::health::Probe {
    probe_once(&w.h.state).await;
    w.h.state.mcp_health.get(w.server_id).expect("a probe")
}

#[tokio::test]
async fn probes_tell_ok_degraded_down_and_back() {
    let w = world().await;
    let first = probe(&w).await;
    assert_eq!(first.status, Status::Ok);
    assert!(first.latency_ms.is_some());
    assert!(first.checked_at.starts_with("20"));
    // The probe asks for a tool list, outside any key.
    assert!(!w.mock.received_method("tools/list").is_empty());
    w.mock.set_failure(Failure::Status500);
    // One failure is a warning, two in a row are down.
    assert_eq!(probe(&w).await.status, Status::Degraded);
    let down = probe(&w).await;
    assert_eq!(down.status, Status::Down);
    assert_eq!(down.latency_ms, None);
    assert_eq!(probe(&w).await.status, Status::Down);
    w.mock.set_failure(Failure::None);
    assert_eq!(probe(&w).await.status, Status::Ok);
}

#[tokio::test]
async fn a_slow_answer_is_degraded() {
    let w = world_with(CallMode::Echo, Duration::ZERO, Failure::None, |_| {}).await;
    w.mock.set_failure(Failure::SlowList(2_300));
    assert_eq!(probe(&w).await.status, Status::Degraded);
}

#[tokio::test]
async fn probes_take_no_permit_cost_no_budget_and_leave_no_log_row() {
    let w = world().await;
    limit_requests(&w, 1).await;
    for _ in 0..5 {
        probe_once(&w.h.state).await;
    }
    // The one request the limit allows is still there for a key.
    let (_, _, reply) = call(&w, "echo", json!({})).await;
    assert_eq!(reply["result"]["isError"], false, "{reply}");
    // And the only record is that call's.
    let records = w.h.sink.records();
    assert_eq!(records.len(), 1, "{records:?}");
    assert_eq!(records[0].endpoint, "mcp");
}

#[tokio::test]
async fn the_gauge_follows_the_probes() {
    let w = world_with(CallMode::Echo, Duration::ZERO, Failure::None, |s| {
        s.metrics_token = Some(METRICS_TOKEN.into());
    })
    .await;
    async fn text(w: &World) -> String {
        let req = axum::http::Request::builder()
            .uri("/metrics")
            .header("authorization", format!("Bearer {METRICS_TOKEN}"))
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = tower::ServiceExt::oneshot(w.h.app.clone(), req)
            .await
            .unwrap();
        String::from_utf8(
            axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap()
    }
    assert!(!text(&w).await.contains("uf_mcp_server_up{"));
    probe_once(&w.h.state).await;
    assert!(text(&w)
        .await
        .contains("uf_mcp_server_up{server=\"mock\"} 1"));
    w.mock.set_failure(Failure::Status500);
    probe_once(&w.h.state).await;
    probe_once(&w.h.state).await;
    assert!(text(&w)
        .await
        .contains("uf_mcp_server_up{server=\"mock\"} 0"));
}

#[tokio::test]
async fn a_server_that_is_off_or_gone_is_not_probed() {
    let w = world().await;
    probe_once(&w.h.state).await;
    let before = w.mock.received_method("tools/list").len();
    let mut tx = w.h.store.begin().await.unwrap();
    let row = tx.server_by_id(w.server_id).await.unwrap().unwrap();
    tx.update_server(
        w.server_id,
        NewMcpServer {
            name: "mock",
            url: &row.url,
            auth_kind: &row.auth_kind,
            auth_header: row.auth_header.as_deref(),
            auth_secret_enc: row.auth_secret_enc.as_deref(),
            enabled: false,
            description: "d",
            ..Default::default()
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    w.h.state.refresh().await.unwrap();
    probe_once(&w.h.state).await;
    assert_eq!(w.mock.received_method("tools/list").len(), before);
}

// ------------------------------------------------------------- combined

/// A second server "other-srv" with `echo` and `b__c`, both on and granted to everyone.
async fn second_server(w: &World) -> MockMcp {
    let mock = MockMcp::builder()
        .tools(vec![tool("echo", "other echo"), tool("b__c", "odd name")])
        .start()
        .await;
    let mut tx = w.h.store.begin().await.unwrap();
    let id = tx
        .insert_server(NewMcpServer {
            name: "other-srv",
            url: &mock.url,
            ..Default::default()
        })
        .await
        .unwrap();
    tx.upsert_tools(id, vec![discovered("echo"), discovered("b__c")])
        .await
        .unwrap();
    tx.commit().await.unwrap();
    for t in w.h.store.list_tools(id).await.unwrap() {
        let mut tx = w.h.store.begin().await.unwrap();
        tx.set_tool_enabled(t.id, true).await.unwrap();
        tx.replace_tool_grants(
            t.id,
            &Grants {
                everyone: true,
                ..Grants::default()
            },
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
    }
    w.h.state.refresh().await.unwrap();
    mock
}

async fn combined(
    w: &World,
    key: Option<&str>,
    method: &str,
    params: Value,
) -> (StatusCode, Value) {
    let bearer = key.map(|k| format!("Bearer {k}"));
    let mut headers = Vec::new();
    if let Some(b) = &bearer {
        headers.push(("authorization", b.as_str()));
    }
    let body = json!({"jsonrpc": "2.0", "id": 3, "method": method, "params": params});
    let (status, _, text) = post_to(&w.h.app, "/mcp", &headers, &body.to_string()).await;
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

#[tokio::test]
async fn the_combined_endpoint_lists_every_callable_tool_by_namespaced_name_and_routes_calls() {
    let w = world().await;
    let other = second_server(&w).await;
    let key = Some(w.h.key.as_str());
    let (status, list) = combined(&w, key, "tools/list", json!({})).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    // `mock/secret` is off, so it is not there.
    assert_eq!(names, ["mock__echo", "other-srv__b__c", "other-srv__echo"]);
    assert_eq!(
        list["result"]["tools"][0]["_meta"]["ultrafast"]["server"],
        "mock"
    );

    // A call goes to the server in the name, with the tool as that server knows it.
    let (_, reply) = combined(
        &w,
        key,
        "tools/call",
        json!({"name": "other-srv__b__c", "arguments": {"q": "x"}}),
    )
    .await;
    assert_eq!(reply["result"]["isError"], false, "{reply}");
    let calls = other.received_method("tools/call");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].body["params"]["name"], "b__c");
    assert!(w.mock.received_method("tools/call").is_empty());
    let (_, reply) = combined(
        &w,
        key,
        "tools/call",
        json!({"name": "mock__echo", "arguments": {}}),
    )
    .await;
    assert_eq!(reply["result"]["isError"], false, "{reply}");
    assert_eq!(w.mock.received_method("tools/call").len(), 1);
    // The record names the server and the tool as before.
    let recs = w.h.sink.wait_for(2).await;
    assert_eq!(recs[0].requested, "mcp:other-srv/b__c");
    assert_eq!(recs[1].requested, "mcp:mock/echo");

    // Names that lead nowhere are the same Unknown tool.
    for name in [
        "echo",
        "nope__x",
        "mock__secret",
        "mock__",
        "__echo",
        "mock__nope",
    ] {
        let (status, reply) = combined(&w, key, "tools/call", json!({"name": name})).await;
        assert_eq!(status, StatusCode::OK, "{name}");
        assert_eq!(reply["error"]["message"], "Unknown tool", "{name}: {reply}");
    }
    assert_eq!(w.mock.received_method("tools/call").len(), 1);
}

#[tokio::test]
async fn the_per_server_endpoints_are_unchanged_beside_the_combined_one() {
    let w = world().await;
    let _other = second_server(&w).await;
    let (_, _, reply) = rpc(&w, "tools/list", json!({})).await;
    let names: Vec<&str> = reply["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["echo"]);
    let (_, _, reply) = call(&w, "echo", json!({})).await;
    assert_eq!(reply["result"]["isError"], false);
    // A namespaced name is not a tool of a single server.
    let (_, _, reply) = call(&w, "mock__echo", json!({})).await;
    assert_eq!(reply["error"]["message"], "Unknown tool");
}

#[tokio::test]
async fn the_combined_endpoint_checks_the_key_the_method_and_the_verb() {
    let w = world().await;
    let (status, _) = combined(&w, None, "ping", json!({})).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, reply) = combined(&w, Some("uf-sk-wrong"), "ping", json!({})).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{reply}");
    let key = Some(w.h.key.as_str());
    let (status, reply) = combined(
        &w,
        key,
        "initialize",
        json!({"protocolVersion": "2025-06-18", "capabilities": {}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(reply["result"]["serverInfo"]["name"], "ultrafast-gateway");
    // Resources and prompts are per server.
    let (_, reply) = combined(&w, key, "resources/list", json!({})).await;
    assert_eq!(reply["error"]["code"], -32601);
    for verb in ["GET", "DELETE"] {
        let req = axum::http::Request::builder()
            .method(verb)
            .uri("/mcp")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = tower::ServiceExt::oneshot(w.h.app.clone(), req)
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED, "{verb}");
    }
}

#[tokio::test]
async fn a_key_limited_to_one_tool_sees_only_it_on_the_combined_endpoint() {
    let w = world().await;
    let _other = second_server(&w).await;
    // A key allowed `mcp:other-srv/echo` only.
    let key = ultrafast_gateway::secrets::generate_key();
    let id =
        w.h.store
            .insert_key("narrow", &key.hash, &key.display, None)
            .await
            .unwrap();
    let mut tx = w.h.store.begin().await.unwrap();
    tx.set_key_allowed(id, Some(&["mcp:other-srv/echo".to_string()]))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let made = key.full.clone();
    w.h.state.refresh().await.unwrap();
    let (_, list) = combined(&w, Some(made.as_str()), "tools/list", json!({})).await;
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["other-srv__echo"]);
    let (_, reply) = combined(
        &w,
        Some(made.as_str()),
        "tools/call",
        json!({"name": "mock__echo"}),
    )
    .await;
    assert_eq!(reply["error"]["message"], "Unknown tool");
}

// ------------------------------------------------------------- discovery

/// Three tools are callable (`mock/echo`, `other-srv/echo`, `other-srv/b__c`) and
/// the list is cut at two.
async fn crowded() -> (World, MockMcp) {
    let w = world_with(CallMode::Echo, Duration::ZERO, Failure::None, |s| {
        s.mcp.limits.list_inline_max = 2;
    })
    .await;
    let other = second_server(&w).await;
    (w, other)
}

fn names_of(list: &Value) -> Vec<String> {
    list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn past_the_threshold_the_list_is_the_search_tools_and_the_pinned_ones() {
    let (w, _other) = crowded().await;
    let key = Some(w.h.key.as_str());
    let (_, list) = combined(&w, key, "tools/list", json!({})).await;
    assert_eq!(names_of(&list), ["find_tools", "describe_tools"]);
    // Pinned: listed after them, in the order pinned, if the key can still call them.
    let id = w.h.store.list_keys().await.unwrap()[0].id;
    let mut tx = w.h.store.begin().await.unwrap();
    tx.set_key_pinned_tools(
        id,
        &[
            "mcp:other-srv/echo".to_string(),
            "mcp:mock/secret".to_string(), // off: not listed
            "mcp:mock/echo".to_string(),
        ],
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    w.h.state.refresh().await.unwrap();
    let (_, list) = combined(&w, key, "tools/list", json!({})).await;
    assert_eq!(
        names_of(&list),
        [
            "find_tools",
            "describe_tools",
            "other-srv__echo",
            "mock__echo"
        ]
    );
    // The per-server endpoint is not cut.
    let (_, _, reply) = rpc(&w, "tools/list", json!({})).await;
    assert_eq!(names_of(&reply), ["echo"]);
}

#[tokio::test]
async fn find_tools_ranks_the_tools_the_key_may_call_and_describe_tools_gives_their_schemas() {
    let (w, other) = crowded().await;
    let key = Some(w.h.key.as_str());
    let (_, found) = combined(
        &w,
        key,
        "tools/call",
        json!({"name": "find_tools", "arguments": {"query": "b c", "limit": 2}}),
    )
    .await;
    assert_eq!(found["result"]["isError"], false, "{found}");
    let tools = found["result"]["structuredContent"]["tools"]
        .as_array()
        .unwrap();
    assert_eq!(tools[0]["name"], "other-srv__b__c");
    assert_eq!(tools[0]["server"], "other-srv");
    assert_eq!(tools[0]["description"], "does b__c");
    assert!(tools.len() <= 2);
    // An exact name comes first.
    let (_, found) = combined(
        &w,
        key,
        "tools/call",
        json!({"name": "find_tools", "arguments": {"query": "echo"}}),
    )
    .await;
    let names: Vec<&str> = found["result"]["structuredContent"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["mock__echo", "other-srv__echo"]);
    let (_, described) = combined(
        &w,
        key,
        "tools/call",
        json!({"name": "describe_tools", "arguments": {"names": ["other-srv__b__c", "mock__secret", "nope", "x__y"]}}),
    )
    .await;
    let tools = described["result"]["structuredContent"]["tools"]
        .as_array()
        .unwrap();
    assert_eq!(tools.len(), 1, "{described}");
    assert_eq!(tools[0]["name"], "other-srv__b__c");
    assert_eq!(tools[0]["inputSchema"]["type"], "object");
    // Neither goes upstream, and neither is a logged call.
    assert!(other.received_method("tools/call").is_empty());
    assert!(w.mock.received_method("tools/call").is_empty());
    assert!(w.h.sink.records().is_empty());
    // Bad arguments are told, not guessed.
    for args in [
        json!({}),
        json!({"query": " "}),
        json!({"query": "a", "limit": 99}),
        json!({"query": "a", "limit": 0}),
    ] {
        let (_, reply) = combined(
            &w,
            key,
            "tools/call",
            json!({"name": "find_tools", "arguments": args}),
        )
        .await;
        assert_eq!(reply["error"]["code"], -32602, "{args}: {reply}");
    }
    let (_, reply) = combined(
        &w,
        key,
        "tools/call",
        json!({"name": "describe_tools", "arguments": {"names": []}}),
    )
    .await;
    assert_eq!(reply["error"]["code"], -32602);
}

#[tokio::test]
async fn a_search_never_shows_a_tool_the_key_may_not_call() {
    let (w, _other) = crowded().await;
    // A key allowed `other-srv/echo` only.
    let key = ultrafast_gateway::secrets::generate_key();
    let id =
        w.h.store
            .insert_key("narrow", &key.hash, &key.display, None)
            .await
            .unwrap();
    let mut tx = w.h.store.begin().await.unwrap();
    tx.set_key_allowed(id, Some(&["mcp:other-srv/echo".to_string()]))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    w.h.state.refresh().await.unwrap();
    let narrow = Some(key.full.as_str());
    // `mock/echo` says "echoes" and `b__c` says "odd name": neither is visible.
    for query in ["echoes", "does echo", "b c", "mock"] {
        let (_, found) = combined(
            &w,
            narrow,
            "tools/call",
            json!({"name": "find_tools", "arguments": {"query": query}}),
        )
        .await;
        let text = found["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default();
        assert!(!text.contains("mock__"), "{query}: {text}");
        assert!(!text.contains("b__c"), "{query}: {text}");
    }
    let (_, found) = combined(
        &w,
        narrow,
        "tools/call",
        json!({"name": "find_tools", "arguments": {"query": "other echo"}}),
    )
    .await;
    assert_eq!(
        found["result"]["structuredContent"]["tools"][0]["name"],
        "other-srv__echo"
    );
    let (_, described) = combined(
        &w,
        narrow,
        "tools/call",
        json!({"name": "describe_tools", "arguments": {"names": ["mock__echo", "other-srv__b__c"]}}),
    )
    .await;
    assert_eq!(described["result"]["structuredContent"]["tools"], json!([]));
}

#[tokio::test]
async fn the_search_tools_count_against_the_rate_limit() {
    let (w, _other) = crowded().await;
    limit_requests(&w, 1).await;
    let key = Some(w.h.key.as_str());
    let call = || {
        combined(
            &w,
            key,
            "tools/call",
            json!({"name": "find_tools", "arguments": {"query": "echo"}}),
        )
    };
    assert_eq!(call().await.0, StatusCode::OK);
    assert_eq!(call().await.0, StatusCode::TOO_MANY_REQUESTS);
}

// ------------------------------------------------------------ projection

async fn project(w: &World, keep: &[&str], drop: &[&str], items: Option<i64>, bytes: Option<i64>) {
    let mut tx = w.h.store.begin().await.unwrap();
    tx.put_projection(&ultrafast_gateway::store::ProjectionRow {
        tool_id: w.echo_id,
        keep: keep.iter().map(|s| (*s).to_string()).collect(),
        drop: drop.iter().map(|s| (*s).to_string()).collect(),
        max_items: items,
        max_bytes: bytes,
    })
    .await
    .unwrap();
    tx.commit().await.unwrap();
    w.h.state.refresh().await.unwrap();
}

#[tokio::test]
async fn a_projection_cuts_the_result_and_marks_it_and_the_log_says_how_much_was_left() {
    let w = world().await;
    project(&w, &["$.public"], &[], None, None).await;
    let (_, _, reply) = call(
        &w,
        "echo",
        json!({"public": "ok", "hidden": "x".repeat(500)}),
    )
    .await;
    assert_eq!(reply["result"]["isError"], false, "{reply}");
    assert_eq!(text_of(&reply), "{\"public\":\"ok\"}");
    assert_eq!(reply["result"]["_meta"]["ultrafast"]["projected"], true);
    let rec = w.h.sink.wait_for(1).await.remove(0);
    let left = rec.attempts[0].projected_bytes.expect("projected bytes");
    assert!(left > 0 && left < 400, "{left}");
    // The log row carries it in the attempts.
    let prices: ultrafast_gateway::logs::PriceLookup = std::sync::Arc::new(|_, _| None);
    let row = ultrafast_gateway::logs::row_of(&rec, &prices);
    let attempts: Value = serde_json::from_str(&row.attempts).unwrap();
    assert_eq!(attempts[0]["projected_bytes"], left);
}

#[tokio::test]
async fn a_projection_runs_before_the_guardrails_read_the_result() {
    let w = world().await;
    guardrail(&w, "g", word_rule("xyzzy", "block", "output")).await;
    let args = json!({"public": "ok", "hidden": "xyzzy"});
    let (_, _, reply) = call(&w, "echo", args.clone()).await;
    assert_eq!(reply["result"]["isError"], true);
    project(&w, &[], &["$.hidden"], None, None).await;
    let (_, _, reply) = call(&w, "echo", args).await;
    assert_eq!(reply["result"]["isError"], false, "{reply}");
    assert_eq!(text_of(&reply), "{\"public\":\"ok\"}");
}

#[tokio::test]
async fn a_projection_that_removes_every_field_leaves_an_object_and_the_marker() {
    let w = world().await;
    project(&w, &["$.nothing"], &[], None, None).await;
    let (_, _, reply) = call(&w, "echo", json!({"a": 1})).await;
    assert_eq!(text_of(&reply), "{}");
    assert_eq!(reply["result"]["_meta"]["ultrafast"]["projected"], true);
    assert_eq!(reply["result"]["isError"], false);
}

#[tokio::test]
async fn a_result_a_projection_does_not_change_has_no_marker_and_other_tools_are_untouched() {
    let w = world().await;
    project(&w, &[], &[], Some(10), None).await;
    let (_, _, reply) = call(&w, "echo", json!({"a": 1})).await;
    assert_eq!(text_of(&reply), "{\"a\":1}");
    assert!(reply["result"].get("_meta").is_none(), "{reply}");
    let rec = w.h.sink.wait_for(1).await.remove(0);
    assert_eq!(rec.attempts[0].projected_bytes, None);
}
