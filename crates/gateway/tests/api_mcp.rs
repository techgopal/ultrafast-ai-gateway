//! The MCP registry API: servers, tools, grants, the key allowlist and the
//! configuration file.

mod common;

use axum::http::StatusCode;
use common::{error_code, org, Org, Signed};
use serde_json::{json, Value};
use ufaigw_gateway::mcp::encrypt_secret;
use ufaigw_gateway::portable::{self, Actor, ConfigFile};
use ufaigw_gateway::store::{DiscoveredTool, NewMcpServer};

const SECRET: &str = "tok-0123456789-secret";

fn tool(name: &str) -> DiscoveredTool {
    DiscoveredTool {
        name: name.into(),
        title: Some(format!("{name} title")),
        description: format!("does {name}"),
        input_schema: r#"{"type":"object","properties":{"q":{"type":"string"}}}"#.into(),
        output_schema: None,
        annotations: Some(r#"{"readOnlyHint":true}"#.into()),
    }
}

/// A server (bearer secret `SECRET`) with the tools `names`, all switched
/// off and granted to nobody, as a sync leaves them. Returns its id and the
/// ids of the tools, in the order of `names`.
async fn seed_server(org: &Org, name: &str, names: &[&str]) -> (i64, Vec<i64>) {
    let enc = encrypt_secret(&org.api.state.cipher, SECRET);
    let mut tx = org.api.store.begin().await.unwrap();
    let id = tx
        .insert_server(NewMcpServer {
            name,
            url: "https://mcp.example.com/mcp",
            auth_kind: "bearer",
            auth_header: None,
            auth_secret_enc: Some(&enc),
            enabled: true,
            description: "d",
            ..Default::default()
        })
        .await
        .unwrap();
    tx.upsert_tools(id, names.iter().map(|n| tool(n)).collect())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    org.api.state.refresh().await.unwrap();
    let tools = org.api.store.list_tools(id).await.unwrap();
    let ids = names
        .iter()
        .map(|n| tools.iter().find(|t| t.name == *n).unwrap().id)
        .collect();
    (id, ids)
}

async fn put_grants(org: &Org, who: &Signed, tool: i64, body: Value) -> (StatusCode, Value) {
    org.call(
        Some(who),
        "PUT",
        &format!("/api/mcp/tools/{tool}/grants"),
        Some(body),
    )
    .await
}

async fn enable(org: &Org, who: &Signed, tool: i64, enabled: bool) -> (StatusCode, Value) {
    org.call(
        Some(who),
        "PATCH",
        &format!("/api/mcp/tools/{tool}"),
        Some(json!({ "enabled": enabled })),
    )
    .await
}

async fn tools_of(org: &Org, who: &Signed, server: i64) -> (StatusCode, Value) {
    let path = format!("/api/mcp/servers/{server}/tools");
    org.call(Some(who), "GET", &path, None).await
}

/// The tool names a caller sees; none when the server is hidden from them.
async fn names_of(org: &Org, who: &Signed, server: i64) -> Vec<String> {
    let (status, body) = tools_of(org, who, server).await;
    if status == StatusCode::NOT_FOUND {
        return Vec::new();
    }
    assert_eq!(status, StatusCode::OK);
    tool_names(&body).into_iter().map(String::from).collect()
}

async fn make_key(org: &Org, who: &Signed, body: Value) -> (StatusCode, Value) {
    org.call(Some(who), "POST", "/api/keys", Some(body)).await
}

fn tool_names(body: &Value) -> Vec<&str> {
    body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect()
}

fn server_names(body: &Value) -> Vec<&str> {
    body["servers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect()
}

fn grants(everyone: bool, teams: &[i64], users: &[i64]) -> Value {
    json!({ "everyone": everyone, "team_ids": teams, "user_ids": users })
}

#[tokio::test]
async fn an_admin_registers_a_server_and_the_secret_never_comes_back() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let (status, body) = org
        .call(
            Some(&maya),
            "POST",
            "/api/mcp/servers",
            Some(json!({
                "name": "git-hub", "url": "https://mcp.example.com/mcp",
                "auth_kind": "bearer", "secret": SECRET, "description": "Code search",
            })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["name"], "git-hub");
    assert_eq!(body["has_secret"], true);
    assert_eq!(body["usable"], true);
    assert_eq!(body["tool_count"], 0);
    assert_eq!(body["enabled"], true);
    assert!(!body.to_string().contains(SECRET));
    let id = body["id"].as_i64().unwrap();

    // Not in the database in the clear, and read by the gateway.
    let row = org.api.store.server_by_id(id).await.unwrap().unwrap();
    assert!(!row.auth_secret_enc.as_deref().unwrap().contains(SECRET));
    let snapshot = org.api.state.snapshot.load();
    let snap = snapshot.mcp_server("git-hub").unwrap();
    assert_eq!(snap.secret.as_deref(), Some(SECRET));
    assert!(snap.usable);
    assert!(!format!("{snap:?}").contains(SECRET));
    assert!(!format!("{snap:?}").contains("mcp.example.com"));

    let (status, again) = org
        .call(Some(&maya), "GET", &format!("/api/mcp/servers/{id}"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again, body);
    assert!(org
        .audit_actions()
        .await
        .contains(&"mcp.create".to_string()));
    assert!(!org.last_summary("mcp.create").await.contains(SECRET));

    let (status, dup) = org
        .call(
            Some(&maya),
            "POST",
            "/api/mcp/servers",
            Some(json!({ "name": "git-hub", "url": "https://other.example.com/mcp" })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&dup), "mcp_server_exists");

    let (status, _) = org
        .call(
            Some(&maya),
            "DELETE",
            &format!("/api/mcp/servers/{id}"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(org
        .api
        .state
        .snapshot
        .load()
        .mcp_server("git-hub")
        .is_none());
    let (status, _) = org
        .call(Some(&maya), "GET", &format!("/api/mcp/servers/{id}"), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn server_fields_are_checked() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let post = |body: Value| {
        let (org, maya) = (&org, &maya);
        async move {
            org.call(Some(maya), "POST", "/api/mcp/servers", Some(body))
                .await
        }
    };
    let url = "https://mcp.example.com/mcp";
    for name in ["", "-a", "A", "a_b", "a b", "a/b", &"a".repeat(64)] {
        let (status, body) = post(json!({ "name": name, "url": url })).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{name}");
        assert!(
            body["error"]["fields"]["name"].is_string(),
            "{name}: {body}"
        );
    }
    for bad in [
        "ftp://x",
        "https://u:p@x.example",
        "https://x.example/?q=1",
        "",
    ] {
        let (status, body) = post(json!({ "name": "ok", "url": bad })).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}");
        assert!(body["error"]["fields"]["url"].is_string(), "{bad}: {body}");
    }
    let cases = [
        (json!({ "auth_kind": "basic" }), "auth_kind"),
        (
            json!({ "auth_kind": "header", "secret": "s" }),
            "auth_header",
        ),
        (
            json!({ "auth_kind": "header", "auth_header": "bad header", "secret": "s" }),
            "auth_header",
        ),
        (json!({ "auth_kind": "bearer" }), "secret"),
        (json!({ "auth_kind": "bearer", "secret": "  " }), "secret"),
        (json!({ "secret": "s" }), "secret"),
        (json!({ "auth_header": "x-key" }), "auth_header"),
    ];
    for (extra, field) in cases {
        let mut body = json!({ "name": "ok", "url": url });
        body.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let (status, answer) = post(body.clone()).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert!(
            answer["error"]["fields"][field].is_string(),
            "{body}: {answer}"
        );
    }
    // A header server works and shows its header, not its secret.
    let (status, body) = post(json!({
        "name": "hdr", "url": url, "auth_kind": "header",
        "auth_header": "x-api-key", "secret": SECRET,
    }))
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["auth_header"], "x-api-key");
    assert!(!body.to_string().contains(SECRET));
}

#[tokio::test]
async fn changing_the_host_needs_the_secret_again() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let (id, _) = seed_server(&org, "s", &["t"]).await;
    let patch = |body: Value| {
        let (org, maya) = (&org, &maya);
        async move {
            org.call(
                Some(maya),
                "PATCH",
                &format!("/api/mcp/servers/{id}"),
                Some(body),
            )
            .await
        }
    };
    let (status, body) = patch(json!({ "url": "https://elsewhere.example.com/mcp" })).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["error"]["fields"]["secret"],
        "Enter the secret again: the host changed."
    );
    // Nothing changed.
    let row = org.api.store.server_by_id(id).await.unwrap().unwrap();
    assert_eq!(row.url, "https://mcp.example.com/mcp");
    assert!(row.auth_secret_enc.is_some());

    // The same host (another path) keeps the secret.
    let (status, body) = patch(json!({ "url": "https://mcp.example.com/v2/mcp" })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["has_secret"], true);

    // Another host with a new secret.
    let (status, body) = patch(json!({
        "url": "https://elsewhere.example.com/mcp", "secret": "new-secret-value",
    }))
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!body.to_string().contains("new-secret-value"));
    let snapshot = org.api.state.snapshot.load();
    assert_eq!(
        snapshot.mcp_server("s").unwrap().secret.as_deref(),
        Some("new-secret-value")
    );
    drop(snapshot);

    // A secret cannot be removed while the kind needs one.
    for body in [
        json!({ "url": "https://third.example.com/mcp", "secret": null }),
        json!({ "secret": null }),
        json!({ "auth_kind": "header", "auth_header": "x-api-key", "secret": null }),
    ] {
        let (status, reply) = patch(body.clone()).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}: {reply}");
        assert_eq!(
            reply["error"]["fields"]["secret"],
            "A secret is needed for this auth kind."
        );
    }
    let row = org.api.store.server_by_id(id).await.unwrap().unwrap();
    assert!(row.auth_secret_enc.is_some());

    // Turning authentication off drops the secret; nothing to send is a 400.
    let (status, _) = patch(json!({})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, body) = patch(json!({ "auth_kind": "none" })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["usable"], true);
    assert_eq!(body["has_secret"], false);
}

