//! Versioned prompt templates: the rules of rendering, immutability, who may
//! manage them, their use on `/v1/chat/completions`, `/v1/responses` and the
//! playground, what a template and a call may each decide, the log and the
//! configuration file. Runs on SQLite, and on PostgreSQL when
//! `UF_TEST_DATABASE_URL` is set.

mod common;

use std::sync::Arc;

use axum::http::StatusCode;
use common::{
    allow_model, call, error_code, org_with_sink, post_to, seed_team, seed_user, MemorySink, Org,
    Signed, ORG_PASSWORD,
};
use serde_json::{json, Value};
use ultrafast_gateway::identity::{Role, TeamRole};
use ultrafast_gateway::limits::{LimitScope, RateLimit};
use ultrafast_gateway::secrets::generate_key;
use ultrafast_gateway::store::{NewGuardrail, NewLog};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const LIST: &str = "/api/prompts";

fn one(id: i64) -> String {
    format!("{LIST}/{id}")
}

fn messages(text: &str) -> Value {
    json!([{ "role": "user", "content": text }])
}

fn body(name: &str, text: &str) -> Value {
    json!({ "name": name, "messages": messages(text) })
}

struct World {
    org: Org,
    sink: Arc<MemorySink>,
    upstream: MockServer,
    key: String,
}

fn ok() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "c1", "model": "m",
        "choices": [{ "message": { "role": "assistant", "content": "hello" }, "finish_reason": "stop" }],
        "usage": { "prompt_tokens": 4, "completion_tokens": 2 }
    }))
}

async fn world() -> World {
    let sink = Arc::new(MemorySink::default());
    let org = org_with_sink(Some(sink.clone())).await;
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ok())
        .mount(&upstream)
        .await;
    let store = &org.api.store;
    store
        .insert_provider("p", "openai", &upstream.uri(), None)
        .await
        .unwrap();
    allow_model(store, "p", "m").await;
    allow_model(store, "p", "other").await;
    let key = generate_key();
    store
        .insert_key("test", &key.hash, &key.display, None)
        .await
        .unwrap();
    org.api.state.refresh().await.unwrap();
    World {
        org,
        sink,
        upstream,
        key: key.full,
    }
}

impl World {
    async fn api(
        &self,
        who: &Signed,
        verb: &str,
        uri: &str,
        b: Option<Value>,
    ) -> (StatusCode, Value) {
        self.org.call(Some(who), verb, uri, b).await
    }

    /// Creates a template and returns its id.
    async fn make(&self, who: &Signed, b: Value) -> i64 {
        let (status, v) = self.api(who, "POST", LIST, Some(b)).await;
        assert_eq!(status, StatusCode::CREATED, "{v}");
        v["id"].as_i64().unwrap()
    }

    async fn v1(&self, uri: &str, b: &Value) -> (StatusCode, Value) {
        let bearer = format!("Bearer {}", self.key);
        let (status, _, text) = post_to(
            &self.org.api.app,
            uri,
            &[("authorization", &bearer)],
            &b.to_string(),
        )
        .await;
        (
            status,
            serde_json::from_str(&text).unwrap_or(Value::String(text)),
        )
    }

    async fn chat(&self, b: Value) -> (StatusCode, Value) {
        self.v1("/v1/chat/completions", &b).await
    }

    async fn responses(&self, b: Value) -> (StatusCode, Value) {
        self.v1("/v1/responses", &b).await
    }

    /// The bodies the provider received, oldest first.
    async fn sent(&self) -> Vec<Value> {
        self.upstream
            .received_requests()
            .await
            .unwrap()
            .iter()
            .map(|r| serde_json::from_slice(&r.body).unwrap())
            .collect()
    }
}

// ---------------------------------------------------------------- rendering

fn many(c: char, n: usize) -> String {
    std::iter::repeat_n(c, n).collect()
}

