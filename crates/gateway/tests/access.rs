//! Who may call which model or route on `/v1`, and `GET /v1/models`.

mod common;

use std::collections::BTreeMap;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{error_code, org, post_chat, Org};
use serde_json::{json, Value};
use tower::ServiceExt;
use ultrafast_gateway::secrets::generate_key;
use ultrafast_gateway::store::{Grants, RouteSettings, TargetsInput};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

const DEFAULTS: RouteSettings = RouteSettings {
    retries: 2,
    first_token_timeout_ms: 30_000,
    total_timeout_ms: 300_000,
    breaker_failures: 5,
    breaker_window_s: 60,
    breaker_open_s: 30,
};

fn openai_ok() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "c1", "model": "x",
        "choices": [{
            "message": { "role": "assistant", "content": "hello" },
            "finish_reason": "stop"
        }],
        "usage": { "prompt_tokens": 1, "completion_tokens": 2 }
    }))
}

fn chat(model: &str) -> String {
    json!({ "model": model, "messages": [{ "role": "user", "content": "hi" }] }).to_string()
}

/// Who may call a model.
#[derive(Clone, Copy, Debug)]
enum Grant {
    Everyone,
    Team,
    User,
    Nobody,
}

/// The organization with a provider "p" that answers every call.
struct World {
    org: Org,
    upstream: MockServer,
    provider_id: i64,
}

async fn world() -> World {
    let org = org().await;
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(openai_ok())
        .mount(&upstream)
        .await;
    let provider_id = org
        .api
        .store
        .insert_provider("p", "openai", &upstream.uri(), None)
        .await
        .unwrap();
    World {
        org,
        upstream,
        provider_id,
    }
}

impl World {
    async fn refresh(&self) {
        self.org.api.state.refresh().await.unwrap();
    }

    /// A model of "p". `grant` is `Team` for Platform, `User` for Lena.
    async fn model(&self, name: &str, enabled: bool, grant: Grant) -> i64 {
        let mut tx = self.org.api.store.begin().await.unwrap();
        let id = tx.insert_model(self.provider_id, name).await.unwrap();
        tx.set_model_enabled(id, enabled).await.unwrap();
        let grants = match grant {
            Grant::Everyone => Grants {
                everyone: true,
                ..Grants::default()
            },
            Grant::Team => Grants {
                team_ids: vec![self.org.platform],
                ..Grants::default()
            },
            Grant::User => Grants {
                user_ids: vec![self.org.lena],
                ..Grants::default()
            },
            Grant::Nobody => Grants::default(),
        };
        tx.replace_grants(id, &grants).await.unwrap();
        tx.commit().await.unwrap();
        id
    }

    /// A route over models; `teams` of `None` is for everyone.
    async fn route(
        &self,
        name: &str,
        primaries: &[i64],
        fallbacks: &[i64],
        teams: Option<&[i64]>,
    ) -> i64 {
        let mut tx = self.org.api.store.begin().await.unwrap();
        let id = tx
            .insert_route(name, &DEFAULTS, teams.is_none())
            .await
            .unwrap();
        tx.replace_targets(
            id,
            &TargetsInput {
                primaries: primaries.iter().map(|m| (*m, 1)).collect(),
                fallbacks: fallbacks.to_vec(),
            },
        )
        .await
        .unwrap();
        tx.replace_route_grants(id, teams.unwrap_or(&[]))
            .await
            .unwrap();
        tx.commit().await.unwrap();
        id
    }

    /// A key of this owner (none: from the CLI) with this allowlist.
    async fn key(&self, owner: Option<i64>, allowed: Option<&[&str]>) -> String {
        let key = generate_key();
        let mut tx = self.org.api.store.begin().await.unwrap();
        let id = tx
            .insert_key("k", &key.hash, &key.display, None, owner, None)
            .await
            .unwrap();
        let allowed: Option<Vec<String>> =
            allowed.map(|names| names.iter().map(|n| n.to_string()).collect());
        tx.set_key_allowed(id, allowed.as_deref()).await.unwrap();
        tx.commit().await.unwrap();
        key.full
    }

    async fn call(&self, key: &str, model: &str) -> (StatusCode, Value) {
        let (status, body) = post_chat(&self.org.api.app, Some(key), &chat(model)).await;
        (status, serde_json::from_str(&body).unwrap_or(Value::Null))
    }