#[tokio::test]
async fn a_failed_sync_is_502_records_our_summary_and_changes_no_tool() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let enc = encrypt_secret(&org.api.state.cipher, SECRET);
    let mut tx = org.api.store.begin().await.unwrap();
    // Nothing listens on port 1.
    let id = tx
        .insert_server(NewMcpServer {
            name: "dead",
            url: "http://127.0.0.1:1/mcp",
            auth_kind: "bearer",
            auth_header: None,
            auth_secret_enc: Some(&enc),
            enabled: true,
            description: "",
            ..Default::default()
        })
        .await
        .unwrap();
    tx.upsert_tools(id, vec![tool("keep")]).await.unwrap();
    tx.commit().await.unwrap();

    let (status, body) = org
        .call(
            Some(&maya),
            "POST",
            &format!("/api/mcp/servers/{id}/sync"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY, "{body}");
    assert_eq!(error_code(&body), "sync_failed");
    let row = org.api.store.server_by_id(id).await.unwrap().unwrap();
    let error = row.last_sync_error.expect("a summary is recorded");
    assert!(row.last_sync_at.is_some());
    for leak in ["127.0.0.1", SECRET, "http://"] {
        assert!(!error.contains(leak), "{error}");
        assert!(!body.to_string().contains(leak), "{body}");
    }
    let tools = org.api.store.list_tools(id).await.unwrap();
    assert_eq!(tools.len(), 1);
    assert!(tools[0].removed_at.is_none());

    // A server whose secret is gone is refused before any call.
    let mut tx = org.api.store.begin().await.unwrap();
    tx.update_server(
        id,
        NewMcpServer {
            name: "dead",
            url: "http://127.0.0.1:1/mcp",
            auth_kind: "bearer",
            auth_header: None,
            auth_secret_enc: None,
            enabled: true,
            description: "",
            ..Default::default()
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let (status, body) = org
        .call(
            Some(&maya),
            "POST",
            &format!("/api/mcp/servers/{id}/sync"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(error_code(&body), "server_unusable");
    let (status, _) = org
        .call(Some(&maya), "POST", "/api/mcp/servers/99999/sync", None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Grants of a tool mean what they mean for a model: everyone, a team, a
/// user, admins always; and it must be switched on.
#[tokio::test]
async fn tool_grants_work_like_model_grants() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let arjun = org.sign_in("arjun").await; // leads Platform, member of Research
    let lena = org.sign_in("lena").await; // Platform
    let tomas = org.sign_in("tomas").await; // Research
    let priya = org.sign_in("priya").await; // no team
    let (server, tools) = seed_server(&org, "s", &["open", "team", "user", "off", "nobody"]).await;

    // Discovered tools are off and granted to nobody.
    let (status, body) = tools_of(&org, &maya, server).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(tool_names(&body).len(), 5);
    assert!(body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .all(|t| t["enabled"] == false));
    for who in [&arjun, &lena, &tomas, &priya] {
        let (status, _) = tools_of(&org, who, server).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "nothing callable yet");
    }
    let (_, servers) = org.call(Some(&lena), "GET", "/api/mcp/servers", None).await;
    assert!(server_names(&servers).is_empty());

    for t in &tools[..4] {
        assert_eq!(enable(&org, &maya, *t, true).await.0, StatusCode::OK);
    }
    // "off" is granted but then switched off again; "nobody" is on in
    // nobody's grants (never enabled).
    let g = |tool: usize, body: Value| put_grants(&org, &maya, tools[tool], body);
    assert_eq!(g(0, grants(true, &[], &[])).await.0, StatusCode::OK);
    assert_eq!(
        g(1, grants(false, &[org.platform], &[])).await.0,
        StatusCode::OK
    );
    assert_eq!(
        g(2, grants(false, &[], &[org.tomas])).await.0,
        StatusCode::OK
    );
    assert_eq!(g(3, grants(true, &[], &[])).await.0, StatusCode::OK);
    assert_eq!(enable(&org, &maya, tools[3], false).await.0, StatusCode::OK);

    assert_eq!(
        names_of(&org, &maya, server).await.len(),
        5,
        "an admin sees all, with grants"
    );
    assert_eq!(names_of(&org, &lena, server).await, ["open", "team"]);
    assert_eq!(names_of(&org, &arjun, server).await, ["open", "team"]);
    assert_eq!(names_of(&org, &tomas, server).await, ["open", "user"]);
    assert_eq!(names_of(&org, &priya, server).await, ["open"]);

    // Non-admins get empty grants; the admin gets the real ones.
    let (_, body) = tools_of(&org, &lena, server).await;
    for t in body["tools"].as_array().unwrap() {
        assert_eq!(t["grants"], grants(false, &[], &[]));
        assert_eq!(t["enabled"], true);
    }
    let (_, body) = tools_of(&org, &maya, server).await;
    let team = body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "team")
        .unwrap();
    assert_eq!(team["grants"], grants(false, &[org.platform], &[]));
    assert_eq!(team["input_schema"]["type"], "object");

    // A lead asking for what a key for a member of Platform may call:
    // only what is everyone's or Platform's, never a user grant.
    let (_, body) = org
        .call(
            Some(&arjun),
            "GET",
            &format!(
                "/api/mcp/servers/{server}/tools?key_team_id={}",
                org.platform
            ),
            None,
        )
        .await;
    assert_eq!(tool_names(&body), ["open", "team"]);
    // Not a team the lead does not lead.
    let (status, _) = org
        .call(
            Some(&arjun),
            "GET",
            &format!(
                "/api/mcp/servers/{server}/tools?key_team_id={}",
                org.research
            ),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    // A team the caller does not lead is refused.
    let (status, _) = org
        .call(
            Some(&lena),
            "GET",
            &format!(
                "/api/mcp/servers/{server}/tools?key_team_id={}",
                org.platform
            ),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // The server list for non-admins: no auth fields, no URL.
    let (_, body) = org.call(Some(&lena), "GET", "/api/mcp/servers", None).await;
    assert_eq!(server_names(&body), ["s"]);
    let s = &body["servers"][0];
    assert_eq!(s["tool_count"], 2);
    for field in [
        "url",
        "auth_kind",
        "auth_header",
        "has_secret",
        "usable",
        "last_sync_at",
        "last_sync_error",
    ] {
        assert_eq!(s[field], Value::Null, "{field}");
    }
    assert!(!body.to_string().contains(SECRET));
    let (_, body) = org.call(Some(&maya), "GET", "/api/mcp/servers", None).await;
    assert_eq!(body["servers"][0]["auth_kind"], "bearer");
    assert_eq!(body["servers"][0]["tool_count"], 5);

    // Validation, as for models.
    let (status, body) =
        put_grants(&org, &maya, tools[0], grants(true, &[org.platform], &[])).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["error"]["fields"]["everyone"].is_string());
    let (status, body) =
        put_grants(&org, &maya, tools[0], grants(false, &[99_999], &[99_999])).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["error"]["fields"]["team_ids"].is_string());
    assert!(body["error"]["fields"]["user_ids"].is_string());
    let (status, _) = put_grants(&org, &maya, 99_999, grants(true, &[], &[])).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // A disabled server hides everything from non-admins.
    let (status, _) = org
        .call(
            Some(&maya),
            "PATCH",
            &format!("/api/mcp/servers/{server}"),
            Some(json!({ "enabled": false })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(names_of(&org, &lena, server).await.is_empty());
    let (_, body) = org.call(Some(&lena), "GET", "/api/mcp/servers", None).await;
    assert!(server_names(&body).is_empty());
}

#[tokio::test]
async fn a_team_losing_its_grant_loses_the_tool_at_once() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let lena = org.sign_in("lena").await;
    let (server, tools) = seed_server(&org, "s", &["t"]).await;
    enable(&org, &maya, tools[0], true).await;
    put_grants(&org, &maya, tools[0], grants(false, &[org.platform], &[])).await;
    let path = format!("/api/mcp/servers/{server}/tools");
    assert_eq!(
        org.call(Some(&lena), "GET", &path, None).await.0,
        StatusCode::OK
    );
    put_grants(&org, &maya, tools[0], grants(false, &[org.research], &[])).await;
    assert_eq!(
        org.call(Some(&lena), "GET", &path, None).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn a_key_allowlist_names_tools_the_creator_can_call() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let arjun = org.sign_in("arjun").await;
    let lena = org.sign_in("lena").await;
    let (_, tools) = seed_server(&org, "s", &["open", "team", "off"]).await;
    enable(&org, &maya, tools[0], true).await;
    enable(&org, &maya, tools[1], true).await;
    put_grants(&org, &maya, tools[0], grants(true, &[], &[])).await;
    put_grants(&org, &maya, tools[1], grants(false, &[org.platform], &[])).await;
    // "off" is granted to everyone but never switched on.
    put_grants(&org, &maya, tools[2], grants(true, &[], &[])).await;

    let allowed = |names: &[&str]| json!({ "name": "k", "allowed": names });

    // Members name what they can call.
    let (status, body) = make_key(&org, &lena, allowed(&["mcp:s/open", "mcp:s/team"])).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["key"]["allowed"], json!(["mcp:s/open", "mcp:s/team"]));
    // Not what is off, not what does not exist, not another server's.
    for name in [
        "mcp:s/off",
        "mcp:s/nope",
        "mcp:other/open",
        "mcp:s",
        "mcp:/open",
        "mcp:s/",
    ] {
        let (status, body) = make_key(&org, &lena, allowed(&[name])).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{name}: {body}");
        assert!(body["error"]["fields"]["allowed"].is_string(), "{name}");
    }
    // A tool of a team the member is not in.
    let tomas = org.sign_in("tomas").await;
    let (status, _) = make_key(&org, &tomas, allowed(&["mcp:s/team"])).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = make_key(&org, &tomas, allowed(&["mcp:s/open"])).await;
    assert_eq!(status, StatusCode::CREATED);

    // Mixed with models and routes is fine to ask; unknown models still fail.
    let (status, _) = make_key(&org, &lena, allowed(&["mcp:s/open", "main/gpt-4o"])).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // An admin may name any tool that exists, a switched-off one too, but
    // not one that does not.
    let (status, _) = make_key(&org, &maya, allowed(&["mcp:s/off", "mcp:s/team"])).await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = make_key(&org, &maya, allowed(&["mcp:s/nope"])).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // A lead's key for a member of their team is cut to everyone's and the
    // team's.
    let (status, _) = make_key(&org,
        &arjun,
        json!({ "name": "k", "owner_id": org.lena, "team_id": org.platform, "allowed": ["mcp:s/team"] }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _) = make_key(&org,
        &arjun,
        json!({ "name": "k", "owner_id": org.lena, "team_id": org.platform, "allowed": ["mcp:s/off"] }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // The gateway's own rule agrees: what the key allowlists and the owner
    // may call is callable, and an allowlist cuts the rest.
    let snapshot = org.api.state.snapshot.load();
    let viewer = ufaigw_gateway::access::Viewer::User {
        id: org.lena,
        admin: false,
        team_ids: &[org.platform],
    };
    let all: Vec<_> = ufaigw_gateway::access::callable_tools(&snapshot, viewer, None)
        .iter()
        .map(|(_, t)| t.name.clone())
        .collect();
    assert_eq!(all, ["open", "team"]);
    let only: std::collections::HashSet<String> = ["mcp:s/team".to_string()].into();
    let cut: Vec<_> = ufaigw_gateway::access::callable_tools(&snapshot, viewer, Some(&only))
        .iter()
        .map(|(_, t)| t.name.clone())
        .collect();
    assert_eq!(cut, ["team"]);
}

fn actor() -> Actor<'static> {
    Actor {
        user_id: None,
        email: "cli",
        cipher: None,
    }
}

async fn export(org: &Org, who: &Signed) -> Value {
    let (status, _, bytes) = common::raw(org, who, "GET", "/api/config/export", None).await;
    assert_eq!(status, StatusCode::OK);
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn the_export_has_servers_and_tool_settings_but_no_secret_and_round_trips() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let (_, tools) = seed_server(&org, "s", &["a", "b", "gone"]).await;
    enable(&org, &maya, tools[0], true).await;
    put_grants(
        &org,
        &maya,
        tools[0],
        grants(false, &[org.platform], &[org.tomas]),
    )
    .await;
    put_grants(&org, &maya, tools[1], grants(true, &[], &[])).await;
    // A tool the server stopped offering is not in the file.
    let id = org.api.store.server_by_name("s").await.unwrap().unwrap().id;
    let mut tx = org.api.store.begin().await.unwrap();
    tx.upsert_tools(id, vec![tool("a"), tool("b")])
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let file = export(&org, &maya).await;
    assert!(!file.to_string().contains(SECRET));
    assert!(!file.to_string().contains("auth_secret"));
    assert_eq!(
        file["mcp_servers"],
        json!([{
            "name": "s",
            "url": "https://mcp.example.com/mcp",
            "auth_kind": "bearer",
            "oauth": null,
            "auth_header": null,
            "description": "d",
            "enabled": true,
            "tools": [
                { "name": "a", "enabled": true, "projection": null,
                  "grants": { "everyone": false, "teams": ["Platform"], "users": ["tomas@example.com"] } },
                { "name": "b", "enabled": false, "projection": null,
                  "grants": { "everyone": true, "teams": [], "users": [] } },
            ],
        }])
    );
    // No servers, no key in the file.
    let empty = common::org().await;
    let maya2 = empty.sign_in("maya").await;
    assert!(export(&empty, &maya2).await.get("mcp_servers").is_none());

    // A second import of its own export changes nothing.
    let parsed: ConfigFile = serde_json::from_value(file.clone()).unwrap();
    let report = portable::import(&org.api.store, &parsed, &actor(), false)
        .await
        .unwrap();
    assert!(report.is_clean(), "{:?}", report.errors);
    assert!(
        report.created.is_empty() && report.updated.is_empty(),
        "{report:?}"
    );

    // Into a gateway that has the teams and users but no server: the server
    // is created with no secret, so it cannot be called, and its tools wait
    // for a sync.
    let fresh = common::org().await;
    let report = portable::import(&fresh.api.store, &parsed, &actor(), false)
        .await
        .unwrap();
    assert!(report.is_clean(), "{:?}", report.errors);
    assert!(report
        .created
        .iter()
        .any(|i| i.kind == "mcp_server" && i.name == "s"));
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.message.contains("no secret")),
        "{:?}",
        report.warnings
    );
    assert!(
        report.warnings.iter().any(|w| w.at.ends_with(".tools")),
        "{:?}",
        report.warnings
    );
    fresh.api.state.refresh().await.unwrap();
    let row = fresh.api.store.server_by_name("s").await.unwrap().unwrap();
    assert_eq!(row.auth_kind, "bearer");
    assert!(row.auth_secret_enc.is_none());
    let snapshot = fresh.api.state.snapshot.load();
    assert!(!snapshot.mcp_server("s").unwrap().usable);
    drop(snapshot);
    let maya3 = fresh.sign_in("maya").await;
    let (_, body) = fresh
        .call(Some(&maya3), "GET", "/api/mcp/servers", None)
        .await;
    assert_eq!(body["servers"][0]["usable"], false);
    assert_eq!(body["servers"][0]["has_secret"], false);

    // Once the tools exist (a sync), importing the file sets their switch
    // and grants.
    let mut tx = fresh.api.store.begin().await.unwrap();
    tx.upsert_tools(row.id, vec![tool("a"), tool("b")])
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let report = portable::import(&fresh.api.store, &parsed, &actor(), false)
        .await
        .unwrap();
    assert!(report.is_clean(), "{:?}", report.errors);
    assert!(
        report.updated.iter().any(|i| i.kind == "mcp_tool"),
        "{report:?}"
    );
    assert_eq!(
        export(&fresh, &maya3).await["mcp_servers"],
        file["mcp_servers"]
    );
}

#[tokio::test]
async fn importing_a_server_to_another_host_drops_its_secret_and_checks_the_file() {
    let org = org().await;
    seed_server(&org, "s", &["a"]).await;
    let maya = org.sign_in("maya").await;
    let mut file = export(&org, &maya).await;
    file["mcp_servers"][0]["url"] = json!("https://elsewhere.example.com/mcp");
    let parsed: ConfigFile = serde_json::from_value(file.clone()).unwrap();
    let report = portable::import(&org.api.store, &parsed, &actor(), false)
        .await
        .unwrap();
    assert!(report.is_clean(), "{:?}", report.errors);
    assert!(report.warnings.iter().any(|w| w.message.contains("secret")));
    let row = org.api.store.server_by_name("s").await.unwrap().unwrap();
    assert_eq!(row.url, "https://elsewhere.example.com/mcp");
    assert!(row.auth_secret_enc.is_none());

    // A same-host change keeps it.
    let org2 = common::org().await;
    seed_server(&org2, "s", &["a"]).await;
    let maya2 = org2.sign_in("maya").await;
    let mut file = export(&org2, &maya2).await;
    file["mcp_servers"][0]["url"] = json!("https://mcp.example.com/other");
    let parsed: ConfigFile = serde_json::from_value(file).unwrap();
    portable::import(&org2.api.store, &parsed, &actor(), false)
        .await
        .unwrap();
    assert!(org2
        .api
        .store
        .server_by_name("s")
        .await
        .unwrap()
        .unwrap()
        .auth_secret_enc
        .is_some());

    // Errors name the place and write nothing.
    let mut bad = export(&org2, &maya2).await;
    bad["mcp_servers"] = json!([
        { "name": "Bad_Name", "url": "ftp://x", "auth_kind": "header", "auth_header": null,
          "description": "", "enabled": true, "tools": [] },
        { "name": "dup", "url": "https://x.example/mcp", "auth_kind": "none", "auth_header": null,
          "description": "", "enabled": true, "tools": [
            { "name": "t", "enabled": true, "grants": { "everyone": true, "teams": ["Nope"], "users": [] } }] },
        { "name": "dup", "url": "https://x.example/mcp", "auth_kind": "none", "auth_header": null,
          "description": "", "enabled": true, "tools": [] },
    ]);
    let parsed: ConfigFile = serde_json::from_value(bad).unwrap();
    let report = portable::import(&org2.api.store, &parsed, &actor(), true)
        .await
        .unwrap();
    let at: Vec<&str> = report.errors.iter().map(|e| e.at.as_str()).collect();
    for want in [
        "mcp_servers[0].name",
        "mcp_servers[0].url",
        "mcp_servers[0].auth_header",
        "mcp_servers[1].tools[0].grants",
        "mcp_servers[2]",
    ] {
        assert!(at.iter().any(|a| a.starts_with(want)), "{want} in {at:?}");
    }
    assert!(report.created.is_empty());
}

#[tokio::test]
async fn callable_tools_of_key_follows_the_owner_the_server_and_removal() {
    use ufaigw_gateway::access::callable_tools_of_key;
    use ufaigw_gateway::snapshot::SnapKey;

    let org = org().await;
    let maya = org.sign_in("maya").await;
    let (id, tools) = seed_server(&org, "s", &["a", "b"]).await;
    for t in &tools {
        assert_eq!(enable(&org, &maya, *t, true).await.0, StatusCode::OK);
        let g = grants(false, &[org.platform], &[]);
        assert_eq!(put_grants(&org, &maya, *t, g).await.0, StatusCode::OK);
    }
    let key = |user: Option<i64>| SnapKey {
        id: 1,
        name: String::new(),
        user_id: user,
        team_id: Some(org.platform),
        expires_at: None,
        allowed: None,
        tags: Default::default(),
        team_only: true,
        guardrails: Vec::new(),
        pinned: Vec::new(),
    };
    let count = |user: Option<i64>| {
        let snapshot = org.api.state.snapshot.load();
        callable_tools_of_key(&snapshot, &key(user)).len()
    };
    assert_eq!(count(Some(org.lena)), 2);

    // A tool the server no longer lists is absent.
    let mut tx = org.api.store.begin().await.unwrap();
    tx.upsert_tools(id, vec![tool("a")]).await.unwrap();
    tx.commit().await.unwrap();
    org.api.state.refresh().await.unwrap();
    assert_eq!(count(Some(org.lena)), 1);

    // An owner who is not there acts for no one.
    assert_eq!(count(Some(9_999)), 0);
    assert_eq!(count(None), 0);

    // An owner who left the team.
    let mut tx = org.api.store.begin().await.unwrap();
    tx.remove_member(org.platform, org.lena).await.unwrap();
    tx.commit().await.unwrap();
    org.api.state.refresh().await.unwrap();
    assert_eq!(count(Some(org.lena)), 0);
    assert_eq!(count(Some(org.arjun)), 1);

    // A disabled server.
    let enc = encrypt_secret(&org.api.state.cipher, SECRET);
    let mut tx = org.api.store.begin().await.unwrap();
    tx.update_server(
        id,
        NewMcpServer {
            name: "s",
            url: "https://mcp.example.com/mcp",
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
    org.api.state.refresh().await.unwrap();
    assert_eq!(count(Some(org.arjun)), 0);
}

async fn own_key(org: &Org, who: &Signed, name: &str) -> i64 {
    let (status, body) = make_key(org, who, json!({ "name": name })).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body["key"]["id"].as_i64().unwrap()
}

async fn pin(org: &Org, who: &Signed, key: i64, body: Value) -> (StatusCode, Value) {
    org.call(Some(who), "PATCH", &format!("/api/keys/{key}"), Some(body))
        .await
}

#[tokio::test]
async fn pinned_tools_are_set_by_an_admin_or_a_lead_and_checked_against_what_the_key_can_call() {
    let org = org().await;
    let maya = org.sign_in("maya").await; // admin
    let arjun = org.sign_in("arjun").await; // lead of Platform
    let lena = org.sign_in("lena").await; // member of Platform
    let (_, tools) = seed_server(&org, "srv", &["a", "b", "off"]).await;
    // `a` and `b` are on and for everyone; `off` is neither.
    for tool in tools.iter().take(2) {
        enable(&org, &maya, *tool, true).await;
        let (status, _) = put_grants(&org, &maya, *tool, grants(true, &[], &[])).await;
        assert_eq!(status, StatusCode::OK);
    }
    let maya_key = own_key(&org, &maya, "maya-key").await;
    let (status, view) = pin(
        &org,
        &maya,
        maya_key,
        json!({"pinned_tools": ["mcp:srv/a", "mcp:srv/b", "mcp:srv/a"]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["pinned_tools"], json!(["mcp:srv/a", "mcp:srv/b"]));
    assert!(org
        .last_summary("key.pinned_tools")
        .await
        .contains("Pinned 2 MCP tools"));
    // Unpinned with an empty list.
    let (_, view) = pin(&org, &maya, maya_key, json!({"pinned_tools": []})).await;
    assert_eq!(view["pinned_tools"], json!([]));
    // What the key cannot call, or is not a tool, is refused in the same words.
    for bad in [
        json!(["mcp:srv/off"]),
        json!(["mcp:srv/nope"]),
        json!(["mcp:nope/a"]),
        json!(["openai/gpt-4o"]),
        Value::from(vec!["mcp:srv/a"; 101]),
    ] {
        let (status, body) = pin(&org, &maya, maya_key, json!({ "pinned_tools": bad })).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}: {body}");
        assert!(body["error"]["fields"]["pinned_tools"].is_string());
    }
    // A lead pins to their own key; a member may not pin to theirs; nobody to another's.
    let arjun_key = own_key(&org, &arjun, "arjun-key").await;
    let (status, _) = pin(
        &org,
        &arjun,
        arjun_key,
        json!({"pinned_tools": ["mcp:srv/a"]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let lena_key = own_key(&org, &lena, "lena-key").await;
    let (status, _) = pin(
        &org,
        &lena,
        lena_key,
        json!({"pinned_tools": ["mcp:srv/a"]}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = pin(
        &org,
        &lena,
        maya_key,
        json!({"pinned_tools": ["mcp:srv/a"]}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // An empty request still says what to send.
    let (status, _) = pin(&org, &maya, maya_key, json!({})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

async fn put(org: &Org, who: &Signed, tool: i64, body: Value) -> (StatusCode, Value) {
    org.call(
        Some(who),
        "PUT",
        &format!("/api/mcp/tools/{tool}/projection"),
        Some(body),
    )
    .await
}

async fn preview(org: &Org, who: &Signed, tool: i64, body: Value) -> (StatusCode, Value) {
    org.call(
        Some(who),
        "POST",
        &format!("/api/mcp/tools/{tool}/projection/preview"),
        Some(body),
    )
    .await
}

#[tokio::test]
async fn a_projection_is_set_shown_checked_previewed_and_taken_off_by_an_admin() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let lena = org.sign_in("lena").await;
    let (server, tools) = seed_server(&org, "srv", &["a"]).await;
    let tool = tools[0];
    // Nothing yet.
    let (_, listed) = tools_of(&org, &maya, server).await;
    assert!(listed["tools"][0]["projection"].is_null(), "{listed}");

    let (status, view) = put(
        &org,
        &maya,
        tool,
        json!({"keep": ["$.items[*].id"], "drop": ["$..secret"], "max_items": 5, "max_bytes": 2048}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(
        view["projection"],
        json!({"keep": ["$.items[*].id"], "drop": ["$..secret"], "max_items": 5, "max_bytes": 2048})
    );
    assert!(org
        .last_summary("mcp.tool_projection")
        .await
        .contains("Set the projection"));
    let (_, listed) = tools_of(&org, &maya, server).await;
    assert_eq!(listed["tools"][0]["projection"]["max_items"], 5);
    // Anybody else sees no projection, and may not set one.
    enable(&org, &maya, tool, true).await;
    let (status, _) = put_grants(&org, &maya, tool, grants(true, &[], &[])).await;
    assert_eq!(status, StatusCode::OK);
    let (_, seen) = tools_of(&org, &lena, server).await;
    assert!(seen["tools"][0]["projection"].is_null());
    let (status, _) = put(&org, &lena, tool, json!({"keep": []})).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Bad queries and limits are told per field.
    for (body, field) in [
        (json!({"keep": ["$["]}), "keep"),
        (json!({"drop": [""]}), "drop"),
        (json!({"max_items": 0}), "max_items"),
        (json!({"max_bytes": 999999999999u64}), "max_bytes"),
    ] {
        let (status, err) = put(&org, &maya, tool, body).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{err}");
        assert!(err["error"]["fields"][field].is_string(), "{field}: {err}");
    }
    let (status, _) = org
        .call(
            Some(&maya),
            "PUT",
            "/api/mcp/tools/99999/projection",
            Some(json!({"keep": []})),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // The preview applies the stored projection, or one sent along, and stores nothing.
    let result = json!({"content": [{"type": "text",
        "text": json!({"items": [{"id": 1, "secret": "s"}, {"id": 2, "secret": "t"}]}).to_string()}],
        "isError": false});
    let (status, p) = preview(&org, &maya, tool, json!({"result": result})).await;
    assert_eq!(status, StatusCode::OK, "{p}");
    assert_eq!(p["changed"], true);
    assert_eq!(
        p["result"]["content"][0]["text"],
        "{\"items\":[{\"id\":1},{\"id\":2}]}"
    );
    assert!(p["bytes_after"].as_u64().unwrap() < p["bytes_before"].as_u64().unwrap());
    let (_, p) = preview(
        &org,
        &maya,
        tool,
        json!({"result": result, "projection": {"max_items": 1}}),
    )
    .await;
    assert_eq!(p["truncated"], true);
    let (status, _) = preview(&org, &maya, tool, json!({"result": [1]})).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = preview(
        &org,
        &maya,
        tool,
        json!({"result": result, "projection": {"keep": ["$["]}}),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        org.api
            .store
            .projection_of(tool)
            .await
            .unwrap()
            .unwrap()
            .max_items,
        Some(5)
    );

    // The export carries it, and an import brings it back.
    let file = export(&org, &maya).await;
    assert_eq!(
        file["mcp_servers"][0]["tools"][0]["projection"]["keep"],
        json!(["$.items[*].id"])
    );
    let (status, _) = put(&org, &maya, tool, json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert!(org.api.store.projection_of(tool).await.unwrap().is_none());
    assert!(org
        .last_summary("mcp.tool_projection")
        .await
        .contains("Took the projection off"));
    let parsed: ConfigFile = serde_json::from_value(file).unwrap();
    let report = portable::import(&org.api.store, &parsed, &actor(), false)
        .await
        .unwrap();
    assert!(report.is_clean(), "{:?}", report.errors);
    let back = org.api.store.projection_of(tool).await.unwrap().unwrap();
    assert_eq!(back.keep, ["$.items[*].id"]);
    assert_eq!(back.max_bytes, Some(2048));

    // A bad projection in a file is an error that names the place.
    let mut bad = export(&org, &maya).await;
    bad["mcp_servers"][0]["tools"][0]["projection"]["keep"] = json!(["$["]);
    let parsed: ConfigFile = serde_json::from_value(bad).unwrap();
    let report = portable::import(&org.api.store, &parsed, &actor(), true)
        .await
        .unwrap();
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.at.contains("projection.keep")),
        "{:?}",
        report.errors
    );
}