/// (what the message says, the values, the rendered text or a part of the
/// refusal). The rules of Review Focus 4: `{{name}}` with a name of
/// `[A-Za-z_][A-Za-z0-9_]{0,63}` is a variable; anything else is text;
/// substitution is literal and happens once.
fn rules() -> Vec<(String, Value, Result<String, String>)> {
    let long = many('a', 64);
    let too_long = many('a', 65);
    let ok = |s: &str| Ok(s.to_string());
    let err = |s: &str| Err(s.to_string());
    vec![
        (
            "Hello {{name}}!".into(),
            json!({"name":"Ada"}),
            ok("Hello Ada!"),
        ),
        (
            "{{a}}, {{a}} and {{b}}".into(),
            json!({"a":"x","b":"y"}),
            ok("x, x and y"),
        ),
        ("plain".into(), json!({}), ok("plain")),
        // The value is not read again, by any variable.
        ("Say {{a}}".into(), json!({"a":"{{a}}!"}), ok("Say {{a}}!")),
        (
            "{{a}}|{{b}}".into(),
            json!({"a":"{{b}}","b":"B"}),
            ok("{{b}}|B"),
        ),
        // No escaping: what the value holds is what is inserted.
        ("<{{a}}>".into(), json!({"a":"\"&\\\n"}), ok("<\"&\\\n>")),
        ("\\{{a}}".into(), json!({"a":"v"}), ok("\\v")),
        ("{{{a}}}".into(), json!({"a":"v"}), ok("{v}")),
        ("[{{a}}]".into(), json!({"a":""}), ok("[]")),
        // What is not a variable is text, and is not asked for.
        ("Hi {{ name }}".into(), json!({}), ok("Hi {{ name }}")),
        (
            "{{}} {{1x}} {{a-b}} {{é}} {{a b}} {a}".into(),
            json!({}),
            ok("{{}} {{1x}} {{a-b}} {{é}} {{a b}} {a}"),
        ),
        ("{{_x1}}".into(), json!({"_x1":"v"}), ok("v")),
        (
            format!("{{{{{long}}}}}"),
            json!({ (long.clone()): "v" }),
            ok("v"),
        ),
        (
            format!("{{{{{too_long}}}}}"),
            json!({}),
            Ok(format!("{{{{{too_long}}}}}")),
        ),
        // Names are case sensitive.
        (
            "{{Name}}".into(),
            json!({"name":"x"}),
            err("unknown variable 'name'"),
        ),
        // Every variable is required; unknown ones are refused; both are named.
        (
            "{{a}} {{b}}".into(),
            json!({"a":"x"}),
            err("missing variable 'b'"),
        ),
        (
            "{{a}} {{b}} {{c}}".into(),
            json!({"b":"x"}),
            err("missing variables 'a', 'c'"),
        ),
        (
            "{{a}}".into(),
            json!({"a":"x","z":"y"}),
            err("unknown variable 'z'"),
        ),
        (
            "{{a}}".into(),
            json!({"a":"x","a-b":"y"}),
            err("unknown variable 'a-b'"),
        ),
        (
            "plain".into(),
            json!({"z":"y"}),
            err("unknown variable 'z'"),
        ),
        // Values are strings of at most 32 KiB.
        (
            "{{a}}".into(),
            json!({"a": many('x', 32 * 1024)}),
            Ok(many('x', 32 * 1024)),
        ),
        (
            "{{a}}".into(),
            json!({"a": many('x', 32 * 1024 + 1)}),
            err("variable 'a' is longer than 32768 bytes"),
        ),
        // The rendered text is bounded too.
        (
            "{{a}}".repeat(10_000),
            json!({"a": many('x', 32 * 1024)}),
            err("larger than 1048576 bytes"),
        ),
    ]
}

#[tokio::test]
async fn the_rules_of_rendering() {
    let w = world().await;
    let maya = w.org.sign_in("maya").await;
    for (i, (text, values, want)) in rules().into_iter().enumerate() {
        let id = w.make(&maya, body(&format!("t{i}"), &text)).await;
        let (status, v) = w
            .api(
                &maya,
                "POST",
                &format!("{}/render", one(id)),
                Some(json!({ "variables": values })),
            )
            .await;
        match want {
            Ok(rendered) => {
                assert_eq!(status, StatusCode::OK, "case {i} {text:?}: {v}");
                assert_eq!(
                    v["messages"],
                    json!([{ "role": "user", "content": rendered }]),
                    "case {i}"
                );
                assert_eq!(v["version"], 1);
            }
            Err(part) => {
                assert_eq!(status, StatusCode::BAD_REQUEST, "case {i} {text:?}: {v}");
                let message = v["error"]["message"].as_str().unwrap();
                assert!(message.contains(&part), "case {i}: {message}");
            }
        }
    }
}

