//! A key that a lead makes for another user of a team they lead is a team
//! key: it calls only what is granted to everyone or to that team, never the
//! owner's own grants or an admin's reach, and only while the owner is in
//! the team. A lead can make one only for a member of the team who is not a
//! lead or an admin.

mod common;

use axum::http::StatusCode;
use common::{error_code, org, post_chat, seed_user, sign_in, Org, Signed, ORG_PASSWORD};
use serde_json::{json, Value};
use ultrafast_gateway::identity::{Role, TeamRole};
use ultrafast_gateway::store::{Grants, RouteSettings, TargetsInput};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

const DEFAULTS: RouteSettings = RouteSettings {
    retries: 0,
    first_token_timeout_ms: 30_000,
    total_timeout_ms: 300_000,
    breaker_failures: 5,
    breaker_window_s: 60,
    breaker_open_s: 30,
};

fn chat(model: &str) -> String {
    json!({ "model": model, "messages": [{ "role": "user", "content": "hi" }] }).to_string()
}

struct World {
    org: Org,
    _upstream: MockServer,
    provider_id: i64,
}

/// Who a model or route is granted to.
enum To {
    Everyone,
    Teams(Vec<i64>),
    Users(Vec<i64>),
    Nobody,
}

async fn world() -> World {
    let org = org().await;
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "c1", "model": "x",
            "choices": [{
                "message": { "role": "assistant", "content": "hello" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 2 }
        })))
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
        _upstream: upstream,
        provider_id,
    }
}

impl World {
    async fn model(&self, name: &str, to: To) -> i64 {
        let mut tx = self.org.api.store.begin().await.unwrap();
        let id = tx.insert_model(self.provider_id, name).await.unwrap();
        tx.set_model_enabled(id, true).await.unwrap();
        let grants = match to {
            To::Everyone => Grants {
                everyone: true,
                ..Grants::default()
            },
            To::Teams(team_ids) => Grants {
                team_ids,
                ..Grants::default()
            },
            To::Users(user_ids) => Grants {
                user_ids,
                ..Grants::default()
            },
            To::Nobody => Grants::default(),
        };
        tx.replace_grants(id, &grants).await.unwrap();
        tx.commit().await.unwrap();
        id
    }

    async fn route(&self, name: &str, target: i64, teams: Option<&[i64]>) {
        let mut tx = self.org.api.store.begin().await.unwrap();
        let id = tx
            .insert_route(name, &DEFAULTS, teams.is_none())
            .await
            .unwrap();
        tx.replace_targets(
            id,
            &TargetsInput {
                primaries: vec![(target, 1)],
                fallbacks: vec![],
            },
        )
        .await
        .unwrap();
        tx.replace_route_grants(id, teams.unwrap_or(&[]))
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }

    /// Every model and route, and to whom they are granted:
    /// - `p/open`, route `open-route`: everyone;
    /// - `p/platform`, route `platform-route`: Platform;
    /// - `p/research`, route `research-route`: Research (Lena is put in it);
    /// - `p/lena-own`: Lena herself; `p/arjun-own`: Arjun himself;
    /// - `p/nobody`, route `nobody-route` (over `p/open`): no one, admins only.
    async fn catalog(&self) {
        let org = &self.org;
        let open = self.model("open", To::Everyone).await;
        let platform = self.model("platform", To::Teams(vec![org.platform])).await;
        let research = self.model("research", To::Teams(vec![org.research])).await;
        self.model("lena-own", To::Users(vec![org.lena])).await;
        self.model("arjun-own", To::Users(vec![org.arjun])).await;
        self.model("nobody", To::Nobody).await;
        self.route("open-route", open, None).await;
        self.route("platform-route", platform, Some(&[org.platform]))
            .await;
        self.route("research-route", research, Some(&[org.research]))
            .await;
        self.route("nobody-route", open, Some(&[])).await;
        self.org.api.state.refresh().await.unwrap();
    }

    async fn create_key(&self, who: &Signed, body: Value) -> (StatusCode, Value) {
        self.org
            .call(Some(who), "POST", "/api/keys", Some(body))
            .await
    }

    async fn status(&self, key: &str, model: &str) -> StatusCode {
        post_chat(&self.org.api.app, Some(key), &chat(model))
            .await
            .0
    }

    /// The names from `names` that the key can call.
    async fn callable(&self, key: &str, names: &[&str]) -> Vec<String> {
        let mut out = Vec::new();
        for name in names {
            let status = self.status(key, name).await;
            assert!(
                status == StatusCode::OK || status == StatusCode::FORBIDDEN,
                "{name}: {status}"
            );
            if status == StatusCode::OK {
                out.push(name.to_string());
            }
        }
        out
    }

