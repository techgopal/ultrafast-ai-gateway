//! Every `/api` endpoint that needs a caller, called as every kind of
//! caller. The table is the last check on access control: a cell that
//! fails means the table or the code is wrong, and neither is changed
//! without finding out which.

mod common;

use std::collections::BTreeSet;

use axum::http::StatusCode;
use common::{error_code, org_on_disk, send, Org, Signed, ORG_PASSWORD};
use serde_json::{json, Value};
use ultrafast_gateway::api::openapi::spec;
use ultrafast_gateway::identity::{Role, UserStatus};
use ultrafast_gateway::secrets::{generate_key, generate_secret, INVITE_PREFIX, TOKEN_PREFIX};
use ultrafast_gateway::store::{after, NewUser};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Caller {
    /// maya, with a session.
    Admin,
    /// arjun, who leads Platform, with a session.
    Lead,
    /// lena, a member of Platform, with a session.
    Member,
    /// No cookie and no Authorization header.
    Nobody,
    /// lena's access token as `Authorization: Bearer`, without a CSRF
    /// header.
    MemberToken,
}

const CALLERS: [Caller; 5] = [
    Caller::Admin,
    Caller::Lead,
    Caller::Member,
    Caller::Nobody,
    Caller::MemberToken,
];

/// An access token: its id and the token itself.
struct Token {
    id: i64,
    full: String,
}

/// The organization of `common::org` with what the table needs on top.
struct World {
    org: Org,
    /// Invited, in no team.
    sam: i64,
    provider: i64,
    /// A provider that lists one model, on a mock server.
    syncable: i64,
    /// A model of `provider`.
    model: i64,
    /// A route over `model`, open to everyone.
    route: i64,
    /// An alert channel that posts to `_upstream`.
    channel: i64,
    /// An alert rule with no channel.
    rule: i64,
    /// Keeps `syncable` answering.
    _upstream: MockServer,
    /// Owned by lena, in Platform.
    lena_key: i64,
    /// Owned by tomas, in Research.
    tomas_key: i64,
    maya_token: Token,
    arjun_token: Token,
    lena_token: Token,
}

async fn seed_token(org: &Org, user_id: i64) -> Token {
    let token = generate_secret(TOKEN_PREFIX);
    let mut tx = org.api.store.begin().await.unwrap();
    let id = tx
        .insert_token(user_id, "table", &token.hash, &token.display, None)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    Token {
        id,
        full: token.full,
    }
}

async fn seed_key(org: &Org, name: &str, owner: i64, team: i64) -> i64 {
    let key = generate_key();
    let mut tx = org.api.store.begin().await.unwrap();
    let id = tx
        .insert_key(name, &key.hash, &key.display, None, Some(owner), Some(team))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    id
}

