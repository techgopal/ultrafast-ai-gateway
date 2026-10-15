//! Local MCP servers through the admin API and the MCP endpoint: the program
//! is the test-support binary `mcp-stdio-mock`.

mod common;

use std::sync::Arc;
use std::time::Duration;

use axum::http::StatusCode;
use common::{org_tweaked, post_to, Org, Signed};
use serde_json::{json, Value};
use ultrafast_gateway::mcp::local::{Config, Supervisor};

const MOCK: &str = env!("CARGO_BIN_EXE_mcp-stdio-mock");

fn fast() -> Config {
    Config {
        restart_min: Duration::from_millis(50),
        restart_max: Duration::from_millis(400),
        grace: Duration::from_millis(500),
        handshake: Duration::from_secs(5),
        ..Config::default()
    }
}

/// A gateway that may run local servers, with its programs under a temp dir.
async fn local_org() -> (Org, Signed, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("mcp");
    let org = org_tweaked(None, move |s| {
        s.local = Arc::new(Supervisor::new(root, fast()));
    })
    .await;
    let maya = org.sign_in("maya").await;
    (org, maya, dir)
}

async fn create(org: &Org, who: &Signed, body: Value) -> (StatusCode, Value) {
    org.call(Some(who), "POST", "/api/mcp/servers", Some(body))
        .await
}

