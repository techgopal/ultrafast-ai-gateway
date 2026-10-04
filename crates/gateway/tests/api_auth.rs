mod common;

use std::net::SocketAddr;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use common::{
    api, api_behind, api_on, call, call_with_token, cookie_pair, harness, post_chat, seed_team,
    seed_user, send, sign_in, Api, Signed,
};
use serde_json::{json, Value};
use tower::ServiceExt;
use ultrafast_gateway::identity::{Role, TeamRole, UserStatus};
use ultrafast_gateway::secrets::{generate_secret, INVITE_PREFIX, TOKEN_PREFIX};
use ultrafast_gateway::store::{after, NewUser, Store};
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

const PASSWORD: &str = "correct horse battery";
const NEW_PASSWORD: &str = "another horse battery";
const EMAIL: &str = "maya@example.com";

fn code(body: &Value) -> &str {
    body["error"]["code"].as_str().unwrap_or("<no code>")
}

async fn login(api: &Api, email: &str, password: &str) -> (StatusCode, Value) {
    let (status, _, body) = call(
        &api.app,
        "POST",
        "/api/auth/login",
        None,
        Some(json!({ "email": email, "password": password })),
    )
    .await;
    (status, body)
}

async fn set_status(store: &Store, user_id: i64, status: UserStatus) {
    let mut tx = store.begin().await.unwrap();
    assert!(tx.set_user_status(user_id, status).await.unwrap());
    tx.commit().await.unwrap();
}

/// Adds an invited user without a password and returns their id.
async fn seed_invited(store: &Store, email: &str) -> i64 {
    let mut tx = store.begin().await.unwrap();
    let id = tx
        .insert_user(NewUser {
            email,
            name: "Invited User",
            role: Role::Member,
            status: UserStatus::Invited,
            password_hash: None,
        })
        .await
        .unwrap();
    tx.commit().await.unwrap();
    id
}

/// Adds an invite for the user and returns the full token.
async fn seed_invite(store: &Store, user_id: i64, expires_at: &str) -> String {
    let invite = generate_secret(INVITE_PREFIX);
    let mut tx = store.begin().await.unwrap();
    tx.insert_invite(user_id, &invite.hash, expires_at)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    invite.full
}

async fn seed_token(store: &Store, user_id: i64) -> String {
    let token = generate_secret(TOKEN_PREFIX);
    let mut tx = store.begin().await.unwrap();
    tx.insert_token(user_id, "ci", &token.hash, &token.display, None)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    token.full
}

async fn me(api: &Api, signed: &Signed) -> (StatusCode, Value) {
    let (status, _, body) = call(&api.app, "GET", "/api/auth/me", Some(signed), None).await;
    (status, body)
}

fn cookie_value(signed: &Signed) -> &str {
    signed.cookie.strip_prefix("uf_session=").unwrap()
}

#[tokio::test]
async fn setup_creates_the_first_admin_once() {
    let api = api().await;
    let (status, _, body) = call(&api.app, "GET", "/api/setup", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "needs_setup": true }));

    let setup_code = api
        .state
        .setup_code
        .clone()
        .expect("a gateway without users has a setup code");
    let request = json!({
        "email": " Maya@Example.com ", "name": " Maya ", "password": PASSWORD,
        "setup_code": setup_code,
    });
    let (status, headers, body) =
        call(&api.app, "POST", "/api/setup", None, Some(request.clone())).await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(headers.get("set-cookie").is_none());
    assert_eq!(body["email"], EMAIL);
    assert_eq!(body["name"], "Maya");
    assert_eq!(body["role"], "admin");
    assert_eq!(body["status"], "active");
    assert!(body["id"].is_i64());
    assert!(body["created_at"].is_string());
    assert!(body["last_active_at"].is_null());
    let text = body.to_string();
    assert!(!text.contains("password"));
    assert!(!text.contains("argon2"));
    assert_eq!(body.as_object().unwrap().len(), 8);
    assert_eq!(body["teams"], json!([]));

    let (_, _, body) = call(&api.app, "GET", "/api/setup", None, None).await;
    assert_eq!(body, json!({ "needs_setup": false }));

    let (status, _, body) = call(&api.app, "POST", "/api/setup", None, Some(request)).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(code(&body), "already_set_up");
    assert!(body["error"].get("fields").is_none());
    assert_eq!(api.store.count_users().await.unwrap(), 1);

    let audit = api.store.list_audit(10, None).await.unwrap();
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].action, "setup.create_admin");
    assert!(audit[0].summary.contains(EMAIL));

    sign_in(&api.app, EMAIL, PASSWORD).await;
}