    async fn listed(&self, key: &str) -> Vec<String> {
        let (status, _, body) = common::send(
            &self.org.api.app,
            "GET",
            "/v1/models",
            &[("authorization", &format!("Bearer {key}"))],
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["id"].as_str().unwrap().to_string())
            .collect()
    }
}

const ALL: [&str; 10] = [
    "p/open",
    "p/platform",
    "p/research",
    "p/lena-own",
    "p/arjun-own",
    "p/nobody",
    "open-route",
    "platform-route",
    "research-route",
    "nobody-route",
];

/// The reviewer's chain: a lead adds an admin to the team they lead by
/// email and makes a key for them. It must not give the lead an admin's
/// reach; a key for a member gives only what is the team's.
#[tokio::test]
async fn a_lead_cannot_borrow_the_access_of_who_they_make_a_key_for() {
    let w = world().await;
    w.catalog().await;
    let org = &w.org;
    let arjun = org.sign_in("arjun").await;

    // Lena is in Research too: a key of Platform must not reach it.
    let mut tx = org.api.store.begin().await.unwrap();
    tx.put_member(org.research, org.lena, TeamRole::Member)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    // The lead adds the admin to the team they lead, by email.
    let (status, body) = org
        .call(
            Some(&arjun),
            "POST",
            &format!("/api/teams/{}/members", org.platform),
            Some(json!({ "email": "maya@example.com" })),
        )
        .await;
    assert!(status.is_success(), "{status} {body}");

    // A key for the admin, in the team: refused.
    let (status, body) = w
        .create_key(
            &arjun,
            json!({ "name": "k", "owner_id": org.maya, "team_id": org.platform }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(error_code(&body), "validation_failed");
    assert_eq!(
        body["error"]["fields"]["owner_id"],
        "must be a member of the team, not a lead or an admin"
    );

    // Another lead of the team: refused too.
    let mut tx = org.api.store.begin().await.unwrap();
    tx.put_member(org.platform, org.priya, TeamRole::Lead)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    org.api.state.refresh().await.unwrap();
    let (status, body) = w
        .create_key(
            &arjun,
            json!({ "name": "k", "owner_id": org.priya, "team_id": org.platform }),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["error"]["fields"]["owner_id"],
        "must be a member of the team, not a lead or an admin"
    );

    // A key for a member works only on what is everyone's or the team's.
    let (status, body) = w
        .create_key(
            &arjun,
            json!({ "name": "k", "owner_id": org.lena, "team_id": org.platform }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let delegated = body["secret"].as_str().unwrap().to_string();
    let team_only = ["p/open", "p/platform", "open-route", "platform-route"];
    assert_eq!(w.callable(&delegated, &ALL).await, team_only);
    let mut listed = w.listed(&delegated).await;
    listed.sort();
    let mut expected: Vec<String> = team_only.iter().map(|s| s.to_string()).collect();
    expected.sort();
    assert_eq!(listed, expected);

    // The member's own key reaches what is hers.
    let lena = org.sign_in("lena").await;
    let (status, body) = w
        .create_key(&lena, json!({ "name": "own", "team_id": org.platform }))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let own = body["secret"].as_str().unwrap().to_string();
    assert_eq!(
        w.callable(&own, &ALL).await,
        [
            "p/open",
            "p/platform",
            "p/research",
            "p/lena-own",
            "open-route",
            "platform-route",
            "research-route"
        ]
    );

    // A key an admin makes for her is hers as if she had made it.
    let maya = org.sign_in("maya").await;
    let (status, body) = w
        .create_key(
            &maya,
            json!({ "name": "by-admin", "owner_id": org.lena, "team_id": org.platform }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let by_admin = body["secret"].as_str().unwrap().to_string();
    assert_eq!(
        w.callable(&by_admin, &ALL).await,
        w.callable(&own, &ALL).await
    );

    // Out of the team, the lead's key for her calls nothing; her own still works.
    let (status, body) = org
        .call(
            Some(&arjun),
            "DELETE",
            &format!("/api/teams/{}/members/{}", org.platform, org.lena),
            None,
        )
        .await;
    assert!(status.is_success(), "{status} {body}");
    assert!(w.callable(&delegated, &ALL).await.is_empty());
    assert!(w.listed(&delegated).await.is_empty());
    assert!(w.callable(&own, &["p/open"]).await == ["p/open"]);

    // Back in the team, it works again.
    let mut tx = org.api.store.begin().await.unwrap();
    tx.put_member(org.platform, org.lena, TeamRole::Member)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    org.api.state.refresh().await.unwrap();
    assert_eq!(w.callable(&delegated, &ALL).await, team_only);
}

/// A key a lead makes for another may be limited only to what the key can
/// call: what is everyone's or the team's.
#[tokio::test]
async fn the_allowlist_of_a_key_for_another_is_cut_to_the_team() {
    let w = world().await;
    w.catalog().await;
    let org = &w.org;
    let arjun = org.sign_in("arjun").await;
    for name in ["p/arjun-own", "p/research", "research-route"] {
        let (status, body) = w
            .create_key(
                &arjun,
                json!({
                    "name": "k", "owner_id": org.lena, "team_id": org.platform,
                    "allowed": [name],
                }),
            )
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{name}: {body}");
        assert_eq!(
            body["error"]["fields"]["allowed"],
            format!("'{name}' is not a model or route of everyone or of this team")
        );
    }
    let (status, body) = w
        .create_key(
            &arjun,
            json!({
                "name": "k", "owner_id": org.lena, "team_id": org.platform,
                "allowed": ["p/open", "platform-route"],
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    // His own key may name what is his.
    let (status, body) = w
        .create_key(&arjun, json!({ "name": "k", "allowed": ["p/arjun-own"] }))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

/// The console lists what a key for another member of a team may call,
/// with the same rule.
#[tokio::test]
async fn models_and_routes_list_what_a_key_for_a_team_member_may_call() {
    let w = world().await;
    w.catalog().await;
    let org = &w.org;
    let arjun = org.sign_in("arjun").await;
    let names = |body: &Value, list: &str, field: &str| -> Vec<String> {
        let mut v: Vec<String> = body[list]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m[field].as_str().unwrap().to_string())
            .collect();
        v.sort();
        v
    };
    let path = format!("/api/models?key_team_id={}", org.platform);
    let (status, body) = org.call(Some(&arjun), "GET", &path, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(names(&body, "models", "name"), ["open", "platform"]);
    let path = format!("/api/routes?key_team_id={}", org.platform);
    let (status, body) = org.call(Some(&arjun), "GET", &path, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        names(&body, "routes", "name"),
        ["open-route", "platform-route"]
    );
    // Only for a team the caller leads.
    for list in ["models", "routes"] {
        let path = format!("/api/{list}?key_team_id={}", org.research);
        let (status, body) = org.call(Some(&arjun), "GET", &path, None).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    }
    // An admin's key for another is not cut: the list stays whole.
    let maya = org.sign_in("maya").await;
    let path = format!("/api/models?key_team_id={}", org.platform);
    let (status, body) = org.call(Some(&maya), "GET", &path, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["models"].as_array().unwrap().len(), 6);
}

/// The maker of a team key going does not make it more: not when they are
/// deleted, and not when their id is given to an admin made later.
#[tokio::test]
async fn a_team_key_stays_one_when_its_maker_is_gone() {
    let w = world().await;
    w.catalog().await;
    let org = &w.org;
    // A lead made last, so that their id is the one given out next.
    let lead = seed_user(
        &org.api.store,
        "kai@example.com",
        Role::Member,
        ORG_PASSWORD,
    )
    .await;
    let mut tx = org.api.store.begin().await.unwrap();
    tx.put_member(org.platform, lead, TeamRole::Lead)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let kai = sign_in(&org.api.app, "kai@example.com", ORG_PASSWORD).await;
    let (status, body) = w
        .create_key(
            &kai,
            json!({ "name": "k", "owner_id": org.lena, "team_id": org.platform }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let key = body["secret"].as_str().unwrap().to_string();
    let team_only = ["p/open", "p/platform", "open-route", "platform-route"];
    assert_eq!(w.callable(&key, &ALL).await, team_only);

    let maya = org.sign_in("maya").await;
    let (status, body) = org
        .call(Some(&maya), "DELETE", &format!("/api/users/{lead}"), None)
        .await;
    assert!(status.is_success(), "{status} {body}");
    assert_eq!(w.callable(&key, &ALL).await, team_only);

    // The same id, now an admin's (made in a later second than the key).
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let admin = seed_user(
        &org.api.store,
        "noor@example.com",
        Role::Admin,
        ORG_PASSWORD,
    )
    .await;
    assert_eq!(admin, lead, "the id is given out again");
    org.api.state.refresh().await.unwrap();
    assert_eq!(w.callable(&key, &ALL).await, team_only);
}