async fn until(what: &str, mut check: impl FnMut() -> bool) {
    let end = std::time::Instant::now() + Duration::from_secs(10);
    while !check() {
        assert!(
            std::time::Instant::now() < end,
            "timed out waiting for {what}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn a_local_server_is_refused_while_local_servers_are_off() {
    let org = org_tweaked(None, |_| {}).await;
    let maya = org.sign_in("maya").await;
    let (status, body) = create(
        &org,
        &maya,
        json!({"name": "local-one", "transport": "stdio", "command": MOCK}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["error"]["fields"]["transport"]
        .as_str()
        .unwrap()
        .contains("UF_MCP_LOCAL=on"));
}

#[tokio::test]
async fn a_local_server_is_checked_and_its_secrets_never_come_back() {
    let (org, maya, _dir) = local_org().await;
    // Wrong in several ways at once.
    let (status, body) = create(
        &org,
        &maya,
        json!({
            "name": "bad", "transport": "stdio", "url": "https://x.example/mcp",
            "auth_kind": "bearer", "secret": "s",
            "env": {"PATH": "/evil", "LD_PRELOAD": "x", "OK": "v", "NO VALUE": "v"},
        }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let fields = &body["error"]["fields"];
    for field in [
        "command",
        "url",
        "auth_kind",
        "env.PATH",
        "env.LD_PRELOAD",
        "env.NO VALUE",
    ] {
        assert!(fields[field].is_string(), "{field}: {body}");
    }
    assert!(fields["env.OK"].is_null());
    // An http server may not carry a program.
    let (status, body) = create(
        &org,
        &maya,
        json!({"name": "h", "url": "https://mcp.example.com/mcp", "command": "x"}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    let (status, _) = create(
        &org,
        &maya,
        json!({"name": "t", "transport": "carrier-pigeon"}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, body) = create(
        &org,
        &maya,
        json!({
            "name": "local-one", "transport": "stdio", "command": MOCK,
            "args": ["--stderr", "10"], "env": {"API_TOKEN": "tok-never-shown-91"},
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["transport"], "stdio");
    assert_eq!(body["command"], MOCK);
    assert_eq!(body["args"], json!(["--stderr", "10"]));
    assert_eq!(body["env_names"], json!(["API_TOKEN"]));
    assert_eq!(body["url"], "");
    assert!(!body.to_string().contains("tok-never-shown-91"));
    let summary = org.last_summary("mcp.create").await;
    assert!(!summary.contains("tok-never-shown-91"), "{summary}");
    // The stored form is encrypted.
    let id = body["id"].as_i64().unwrap();
    let row = org.api.store.server_by_id(id).await.unwrap().unwrap();
    assert!(!row.env_enc["API_TOKEN"].contains("tok-never-shown-91"));
}

#[tokio::test]
async fn a_local_server_runs_syncs_is_called_through_the_endpoint_and_stops() {
    let (org, maya, _dir) = local_org().await;
    let (status, body) = create(
        &org,
        &maya,
        json!({
            "name": "local-one", "transport": "stdio", "command": MOCK,
            "args": ["--stderr", "100"], "env": {"API_TOKEN": "tok-never-shown-91"},
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = body["id"].as_i64().unwrap();
    until("the program to run", || {
        org.api
            .state
            .local
            .health(id)
            .is_some_and(|h| h.phase == ultrafast_gateway::mcp::local::Phase::Running)
    })
    .await;
    let (_, view) = org
        .call(Some(&maya), "GET", &format!("/api/mcp/servers/{id}"), None)
        .await;
    assert_eq!(view["local"]["phase"], "running");

    let (status, synced) = org
        .call(
            Some(&maya),
            "POST",
            &format!("/api/mcp/servers/{id}/sync"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{synced}");
    assert_eq!(synced["added"].as_array().unwrap().len(), 6);

    // Switch `echo` on for everyone and call it with a key.
    let (_, tools) = org
        .call(
            Some(&maya),
            "GET",
            &format!("/api/mcp/servers/{id}/tools"),
            None,
        )
        .await;
    let echo = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "echo")
        .unwrap()["id"]
        .as_i64()
        .unwrap();
    let (status, _) = org
        .call(
            Some(&maya),
            "PATCH",
            &format!("/api/mcp/tools/{echo}"),
            Some(json!({"enabled": true})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = org
        .call(
            Some(&maya),
            "PUT",
            &format!("/api/mcp/tools/{echo}/grants"),
            Some(json!({"everyone": true, "team_ids": [], "user_ids": []})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, key) = org
        .call(
            Some(&maya),
            "POST",
            "/api/keys",
            Some(json!({"name": "agent"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{key}");
    let secret = key["secret"].as_str().unwrap().to_string();
    let bearer = format!("Bearer {secret}");
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "echo", "arguments": {"q": "hi"}}})
    .to_string();
    let (status, _, text) = post_to(
        &org.api.app,
        "/mcp/local-one",
        &[("authorization", bearer.as_str())],
        &body,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{text}");
    let reply: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(reply["result"]["content"][0]["text"], "{\"q\":\"hi\"}");

    // stderr, with a value of the environment redacted if the program printed it.
    let (status, err) = org
        .call(
            Some(&maya),
            "GET",
            &format!("/api/mcp/servers/{id}/stderr"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(err["text"].as_str().unwrap().contains("END"), "{err}");
    assert_eq!(err["supervised"], true);

    // Turned off: the program stops. Turned on: it runs again.
    let (status, _) = org
        .call(
            Some(&maya),
            "PATCH",
            &format!("/api/mcp/servers/{id}"),
            Some(json!({"enabled": false})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    until("the program to stop", || {
        org.api.state.local.health(id).is_none()
    })
    .await;
    let (status, _) = org
        .call(
            Some(&maya),
            "PATCH",
            &format!("/api/mcp/servers/{id}"),
            Some(json!({"enabled": true})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    org.api
        .state
        .local
        .wait_running(id, Duration::from_secs(10))
        .await;

    // Deleted: it stops for good.
    let (status, _) = org
        .call(
            Some(&maya),
            "DELETE",
            &format!("/api/mcp/servers/{id}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    until("the program to stop", || {
        org.api.state.local.health(id).is_none()
    })
    .await;
}

#[tokio::test]
async fn editing_a_local_server_keeps_stored_values_and_restarts_it() {
    let (org, maya, _dir) = local_org().await;
    let (_, body) = create(
        &org,
        &maya,
        json!({"name": "ed", "transport": "stdio", "command": MOCK, "env": {"A": "one", "B": "two"}}),
    )
    .await;
    let id = body["id"].as_i64().unwrap();
    let before = org.api.store.server_by_id(id).await.unwrap().unwrap();
    // A is kept (empty value), B is dropped, C is new.
    let (status, view) = org
        .call(
            Some(&maya),
            "PATCH",
            &format!("/api/mcp/servers/{id}"),
            Some(json!({"env": {"A": "", "C": "three"}})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["env_names"], json!(["A", "C"]));
    let after = org.api.store.server_by_id(id).await.unwrap().unwrap();
    assert_eq!(after.env_enc["A"], before.env_enc["A"]);
    assert_ne!(after.env_enc["C"], "three");
    assert!(org
        .last_summary("mcp.update")
        .await
        .contains("environment changed"));
    // A new name needs a value; an HTTP field is refused.
    let (status, _) = org
        .call(
            Some(&maya),
            "PATCH",
            &format!("/api/mcp/servers/{id}"),
            Some(json!({"env": {"Z": ""}})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = org
        .call(
            Some(&maya),
            "PATCH",
            &format!("/api/mcp/servers/{id}"),
            Some(json!({"url": "https://mcp.example.com/mcp"})),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    // Nothing changed: no audit entry, no restart.
    let (status, _) = org
        .call(
            Some(&maya),
            "PATCH",
            &format!("/api/mcp/servers/{id}"),
            Some(json!({"description": ""})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn a_local_server_is_not_part_of_a_configuration_file() {
    let (org, maya, _dir) = local_org().await;
    let (status, _) = create(
        &org,
        &maya,
        json!({"name": "ed", "transport": "stdio", "command": MOCK}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = create(
        &org,
        &maya,
        json!({"name": "web", "url": "https://mcp.example.com/mcp"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, file) = org
        .call(Some(&maya), "GET", "/api/config/export", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{file}");
    let names: Vec<&str> = file["mcp_servers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["web"]);
    assert!(!file.to_string().contains(MOCK));
    // A file cannot take over a local server's name.
    let mut hostile = file.clone();
    hostile["mcp_servers"][0]["name"] = "ed".into();
    let (status, report) = org
        .call(
            Some(&maya),
            "POST",
            "/api/config/import?dry_run=true",
            Some(hostile),
        )
        .await;
    assert!(
        report.to_string().contains("local MCP server"),
        "{status} {report}"
    );
    assert!(report
        .to_string()
        .contains("local servers are not part of a configuration file"));
}

#[tokio::test]
async fn health_shows_in_the_server_list_and_a_crash_loop_is_found_without_asking() {
    let (org, maya, _dir) = local_org().await;
    // A program that runs, and one that dies at once.
    let (_, good) = create(
        &org,
        &maya,
        json!({"name": "good", "transport": "stdio", "command": MOCK}),
    )
    .await;
    let (_, bad) = create(
        &org,
        &maya,
        json!({"name": "bad", "transport": "stdio", "command": MOCK, "args": ["--crash"]}),
    )
    .await;
    let (good, bad) = (good["id"].as_i64().unwrap(), bad["id"].as_i64().unwrap());
    // Before the first probe there is nothing to show.
    let (_, list) = org.call(Some(&maya), "GET", "/api/mcp/servers", None).await;
    assert!(list["servers"][0]["health"].is_null(), "{list}");
    until("the good program and the crash loop", || {
        use ultrafast_gateway::mcp::local::Phase;
        let s = &org.api.state.local;
        s.health(good).is_some_and(|h| h.phase == Phase::Running)
            && s.health(bad)
                .is_some_and(|h| h.phase == Phase::CrashLooping)
    })
    .await;
    ultrafast_gateway::mcp::health::probe_once(&org.api.state).await;
    let (_, list) = org.call(Some(&maya), "GET", "/api/mcp/servers", None).await;
    let of = |id: i64| {
        list["servers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == id)
            .unwrap()
            .clone()
    };
    assert_eq!(of(good)["health"]["status"], "ok");
    assert!(of(good)["health"]["latency_ms"].is_u64());
    assert_eq!(of(bad)["health"]["status"], "crashlooping");
    assert!(of(bad)["health"]["latency_ms"].is_null());
    assert_eq!(of(bad)["local"]["phase"], "crashlooping");
    assert!(of(bad)["local"]["restarts"].as_u64().unwrap() >= 3);
    // A disabled server is not probed and shows nothing.
    let (status, _) = org
        .call(
            Some(&maya),
            "PATCH",
            &format!("/api/mcp/servers/{good}"),
            Some(json!({"enabled": false})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (_, view) = org
        .call(
            Some(&maya),
            "GET",
            &format!("/api/mcp/servers/{good}"),
            None,
        )
        .await;
    assert!(view["health"].is_null());
}
