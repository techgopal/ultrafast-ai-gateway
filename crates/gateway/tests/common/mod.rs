#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::Router;
use tower::ServiceExt;
use ultrafast_gateway::app::{
    router, AppState, DEFAULT_MAX_BODY_BYTES, DEFAULT_MAX_PROVIDER_RESPONSE_BYTES,
};
use ultrafast_gateway::identity::password::{hash_password, warm_up};
use ultrafast_gateway::identity::{Role, TeamRole, UserStatus};
use ultrafast_gateway::secrets::{generate_key, Cipher};
use ultrafast_gateway::store::{NewUser, Store};
use wiremock::MockServer;

pub struct Harness {
    pub app: Router,
    pub upstream: MockServer,
    pub key: String,
    pub store: Store,
}

/// A gateway with one provider named "p" of the given kind, pointing at a mock server.
pub async fn harness(kind: &str) -> Harness {
    harness_with_limit(kind, DEFAULT_MAX_BODY_BYTES).await
}

pub async fn harness_with_limit(kind: &str, max_body_bytes: usize) -> Harness {
    harness_with_limits(kind, max_body_bytes, DEFAULT_MAX_PROVIDER_RESPONSE_BYTES).await
}

/// Like [`harness`], with a cap on the provider response size.
pub async fn harness_with_response_limit(kind: &str, max_response_bytes: usize) -> Harness {
    harness_with_limits(kind, DEFAULT_MAX_BODY_BYTES, max_response_bytes).await
}

async fn harness_with_limits(
    kind: &str,
    max_body_bytes: usize,
    max_provider_response_bytes: usize,
) -> Harness {
    let upstream = MockServer::start().await;
    let store = Store::open_in_memory().await.unwrap();
    let cipher = Cipher::from_hex(&Cipher::generate_master_hex()).unwrap();
    let credential = cipher.encrypt(b"provider-secret");
    store
        .insert_provider("p", kind, &upstream.uri(), Some(&credential))
        .await
        .unwrap();
    let key = generate_key();
    store
        .insert_key("test", &key.hash, &key.display, None)
        .await
        .unwrap();
    warm_up().unwrap();
    let mut state = AppState::new(store.clone(), cipher);
    state.max_body_bytes = max_body_bytes;
    state.max_provider_response_bytes = max_provider_response_bytes;
    Harness {
        app: router(Arc::new(state)),
        upstream,
        key: key.full,
        store,
    }
}