#[tokio::test]
async fn the_variables_of_a_version_are_those_its_messages_use() {
    let w = world().await;
    let maya = w.org.sign_in("maya").await;
    let (status, v) = w
        .api(
            &maya,
            "POST",
            LIST,
            Some(json!({ "name": "t", "messages": [
                { "role": "system", "content": "You are {{role}}. {{role}}." },
                { "role": "user", "content": "{{question}} {{ not }}" },
            ]})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{v}");
    assert_eq!(v["latest_version"], 1);
    assert_eq!(v["versions"][0]["variables"], json!(["question", "role"]));
    // A second version may use others.
    let (status, v) = w
        .api(
            &maya,
            "POST",
            &format!("{}/versions", one(v["id"].as_i64().unwrap())),
            Some(json!({ "messages": messages("{{only}}") })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{v}");
    assert_eq!(
        (v["version"].clone(), v["variables"].clone()),
        (json!(2), json!(["only"]))
    );
}

#[tokio::test]
async fn a_template_is_checked_when_it_is_written() {
    let w = world().await;
    let maya = w.org.sign_in("maya").await;
    let user = |c: &str| json!([{ "role": "user", "content": c }]);
    let cases: Vec<(&str, Value)> = vec![
        ("name", json!({ "name": "", "messages": user("x") })),
        ("name", json!({ "name": "a\nb", "messages": user("x") })),
        (
            "name",
            json!({ "name": many('n', 101), "messages": user("x") }),
        ),
        (
            "description",
            json!({ "name": "a", "description": many('d', 501), "messages": user("x") }),
        ),
        ("messages", json!({ "name": "a", "messages": [] })),
        (
            "messages",
            json!({ "name": "a", "messages": (0..65).map(|_| json!({"role":"user","content":"x"})).collect::<Vec<_>>() }),
        ),
        (
            "messages[0].role",
            json!({ "name": "a", "messages": [{"role":"tool","content":"x"}] }),
        ),
        (
            "messages[0].content",
            json!({ "name": "a", "messages": [{"role":"user","content":""}] }),
        ),
        (
            "messages",
            json!({ "name": "a", "messages": [{"role":"user","content": (0..65).map(|i| format!("{{{{v{i}}}}}")).collect::<String>() }] }),
        ),
        (
            "model",
            json!({ "name": "a", "messages": user("x"), "model": "" }),
        ),
        (
            "model",
            json!({ "name": "a", "messages": user("x"), "model": many('m', 201) }),
        ),
        (
            "params.temperature",
            json!({ "name": "a", "messages": user("x"), "params": {"temperature": 2.5} }),
        ),
        (
            "params.temperature",
            json!({ "name": "a", "messages": user("x"), "params": {"temperature": -0.1} }),
        ),
        (
            "params.top_p",
            json!({ "name": "a", "messages": user("x"), "params": {"top_p": 1.5} }),
        ),
        (
            "params.max_tokens",
            json!({ "name": "a", "messages": user("x"), "params": {"max_tokens": 0} }),
        ),
        (
            "params.response_format",
            json!({ "name": "a", "messages": user("x"), "params": {"response_format": {"type": "nope"}} }),
        ),
    ];
    for (field, b) in cases {
        let (status, v) = w.api(&maya, "POST", LIST, Some(b)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{field}: {v}");
        assert!(v["error"]["fields"][field].is_string(), "{field}: {v}");
    }
    // The API takes bodies up to 64 KiB; the limits on one message (64 KiB)
    // and on all of them (256 KiB) are reached by a file (see the import test).
    let (status, v) = w
        .api(
            &maya,
            "POST",
            LIST,
            Some(body("big", &many('x', 64 * 1024))),
        )
        .await;
    assert_eq!(
        (status, error_code(&v)),
        (StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large")
    );
    // Unknown fields and params are refused too.
    for b in [
        json!({ "name": "a", "messages": user("x"), "nope": 1 }),
        json!({ "name": "a", "messages": user("x"), "params": {"stop": ["x"]} }),
        json!({ "name": "a", "messages": [{"role":"user","content":"x","name":"n"}] }),
    ] {
        let (status, v) = w.api(&maya, "POST", LIST, Some(b)).await;
        assert!(status.is_client_error(), "{v}");
        assert!(w
            .org
            .api
            .store
            .list_prompt_templates()
            .await
            .unwrap()
            .is_empty());
    }
    // The name is taken, with the same name in any case being another name.
    w.make(&maya, body("greet", "x")).await;
    let (status, v) = w.api(&maya, "POST", LIST, Some(body("greet", "y"))).await;
    assert_eq!(
        (status, error_code(&v)),
        (StatusCode::CONFLICT, "prompt_exists")
    );
    w.make(&maya, body("Greet", "y")).await;
}

// ------------------------------------------------------------- immutability

#[tokio::test]
async fn a_version_cannot_be_changed_and_a_new_text_is_a_new_version() {
    let w = world().await;
    let maya = w.org.sign_in("maya").await;
    let id = w
        .make(
            &maya,
            json!({ "name": "greet", "description": "d", "messages": messages("v1 {{a}}"), "model": "p/m",
                    "params": { "temperature": 0.25, "max_tokens": 7 } }),
        )
        .await;
    let versions = format!("{}/versions", one(id));
    let (status, v2) = w
        .api(
            &maya,
            "POST",
            &versions,
            Some(json!({ "messages": messages("v2 {{b}}") })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{v2}");
    assert_eq!(v2["version"], 2);
    // The same text again is not "the same version": it is the next.
    let (_, v3) = w
        .api(
            &maya,
            "POST",
            &versions,
            Some(json!({ "messages": messages("v2 {{b}}") })),
        )
        .await;
    assert_eq!(v3["version"], 3);
    // Version 1 reads as it was written.
    let (status, v1) = w.api(&maya, "GET", &format!("{versions}/1"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(v1["messages"], messages("v1 {{a}}"));
    assert_eq!(v1["model"], "p/m");
    assert_eq!(
        v1["params"],
        json!({ "temperature": 0.25, "max_tokens": 7 })
    );
    assert_eq!(v1["variables"], json!(["a"]));
    // Version 2 inherits nothing: a version stands for itself.
    let (_, v2) = w.api(&maya, "GET", &format!("{versions}/2"), None).await;
    assert_eq!(
        (v2["model"].clone(), v2["params"].clone()),
        (Value::Null, json!({}))
    );
    // There is no way to change or remove one.
    for verb in ["PUT", "PATCH", "DELETE", "POST"] {
        let (status, v) = w
            .api(
                &maya,
                verb,
                &format!("{versions}/1"),
                Some(json!({ "messages": messages("evil") })),
            )
            .await;
        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED, "{verb}: {v}");
    }
    let (status, _) = w
        .api(&maya, "PATCH", &one(id), Some(json!({ "name": "x" })))
        .await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    let (_, view) = w.api(&maya, "GET", &one(id), None).await;
    assert_eq!(view["latest_version"], 3);
    assert_eq!(view["versions"].as_array().unwrap().len(), 3);
    assert_eq!(view["versions"][0]["messages"], messages("v1 {{a}}"));
    // A missing version, and a path that is not one.
    for p in ["0", "4", "x", "-1", "01x"] {
        let (status, _) = w.api(&maya, "GET", &format!("{versions}/{p}"), None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{p}");
    }
    // The audit log says what happened, with the names.
    let actions = w.org.audit_actions().await;
    let prompt_actions: Vec<&String> = actions
        .iter()
        .filter(|a| a.starts_with("prompt."))
        .collect();
    assert_eq!(
        prompt_actions,
        ["prompt.create", "prompt.version", "prompt.version"]
    );
    assert!(w.org.last_summary("prompt.version").await.contains("greet"));
}

#[tokio::test]
async fn concurrent_new_versions_get_distinct_numbers() {
    // Writers that really run side by side (the in-memory store has one).
    let org = common::org_concurrent().await;
    let w = Arc::new(World {
        sink: Arc::new(MemorySink::default()),
        upstream: MockServer::start().await,
        key: String::new(),
        org,
    });
    let maya = Arc::new(w.org.sign_in("maya").await);
    let id = w.make(&maya, body("race", "x")).await;
    let mut tasks = Vec::new();
    for i in 0..6 {
        let (w, maya) = (w.clone(), maya.clone());
        tasks.push(tokio::spawn(async move {
            let (status, v) = w
                .api(
                    &maya,
                    "POST",
                    &format!("{}/versions", one(id)),
                    Some(json!({ "messages": messages(&format!("t{i}")) })),
                )
                .await;
            (status, v["version"].as_i64())
        }));
    }
    let mut numbers = Vec::new();
    for t in tasks {
        let (status, n) = t.await.unwrap();
        assert_eq!(status, StatusCode::CREATED);
        numbers.push(n.unwrap());
    }
    numbers.sort_unstable();
    assert_eq!(numbers, [2, 3, 4, 5, 6, 7]);
}

// -------------------------------------------------------------------- roles

#[tokio::test]
async fn admins_manage_all_leads_their_own_and_anyone_may_use_any() {
    let w = world().await;
    let maya = w.org.sign_in("maya").await;
    let arjun = w.org.sign_in("arjun").await; // lead of Platform
    let lena = w.org.sign_in("lena").await; // member
    let priya = w.org.sign_in("priya").await; // no team
                                              // Another lead.
    let zed = seed_user(
        &w.org.api.store,
        "zed@example.com",
        Role::Member,
        ORG_PASSWORD,
    )
    .await;
    seed_team(&w.org.api.store, "Ops", &[(zed, TeamRole::Lead)]).await;
    let zed = common::sign_in(&w.org.api.app, "zed@example.com", ORG_PASSWORD).await;

    let admins = w.make(&maya, body("by-admin", "a {{x}}")).await;
    let arjuns = w.make(&arjun, body("by-arjun", "b {{x}}")).await;
    // Created by the signed-in user.
    let (_, v) = w.api(&arjun, "GET", &one(arjuns), None).await;
    assert_eq!(v["created_by"], w.org.arjun);
    assert_eq!(v["versions"][0]["created_by"], w.org.arjun);
    let (_, v) = w.api(&maya, "GET", &one(admins), None).await;
    assert_eq!(v["created_by"], w.org.maya);

    // Members and people without a team cannot write at all.
    for who in [&lena, &priya] {
        let (status, v) = w.api(who, "POST", LIST, Some(body("nope", "x"))).await;
        assert_eq!(
            (status, error_code(&v)),
            (StatusCode::FORBIDDEN, "forbidden")
        );
    }
    // A lead changes theirs, not an admin's, not another lead's.
    let add = |id| {
        (
            format!("{}/versions", one(id)),
            json!({ "messages": messages("more") }),
        )
    };
    let (uri, b) = add(arjuns);
    assert_eq!(
        w.api(&arjun, "POST", &uri, Some(b.clone())).await.0,
        StatusCode::CREATED
    );
    for (who, id) in [
        (&arjun, admins),
        (&zed, arjuns),
        (&lena, arjuns),
        (&priya, admins),
    ] {
        let (uri, b) = add(id);
        let (status, v) = w.api(who, "POST", &uri, Some(b)).await;
        assert_eq!(
            (status, error_code(&v)),
            (StatusCode::FORBIDDEN, "forbidden"),
            "{uri}"
        );
        let (status, _) = w.api(who, "DELETE", &one(id), None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
    // An admin changes anyone's.
    let (uri, b) = add(arjuns);
    assert_eq!(
        w.api(&maya, "POST", &uri, Some(b)).await.0,
        StatusCode::CREATED
    );

    // Everyone signed in reads and renders any template.
    for who in [&maya, &arjun, &zed, &lena, &priya] {
        let (status, list) = w.api(who, "GET", LIST, None).await;
        assert_eq!(status, StatusCode::OK);
        let names: Vec<&str> = list["prompts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["by-admin", "by-arjun"]);
        for id in [admins, arjuns] {
            assert_eq!(w.api(who, "GET", &one(id), None).await.0, StatusCode::OK);
            assert_eq!(
                w.api(who, "GET", &format!("{}/versions/1", one(id)), None)
                    .await
                    .0,
                StatusCode::OK
            );
            let (status, v) = w
                .api(
                    who,
                    "POST",
                    &format!("{}/render", one(id)),
                    Some(json!({ "version": 1, "variables": { "x": "1" } })),
                )
                .await;
            assert_eq!(status, StatusCode::OK, "{v}");
        }
    }
    // Nobody signed in sees nothing.
    let (status, _, v) = call(&w.org.api.app, "GET", LIST, None, None).await;
    assert_eq!(
        (status, error_code(&v)),
        (StatusCode::UNAUTHORIZED, "unauthenticated")
    );

    // A lead deletes theirs; another lead cannot; an admin can.
    assert_eq!(
        w.api(&arjun, "DELETE", &one(arjuns), None).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        w.api(&zed, "DELETE", &one(admins), None).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        w.api(&maya, "DELETE", &one(admins), None).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        w.api(&maya, "GET", &one(admins), None).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        w.api(&maya, "DELETE", &one(admins), None).await.0,
        StatusCode::NOT_FOUND
    );
    // A template whose maker is gone belongs to the admins.
    let orphan = w.make(&zed, body("orphan", "x")).await;
    {
        let mut tx = w.org.api.store.begin().await.unwrap();
        assert!(tx.delete_user(zed.user_id).await.unwrap());
        tx.commit().await.unwrap();
    }
    let (_, v) = w.api(&maya, "GET", &one(orphan), None).await;
    assert_eq!(v["created_by"], Value::Null);
    assert_eq!(
        w.api(&arjun, "DELETE", &one(orphan), None).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        w.api(&maya, "DELETE", &one(orphan), None).await.0,
        StatusCode::NO_CONTENT
    );
}

// ----------------------------------------------------------- use on /v1

async fn greet(w: &World) -> (i64, Signed) {
    let maya = w.org.sign_in("maya").await;
    let id = w
        .make(
            &maya,
            json!({ "name": "greet", "model": "p/m",
                "messages": [
                    { "role": "system", "content": "Be {{tone}}." },
                    { "role": "user", "content": "Hello {{name}}" },
                ] }),
        )
        .await;
    (id, maya)
}

#[tokio::test]
async fn chat_renders_the_template_before_the_messages_of_the_request() {
    let w = world().await;
    let (id, maya) = greet(&w).await;
    let (status, v) = w
        .chat(json!({
            "prompt": { "id": "greet", "variables": { "tone": "brief", "name": "Ada" } },
            "messages": [{ "role": "user", "content": "and then?" }],
        }))
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let sent = w.sent().await;
    assert_eq!(sent[0]["model"], "m");
    assert_eq!(
        sent[0]["messages"],
        json!([
            { "role": "system", "content": "Be brief." },
            { "role": "user", "content": "Hello Ada" },
            { "role": "user", "content": "and then?" },
        ])
    );
    // No messages of its own is fine; the version is a number or digits.
    let (_, v2) = w
        .api(
            &maya,
            "POST",
            &format!("{}/versions", one(id)),
            Some(json!({ "messages": messages("second {{name}}"), "model": "p/other" })),
        )
        .await;
    assert_eq!(v2["version"], 2);
    for version in [json!(1), json!("1")] {
        let (status, v) = w
            .chat(json!({ "prompt": { "id": "greet", "version": version, "variables": { "tone": "x", "name": "Bo" } } }))
            .await;
        assert_eq!(status, StatusCode::OK, "{v}");
        let sent = w.sent().await;
        assert_eq!(sent.last().unwrap()["messages"][1]["content"], "Hello Bo");
        assert_eq!(
            sent.last().unwrap()["messages"].as_array().unwrap().len(),
            2
        );
    }
    // Without a version, the latest; with one, that one.
    let (status, _) = w
        .chat(json!({ "prompt": { "id": "greet", "variables": { "name": "Cy" } } }))
        .await;
    assert_eq!(status, StatusCode::OK);
    let sent = w.sent().await;
    assert_eq!(sent.last().unwrap()["model"], "other");
    assert_eq!(
        sent.last().unwrap()["messages"],
        json!([{ "role": "user", "content": "second Cy" }])
    );
    // The record names what was used.
    let records = w.sink.records();
    let used: Vec<Option<&str>> = records.iter().map(|r| r.prompt.as_deref()).collect();
    assert_eq!(
        used,
        [
            Some("greet@1"),
            Some("greet@1"),
            Some("greet@1"),
            Some("greet@2")
        ]
    );
}

#[tokio::test]
async fn a_call_that_cannot_use_its_template_is_refused_before_any_provider() {
    let w = world().await;
    let (_, maya) = greet(&w).await;
    let call = |prompt: Value| json!({ "prompt": prompt, "messages": [{ "role": "user", "content": "x" }] });
    let cases: Vec<(Value, StatusCode, &str)> = vec![
        (
            json!({ "id": "greet", "variables": { "name": "A" } }),
            StatusCode::BAD_REQUEST,
            "missing variable 'tone'",
        ),
        (
            json!({ "id": "greet", "variables": { "tone": "t", "name": "A", "z": "1" } }),
            StatusCode::BAD_REQUEST,
            "unknown variable 'z'",
        ),
        (
            json!({ "id": "greet", "variables": { "tone": "t", "name": 3 } }),
            StatusCode::BAD_REQUEST,
            "variables",
        ),
        (
            json!({ "id": "greet", "version": 0, "variables": {} }),
            StatusCode::BAD_REQUEST,
            "version",
        ),
        (
            json!({ "id": "greet", "version": "1.5" }),
            StatusCode::BAD_REQUEST,
            "version",
        ),
        (
            json!({ "id": "greet", "version": 99, "variables": { "tone": "t", "name": "A" } }),
            StatusCode::NOT_FOUND,
            "version 99",
        ),
        (json!({ "id": "nope" }), StatusCode::NOT_FOUND, "nope"),
        (json!({ "id": "Greet" }), StatusCode::NOT_FOUND, "Greet"),
        (json!({ "version": 1 }), StatusCode::BAD_REQUEST, "id"),
        (json!("greet"), StatusCode::BAD_REQUEST, "prompt"),
    ];
    for (prompt, want, part) in cases {
        let (status, v) = w.chat(call(prompt.clone())).await;
        assert_eq!(status, want, "{prompt}: {v}");
        let message = v["error"]["message"].as_str().unwrap();
        assert!(message.contains(part), "{prompt}: {message}");
        assert!(v["error"]["type"].is_string(), "OpenAI error shape: {v}");
        let (status, v) = w.responses(json!({ "prompt": prompt })).await;
        assert_eq!(status, want, "responses {prompt}: {v}");
    }
    assert!(w.sent().await.is_empty());
    // A template without a model, called without one.
    w.make(&maya, body("modelless", "hi")).await;
    let (status, v) = w.chat(json!({ "prompt": { "id": "modelless" } })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
    assert!(v["error"]["message"].as_str().unwrap().contains("model"));
    // Without a prompt, a model and messages are still needed.
    for b in [
        json!({ "messages": messages("x") }),
        json!({ "model": "p/m" }),
    ] {
        assert_eq!(w.chat(b).await.0, StatusCode::BAD_REQUEST);
    }
    assert!(w.sent().await.is_empty());
}

#[tokio::test]
async fn responses_render_the_same_template() {
    let w = world().await;
    let (_, maya) = greet(&w).await;
    w.api(
        &maya,
        "POST",
        &format!(
            "{}/versions",
            one(w.org.api.store.list_prompt_templates().await.unwrap()[0].id)
        ),
        Some(json!({ "messages": messages("v2 {{name}}"), "model": "p/m" })),
    )
    .await;
    // The version is a string in OpenAI\'s API; a number is accepted too.
    for version in [json!("1"), json!(1)] {
        let (status, v) = w
            .responses(json!({
                "prompt": { "id": "greet", "version": version, "variables": { "tone": "calm", "name": "Ada" } },
                "input": "next",
            }))
            .await;
        assert_eq!(status, StatusCode::OK, "{v}");
        assert_eq!(v["object"], "response");
        let sent = w.sent().await;
        assert_eq!(
            sent.last().unwrap()["messages"],
            json!([
                { "role": "system", "content": "Be calm." },
                { "role": "user", "content": "Hello Ada" },
                { "role": "user", "content": "next" },
            ])
        );
        assert_eq!(sent.last().unwrap()["model"], "m");
    }
    // No input at all, `instructions` after the template, and the latest.
    let (status, v) = w
        .responses(json!({ "prompt": { "id": "greet", "variables": { "name": "Bo" } }, "instructions": "short" }))
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(
        w.sent().await.last().unwrap()["messages"],
        json!([
            { "role": "user", "content": "v2 Bo" },
            { "role": "system", "content": "short" },
        ])
    );
    let records = w.sink.records();
    assert_eq!(records.last().unwrap().prompt.as_deref(), Some("greet@2"));
    assert_eq!(records.last().unwrap().endpoint, "responses");
    // The model of the request wins over the template\'s.
    let (status, _) = w
        .responses(json!({ "model": "p/other", "prompt": { "id": "greet", "version": "1", "variables": { "tone": "t", "name": "n" } } }))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(w.sent().await.last().unwrap()["model"], "other");
}

#[tokio::test]
async fn the_request_decides_where_the_template_and_the_request_both_speak() {
    let w = world().await;
    let maya = w.org.sign_in("maya").await;
    w.make(
        &maya,
        json!({ "name": "p1", "model": "p/m", "messages": messages("hi"),
                "params": { "temperature": 0.2, "max_tokens": 50, "top_p": 0.9,
                            "response_format": { "type": "json_object" } } }),
    )
    .await;
    // The template\'s settings fill what the request leaves out.
    let (status, v) = w.chat(json!({ "prompt": { "id": "p1" } })).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    let sent = &w.sent().await[0];
    assert_eq!(sent["model"], "m");
    assert_eq!(sent["temperature"], json!(0.2f32));
    assert_eq!(sent["max_tokens"], 50);
    assert_eq!(sent["top_p"], json!(0.9f32));
    assert_eq!(sent["response_format"], json!({ "type": "json_object" }));
    // What the request sets wins, field by field.
    let (status, _) = w
        .chat(json!({
            "model": "p/other", "prompt": { "id": "p1" }, "temperature": 1.0, "max_tokens": 5,
            "response_format": { "type": "text" },
        }))
        .await;
    assert_eq!(status, StatusCode::OK);
    let sent = &w.sent().await[1];
    assert_eq!(sent["model"], "other");
    assert_eq!(sent["temperature"], 1.0);
    assert_eq!(sent["max_tokens"], 5);
    assert_eq!(sent["top_p"], json!(0.9f32));
    assert_eq!(sent["response_format"], json!({ "type": "text" }));
    // Responses: max_output_tokens and text.format are the request\'s too.
    let (status, _) = w
        .responses(json!({ "prompt": { "id": "p1" }, "input": "x", "max_output_tokens": 9, "temperature": 0.0 }))
        .await;
    assert_eq!(status, StatusCode::OK);
    let sent = &w.sent().await[2];
    assert_eq!(
        (sent["max_tokens"].clone(), sent["temperature"].clone()),
        (json!(9), json!(0.0))
    );
    assert_eq!(sent["response_format"], json!({ "type": "json_object" }));
    // A template can name a model the key may not call: the key decides.
    w.make(
        &maya,
        json!({ "name": "gone", "model": "p/not-there", "messages": messages("x") }),
    )
    .await;
    let (status, _) = w.chat(json!({ "prompt": { "id": "gone" } })).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn guardrails_and_limits_see_the_rendered_messages() {
    let w = world().await;
    let maya = w.org.sign_in("maya").await;
    let rules = json!([{ "id": "w", "matcher": { "keywords": { "words": ["swordfish"] } },
                         "action": "block", "directions": "input" }])
    .to_string();
    let mut tx = w.org.api.store.begin().await.unwrap();
    tx.insert_guardrail(NewGuardrail {
        name: "no-secrets",
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
    w.make(
        &maya,
        json!({ "name": "ask", "model": "p/m", "messages": messages("About {{topic}}.") }),
    )
    .await;
    w.make(
        &maya,
        json!({ "name": "leaky", "model": "p/m", "messages": messages("the swordfish") }),
    )
    .await;
    w.org.api.state.refresh().await.unwrap();
    // The value, and the text of the template itself, are over the rule; the
    // request\'s own messages are innocent.
    for prompt in [
        json!({ "id": "ask", "variables": { "topic": "the swordfish" } }),
        json!({ "id": "leaky" }),
    ] {
        let (status, v) = w
            .chat(json!({ "prompt": prompt, "messages": messages("fine") }))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{v}");
        assert_eq!(v["error"]["code"], "guardrail_blocked");
        assert!(!v.to_string().contains("swordfish"));
    }
    let (status, _) = w
        .chat(json!({ "prompt": { "id": "ask", "variables": { "topic": "cats" } } }))
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(w.sent().await.len(), 1);

    // The rate limit counts what is rendered: a template of 2 000 characters
    // is 500 tokens against a limit of 100 a minute.
    let mut tx = w.org.api.store.begin().await.unwrap();
    tx.upsert_limit(
        LimitScope::Gateway,
        None,
        &RateLimit {
            requests_per_minute: None,
            tokens_per_minute: Some(100),
            concurrent: None,
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    w.make(
        &maya,
        json!({ "name": "big", "model": "p/m", "messages": messages(&many('b', 2_000)) }),
    )
    .await;
    w.org.api.state.refresh().await.unwrap();
    let small = json!({ "model": "p/m", "max_tokens": 10, "messages": messages("hi") });
    assert_eq!(w.chat(small).await.0, StatusCode::OK);
    let (status, _) = w
        .chat(json!({ "prompt": { "id": "big" }, "max_tokens": 10, "messages": messages("hi") }))
        .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(w.sent().await.len(), 2);
}

#[tokio::test]
async fn the_playground_uses_templates_too() {
    let w = world().await;
    let (_, _) = greet(&w).await;
    let lena = w.org.sign_in("lena").await;
    let (status, v) = w
        .api(
            &lena,
            "POST",
            "/api/playground/chat",
            Some(json!({ "prompt": { "id": "greet", "variables": { "tone": "kind", "name": "Lena" } } })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(w.sent().await[0]["messages"][0]["content"], "Be kind.");
    assert_eq!(w.sink.records()[0].prompt.as_deref(), Some("greet@1"));
}

// -------------------------------------------------- logs and the snapshot

fn log(prompt: Option<&str>) -> NewLog {
    NewLog {
        at: "2999-01-01 00:00:00".into(),
        key_id: None,
        user_id: None,
        team_id: None,
        requested: "p/m".into(),
        endpoint: "chat".into(),
        stream: false,
        status: 200,
        provider: Some("p".into()),
        model: Some("m".into()),
        input_tokens: Some(1),
        output_tokens: Some(1),
        cost_micros: 0,
        priced: false,
        cached: false,
        estimated: false,
        duration_ms: 1,
        attempts: "[]".into(),
        tags: None,
        guardrails: None,
        prompt: prompt.map(str::to_string),
    }
}

#[tokio::test]
async fn deleting_a_template_leaves_its_logs_as_they_were() {
    let w = world().await;
    let maya = w.org.sign_in("maya").await;
    let id = w.make(&maya, body("greet", "hi")).await;
    w.org
        .api
        .store
        .insert_logs(&[log(Some("greet@1")), log(None)])
        .await
        .unwrap();
    let (_, list) = w.api(&maya, "GET", "/api/logs", None).await;
    let prompts: Vec<Value> = list["logs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["prompt"].clone())
        .collect();
    assert_eq!(prompts, [Value::Null, json!("greet@1")]);
    let log_id = list["logs"][1]["id"].as_i64().unwrap();

    assert_eq!(
        w.api(&maya, "DELETE", &one(id), None).await.0,
        StatusCode::NO_CONTENT
    );
    let (status, row) = w
        .api(&maya, "GET", &format!("/api/logs/{log_id}"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(row["prompt"], "greet@1");
    // Its versions went with it, and a call by that name is refused.
    assert!(w
        .org
        .api
        .store
        .list_prompt_templates()
        .await
        .unwrap()
        .is_empty());
    let (status, _) = w.chat(json!({ "prompt": { "id": "greet" } })).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    // The name is free again, and starts again at version 1 (a new template).
    let again = w.make(&maya, body("greet", "other")).await;
    let (_, v) = w.api(&maya, "GET", &one(again), None).await;
    assert_eq!(v["latest_version"], 1);
}

#[tokio::test]
async fn the_snapshot_and_the_cache_fingerprint_follow_the_templates() {
    let w = world().await;
    let maya = w.org.sign_in("maya").await;
    let prints = || w.org.api.state.snapshot.load().cache_fingerprint();
    let empty = prints();
    let id = w.make(&maya, body("greet", "one")).await;
    let one_version = prints();
    assert_ne!(one_version, empty, "a template changes the fingerprint");
    w.api(
        &maya,
        "POST",
        &format!("{}/versions", one(id)),
        Some(json!({ "messages": messages("two") })),
    )
    .await;
    let two_versions = prints();
    assert_ne!(two_versions, one_version, "a version changes it");
    w.api(&maya, "DELETE", &one(id), None).await;
    assert_eq!(prints(), empty, "and so does a delete");
    // Made again with other text under the same name (SQLite gives the id
    // out again): not the fingerprint of before.
    w.make(&maya, body("greet", "other")).await;
    assert_ne!(prints(), one_version);
    assert_ne!(prints(), empty);
}

// ------------------------------------------------------- export and import

#[tokio::test]
async fn templates_travel_in_the_configuration_file_with_all_their_versions() {
    let w = world().await;
    let maya = w.org.sign_in("maya").await;
    let id = w
        .make(
            &maya,
            json!({ "name": "greet", "description": "says hello", "model": "p/m",
                    "messages": messages("v1 {{a}}"), "params": { "temperature": 0.5 } }),
        )
        .await;
    w.api(&maya, "POST", &format!("{}/versions", one(id)), Some(json!({ "messages": messages("v2 {{b}}"), "params": { "response_format": { "type": "json_object" } } }))).await;
    let (status, file) = w.api(&maya, "GET", "/api/config/export", None).await;
    assert_eq!(status, StatusCode::OK);
    let prompts = file["prompts"].as_array().unwrap();
    assert_eq!(prompts.len(), 1);
    assert_eq!(prompts[0]["name"], "greet");
    assert_eq!(prompts[0]["description"], "says hello");
    let versions = prompts[0]["versions"].as_array().unwrap();
    assert_eq!(versions.len(), 2);
    assert_eq!(
        versions[0],
        json!({ "version": 1, "messages": messages("v1 {{a}}"), "model": "p/m", "params": { "temperature": 0.5 } })
    );
    assert_eq!(versions[1]["version"], 2);
    assert_eq!(versions[1]["model"], Value::Null);

    // A fresh gateway takes it; a second import changes nothing.
    let fresh = world().await;
    let fmaya = fresh.org.sign_in("maya").await;
    let import = |dry: bool, f: Value| {
        let (fresh, fmaya) = (&fresh, &fmaya);
        async move {
            fresh
                .api(
                    fmaya,
                    "POST",
                    &format!("/api/config/import?dry_run={dry}"),
                    Some(f),
                )
                .await
        }
    };
    let (status, report) = import(true, file.clone()).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert!(
        fresh
            .org
            .api
            .store
            .list_prompt_templates()
            .await
            .unwrap()
            .is_empty(),
        "a dry run writes nothing"
    );
    let item = &report["created"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == "prompt")
        .cloned()
        .unwrap();
    assert_eq!(item["name"], "greet");
    let (status, report) = import(false, file.clone()).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    let (_, list) = fresh.api(&fmaya, "GET", LIST, None).await;
    assert_eq!(list["prompts"][0]["latest_version"], 2);
    assert_eq!(list["prompts"][0]["created_by"], fmaya.user_id);
    let (_, back) = fresh.api(&fmaya, "GET", "/api/config/export", None).await;
    assert_eq!(back["prompts"], file["prompts"]);
    let (_, report) = import(false, file.clone()).await;
    assert_eq!(
        (report["created"].clone(), report["updated"].clone()),
        (json!([]), json!([]))
    );
    // And it can be used there.
    let (status, _) = fresh
        .chat(json!({ "prompt": { "id": "greet", "version": 1, "variables": { "a": "x" } } }))
        .await;
    assert_eq!(status, StatusCode::OK);

    // A new version and a new description are taken; versions already there
    // are never rewritten.
    let mut more = file.clone();
    more["prompts"][0]["description"] = json!("new words");
    more["prompts"][0]["versions"]
        .as_array_mut()
        .unwrap()
        .push(json!({ "version": 3, "messages": messages("v3"), "model": null, "params": {} }));
    let (status, report) = import(false, more.clone()).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["updated"][0]["name"], "greet");
    let (_, list) = fresh.api(&fmaya, "GET", LIST, None).await;
    assert_eq!(
        (
            list["prompts"][0]["latest_version"].clone(),
            list["prompts"][0]["description"].clone()
        ),
        (json!(3), json!("new words"))
    );

    let mut edited = file.clone();
    edited["prompts"][0]["versions"][0]["messages"] = messages("sneaky");
    let (status, report) = import(false, edited).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{report}");
    assert!(
        report["errors"][0]["at"]
            .as_str()
            .unwrap()
            .starts_with("prompts[0].versions[0]"),
        "{report}"
    );
    assert!(
        report["errors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("never change"),
        "{report}"
    );
    let (_, v1) = fresh
        .api(
            &fmaya,
            "GET",
            &format!(
                "{}/versions/1",
                one(list["prompts"][0]["id"].as_i64().unwrap())
            ),
            None,
        )
        .await;
    assert_eq!(v1["messages"], messages("v1 {{a}}"));

    // A file that is not a valid template list changes nothing.
    for (path, value, part) in [
        ("versions", json!([]), "at least one"),
        (
            "versions",
            json!([{ "version": 2, "messages": messages("x") }]),
            "version 1",
        ),
        ("name", json!(""), "name"),
        ("name", json!("a\nb"), "name"),
    ] {
        let mut bad = file.clone();
        bad["prompts"][0]["name"] = json!("other-name");
        bad["prompts"][0][path] = value;
        let (_, report) = import(false, bad).await;
        let errors = report["errors"].to_string();
        assert!(errors.contains(part), "{path}: {errors}");
    }
    let mut dup = file.clone();
    dup["prompts"]
        .as_array_mut()
        .unwrap()
        .push(file["prompts"][0].clone());
    let (_, report) = import(false, dup).await;
    assert!(
        report["errors"].to_string().contains("more than once"),
        "{report}"
    );
    let mut bad_message = file.clone();
    bad_message["prompts"][0]["name"] = json!("fresh");
    bad_message["prompts"][0]["versions"][0]["messages"] =
        json!([{ "role": "robot", "content": "x" }]);
    let (_, report) = import(false, bad_message).await;
    assert!(report["errors"].to_string().contains("role"), "{report}");
    // The size limits of a version are the file's to enforce: the API's body is too small to reach them.
    for (messages, part) in [
        (
            json!([{ "role": "user", "content": many('x', 64 * 1024 + 1) }]),
            "messages[0].content",
        ),
        (
            json!((0..5)
                .map(|_| json!({ "role": "user", "content": many('x', 60 * 1024) }))
                .collect::<Vec<_>>()),
            "together",
        ),
    ] {
        let mut big = file.clone();
        big["prompts"][0]["name"] = json!("big");
        big["prompts"][0]["versions"] = json!([{ "version": 1, "messages": messages }]);
        let (_, report) = import(false, big).await;
        assert!(
            report["errors"].to_string().contains(part),
            "{part}: {report}"
        );
    }
    assert_eq!(
        fresh
            .org
            .api
            .store
            .list_prompt_templates()
            .await
            .unwrap()
            .len(),
        1
    );
}