async fn world() -> World {
    // On disk: a backup is of a file.
    let org = org_on_disk().await;
    let store = &org.api.store;

    let mut tx = store.begin().await.unwrap();
    let sam = tx
        .insert_user(NewUser {
            email: "sam@example.com",
            name: "Sam",
            role: Role::Member,
            status: UserStatus::Invited,
            password_hash: None,
        })
        .await
        .unwrap();
    let invite = generate_secret(INVITE_PREFIX);
    tx.insert_invite(sam, &invite.hash, &after(3600))
        .await
        .unwrap();
    let provider = tx
        .insert_provider("main", "openai", "https://api.openai.com/v1", None)
        .await
        .unwrap();
    tx.commit().await.unwrap();

    let upstream = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "id": "gpt-4o" }],
        })))
        .mount(&upstream)
        .await;
    let syncable = store
        .insert_provider("syncable", "openai", &upstream.uri(), None)
        .await
        .unwrap();
    let mut tx = store.begin().await.unwrap();
    let model = tx.insert_model(provider, "gpt-4o").await.unwrap();
    let route = tx
        .insert_route(
            "main-route",
            &ultrafast_gateway::store::RouteSettings {
                retries: 2,
                first_token_timeout_ms: 30_000,
                total_timeout_ms: 300_000,
                breaker_failures: 5,
                breaker_window_s: 60,
                breaker_open_s: 30,
            },
            true,
        )
        .await
        .unwrap();
    tx.replace_targets(
        route,
        &ultrafast_gateway::store::TargetsInput {
            primaries: vec![(model, 1)],
            fallbacks: vec![],
        },
    )
    .await
    .unwrap();
    let channel = tx
        .insert_alert_channel(
            "table",
            "webhook",
            &org.api
                .state
                .cipher
                .encrypt(format!("{}/hook", upstream.uri()).as_bytes()),
            &upstream.uri(),
            &org.api.state.cipher.encrypt(b"whsec_table"),
            true,
        )
        .await
        .unwrap();
    let rule = tx
        .insert_alert_rule("table", "circuit_open", "{}", true)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    org.api.state.refresh().await.unwrap();

    let log = |user: i64, team: i64| ultrafast_gateway::store::NewLog {
        tags: None,
        at: "2026-01-01 10:00:00".into(),
        key_id: None,
        user_id: Some(user),
        team_id: Some(team),
        requested: "gpt-4o".into(),
        endpoint: "chat".into(),
        stream: false,
        status: 200,
        provider: Some("main".into()),
        model: Some("gpt-4o".into()),
        input_tokens: Some(1),
        output_tokens: Some(1),
        cost_micros: 0,
        priced: false,
        cached: false,
        estimated: false,
        duration_ms: 1,
        attempts: "[]".into(),
    };
    org.api
        .store
        .insert_logs(&[log(org.lena, org.platform), log(org.tomas, org.research)])
        .await
        .unwrap();
    // Limit 1, which the table's DELETE row removes.
    let mut tx = org.api.store.begin().await.unwrap();
    tx.upsert_limit(
        ultrafast_gateway::limits::LimitScope::Gateway,
        None,
        &ultrafast_gateway::limits::RateLimit {
            requests_per_minute: Some(100),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    // Budget 1, which the table's DELETE row removes.
    tx.upsert_budget(
        ultrafast_gateway::limits::LimitScope::Gateway,
        None,
        1_000_000,
        ultrafast_gateway::budgets::Period::Daily,
        ultrafast_gateway::budgets::BudgetAction::Block,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let lena_key = seed_key(&org, "lena", org.lena, org.platform).await;
    let tomas_key = seed_key(&org, "tomas", org.tomas, org.research).await;
    let maya_token = seed_token(&org, org.maya).await;
    let arjun_token = seed_token(&org, org.arjun).await;
    let lena_token = seed_token(&org, org.lena).await;
    World {
        org,
        sam,
        provider,
        syncable,
        model,
        route,
        channel,
        rule,
        _upstream: upstream,
        lena_key,
        tomas_key,
        maya_token,
        arjun_token,
        lena_token,
    }
}

impl World {
    /// The caller's own access token. Nobody gets lena's, which exists.
    fn token_of(&self, caller: Caller) -> &Token {
        match caller {
            Caller::Admin => &self.maya_token,
            Caller::Lead => &self.arjun_token,
            Caller::Member | Caller::MemberToken | Caller::Nobody => &self.lena_token,
        }
    }

    /// A session made in the store: signing in is not what the table
    /// tests, and it would hash a password for every cell.
    async fn session_of(&self, user_id: i64) -> Signed {
        let session = self.org.api.store.create_session(user_id).await.unwrap();
        Signed {
            cookie: format!("uf_session={}", session.id),
            csrf: session.csrf_token,
            user_id,
        }
    }

    async fn call(
        &self,
        caller: Caller,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let body = body.map(|b| serde_json::to_vec(&b).unwrap());
        let app = &self.org.api.app;
        let user_id = match caller {
            Caller::Admin => self.org.maya,
            Caller::Lead => self.org.arjun,
            Caller::Member => self.org.lena,
            Caller::Nobody => {
                let (status, _, body) = send(app, method, path, &[], body).await;
                return (status, body);
            }
            Caller::MemberToken => {
                let bearer = format!("Bearer {}", self.lena_token.full);
                let headers = [("authorization", bearer.as_str())];
                let (status, _, body) = send(app, method, path, &headers, body).await;
                return (status, body);
            }
        };
        let signed = self.session_of(user_id).await;
        let mut headers = vec![("cookie", signed.cookie.as_str())];
        if method != "GET" {
            headers.push(("x-csrf-token", signed.csrf.as_str()));
        }
        let (status, _, body) = send(app, method, path, &headers, body).await;
        (status, body)
    }
}

type PathOf = fn(&World, Caller) -> String;
type BodyOf = fn() -> Option<Value>;

struct Row {
    number: u32,
    method: &'static str,
    /// The path as the OpenAPI spec writes it.
    template: &'static str,
    /// What the row is about, for the failure message.
    note: &'static str,
    path: PathOf,
    body: BodyOf,
    /// admin, lead, member, nobody.
    expect: [u16; 4],
}

fn no_body() -> Option<Value> {
    None
}

const NEW_PASSWORD: &str = "another horse staple";

#[rustfmt::skip]
fn table() -> Vec<Row> {
    let row = |number, method, template, note, path: PathOf, body: BodyOf, expect| Row {
        number, method, template, note, path, body, expect,
    };
    vec![
        row(1, "GET", "/api/auth/me", "", |_, _| "/api/auth/me".into(), no_body, [200, 200, 200, 401]),
        row(2, "GET", "/api/users", "", |_, _| "/api/users".into(), no_body, [200, 200, 200, 401]),
        row(3, "POST", "/api/users", "valid body", |_, _| "/api/users".into(),
            || Some(json!({ "email": "nora@example.com", "name": "Nora", "role": "member" })),
            [201, 403, 403, 401]),
        row(4, "GET", "/api/users/{id}", "lena", |w, _| format!("/api/users/{}", w.org.lena), no_body,
            [200, 200, 200, 401]),
        row(5, "GET", "/api/users/{id}", "tomas", |w, _| format!("/api/users/{}", w.org.tomas), no_body,
            [200, 404, 404, 401]),
        row(6, "PATCH", "/api/users/{id}", "lena, name", |w, _| format!("/api/users/{}", w.org.lena),
            || Some(json!({ "name": "Lena K" })),
            [200, 404, 200, 401]),
        row(7, "PATCH", "/api/users/{id}", "lena, role admin", |w, _| format!("/api/users/{}", w.org.lena),
            || Some(json!({ "role": "admin" })),
            [200, 404, 403, 401]),
        row(8, "DELETE", "/api/users/{id}", "priya", |w, _| format!("/api/users/{}", w.org.priya), no_body,
            [204, 404, 404, 401]),
        row(9, "GET", "/api/teams", "", |_, _| "/api/teams".into(), no_body, [200, 200, 200, 401]),
        row(10, "POST", "/api/teams", "", |_, _| "/api/teams".into(),
            || Some(json!({ "name": "Design" })),
            [201, 403, 403, 401]),
        row(11, "GET", "/api/teams/{id}", "platform", |w, _| format!("/api/teams/{}", w.org.platform), no_body,
            [200, 200, 200, 401]),
        row(12, "GET", "/api/teams/{id}", "growth", |w, _| format!("/api/teams/{}", w.org.growth), no_body,
            [200, 404, 404, 401]),
        row(13, "PATCH", "/api/teams/{id}", "platform", |w, _| format!("/api/teams/{}", w.org.platform),
            || Some(json!({ "name": "Platform Core" })),
            [200, 200, 403, 401]),
        row(14, "DELETE", "/api/teams/{id}", "growth", |w, _| format!("/api/teams/{}", w.org.growth), no_body,
            [204, 404, 404, 401]),
        row(15, "PUT", "/api/teams/{id}/members/{user_id}", "platform, priya as member",
            |w, _| format!("/api/teams/{}/members/{}", w.org.platform, w.org.priya),
            || Some(json!({ "role": "member" })),
            [204, 403, 403, 401]),
        row(16, "PUT", "/api/teams/{id}/members/{user_id}", "platform, priya as lead",
            |w, _| format!("/api/teams/{}/members/{}", w.org.platform, w.org.priya),
            || Some(json!({ "role": "lead" })),
            [204, 403, 403, 401]),
        row(17, "DELETE", "/api/teams/{id}/members/{user_id}", "platform, lena",
            |w, _| format!("/api/teams/{}/members/{}", w.org.platform, w.org.lena), no_body,
            [204, 204, 403, 401]),
        row(18, "GET", "/api/keys", "", |_, _| "/api/keys".into(), no_body, [200, 200, 200, 401]),
        row(19, "POST", "/api/keys", "own, no team", |_, _| "/api/keys".into(),
            || Some(json!({ "name": "mine" })),
            [201, 201, 201, 401]),
        row(20, "GET", "/api/keys/{id}", "lena's", |w, _| format!("/api/keys/{}", w.lena_key), no_body,
            [200, 200, 200, 401]),
        row(21, "GET", "/api/keys/{id}", "tomas's", |w, _| format!("/api/keys/{}", w.tomas_key), no_body,
            [200, 404, 404, 401]),
        row(22, "DELETE", "/api/keys/{id}", "lena's", |w, _| format!("/api/keys/{}", w.lena_key), no_body,
            [204, 204, 204, 401]),
        row(23, "GET", "/api/providers", "", |_, _| "/api/providers".into(), no_body, [200, 200, 200, 401]),
        row(24, "POST", "/api/providers", "", |_, _| "/api/providers".into(),
            || Some(json!({
                "name": "extra", "kind": "openai",
                "base_url": "https://api.example.com/v1", "api_key": "sk-test",
            })),
            [201, 403, 403, 401]),
        row(25, "PATCH", "/api/providers/{id}", "", |w, _| format!("/api/providers/{}", w.provider),
            || Some(json!({ "base_url": "https://other.example.com/v1" })),
            [200, 403, 403, 401]),
        row(26, "DELETE", "/api/providers/{id}", "", |w, _| format!("/api/providers/{}", w.provider), no_body,
            [204, 403, 403, 401]),
        row(27, "GET", "/api/tokens", "", |_, _| "/api/tokens".into(), no_body, [200, 200, 200, 401]),
        row(28, "POST", "/api/tokens", "", |_, _| "/api/tokens".into(),
            || Some(json!({ "name": "ci" })),
            [201, 201, 201, 401]),
        row(29, "GET", "/api/audit", "", |_, _| "/api/audit".into(), no_body, [200, 403, 403, 401]),
        row(30, "POST", "/api/auth/logout", "", |_, _| "/api/auth/logout".into(), no_body,
            [204, 204, 204, 401]),
        row(31, "POST", "/api/auth/password", "valid body", |_, _| "/api/auth/password".into(),
            || Some(json!({ "current_password": ORG_PASSWORD, "new_password": NEW_PASSWORD })),
            [204, 204, 204, 401]),
        row(32, "POST", "/api/users/{id}/invite", "sam, who is invited",
            |w, _| format!("/api/users/{}/invite", w.sam), no_body,
            [201, 403, 403, 401]),
        row(33, "DELETE", "/api/tokens/{id}", "the caller's own token",
            |w, caller| format!("/api/tokens/{}", w.token_of(caller).id), no_body,
            [204, 204, 204, 401]),
        row(34, "POST", "/api/teams/{id}/members", "platform, priya by email",
            |w, _| format!("/api/teams/{}/members", w.org.platform),
            || Some(json!({ "email": "priya@example.com" })),
            [201, 201, 403, 401]),
        row(35, "GET", "/api/models", "", |_, _| "/api/models".into(), no_body, [200, 200, 200, 401]),
        row(36, "POST", "/api/models", "", |_, _| "/api/models".into(),
            // "main" is the first provider of every world.
            || Some(json!({ "provider_id": 1, "name": "gpt-4o-mini" })),
            [201, 403, 403, 401]),
        row(37, "PATCH", "/api/models/{id}", "", |w, _| format!("/api/models/{}", w.model),
            || Some(json!({ "enabled": true })),
            [200, 403, 403, 401]),
        row(38, "PUT", "/api/models/{id}/grants", "", |w, _| format!("/api/models/{}/grants", w.model),
            || Some(json!({ "everyone": true, "team_ids": [], "user_ids": [] })),
            [200, 403, 403, 401]),
        row(39, "DELETE", "/api/models/{id}", "", |w, _| format!("/api/models/{}", w.model), no_body,
            [204, 403, 403, 401]),
        row(40, "POST", "/api/providers/{id}/sync", "a provider that answers",
            |w, _| format!("/api/providers/{}/sync", w.syncable), no_body,
            [200, 403, 403, 401]),
        row(41, "GET", "/api/routes", "", |_, _| "/api/routes".into(), no_body, [200, 200, 200, 401]),
        row(42, "POST", "/api/routes", "", |_, _| "/api/routes".into(),
            || Some(route_body("new-route")),
            [201, 403, 403, 401]),
        row(43, "GET", "/api/routes/{id}", "", |w, _| format!("/api/routes/{}", w.route), no_body,
            [200, 200, 200, 401]),
        row(44, "PUT", "/api/routes/{id}", "", |w, _| format!("/api/routes/{}", w.route),
            || Some(route_body("renamed")),
            [200, 403, 403, 401]),
        row(45, "DELETE", "/api/routes/{id}", "", |w, _| format!("/api/routes/{}", w.route), no_body,
            [204, 403, 403, 401]),
        row(46, "GET", "/api/routing/health", "", |_, _| "/api/routing/health".into(), no_body,
            [200, 403, 403, 401]),
        row(47, "GET", "/api/settings", "", |_, _| "/api/settings".into(), no_body,
            [200, 403, 403, 401]),
        row(48, "PATCH", "/api/settings", "", |_, _| "/api/settings".into(),
            || Some(json!({ "log_retention_days": 30 })),
            [200, 403, 403, 401]),
        row(49, "GET", "/api/logs", "", |_, _| "/api/logs".into(), no_body, [200, 200, 200, 401]),
        // Log 1 is lena's, in Platform; log 2 is tomas's, in Research.
        row(50, "GET", "/api/logs/{id}", "lena's", |_, _| "/api/logs/1".into(), no_body,
            [200, 200, 200, 401]),
        row(51, "GET", "/api/logs/{id}", "tomas's", |_, _| "/api/logs/2".into(), no_body,
            [200, 404, 404, 401]),
        row(52, "GET", "/api/usage", "", |_, _| "/api/usage".into(), no_body, [200, 200, 200, 401]),
        row(53, "GET", "/api/limits", "", |_, _| "/api/limits".into(), no_body, [200, 200, 200, 401]),
        row(54, "PUT", "/api/limits", "", |_, _| "/api/limits".into(),
            || Some(json!({ "scope": "gateway", "concurrent": 10 })),
            [200, 403, 403, 401]),
        // Limit 1 exists in the world's store for the table: see `world`.
        row(55, "DELETE", "/api/limits/{id}", "the gateway's limit", |_, _| "/api/limits/1".into(), no_body,
            [204, 403, 403, 401]),
        row(56, "GET", "/api/budgets", "", |_, _| "/api/budgets".into(), no_body, [200, 200, 200, 401]),
        row(57, "PUT", "/api/budgets", "", |_, _| "/api/budgets".into(),
            || Some(json!({ "scope": "gateway", "amount_micros": 5_000_000, "period": "weekly", "action": "alert" })),
            [200, 403, 403, 401]),
        // Budget 1 exists in the world's store for the table: see `world`.
        row(58, "DELETE", "/api/budgets/{id}", "the gateway's budget", |_, _| "/api/budgets/1".into(), no_body,
            [204, 403, 403, 401]),
        // A model nobody has: the answer is the pipeline's own, in the OpenAI shape.
        row(59, "POST", "/api/playground/chat", "an unknown model", |_, _| "/api/playground/chat".into(),
            || Some(json!({ "model": "nothing", "messages": [{ "role": "user", "content": "hi" }] })),
            [404, 404, 404, 401]),
        row(60, "GET", "/api/config/export", "", |_, _| "/api/config/export".into(), no_body,
            [200, 403, 403, 401]),
        row(61, "POST", "/api/config/import", "an empty file, as a dry run", |_, _| "/api/config/import".into(),
            || Some(json!({ "format": "ultrafast-config", "version": 1 })),
            [200, 403, 403, 401]),
        row(62, "GET", "/api/backup", "", |_, _| "/api/backup".into(), no_body,
            [200, 403, 403, 401]),
        row(63, "PATCH", "/api/keys/{id}", "lena's tags", |w, _| format!("/api/keys/{}", w.lena_key),
            || Some(json!({ "tags": { "team": "platform" } })),
            [200, 403, 403, 401]),
        row(64, "PATCH", "/api/keys/{id}", "tomas's tags", |w, _| format!("/api/keys/{}", w.tomas_key),
            || Some(json!({ "tags": { "team": "research" } })),
            [200, 404, 404, 401]),
        row(65, "GET", "/api/alerts/channels", "", |_, _| "/api/alerts/channels".into(), no_body,
            [200, 403, 403, 401]),
        row(66, "POST", "/api/alerts/channels", "", |_, _| "/api/alerts/channels".into(),
            || Some(json!({ "name": "chat", "kind": "slack", "url": "https://hooks.example.com/T/x" })),
            [201, 403, 403, 401]),
        row(67, "PATCH", "/api/alerts/channels/{id}", "", |w, _| format!("/api/alerts/channels/{}", w.channel),
            || Some(json!({ "enabled": false })),
            [200, 403, 403, 401]),
        row(68, "DELETE", "/api/alerts/channels/{id}", "", |w, _| format!("/api/alerts/channels/{}", w.channel), no_body,
            [204, 403, 403, 401]),
        row(69, "POST", "/api/alerts/channels/{id}/rotate-secret", "",
            |w, _| format!("/api/alerts/channels/{}/rotate-secret", w.channel), no_body,
            [200, 403, 403, 401]),
        // The mock answers 404 to the post; the test still answers 200 with `ok: false`.
        row(70, "POST", "/api/alerts/channels/{id}/test", "",
            |w, _| format!("/api/alerts/channels/{}/test", w.channel), no_body,
            [200, 403, 403, 401]),
        row(71, "GET", "/api/alerts/rules", "", |_, _| "/api/alerts/rules".into(), no_body,
            [200, 403, 403, 401]),
        row(72, "POST", "/api/alerts/rules", "", |_, _| "/api/alerts/rules".into(),
            || Some(json!({ "name": "errors", "kind": "error_rate",
                            "params": { "scope": "gateway", "percent": 10 }, "channel_ids": [] })),
            [201, 403, 403, 401]),
        row(73, "PATCH", "/api/alerts/rules/{id}", "", |w, _| format!("/api/alerts/rules/{}", w.rule),
            || Some(json!({ "enabled": false })),
            [200, 403, 403, 401]),
        row(74, "DELETE", "/api/alerts/rules/{id}", "", |w, _| format!("/api/alerts/rules/{}", w.rule), no_body,
            [204, 403, 403, 401]),
        row(75, "GET", "/api/alerts/events", "", |_, _| "/api/alerts/events".into(), no_body,
            [200, 403, 403, 401]),
        row(76, "GET", "/api/settings/oidc", "", |_, _| "/api/settings/oidc".into(), no_body,
            [200, 403, 403, 401]),
        row(77, "PUT", "/api/settings/oidc", "", |_, _| "/api/settings/oidc".into(),
            || Some(json!({ "enabled": false, "label": "SSO" })),
            [200, 403, 403, 401]),
        // Nothing listens on port 1: the test answers 200 with `ok: false`.
        row(78, "POST", "/api/settings/oidc/test", "an unreachable issuer", |_, _| "/api/settings/oidc/test".into(),
            || Some(json!({ "issuer": "http://127.0.0.1:1" })),
            [200, 403, 403, 401]),
    ]
}

/// A valid route over the first model of every world. Models are made
/// before the table is read, so its id is known.
fn route_body(name: &str) -> Value {
    json!({
        "name": name,
        "primaries": [{ "model_id": 1, "weight": 1 }],
        "fallbacks": [],
        "retries": 2,
        "first_token_timeout_ms": 30000,
        "total_timeout_ms": 300000,
        "breaker_failures": 5,
        "breaker_window_s": 60,
        "breaker_open_s": 30,
        "everyone": true,
        "team_ids": [],
    })
}

/// The status and, for an error, the code a cell must give.
fn expected(row: &Row, caller: Caller) -> (u16, Option<&'static str>) {
    let status = match caller {
        Caller::Admin => row.expect[0],
        Caller::Lead => row.expect[1],
        Caller::Member => row.expect[2],
        Caller::Nobody => row.expect[3],
        // A token carries the role of its owner. It cannot be signed out.
        Caller::MemberToken if row.number == 30 => return (400, Some("bad_request")),
        // The playground is for a browser session: a token is refused first.
        Caller::MemberToken if row.number == 59 => return (403, Some("forbidden")),
        // Access tokens are made from a browser session only.
        Caller::MemberToken if row.number == 28 => return (403, Some("forbidden")),
        Caller::MemberToken => row.expect[2],
    };
    // The code tells a refusal by the policy from a failed CSRF check,
    // which is a 403 too.
    let code = match status {
        401 => Some("unauthenticated"),
        403 => Some("forbidden"),
        // The playground answers as `/v1` does: no `/api` error code.
        404 if row.number == 59 => None,
        404 => Some("not_found"),
        _ => None,
    };
    (status, code)
}

/// The top-level properties of the body the spec documents for this
/// status of the row's operation.
fn documented_keys<'a>(spec: &'a Value, row: &Row, status: u16) -> BTreeSet<&'a str> {
    let operation = &spec["paths"][row.template][row.method.to_lowercase()];
    let response = &operation["responses"][status.to_string()];
    let schema = &response["content"]["application/json"]["schema"];
    let schema = match schema["$ref"].as_str() {
        Some(reference) => {
            let name = reference.strip_prefix("#/components/schemas/").unwrap();
            &spec["components"]["schemas"][name]
        }
        None => schema,
    };
    schema["properties"]
        .as_object()
        .map(|properties| properties.keys().map(String::as_str).collect())
        .unwrap_or_default()
}