pub async fn post_chat(app: &Router, key: Option<&str>, body: &str) -> (StatusCode, String) {
    let mut req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json");
    if let Some(k) = key {
        req = req.header("authorization", format!("Bearer {k}"));
    }
    let resp = app
        .clone()
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

/// A gateway for `/api` tests.
pub struct Api {
    pub app: Router,
    pub store: Store,
    /// The state behind `app`, for tests that need its cipher.
    pub state: Arc<AppState>,
}

/// A signed-in browser session.
pub struct Signed {
    pub cookie: String,
    pub csrf: String,
    pub user_id: i64,
}

/// An empty in-memory database, with insecure cookies allowed.
pub async fn api() -> Api {
    api_on(Store::open_in_memory().await.unwrap(), false)
}

/// A gateway over the given store.
pub fn api_on(store: Store, cookie_secure: bool) -> Api {
    warm_up().unwrap();
    let cipher = Cipher::from_hex(&Cipher::generate_master_hex()).unwrap();
    let mut state = AppState::new(store.clone(), cipher);
    state.cookie_secure = cookie_secure;
    let state = Arc::new(state);
    Api {
        app: router(state.clone()),
        store,
        state,
    }
}

/// Hashing is slow on purpose, so each test password is hashed once.
fn hash_of(password: &str) -> String {
    static HASHES: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    let mut hashes = HASHES.get_or_init(Default::default).lock().unwrap();
    hashes
        .entry(password.to_string())
        .or_insert_with(|| hash_password(password).unwrap())
        .clone()
}

/// Adds an active user with a password. `email` must be in normalized form.
pub async fn seed_user(store: &Store, email: &str, role: Role, password: &str) -> i64 {
    let hash = hash_of(password);
    let mut tx = store.begin().await.unwrap();
    let id = tx
        .insert_user(NewUser {
            email,
            name: "Test User",
            role,
            status: UserStatus::Active,
            password_hash: Some(&hash),
        })
        .await
        .unwrap();
    tx.commit().await.unwrap();
    id
}

pub async fn seed_team(store: &Store, name: &str, members: &[(i64, TeamRole)]) -> i64 {
    let mut tx = store.begin().await.unwrap();
    let id = tx.insert_team(name).await.unwrap();
    for (user_id, role) in members {
        tx.put_member(id, *user_id, *role).await.unwrap();
    }
    tx.commit().await.unwrap();
    id
}

/// The `uf_session` value of a `Set-Cookie` header, as `uf_session=<value>`.
pub fn cookie_pair(headers: &HeaderMap) -> String {
    let set = headers
        .get("set-cookie")
        .expect("a Set-Cookie header")
        .to_str()
        .unwrap();
    set.split(';').next().unwrap().trim().to_string()
}

/// Signs in and panics unless that works.
pub async fn sign_in(app: &Router, email: &str, password: &str) -> Signed {
    let (status, headers, body) = call(
        app,
        "POST",
        "/api/auth/login",
        None,
        Some(serde_json::json!({ "email": email, "password": password })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "sign-in failed");
    Signed {
        cookie: cookie_pair(&headers),
        csrf: body["csrf_token"].as_str().unwrap().to_string(),
        user_id: body["user"]["id"].as_i64().unwrap(),
    }
}

/// Sends a request with exactly the given headers.
pub async fn send(
    app: &Router,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<Vec<u8>>,
) -> (StatusCode, HeaderMap, serde_json::Value) {
    let mut req = Request::builder().method(method).uri(path);
    for (name, value) in headers {
        req = req.header(*name, *value);
    }
    let body = match body {
        Some(bytes) => {
            req = req.header("content-type", "application/json");
            Body::from(bytes)
        }
        None => Body::empty(),
    };
    let resp = app.clone().oneshot(req.body(body).unwrap()).await.unwrap();
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let value = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("the response body must be JSON")
    };
    (status, headers, value)
}

fn encode(body: Option<serde_json::Value>) -> Option<Vec<u8>> {
    body.map(|b| serde_json::to_vec(&b).unwrap())
}

/// Sends the session cookie and, for methods other than GET, the CSRF header.
pub async fn call(
    app: &Router,
    method: &str,
    path: &str,
    auth: Option<&Signed>,
    body: Option<serde_json::Value>,
) -> (StatusCode, HeaderMap, serde_json::Value) {
    let mut headers: Vec<(&str, &str)> = Vec::new();
    if let Some(signed) = auth {
        headers.push(("cookie", &signed.cookie));
        if method != "GET" {
            headers.push(("x-csrf-token", &signed.csrf));
        }
    }
    send(app, method, path, &headers, encode(body)).await
}

pub async fn call_with_token(
    app: &Router,
    method: &str,
    path: &str,
    token: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, HeaderMap, serde_json::Value) {
    let bearer = format!("Bearer {token}");
    send(
        app,
        method,
        path,
        &[("authorization", &bearer)],
        encode(body),
    )
    .await
}

/// The password of every user of [`org`].
pub const ORG_PASSWORD: &str = "correct horse battery";

/// A small organization: an admin, a lead, two members and a user without
/// a team, in three teams.
pub struct Org {
    pub api: Api,
    /// Admin.
    pub maya: i64,
    /// Lead of Platform, member of Research.
    pub arjun: i64,
    /// Member of Platform.
    pub lena: i64,
    /// Member of Research.
    pub tomas: i64,
    /// In no team.
    pub priya: i64,
    pub platform: i64,
    pub research: i64,
    /// A team without members.
    pub growth: i64,
}

/// The email of a user of [`org`], from their first name.
pub fn email_of(name: &str) -> String {
    format!("{name}@example.com")
}

pub async fn org() -> Org {
    let api = api().await;
    let store = &api.store;
    let maya = seed_user(store, &email_of("maya"), Role::Admin, ORG_PASSWORD).await;
    let arjun = seed_user(store, &email_of("arjun"), Role::Member, ORG_PASSWORD).await;
    let lena = seed_user(store, &email_of("lena"), Role::Member, ORG_PASSWORD).await;
    let tomas = seed_user(store, &email_of("tomas"), Role::Member, ORG_PASSWORD).await;
    let priya = seed_user(store, &email_of("priya"), Role::Member, ORG_PASSWORD).await;
    let platform = seed_team(
        store,
        "Platform",
        &[(arjun, TeamRole::Lead), (lena, TeamRole::Member)],
    )
    .await;
    let research = seed_team(
        store,
        "Research",
        &[(arjun, TeamRole::Member), (tomas, TeamRole::Member)],
    )
    .await;
    let growth = seed_team(store, "Growth", &[]).await;
    Org {
        api,
        maya,
        arjun,
        lena,
        tomas,
        priya,
        platform,
        research,
        growth,
    }
}

impl Org {
    /// Signs in the user with this first name.
    pub async fn sign_in(&self, name: &str) -> Signed {
        sign_in(&self.api.app, &email_of(name), ORG_PASSWORD).await
    }

    /// Sends a request as `who`, or without credentials.
    pub async fn call(
        &self,
        who: Option<&Signed>,
        method: &str,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> (StatusCode, serde_json::Value) {
        let (status, _, body) = call(&self.api.app, method, path, who, body).await;
        (status, body)
    }

    /// The `action` of every audit entry, oldest first.
    pub async fn audit_actions(&self) -> Vec<String> {
        let mut rows = self.api.store.list_audit(200, None).await.unwrap();
        rows.reverse();
        rows.into_iter().map(|r| r.action).collect()
    }

    /// The summary of the newest audit entry with this action.
    pub async fn last_summary(&self, action: &str) -> String {
        let rows = self.api.store.list_audit(200, None).await.unwrap();
        rows.into_iter()
            .find(|r| r.action == action)
            .unwrap_or_else(|| panic!("no audit entry for {action}"))
            .summary
    }
}

/// The `error.code` of an error body.
pub fn error_code(body: &serde_json::Value) -> &str {
    body["error"]["code"].as_str().unwrap_or("<no code>")
}

/// The status, the headers and the exact bytes of an answer.
pub async fn raw(
    org: &Org,
    who: &Signed,
    method: &str,
    path: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, Vec<(String, String)>, Vec<u8>) {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header("cookie", &who.cookie)
        .header("x-csrf-token", &who.csrf);
    let body = match body {
        Some(value) => {
            req = req.header("content-type", "application/json");
            Body::from(serde_json::to_vec(&value).unwrap())
        }
        None => Body::empty(),
    };
    let resp = org
        .api
        .app
        .clone()
        .oneshot(req.body(body).unwrap())
        .await
        .unwrap();
    let status = resp.status();
    let headers = resp
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap().to_string()))
        .collect();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, headers, bytes.to_vec())
}