    async fn status(&self, key: &str, model: &str) -> StatusCode {
        self.call(key, model).await.0
    }

    /// The `model` of the last request the provider got.
    async fn last_upstream_model(&self) -> String {
        let requests = self.upstream.received_requests().await.unwrap();
        let body: Value = serde_json::from_slice(&requests.last().unwrap().body).unwrap();
        body["model"].as_str().unwrap().to_string()
    }

    async fn listed(&self, key: &str) -> (StatusCode, Value) {
        let resp = self
            .org
            .api
            .app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/models")
                    .header("authorization", format!("Bearer {key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn listed_ids(&self, key: &str) -> Vec<String> {
        let (status, body) = self.listed(key).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["id"].as_str().unwrap().to_string())
            .collect()
    }
}

#[tokio::test]
async fn access_matrix() {
    let w = world().await;
    let grants = [Grant::Everyone, Grant::Team, Grant::User, Grant::Nobody];
    let mut models = Vec::new();
    for enabled in [true, false] {
        for grant in grants {
            let name = format!("m-{enabled}-{grant:?}").to_lowercase();
            w.model(&name, enabled, grant).await;
            models.push((name, enabled, grant));
        }
    }
    let all: Vec<String> = models.iter().map(|(n, _, _)| format!("p/{n}")).collect();
    let all: Vec<&str> = all.iter().map(String::as_str).collect();

    // (who, owner)
    let org = &w.org;
    let owners = [
        ("admin", Some(org.maya)),
        ("lead", Some(org.arjun)),
        ("member", Some(org.lena)),
        ("tomas", Some(org.tomas)),
        ("ownerless", None),
    ];
    // Platform: arjun (lead) and lena. User grant: lena.
    let granted = |who: &str, grant: Grant| {
        matches!(
            (who, grant),
            ("admin", _)
                | (_, Grant::Everyone)
                | ("lead" | "member", Grant::Team)
                | ("member", Grant::User)
        )
    };

    for (who, owner) in owners {
        for allow in ["yes", "no", "absent"] {
            let allowed: Option<&[&str]> = match allow {
                "yes" => Some(&all),
                "no" => Some(&["p/unrelated"]),
                _ => None,
            };
            let key = w.key(owner, allowed).await;
            w.refresh().await;
            for (name, enabled, grant) in &models {
                let expected = if *enabled && granted(who, *grant) && allow != "no" {
                    StatusCode::OK
                } else {
                    StatusCode::FORBIDDEN
                };
                let (status, body) = w.call(&key, &format!("p/{name}")).await;
                assert_eq!(status, expected, "{who} allowlist={allow} {name}: {body}");
                if expected == StatusCode::FORBIDDEN {
                    assert_eq!(body["error"]["type"], "permission_error");
                    assert_eq!(
                        body["error"]["message"],
                        format!("You do not have access to model 'p/{name}'.")
                    );
                }
            }
            // A name that does not exist is 404 whoever asks.
            let (status, body) = w.call(&key, "p/nothing").await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{who}");
            assert_eq!(body["error"]["type"], "not_found_error");
            assert_eq!(body["error"]["message"], "Unknown model 'p/nothing'.");
        }
    }
}

#[tokio::test]
async fn left_team_loses_team_grant() {
    let w = world().await;
    w.model("shared", true, Grant::Team).await;
    let org = &w.org;
    let maya = org.sign_in("maya").await;
    let (status, body) = org
        .call(
            Some(&maya),
            "POST",
            "/api/keys",
            Some(json!({ "name": "lena", "owner_id": org.lena })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let secret = body["secret"].as_str().unwrap().to_string();
    assert_eq!(w.status(&secret, "p/shared").await, StatusCode::OK);

    let path = format!("/api/teams/{}/members/{}", org.platform, org.lena);
    let (status, _) = org.call(Some(&maya), "DELETE", &path, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    // No refresh by hand: the write did it.
    assert_eq!(w.status(&secret, "p/shared").await, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_role_change_takes_effect_on_v1_without_a_refresh() {
    let w = world().await;
    w.model("private", true, Grant::Nobody).await;
    let org = &w.org;
    let maya = org.sign_in("maya").await;
    let (status, body) = org
        .call(
            Some(&maya),
            "POST",
            "/api/keys",
            Some(json!({ "name": "lena", "owner_id": org.lena })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let secret = body["secret"].as_str().unwrap().to_string();
    assert_eq!(w.status(&secret, "p/private").await, StatusCode::FORBIDDEN);

    let path = format!("/api/users/{}", org.lena);
    let (status, body) = org
        .call(
            Some(&maya),
            "PATCH",
            &path,
            Some(json!({ "role": "admin" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(w.status(&secret, "p/private").await, StatusCode::OK);

    let (status, body) = org
        .call(
            Some(&maya),
            "PATCH",
            &path,
            Some(json!({ "role": "member" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // No refresh by hand: the write did it.
    assert_eq!(w.status(&secret, "p/private").await, StatusCode::FORBIDDEN);
}

/// One table drives the console's lists, its allowlist check and `/v1`: they
/// answer from the same predicates, so for every user what the console shows
/// as callable is what `/v1` lists, and what a key may name.
#[tokio::test]
async fn the_console_and_v1_agree_on_who_may_call_what() {
    let w = world().await;
    let grants = [Grant::Everyone, Grant::Team, Grant::User, Grant::Nobody];
    let mut ids = Vec::new();
    for enabled in [true, false] {
        for grant in grants {
            let name = format!("m-{enabled}-{grant:?}").to_lowercase();
            ids.push((name.clone(), w.model(&name, enabled, grant).await));
        }
    }
    let org = &w.org;
    let first = ids[0].1;
    let platform_only = ids[1].1;
    w.route("open", &[first], &[], None).await;
    w.route("platform", &[first], &[], Some(&[org.platform]))
        .await;
    w.route("research", &[platform_only], &[], Some(&[org.research]))
        .await;
    w.route("admins", &[first], &[], Some(&[])).await;
    w.refresh().await;

    let everything: Vec<String> = ids
        .iter()
        .map(|(n, _)| format!("p/{n}"))
        .chain(["open", "platform", "research", "admins"].map(String::from))
        .collect();

    for (who, owner) in [
        ("arjun", org.arjun),
        ("lena", org.lena),
        ("tomas", org.tomas),
        ("priya", org.priya),
    ] {
        let me = org.sign_in(who).await;
        let (status, body) = org.call(Some(&me), "GET", "/api/models", None).await;
        assert_eq!(status, StatusCode::OK, "{who}: {body}");
        let mut console_models: Vec<String> = body["models"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| {
                format!(
                    "{}/{}",
                    m["provider_name"].as_str().unwrap(),
                    m["name"].as_str().unwrap()
                )
            })
            .collect();
        console_models.sort();
        let (status, body) = org.call(Some(&me), "GET", "/api/routes", None).await;
        assert_eq!(status, StatusCode::OK, "{who}: {body}");
        let console_routes: Vec<String> = body["routes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["name"].as_str().unwrap().to_string())
            .collect();

        let key = w.key(Some(owner), None).await;
        w.refresh().await;
        let v1 = w.listed_ids(&key).await;
        let v1_models: Vec<String> = v1.iter().filter(|n| n.contains('/')).cloned().collect();
        assert_eq!(console_models, v1_models, "{who}: models");
        // Every route /v1 lists is one the console shows as usable (a route
        // whose targets cannot be called is shown, as broken or not, but not
        // listed by /v1).
        for route in v1.iter().filter(|n| !n.contains('/')) {
            assert!(console_routes.contains(route), "{who}: route {route}");
        }

        // What a key may name: the models the console shows and the routes
        // /v1 would let the owner call (open to them, with a callable
        // target), no more.
        for name in &everything {
            let shown = console_models.contains(name) || v1.contains(name);
            let (status, body) = org
                .call(
                    Some(&me),
                    "POST",
                    "/api/keys",
                    Some(json!({ "name": "t", "allowed": [name] })),
                )
                .await;
            let expected = if shown {
                StatusCode::CREATED
            } else {
                StatusCode::UNPROCESSABLE_ENTITY
            };
            assert_eq!(status, expected, "{who} may name {name}: {body}");
        }
    }

    // A key whose owner the snapshot does not hold agrees with the
    // predicates: nothing is usable or callable for a missing owner.
    use ultrafast_gateway::access::{
        callable_names, model_callable, route_usable, ModelFacts, RouteFacts, Viewer,
    };
    use ultrafast_gateway::snapshot::{SnapKey, Snapshot};
    let snapshot = Snapshot::load(&w.org.api.store, &w.org.api.state.cipher)
        .await
        .unwrap();
    let ghost = SnapKey {
        tags: Default::default(),
        id: 1,
        name: "k".into(),
        user_id: Some(999_999),
        team_id: None,
        expires_at: None,
        allowed: None,
        team_only: false,
        guardrails: Vec::new(),
    };
    assert!(callable_names(&snapshot, &ghost).is_empty());
    assert!(!route_usable(
        Viewer::Missing,
        &RouteFacts {
            everyone: true,
            team_ids: &[],
        }
    ));
    assert!(!model_callable(
        Viewer::Missing,
        &ModelFacts {
            enabled: true,
            provider_present: true,
            everyone: true,
            team_ids: &[],
            user_ids: &[],
        }
    ));
}

#[tokio::test]
async fn route_access() {
    let w = world().await;
    let m1 = w.model("m1", true, Grant::Everyone).await;
    let m2 = w.model("m2", true, Grant::Everyone).await;
    let off = w.model("off", false, Grant::Everyone).await;
    let team_only = w.model("team-only", true, Grant::Team).await;
    let org = &w.org;

    w.route("open", &[m1], &[], None).await;
    w.route("research", &[m1], &[], Some(&[org.research])).await;
    w.route("admins", &[m1], &[], Some(&[])).await;
    // The first primary is disabled: the next one in order is used.
    w.route("skips", &[off, m2], &[], None).await;
    // Only a fallback can be called.
    w.route("falls-back", &[off], &[m1], None).await;
    // Nothing in it can be called by everyone.
    w.route("platform-model", &[team_only], &[], None).await;
    w.route("dead", &[off], &[], None).await;

    let admin = w.key(Some(org.maya), None).await;
    let lena = w.key(Some(org.lena), None).await;
    let arjun = w.key(Some(org.arjun), None).await;
    let tomas = w.key(Some(org.tomas), None).await;
    let priya = w.key(Some(org.priya), None).await;
    let cli = w.key(None, None).await;
    w.refresh().await;

    let expected = [
        ("open", [true, true, true, true, true, true]),
        ("research", [true, false, true, true, false, false]),
        ("admins", [true, false, false, false, false, false]),
    ];
    for (route, allowed) in expected {
        for (key, who, ok) in [
            (&admin, "admin", allowed[0]),
            (&lena, "lena", allowed[1]),
            (&arjun, "arjun", allowed[2]),
            (&tomas, "tomas", allowed[3]),
            (&priya, "priya", allowed[4]),
            (&cli, "cli", allowed[5]),
        ] {
            let want = if ok {
                StatusCode::OK
            } else {
                StatusCode::FORBIDDEN
            };
            assert_eq!(w.status(key, route).await, want, "{route} as {who}");
        }
    }
    let (status, body) = w.call(&priya, "research").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        body["error"]["message"],
        "You do not have access to model 'research'."
    );

    // Which target was called.
    assert_eq!(w.status(&lena, "open").await, StatusCode::OK);
    assert_eq!(w.last_upstream_model().await, "m1");
    assert_eq!(w.status(&lena, "skips").await, StatusCode::OK);
    assert_eq!(w.last_upstream_model().await, "m2");
    assert_eq!(w.status(&lena, "falls-back").await, StatusCode::OK);
    assert_eq!(w.last_upstream_model().await, "m1");

    // A route is allowed when one target can be called by this caller.
    assert_eq!(w.status(&lena, "platform-model").await, StatusCode::OK);
    assert_eq!(w.last_upstream_model().await, "team-only");
    assert_eq!(
        w.status(&tomas, "platform-model").await,
        StatusCode::FORBIDDEN
    );
    // Nothing in it is enabled: unavailable, not refused (the console shows
    // it as broken). Refused is a route with an enabled model that this key
    // may not call.
    let (status, body) = w.call(&admin, "dead").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["error"]["type"], "upstream_error");
    assert_eq!(
        w.status(&lena, "dead").await,
        StatusCode::SERVICE_UNAVAILABLE
    );

    assert_eq!(
        w.status(&lena, "no-such-route").await,
        StatusCode::NOT_FOUND
    );

    // The allowlist of the key names the route, not its targets.
    let by_route = w.key(Some(org.lena), Some(&["open"])).await;
    let by_model = w.key(Some(org.lena), Some(&["p/m1"])).await;
    w.refresh().await;
    assert_eq!(w.status(&by_route, "open").await, StatusCode::OK);
    assert_eq!(w.status(&by_route, "p/m1").await, StatusCode::FORBIDDEN);
    assert_eq!(w.status(&by_model, "open").await, StatusCode::FORBIDDEN);
    assert_eq!(w.status(&by_model, "p/m1").await, StatusCode::OK);
}

#[tokio::test]
async fn a_route_whose_models_are_gone_answers_503() {
    let w = world().await;
    let m1 = w.model("m1", true, Grant::Everyone).await;
    w.route("r", &[m1], &[], None).await;
    let key = w.key(Some(w.org.lena), None).await;
    w.refresh().await;
    assert_eq!(w.status(&key, "r").await, StatusCode::OK);

    let mut tx = w.org.api.store.begin().await.unwrap();
    assert!(tx.delete_model(m1).await.unwrap());
    tx.commit().await.unwrap();
    w.refresh().await;
    let (status, body) = w.call(&key, "r").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["error"]["type"], "upstream_error");
    assert!(!body.to_string().contains("p/"));
}

#[tokio::test]
async fn v1_models_lists_only_callable() {
    let w = world().await;
    let open = w.model("open", true, Grant::Everyone).await;
    w.model("team", true, Grant::Team).await;
    w.model("mine", true, Grant::User).await;
    w.model("off", false, Grant::Everyone).await;
    w.model("nobody", true, Grant::Nobody).await;
    let team_only = w.model("team-only", true, Grant::Team).await;
    let org = &w.org;
    w.route("everyone", &[open], &[], None).await;
    w.route("platform", &[open], &[], Some(&[org.platform]))
        .await;
    w.route("unusable", &[team_only], &[], None).await;

    let admin = w.key(Some(org.maya), None).await;
    let lena = w.key(Some(org.lena), None).await;
    let tomas = w.key(Some(org.tomas), None).await;
    let cli = w.key(None, None).await;
    let narrow = w
        .key(Some(org.lena), Some(&["p/open", "platform", "p/off"]))
        .await;
    let empty = w.key(Some(org.lena), Some(&["nothing"])).await;
    w.refresh().await;

    assert_eq!(
        w.listed_ids(&admin).await,
        [
            "everyone",
            "p/mine",
            "p/nobody",
            "p/open",
            "p/team",
            "p/team-only",
            "platform",
            "unusable"
        ]
    );
    assert_eq!(
        w.listed_ids(&lena).await,
        [
            "everyone",
            "p/mine",
            "p/open",
            "p/team",
            "p/team-only",
            "platform",
            "unusable"
        ]
    );
    assert_eq!(w.listed_ids(&tomas).await, ["everyone", "p/open"]);
    assert_eq!(w.listed_ids(&cli).await, ["everyone", "p/open"]);
    assert_eq!(w.listed_ids(&narrow).await, ["p/open", "platform"]);
    assert!(w.listed_ids(&empty).await.is_empty());

    let (_, body) = w.listed(&tomas).await;
    assert_eq!(body["object"], "list");
    assert_eq!(
        body["data"][0],
        json!({ "id": "everyone", "object": "model", "created": 0, "owned_by": "ultrafast-route" })
    );
    assert_eq!(
        body["data"][1],
        json!({ "id": "p/open", "object": "model", "created": 0, "owned_by": "p" })
    );

    // Everything listed can be called, and nothing else.
    for id in w.listed_ids(&tomas).await {
        assert_eq!(w.status(&tomas, &id).await, StatusCode::OK, "{id}");
    }

    let (status, body) = w.listed("uf-sk-nope").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["type"], "authentication_error");
}

#[tokio::test]
async fn names_split_at_first_slash() {
    let w = world().await;
    let names = [
        "gpt-4o-2024-08-06",
        "models/gemini-2.0-flash",
        "meta-llama/Llama-3.3-70B",
        "llama3.2:3b",
    ];
    for name in names {
        w.model(name, true, Grant::Everyone).await;
    }
    let key = w.key(Some(w.org.lena), None).await;
    w.refresh().await;

    for name in names {
        let (status, body) = w.call(&key, &format!("p/{name}")).await;
        assert_eq!(status, StatusCode::OK, "{name}: {body}");
        assert_eq!(w.last_upstream_model().await, name);
    }
    let mut want: Vec<String> = names.iter().map(|n| format!("p/{n}")).collect();
    want.sort();
    assert_eq!(w.listed_ids(&key).await, want);

    // Only the first slash separates the provider from the model.
    for missing in [
        "p/meta-llama",
        "p/models",
        "meta-llama/Llama-3.3-70B",
        "/gpt-4o-2024-08-06",
        "p/",
    ] {
        assert_eq!(
            w.status(&key, missing).await,
            StatusCode::NOT_FOUND,
            "{missing}"
        );
    }
    // A name with no slash is a route name.
    assert_eq!(w.status(&key, "llama3.2:3b").await, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn key_allowlist_validation() {
    let w = world().await;
    let m1 = w.model("m1", true, Grant::Everyone).await;
    w.model("meta-llama/Llama-3.3-70B", false, Grant::Nobody)
        .await;
    w.route("r", &[m1], &[], None).await;
    let org = &w.org;
    let maya = org.sign_in("maya").await;

    let create = |allowed: Value| {
        let org = &w.org;
        let maya = &maya;
        async move {
            org.call(
                Some(maya),
                "POST",
                "/api/keys",
                Some(json!({ "name": "k", "allowed": allowed })),
            )
            .await
        }
    };

    let (status, body) = create(json!(["p/m1", "r", "p/meta-llama/Llama-3.3-70B", "r"])).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(
        body["key"]["allowed"],
        json!(["p/m1", "r", "p/meta-llama/Llama-3.3-70B"])
    );
    let id = body["key"]["id"].as_i64().unwrap();
    let (_, shown) = org
        .call(Some(&maya), "GET", &format!("/api/keys/{id}"), None)
        .await;
    assert_eq!(shown["allowed"], body["key"]["allowed"]);
    let secret = body["secret"].as_str().unwrap();
    // It is in force at once.
    assert_eq!(w.status(secret, "p/m1").await, StatusCode::OK);
    assert_eq!(w.status(secret, "r").await, StatusCode::OK);

    let (status, body) = org
        .call(
            Some(&maya),
            "POST",
            "/api/keys",
            Some(json!({ "name": "plain" })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["key"]["allowed"], Value::Null);
    let (_, list) = org.call(Some(&maya), "GET", "/api/keys", None).await;
    let by_name: BTreeMap<&str, &Value> = list["keys"]
        .as_array()
        .unwrap()
        .iter()
        .map(|k| (k["name"].as_str().unwrap(), &k["allowed"]))
        .collect();
    assert_eq!(by_name["plain"], &Value::Null);
    assert_eq!(
        by_name["k"],
        &json!(["p/m1", "r", "p/meta-llama/Llama-3.3-70B"])
    );

    let before = list["keys"].as_array().unwrap().len();
    for bad in [
        json!(["p/nothing"]),
        json!(["nothing"]),
        json!(["p/"]),
        json!([""]),
        json!([]),
        json!(["p/m1", "q/m1"]),
    ] {
        let (status, body) = create(bad.clone()).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}: {body}");
        assert_eq!(error_code(&body), "validation_failed");
        assert!(
            body["error"]["fields"]["allowed"].is_string(),
            "{bad}: {body}"
        );
    }
    let (_, list) = org.call(Some(&maya), "GET", "/api/keys", None).await;
    assert_eq!(list["keys"].as_array().unwrap().len(), before);
    // Not a list of strings.
    let (status, _) = create(json!("p/m1")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_disabled_owner_is_still_refused_before_access() {
    let w = world().await;
    w.model("m1", true, Grant::Everyone).await;
    let key = w.key(Some(w.org.lena), None).await;
    w.refresh().await;
    assert_eq!(w.status(&key, "p/m1").await, StatusCode::OK);
    let mut tx = w.org.api.store.begin().await.unwrap();
    tx.set_user_status(
        w.org.lena,
        ultrafast_gateway::identity::UserStatus::Disabled,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    w.refresh().await;
    assert_eq!(w.status(&key, "p/m1").await, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_key_whose_owner_is_missing_calls_nothing() {
    use ultrafast_gateway::access::{callable_names, resolve, Denied};
    use ultrafast_gateway::snapshot::{SnapKey, Snapshot};

    let w = world().await;
    let open = w.model("open", true, Grant::Everyone).await;
    w.route("everyone", &[open], &[], None).await;
    let snapshot = Snapshot::load(&w.org.api.store, &w.org.api.state.cipher)
        .await
        .unwrap();
    let key = |user_id| SnapKey {
        tags: Default::default(),
        id: 1,
        name: "k".into(),
        user_id,
        team_id: None,
        expires_at: None,
        allowed: None,
        team_only: false,
        guardrails: Vec::new(),
    };

    // No owner at all: only what is for everyone.
    let cli = key(None);
    assert!(resolve(&snapshot, &cli, "p/open").is_ok());
    assert!(resolve(&snapshot, &cli, "everyone").is_ok());
    assert_eq!(callable_names(&snapshot, &cli).len(), 2);

    // An owner the snapshot does not know is not "no owner".
    let ghost = key(Some(999_999));
    assert_eq!(
        resolve(&snapshot, &ghost, "p/open").err(),
        Some(Denied::Forbidden)
    );
    assert_eq!(
        resolve(&snapshot, &ghost, "everyone").err(),
        Some(Denied::Forbidden)
    );
    assert!(callable_names(&snapshot, &ghost).is_empty());
}

#[tokio::test]
async fn v1_models_leaves_out_a_route_with_no_targets() {
    let w = world().await;
    let m1 = w.model("m1", true, Grant::Everyone).await;
    let m2 = w.model("m2", true, Grant::Everyone).await;
    w.route("broken", &[m1], &[], None).await;
    w.route("fine", &[m2], &[], None).await;
    let key = w.key(Some(w.org.lena), None).await;
    w.refresh().await;
    assert_eq!(w.listed_ids(&key).await, ["broken", "fine", "p/m1", "p/m2"]);
    let mut tx = w.org.api.store.begin().await.unwrap();
    assert!(tx.delete_model(m1).await.unwrap());
    tx.commit().await.unwrap();
    w.refresh().await;
    assert_eq!(w.listed_ids(&key).await, ["fine", "p/m2"]);
    assert_eq!(
        w.status(&key, "broken").await,
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[tokio::test]
async fn allowlist_errors_do_not_reveal_hidden_routes() {
    let w = world().await;
    let m1 = w.model("m1", true, Grant::Everyone).await;
    w.model("hidden-model", true, Grant::Nobody).await;
    w.route("admins-only", &[m1], &[], Some(&[])).await;
    let org = &w.org;
    let maya = org.sign_in("maya").await;
    let lena = org.sign_in("lena").await;

    let (status, hidden) = message(&w, &lena, "admins-only").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (_, missing) = message(&w, &lena, "no-such-route").await;
    let (_, hidden_model) = message(&w, &lena, "p/hidden-model").await;
    let (_, missing_model) = message(&w, &lena, "p/nothing").await;
    // The same words, whatever the name.
    let words = |v: &Value, name: &str| v.as_str().unwrap().replace(name, "X");
    assert_eq!(
        words(&hidden, "admins-only"),
        words(&missing, "no-such-route")
    );
    assert_eq!(
        words(&hidden_model, "p/hidden-model"),
        words(&missing_model, "p/nothing")
    );
    assert_eq!(
        words(&hidden, "admins-only"),
        words(&hidden_model, "p/hidden-model")
    );
    assert!(hidden.as_str().unwrap().contains("you can use"));

    // Admins keep the precise message, and may name a route of admins.
    let (_, precise) = message(&w, &maya, "no-such-route").await;
    assert!(precise.as_str().unwrap().contains("exists"));
    let (status, _) = org
        .call(
            Some(&maya),
            "POST",
            "/api/keys",
            Some(json!({ "name": "k", "allowed": ["admins-only"] })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    // What a member can use is accepted.
    let (status, _) = org
        .call(
            Some(&lena),
            "POST",
            "/api/keys",
            Some(json!({ "name": "k", "allowed": ["p/m1"] })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
}

/// The `fields.allowed` answer to creating a key with this one name.
async fn message(w: &World, who: &common::Signed, name: &str) -> (StatusCode, Value) {
    let (status, body) = w
        .org
        .call(
            Some(who),
            "POST",
            "/api/keys",
            Some(json!({ "name": "k", "allowed": [name] })),
        )
        .await;
    (status, body["error"]["fields"]["allowed"].clone())
}