#[tokio::test]
async fn setup_validates_each_field() {
    let api = api().await;
    let (status, _, body) = call(
        &api.app,
        "POST",
        "/api/setup",
        None,
        Some(json!({
            "email": "x", "name": "", "password": "short",
            "setup_code": api.state.setup_code.clone().unwrap(),
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(code(&body), "validation_failed");
    let fields = body["error"]["fields"].as_object().unwrap();
    assert_eq!(fields.len(), 3);
    for key in ["email", "name", "password"] {
        assert!(fields[key].is_string(), "{key} is missing");
    }
    assert!(!body.to_string().contains("short"));

    let long_name = "n".repeat(101);
    let (status, _, body) = call(
        &api.app,
        "POST",
        "/api/setup",
        None,
        Some(json!({
            "email": EMAIL, "name": long_name, "password": PASSWORD,
            "setup_code": api.state.setup_code.clone().unwrap(),
        })),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let fields = body["error"]["fields"].as_object().unwrap();
    assert_eq!(fields.len(), 1);
    assert!(fields.contains_key("name"));

    assert_eq!(api.store.count_users().await.unwrap(), 0);
    assert!(api.store.list_audit(10, None).await.unwrap().is_empty());
}

#[tokio::test]
async fn login_sets_a_strict_cookie() {
    let api = api().await;
    seed_user(&api.store, EMAIL, Role::Admin, PASSWORD).await;
    let (status, headers, body) = call(
        &api.app,
        "POST",
        "/api/auth/login",
        None,
        Some(json!({ "email": "  MAYA@example.com", "password": PASSWORD })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["user"]["email"], EMAIL);
    assert!(body["user"].get("password_hash").is_none());
    assert_eq!(body["csrf_token"].as_str().unwrap().len(), 64);
    let cookie = headers.get("set-cookie").unwrap().to_str().unwrap();
    assert!(cookie.starts_with("uf_session="));
    for part in ["HttpOnly", "SameSite=Strict", "Path=/", "Max-Age=43200"] {
        assert!(cookie.split("; ").any(|p| p == part), "{part} is missing");
    }
    assert!(!cookie.contains("Secure"));
    assert_eq!(cookie_pair(&headers).len(), "uf_session=".len() + 64);
    assert!(!body
        .to_string()
        .contains(&cookie_pair(&headers)["uf_session=".len()..]));

    let secure = api_on(Store::open_in_memory().await.unwrap(), true).await;
    seed_user(&secure.store, EMAIL, Role::Admin, PASSWORD).await;
    let (status, headers, _) = call(
        &secure.app,
        "POST",
        "/api/auth/login",
        None,
        Some(json!({ "email": EMAIL, "password": PASSWORD })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let cookie = headers.get("set-cookie").unwrap().to_str().unwrap();
    assert!(cookie.split("; ").any(|p| p == "Secure"));
    assert!(cookie.contains("HttpOnly"));
}

#[tokio::test]
async fn login_failures_are_indistinguishable() {
    let api = api().await;
    seed_user(&api.store, EMAIL, Role::Admin, PASSWORD).await;
    let disabled = seed_user(&api.store, "off@example.com", Role::Member, PASSWORD).await;
    set_status(&api.store, disabled, UserStatus::Disabled).await;
    seed_invited(&api.store, "new@example.com").await;

    let expected = json!({
        "error": { "code": "invalid_credentials", "message": "Email or password is incorrect." }
    });
    let cases = [
        ("nobody@example.com", PASSWORD),
        (EMAIL, "wrong horse battery"),
        ("off@example.com", PASSWORD),
        ("new@example.com", PASSWORD),
        ("not an email", PASSWORD),
    ];
    for (email, password) in cases {
        let (status, headers, body) = call(
            &api.app,
            "POST",
            "/api/auth/login",
            None,
            Some(json!({ "email": email, "password": password })),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{email}");
        assert_eq!(body, expected, "{email}");
        assert!(headers.get("set-cookie").is_none(), "{email}");
    }
    assert!(api.store.list_audit(10, None).await.unwrap().is_empty());
}

#[tokio::test]
async fn login_is_limited_per_email() {
    let api = api().await;
    seed_user(&api.store, EMAIL, Role::Admin, PASSWORD).await;
    seed_user(&api.store, "omar@example.com", Role::Member, PASSWORD).await;
    for _ in 0..5 {
        let (status, _) = login(&api, EMAIL, "wrong horse battery").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
    let (status, body) = login(&api, EMAIL, PASSWORD).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(code(&body), "too_many_attempts");
    // The limit follows the normalized email.
    let (status, _) = login(&api, " MAYA@example.com ", PASSWORD).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    // Another account is not affected.
    let (status, _) = login(&api, "omar@example.com", PASSWORD).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_failures_cannot_exceed_the_limit() {
    let api = api().await;
    seed_user(&api.store, EMAIL, Role::Admin, PASSWORD).await;
    let attempts = (0..12).map(|_| login(&api, EMAIL, "wrong horse battery"));
    let results = futures::future::join_all(attempts).await;
    let count = |status: StatusCode| results.iter().filter(|(s, _)| *s == status).count();
    assert_eq!(count(StatusCode::UNAUTHORIZED), 5);
    assert_eq!(count(StatusCode::TOO_MANY_REQUESTS), 7);
    for (status, body) in &results {
        if *status == StatusCode::TOO_MANY_REQUESTS {
            assert_eq!(code(body), "too_many_attempts");
        }
    }
    let (status, _) = login(&api, EMAIL, PASSWORD).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_wrong_current_passwords_cannot_exceed_the_limit() {
    let api = api().await;
    seed_user(&api.store, EMAIL, Role::Member, PASSWORD).await;
    let signed = sign_in(&api.app, EMAIL, PASSWORD).await;
    let attempts =
        (0..12).map(|_| change_password(&api, &signed, "wrong horse battery", NEW_PASSWORD));
    let results = futures::future::join_all(attempts).await;
    let count = |status: StatusCode| results.iter().filter(|(s, _)| *s == status).count();
    assert_eq!(count(StatusCode::UNAUTHORIZED), 5);
    assert_eq!(count(StatusCode::TOO_MANY_REQUESTS), 7);
}

#[tokio::test]
async fn a_successful_sign_in_costs_nothing() {
    let api = api().await;
    seed_user(&api.store, EMAIL, Role::Admin, PASSWORD).await;
    // 19 of the address's 20 failures are used up by other emails.
    for n in 0..19 {
        let (status, _) = login(&api, &format!("user{n}@example.com"), PASSWORD).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
    for _ in 0..4 {
        let (status, _) = login(&api, EMAIL, PASSWORD).await;
        assert_eq!(status, StatusCode::OK);
    }
    // A changed password is a success too.
    let signed = sign_in(&api.app, EMAIL, PASSWORD).await;
    for _ in 0..3 {
        let (status, _) = change_password(&api, &signed, PASSWORD, PASSWORD).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }
    // The one slot left is still there, and it is the last.
    let (status, _) = login(&api, "last@example.com", PASSWORD).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = login(&api, EMAIL, PASSWORD).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn login_limit_clears_on_success() {
    let api = api().await;
    seed_user(&api.store, EMAIL, Role::Admin, PASSWORD).await;
    for _ in 0..2 {
        for _ in 0..4 {
            let (status, _) = login(&api, EMAIL, "wrong horse battery").await;
            assert_eq!(status, StatusCode::UNAUTHORIZED);
        }
        let (status, _) = login(&api, EMAIL, PASSWORD).await;
        assert_eq!(status, StatusCode::OK);
    }
}

#[tokio::test]
async fn me_returns_user_teams_and_csrf() {
    let api = api().await;
    let id = seed_user(&api.store, EMAIL, Role::Member, PASSWORD).await;
    let other = seed_user(&api.store, "omar@example.com", Role::Member, PASSWORD).await;
    let platform = seed_team(&api.store, "platform", &[(id, TeamRole::Lead)]).await;
    let search = seed_team(
        &api.store,
        "search",
        &[(id, TeamRole::Member), (other, TeamRole::Lead)],
    )
    .await;
    seed_team(&api.store, "elsewhere", &[(other, TeamRole::Member)]).await;

    let signed = sign_in(&api.app, EMAIL, PASSWORD).await;
    assert_eq!(signed.user_id, id);
    let (status, body) = me(&api, &signed).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["user"]["id"], id);
    assert_eq!(body["user"]["email"], EMAIL);
    assert_eq!(body["user"]["role"], "member");
    assert!(body["user"].get("password_hash").is_none());
    assert_eq!(body["csrf_token"], signed.csrf.as_str());
    let mut teams = body["teams"].as_array().unwrap().clone();
    teams.sort_by_key(|t| t["team_id"].as_i64());
    assert_eq!(
        teams,
        vec![
            json!({ "team_id": platform, "name": "platform", "role": "lead" }),
            json!({ "team_id": search, "name": "search", "role": "member" }),
        ]
    );

    let token = seed_token(&api.store, id).await;
    let (status, _, body) = call_with_token(&api.app, "GET", "/api/auth/me", &token, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["user"]["id"], id);
    assert!(body["csrf_token"].is_null());
    assert_eq!(body["teams"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn requests_without_credentials_are_401() {
    let api = api().await;
    seed_user(&api.store, EMAIL, Role::Admin, PASSWORD).await;
    let random = format!("uf_session={}", "ab".repeat(32));
    let bearer = format!("Bearer uf-at-{}", "0".repeat(64));
    let cases: [Vec<(&str, &str)>; 5] = [
        vec![],
        vec![("cookie", &random)],
        vec![("authorization", &bearer)],
        vec![("cookie", "uf_session=")],
        vec![("authorization", "Basic abc")],
    ];
    for headers in cases {
        let (status, _, body) = send(&api.app, "GET", "/api/auth/me", &headers, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{headers:?}");
        assert_eq!(code(&body), "unauthenticated");
    }
}

#[tokio::test]
async fn csrf_is_required_for_session_writes() {
    let api = api().await;
    seed_user(&api.store, EMAIL, Role::Admin, PASSWORD).await;
    let signed = sign_in(&api.app, EMAIL, PASSWORD).await;
    let other = sign_in(&api.app, EMAIL, PASSWORD).await;

    let attempts: [Vec<(&str, &str)>; 4] = [
        vec![("cookie", &signed.cookie)],
        vec![("cookie", &signed.cookie), ("x-csrf-token", "wrong")],
        vec![("cookie", &signed.cookie), ("x-csrf-token", "")],
        // Another session's token does not work either.
        vec![("cookie", &signed.cookie), ("x-csrf-token", &other.csrf)],
    ];
    for headers in attempts {
        let (status, _, body) = send(&api.app, "POST", "/api/auth/logout", &headers, None).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(code(&body), "csrf_failed");
    }
    let (status, _) = me(&api, &signed).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a failed check must not end the session"
    );

    let (status, _, body) = call(&api.app, "POST", "/api/auth/logout", Some(&signed), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null);
}

#[tokio::test]
async fn token_requests_need_no_csrf_header() {
    let api = api().await;
    let id = seed_user(&api.store, EMAIL, Role::Admin, PASSWORD).await;
    let token = seed_token(&api.store, id).await;
    // Reaches the handler, which refuses to sign a token out.
    let (status, _, body) =
        call_with_token(&api.app, "POST", "/api/auth/logout", &token, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(code(&body), "bad_request");
    let lower = format!("bearer {token}");
    let (status, _, _) = send(
        &api.app,
        "GET",
        "/api/auth/me",
        &[("authorization", &lower)],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn a_bad_token_is_not_rescued_by_a_cookie() {
    let api = api().await;
    seed_user(&api.store, EMAIL, Role::Admin, PASSWORD).await;
    let signed = sign_in(&api.app, EMAIL, PASSWORD).await;
    for authorization in ["Bearer uf-at-bad", "Bearer", "Basic abc", ""] {
        let (status, _, body) = send(
            &api.app,
            "GET",
            "/api/auth/me",
            &[("cookie", &signed.cookie), ("authorization", authorization)],
            None,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{authorization:?}");
        assert_eq!(code(&body), "unauthenticated");
    }
    let (status, _) = me(&api, &signed).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn logout_ends_the_session() {
    let api = api().await;
    seed_user(&api.store, EMAIL, Role::Admin, PASSWORD).await;
    let signed = sign_in(&api.app, EMAIL, PASSWORD).await;
    let kept = sign_in(&api.app, EMAIL, PASSWORD).await;
    let (status, headers, _) =
        call(&api.app, "POST", "/api/auth/logout", Some(&signed), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let cookie = headers.get("set-cookie").unwrap().to_str().unwrap();
    assert!(cookie.starts_with("uf_session=;"));
    for part in ["Max-Age=0", "HttpOnly", "SameSite=Strict", "Path=/"] {
        assert!(cookie.split("; ").any(|p| p == part), "{part} is missing");
    }
    let (status, _) = me(&api, &signed).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = me(&api, &kept).await;
    assert_eq!(status, StatusCode::OK);
    assert!(api
        .store
        .live_session(cookie_value(&signed))
        .await
        .unwrap()
        .is_none());

    let audit = api.store.list_audit(20, None).await.unwrap();
    let entries: Vec<_> = audit.iter().filter(|e| e.action == "auth.logout").collect();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].actor_email, EMAIL);
    assert_eq!(entries[0].target_id, Some(signed.user_id));
    assert!(entries[0].summary.contains(EMAIL));
    assert!(!entries[0].summary.contains(cookie_value(&signed)));
    assert!(!entries[0].summary.contains(&signed.csrf));
}

#[tokio::test]
async fn disabling_a_user_ends_access_at_once() {
    let api = api().await;
    let id = seed_user(&api.store, EMAIL, Role::Admin, PASSWORD).await;
    let signed = sign_in(&api.app, EMAIL, PASSWORD).await;
    let token = seed_token(&api.store, id).await;
    assert_eq!(me(&api, &signed).await.0, StatusCode::OK);

    set_status(&api.store, id, UserStatus::Disabled).await;
    let (status, body) = me(&api, &signed).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(code(&body), "unauthenticated");
    assert!(api
        .store
        .live_session(cookie_value(&signed))
        .await
        .unwrap()
        .is_none());
    let (status, _, _) = call_with_token(&api.app, "GET", "/api/auth/me", &token, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Enabling the user again does not bring the session back.
    set_status(&api.store, id, UserStatus::Active).await;
    assert_eq!(me(&api, &signed).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn deleting_a_user_ends_access_at_once() {
    let api = api().await;
    let id = seed_user(&api.store, EMAIL, Role::Member, PASSWORD).await;
    let signed = sign_in(&api.app, EMAIL, PASSWORD).await;
    let mut tx = api.store.begin().await.unwrap();
    assert!(tx.delete_user(id).await.unwrap());
    tx.commit().await.unwrap();
    assert_eq!(me(&api, &signed).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn role_change_is_seen_on_the_next_request() {
    let api = api().await;
    let id = seed_user(&api.store, EMAIL, Role::Admin, PASSWORD).await;
    seed_user(&api.store, "omar@example.com", Role::Admin, PASSWORD).await;
    let signed = sign_in(&api.app, EMAIL, PASSWORD).await;
    assert_eq!(me(&api, &signed).await.1["user"]["role"], "admin");

    let mut tx = api.store.begin().await.unwrap();
    assert!(tx.set_user_role(id, Role::Member).await.unwrap());
    tx.commit().await.unwrap();

    let (status, body) = me(&api, &signed).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["user"]["role"], "member");
}

#[tokio::test]
async fn expired_session_is_401() {
    // A database on disk, so the test can change a row the store does not
    // let anyone change.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("test.db");
    let api = api_on(Store::open(&file).await.unwrap(), false).await;
    seed_user(&api.store, EMAIL, Role::Admin, PASSWORD).await;
    let signed = sign_in(&api.app, EMAIL, PASSWORD).await;
    assert_eq!(me(&api, &signed).await.0, StatusCode::OK);

    let options = sqlx::sqlite::SqliteConnectOptions::new().filename(&file);
    let pool = sqlx::SqlitePool::connect_with(options).await.unwrap();
    let changed = sqlx::query("UPDATE sessions SET expires_at = ?")
        .bind(after(-1))
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(changed.rows_affected(), 1);
    pool.close().await;

    let (status, body) = me(&api, &signed).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(code(&body), "unauthenticated");
}

#[tokio::test]
async fn accept_invite_activates_the_user() {
    let api = api().await;
    let id = seed_invited(&api.store, EMAIL).await;
    let token = seed_invite(&api.store, id, &after(3600)).await;
    let request = json!({ "token": token, "password": PASSWORD });

    let (status, headers, body) = call(
        &api.app,
        "POST",
        "/api/auth/accept-invite",
        None,
        Some(request.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null);
    assert!(headers.get("set-cookie").is_none());
    let user = api.store.user_by_id(id).await.unwrap().unwrap();
    assert_eq!(user.status, UserStatus::Active);

    let signed = sign_in(&api.app, EMAIL, PASSWORD).await;
    assert_eq!(signed.user_id, id);

    let (status, _, body) = call(
        &api.app,
        "POST",
        "/api/auth/accept-invite",
        None,
        Some(request),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(code(&body), "not_found");

    let audit = api.store.list_audit(10, None).await.unwrap();
    let entry = audit
        .iter()
        .find(|a| a.action == "user.accept_invite")
        .unwrap();
    assert_eq!(entry.actor_email, EMAIL);
    assert_eq!(entry.target_id, Some(id));
    assert!(!entry.summary.contains(&token));
}

#[tokio::test]
async fn accept_invite_rejects_bad_input() {
    let api = api().await;
    let accept = |token: String, password: &'static str| {
        let app = api.app.clone();
        async move {
            let (status, _, body) = call(
                &app,
                "POST",
                "/api/auth/accept-invite",
                None,
                Some(json!({ "token": token, "password": password })),
            )
            .await;
            (status, body)
        }
    };

    let unknown = generate_secret(INVITE_PREFIX).full;
    let (status, body) = accept(unknown, PASSWORD).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(code(&body), "not_found");

    let id = seed_invited(&api.store, EMAIL).await;
    let expired = seed_invite(&api.store, id, "2000-01-01 00:00:00").await;
    let (status, _) = accept(expired, PASSWORD).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let live = seed_invite(&api.store, id, &after(3600)).await;
    // The right secret under another prefix is not an invite token.
    let bare = live[INVITE_PREFIX.len()..].to_string();
    let (status, _) = accept(bare.clone(), PASSWORD).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = accept(format!("uf-at-{bare}"), PASSWORD).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, body) = accept(live.clone(), "short").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(code(&body), "validation_failed");
    assert!(body["error"]["fields"]["password"].is_string());
    let user = api.store.user_by_id(id).await.unwrap().unwrap();
    assert_eq!(user.status, UserStatus::Invited);
    assert_eq!(user.password_hash, None);

    let off = seed_invited(&api.store, "off@example.com").await;
    set_status(&api.store, off, UserStatus::Disabled).await;
    let for_disabled = seed_invite(&api.store, off, &after(3600)).await;
    let (status, body) = accept(for_disabled, PASSWORD).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(code(&body), "not_found");
    let user = api.store.user_by_id(off).await.unwrap().unwrap();
    assert_eq!(user.status, UserStatus::Disabled);
    assert_eq!(user.password_hash, None);

    // A user who is already active cannot use an invite to set a password.
    let active = seed_user(&api.store, "active@example.com", Role::Member, PASSWORD).await;
    let before = api.store.user_by_id(active).await.unwrap().unwrap();
    let for_active = seed_invite(&api.store, active, &after(3600)).await;
    let (status, body) = accept(for_active.clone(), NEW_PASSWORD).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        body,
        json!({ "error": { "code": "not_found", "message": "Not found." } })
    );
    let now = api.store.user_by_id(active).await.unwrap().unwrap();
    assert_eq!(now.password_hash, before.password_hash);
    // The invite was left unused.
    let hash = ultrafast_gateway::secrets::hash_key(&for_active);
    assert!(api.store.invite_by_hash(&hash).await.unwrap().is_some());
    let audit = api.store.list_audit(20, None).await.unwrap();
    assert!(audit.iter().all(|e| e.action != "user.accept_invite"));

    // The invite that met a weak password still works.
    let (status, _) = accept(live, PASSWORD).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

async fn change_password(
    api: &Api,
    signed: &Signed,
    current: &str,
    new: &str,
) -> (StatusCode, Value) {
    let (status, _, body) = call(
        &api.app,
        "POST",
        "/api/auth/password",
        Some(signed),
        Some(json!({ "current_password": current, "new_password": new })),
    )
    .await;
    (status, body)
}

#[tokio::test]
async fn changing_password_ends_other_sessions_and_tokens() {
    let api = api().await;
    let id = seed_user(&api.store, EMAIL, Role::Member, PASSWORD).await;
    let other_user = seed_user(&api.store, "omar@example.com", Role::Member, PASSWORD).await;
    let a = sign_in(&api.app, EMAIL, PASSWORD).await;
    let b = sign_in(&api.app, EMAIL, PASSWORD).await;
    let bystander = sign_in(&api.app, "omar@example.com", PASSWORD).await;
    let token = seed_token(&api.store, id).await;
    let bystander_token = seed_token(&api.store, other_user).await;

    let (status, body) = change_password(&api, &a, PASSWORD, NEW_PASSWORD).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null);

    assert_eq!(me(&api, &a).await.0, StatusCode::OK);
    assert_eq!(me(&api, &b).await.0, StatusCode::UNAUTHORIZED);
    let (status, _, _) = call_with_token(&api.app, "GET", "/api/auth/me", &token, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(me(&api, &bystander).await.0, StatusCode::OK);
    let (status, _, _) =
        call_with_token(&api.app, "GET", "/api/auth/me", &bystander_token, None).await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = login(&api, EMAIL, PASSWORD).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(code(&body), "invalid_credentials");
    sign_in(&api.app, EMAIL, NEW_PASSWORD).await;
    sign_in(&api.app, "omar@example.com", PASSWORD).await;

    let audit = api.store.list_audit(20, None).await.unwrap();
    let entries: Vec<_> = audit
        .iter()
        .filter(|e| e.action == "user.change_password")
        .collect();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].actor_email, EMAIL);
    assert_eq!(entries[0].target_id, Some(id));
}

#[tokio::test]
async fn changing_password_with_a_token_ends_every_session() {
    let api = api().await;
    let id = seed_user(&api.store, EMAIL, Role::Member, PASSWORD).await;
    let session = sign_in(&api.app, EMAIL, PASSWORD).await;
    let token = seed_token(&api.store, id).await;
    let (status, _, _) = call_with_token(
        &api.app,
        "POST",
        "/api/auth/password",
        &token,
        Some(json!({ "current_password": PASSWORD, "new_password": NEW_PASSWORD })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(me(&api, &session).await.0, StatusCode::UNAUTHORIZED);
    let (status, _, _) = call_with_token(&api.app, "GET", "/api/auth/me", &token, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn changing_password_needs_the_current_one() {
    let api = api().await;
    let id = seed_user(&api.store, EMAIL, Role::Member, PASSWORD).await;
    let before = api.store.user_by_id(id).await.unwrap().unwrap();
    let signed = sign_in(&api.app, EMAIL, PASSWORD).await;

    let (status, body) = change_password(&api, &signed, "wrong horse battery", NEW_PASSWORD).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(code(&body), "invalid_credentials");

    let (status, body) = change_password(&api, &signed, PASSWORD, "short").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["error"]["fields"]["new_password"].is_string());

    let after_attempts = api.store.user_by_id(id).await.unwrap().unwrap();
    assert_eq!(after_attempts.password_hash, before.password_hash);
    assert_eq!(me(&api, &signed).await.0, StatusCode::OK);
    let audit = api.store.list_audit(20, None).await.unwrap();
    assert!(audit.iter().all(|e| e.action != "user.change_password"));
}

#[tokio::test]
async fn wrong_current_passwords_count_against_the_limit() {
    let api = api().await;
    seed_user(&api.store, EMAIL, Role::Member, PASSWORD).await;
    let signed = sign_in(&api.app, EMAIL, PASSWORD).await;
    for _ in 0..5 {
        let (status, _) = change_password(&api, &signed, "wrong horse battery", NEW_PASSWORD).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
    let (status, body) = change_password(&api, &signed, PASSWORD, NEW_PASSWORD).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(code(&body), "too_many_attempts");
    let (status, _) = login(&api, EMAIL, PASSWORD).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn unknown_fields_and_bad_json_are_400() {
    let api = api().await;
    seed_user(&api.store, "a@b.co", Role::Member, PASSWORD).await;
    let bodies: [&[u8]; 4] = [
        br#"{"email":"a@b.co","password":"x","extra":1}"#,
        b"{not json",
        br#"{"email":"a@b.co"}"#,
        br#"{"email":"a@b.co","password":12345678901234}"#,
    ];
    for bytes in bodies {
        let (status, _, body) = send(
            &api.app,
            "POST",
            "/api/auth/login",
            &[],
            Some(bytes.to_vec()),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(code(&body), "bad_request");
        assert!(body["error"].get("fields").is_none());
        // What was sent is not echoed.
        assert!(!body.to_string().contains("12345678901234"));
    }
}

#[tokio::test]
async fn large_bodies_are_413() {
    let api = api().await;
    let request = json!({ "email": EMAIL, "password": "p".repeat(70 * 1024) });
    let (status, _, body) = call(&api.app, "POST", "/api/auth/login", None, Some(request)).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(code(&body), "payload_too_large");
}

#[tokio::test]
async fn unknown_api_path_is_404_json() {
    let api = api().await;
    seed_user(&api.store, EMAIL, Role::Admin, PASSWORD).await;
    let signed = sign_in(&api.app, EMAIL, PASSWORD).await;
    for (method, path, auth) in [
        ("GET", "/api/nope", None),
        ("GET", "/api/nope", Some(&signed)),
        ("POST", "/api/auth/nope/deeper", None),
        ("GET", "/api", None),
        ("GET", "/api/", None),
    ] {
        let (status, _, body) = call(&api.app, method, path, auth, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {path}");
        assert_eq!(code(&body), "not_found", "{method} {path}");
        assert!(body["error"]["message"].is_string());
    }
}

#[tokio::test]
async fn wrong_method_is_405_json() {
    let api = api().await;
    let (status, _, body) = call(&api.app, "GET", "/api/auth/login", None, None).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(code(&body), "method_not_allowed");
}

#[tokio::test]
async fn nothing_secret_is_audited() {
    let api = api().await;
    let (status, _, _) = call(
        &api.app,
        "POST",
        "/api/setup",
        None,
        Some(json!({
            "email": EMAIL, "name": "Maya", "password": PASSWORD,
            "setup_code": api.state.setup_code.clone().unwrap(),
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let signed = sign_in(&api.app, EMAIL, PASSWORD).await;
    let (status, _) = change_password(&api, &signed, PASSWORD, NEW_PASSWORD).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let hash = api
        .store
        .user_by_email(EMAIL)
        .await
        .unwrap()
        .unwrap()
        .password_hash
        .unwrap();

    let audit = api.store.list_audit(50, None).await.unwrap();
    let mut actions: Vec<&str> = audit.iter().map(|e| e.action.as_str()).collect();
    actions.sort_unstable();
    assert_eq!(
        actions,
        ["auth.login", "setup.create_admin", "user.change_password"]
    );
    let secrets = [
        PASSWORD,
        NEW_PASSWORD,
        cookie_value(&signed),
        signed.csrf.as_str(),
        hash.as_str(),
    ];
    for entry in &audit {
        assert!(entry.summary.contains(EMAIL), "{}", entry.action);
        for secret in secrets {
            assert!(!entry.summary.contains(secret), "{}", entry.action);
            assert!(!entry.actor_email.contains(secret));
            assert!(!entry.target_type.contains(secret));
        }
    }
}

#[tokio::test]
async fn v1_still_works() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "c1", "model": "gpt-4o",
            "choices": [{ "message": { "role": "assistant", "content": "hello" }, "finish_reason": "stop" }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 2 }
        })))
        .expect(1)
        .mount(&h.upstream)
        .await;
    let body = r#"{"model":"p/gpt-4o","messages":[{"role":"user","content":"hi"}]}"#;
    let (status, text) = post_chat(&h.app, Some(&h.key), body).await;
    assert_eq!(status, StatusCode::OK);
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["choices"][0]["message"]["content"], "hello");

    // The same router serves /api, and a virtual key is not an /api credential.
    let (status, _, body) = call(&h.app, "GET", "/api/setup", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "needs_setup": true }));
    let (status, _, _) = call_with_token(&h.app, "GET", "/api/auth/me", &h.key, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// A failed sign-in as seen from `peer`, with extra headers.
async fn login_from(api: &Api, peer: &str, headers: &[(&str, &str)], email: &str) -> StatusCode {
    let mut req = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header("content-type", "application/json");
    for (name, value) in headers {
        req = req.header(*name, *value);
    }
    let body = json!({ "email": email, "password": "wrong horse battery" });
    let mut req = req.body(Body::from(body.to_string())).unwrap();
    let peer: SocketAddr = format!("{peer}:4000").parse().unwrap();
    req.extensions_mut().insert(ConnectInfo(peer));
    api.app.clone().oneshot(req).await.unwrap().status()
}

/// Uses up the 20 failures an address may have, each for another email.
async fn use_up_address(api: &Api, peer: &str, headers: &[(&str, &str)]) {
    for n in 0..20 {
        let status = login_from(api, peer, headers, &format!("user{n}@example.com")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "attempt {n}");
    }
}

#[tokio::test]
async fn trusted_proxy_uses_forwarded_address() {
    let store = Store::open_in_memory().await.unwrap();
    let api = api_behind(store, false, &["10.0.0.0/8"]).await;
    let cf = |ip| [("cf-connecting-ip", ip)];

    // A trusted peer: the limiter is keyed on the Cloudflare address.
    use_up_address(&api, "10.0.0.1", &cf("203.0.113.9")).await;
    let status = login_from(&api, "10.0.0.1", &cf("203.0.113.9"), "more@example.com").await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    // Another client behind the same proxy, and the proxy itself, are fresh.
    let status = login_from(&api, "10.0.0.1", &cf("203.0.113.10"), "more@example.com").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let status = login_from(&api, "10.0.0.1", &[], "more@example.com").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // A header that is not an address is not used.
    let status = login_from(&api, "10.0.0.1", &cf("nonsense"), "more@example.com").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // An untrusted peer: the header is ignored, so the peer's own address
    // is what runs out, however the header changes.
    use_up_address(&api, "198.51.100.7", &[]).await;
    let status = login_from(&api, "198.51.100.7", &cf("203.0.113.77"), "x@example.com").await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);

    // A forwarded chain: the last address that is not trusted is the client.
    let chain = [("x-forwarded-for", "1.1.1.1, 198.51.100.1, 10.0.0.2")];
    use_up_address(&api, "10.0.0.1", &chain).await;
    let spoofed = [("x-forwarded-for", "2.2.2.2, 198.51.100.1, 10.0.0.2")];
    let status = login_from(&api, "10.0.0.1", &spoofed, "more@example.com").await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    let other = [("x-forwarded-for", "198.51.100.2, 10.0.0.2")];
    let status = login_from(&api, "10.0.0.1", &other, "more@example.com").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn setup_without_the_setup_code_is_refused() {
    let api = api().await;
    for request in [
        json!({ "email": EMAIL, "name": "Maya", "password": PASSWORD }),
        json!({ "email": EMAIL, "name": "Maya", "password": PASSWORD, "setup_code": "" }),
        json!({ "email": EMAIL, "name": "Maya", "password": PASSWORD, "setup_code": "AAAA-AAAA-AAAA" }),
        // A wrong code is refused before the fields are looked at.
        json!({ "email": "x", "name": "", "password": "short", "setup_code": "nope" }),
    ] {
        let (status, _, body) = call(&api.app, "POST", "/api/setup", None, Some(request)).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert_eq!(code(&body), "setup_code_invalid");
        assert_eq!(
            body["error"]["message"],
            "The setup code is missing or wrong. It is printed in the gateway's log when it starts."
        );
        assert!(body["error"].get("fields").is_none());
    }
    assert_eq!(api.store.count_users().await.unwrap(), 0);
    assert!(api.store.list_audit(10, None).await.unwrap().is_empty());
}

#[tokio::test]
async fn setup_with_the_setup_code_creates_the_admin_as_typed() {
    let api = api().await;
    let setup_code = api.state.setup_code.clone().unwrap();
    // As people type it: in lower case, without dashes.
    let typed = setup_code.replace('-', "").to_lowercase();
    let (status, _, body) = call(
        &api.app,
        "POST",
        "/api/setup",
        None,
        Some(json!({ "email": EMAIL, "name": "Maya", "password": PASSWORD, "setup_code": typed })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    // Done once: the code is not in any answer, and setup is over.
    assert!(!body.to_string().contains(&setup_code));
    let (status, _, body) = call(
        &api.app,
        "POST",
        "/api/setup",
        None,
        Some(json!({ "email": "x@example.com", "name": "X", "password": PASSWORD, "setup_code": setup_code })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(code(&body), "already_set_up");
}

#[tokio::test]
async fn a_gateway_that_starts_with_a_user_has_no_setup_code() {
    let store = Store::open_in_memory().await.unwrap();
    seed_user(&store, EMAIL, Role::Admin, PASSWORD).await;
    let api = common::api_on(store, false).await;
    assert!(api.state.setup_code.is_none());
}

/// Failures for one email from many addresses do not lock its owner out:
/// they count for each address and email together, and for each address.
#[tokio::test]
async fn failures_from_other_addresses_do_not_lock_an_email_out() {
    let api = api().await;
    seed_user(&api.store, EMAIL, Role::Admin, PASSWORD).await;
    for n in 1..=6u8 {
        for _ in 0..5 {
            let status = login_from(&api, &format!("198.51.100.{n}"), &[], EMAIL).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "address {n}");
        }
        // That address has used up its tries for this email.
        let status = login_from(&api, &format!("198.51.100.{n}"), &[], EMAIL).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "address {n}");
    }
    // The owner, from their own address, still signs in.
    let mut req = Request::builder()
        .method("POST")
        .uri("/api/auth/login")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({ "email": EMAIL, "password": PASSWORD }).to_string(),
        ))
        .unwrap();
    let peer: SocketAddr = "203.0.113.50:4000".parse().unwrap();
    req.extensions_mut().insert(ConnectInfo(peer));
    let status = api.app.clone().oneshot(req).await.unwrap().status();
    assert_eq!(status, StatusCode::OK);
}