#[tokio::test]
async fn every_endpoint_for_every_role() {
    let rows = table();
    let numbers: Vec<u32> = rows.iter().map(|r| r.number).collect();
    assert_eq!(numbers, (1..=78).collect::<Vec<u32>>());

    let spec = serde_json::to_value(spec()).unwrap();
    let mut failures = Vec::new();
    let mut bodies_compared = 0;
    for row in &rows {
        for caller in CALLERS {
            // A world of its own, so no call changes the result of another.
            let world = world().await;
            let path = (row.path)(&world, caller);
            let (status, body) = world.call(caller, row.method, &path, (row.body)()).await;
            let (want_status, want_code) = expected(row, caller);
            if let Some(sent) = body
                .as_object()
                .filter(|_| matches!(status.as_u16(), 200 | 201))
            {
                let sent: BTreeSet<&str> = sent.keys().map(String::as_str).collect();
                let documented = documented_keys(&spec, row, status.as_u16());
                bodies_compared += 1;
                if sent != documented {
                    failures.push(format!(
                        "row {} {} {} ({}) as {:?}: the body has the keys {:?}, the spec documents {:?}",
                        row.number, row.method, row.template, row.note, caller, sent, documented,
                    ));
                }
            }
            let code_matches = want_code.is_none_or(|code| error_code(&body) == code);
            if status.as_u16() != want_status || !code_matches {
                failures.push(format!(
                    "row {} {} {} ({}) as {:?}: expected {} {}, got {} {}",
                    row.number,
                    row.method,
                    row.template,
                    row.note,
                    caller,
                    want_status,
                    want_code.unwrap_or(""),
                    status.as_u16(),
                    body,
                ));
            }
        }
    }
    assert!(bodies_compared > 50, "only {bodies_compared} bodies");
    assert!(
        failures.is_empty(),
        "{} cells failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// The operations anyone may call. A new one is added here on purpose.
const PUBLIC: [(&str, &str); 4] = [
    ("GET", "/api/setup"),
    ("POST", "/api/setup"),
    ("POST", "/api/auth/login"),
    ("POST", "/api/auth/accept-invite"),
];

#[test]
fn every_documented_operation_is_in_the_role_table() {
    let spec = serde_json::to_value(spec()).unwrap();
    let mut secured = BTreeSet::new();
    let mut open = BTreeSet::new();
    for (path, item) in spec["paths"].as_object().expect("paths") {
        for (method, operation) in item.as_object().expect("a path item") {
            let needs_caller = operation["security"]
                .as_array()
                .is_some_and(|schemes| !schemes.is_empty());
            let operation = (method.to_uppercase(), path.clone());
            if needs_caller {
                secured.insert(operation);
            } else {
                open.insert(operation);
            }
        }
    }
    let owned = |(method, path): (&str, &str)| (method.to_string(), path.to_string());
    let in_table: BTreeSet<_> = table()
        .iter()
        .map(|row| owned((row.method, row.template)))
        .collect();
    let public: BTreeSet<_> = PUBLIC.into_iter().map(owned).collect();

    let without_row: Vec<_> = secured.difference(&in_table).collect();
    assert!(
        without_row.is_empty(),
        "operations without a row in the role table: {without_row:?}"
    );
    let undocumented: Vec<_> = in_table.difference(&secured).collect();
    assert!(
        undocumented.is_empty(),
        "rows that are not a documented operation with a security requirement: {undocumented:?}"
    );
    assert_eq!(
        open, public,
        "the operations without a security requirement must be exactly the public ones"
    );
}

/// The router and the spec come from one list of routes: what the spec
/// names is served, and what it does not name is not.
#[tokio::test]
async fn every_documented_operation_is_routed_and_nothing_else() {
    let world = world().await;
    let app = &world.org.api.app;
    let spec = serde_json::to_value(spec()).unwrap();
    let mut operations = 0;
    for (template, item) in spec["paths"].as_object().expect("paths") {
        let path = template.replace("{id}", "1").replace("{user_id}", "1");
        assert!(!path.contains('{'), "{template}");
        for method in item.as_object().expect("a path item").keys() {
            let method = method.to_uppercase();
            // Without credentials no handler answers 404, so a 404 here
            // comes from the fallback.
            let (status, _, body) = send(app, &method, &path, &[], None).await;
            assert_ne!(status, StatusCode::NOT_FOUND, "{method} {path}: {body}");
            assert_ne!(
                status,
                StatusCode::METHOD_NOT_ALLOWED,
                "{method} {path}: {body}"
            );
            operations += 1;
        }
    }
    assert_eq!(operations, 75);

    for (method, path) in [
        ("GET", "/api/nothing"),
        ("GET", "/api/users/1/keys"),
        ("POST", "/api/api/users"),
    ] {
        let (status, _, body) = send(app, method, path, &[], None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {path}");
        assert_eq!(error_code(&body), "not_found", "{method} {path}");
    }
    let (status, _, body) = send(app, "PUT", "/api/users", &[], None).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(error_code(&body), "method_not_allowed");
}
