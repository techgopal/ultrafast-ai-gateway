mod common;

use axum::http::StatusCode;
use common::{call_with_token, error_code, org, seed_user, Org, Signed, ORG_PASSWORD};
use serde_json::{json, Value};
use ultrafast_gateway::identity::Role;
use ultrafast_gateway::secrets::{generate_secret, hash_key, TOKEN_PREFIX};

const PAST: &str = "2000-01-01 00:00:00";
const FUTURE: &str = "2999-01-01 00:00:00";

/// The audit actions of this area, oldest first. Signing in is audited
/// too, and is left out.
async fn audited(org: &Org) -> Vec<String> {
    let all = org.audit_actions().await;
    all.into_iter()
        .filter(|a| a.starts_with("token."))
        .collect()
}

fn token_path(id: i64) -> String {
    format!("/api/tokens/{id}")
}

/// Creates a token and returns its id and its secret.
async fn create(org: &Org, who: &Signed, name: &str) -> (i64, String) {
    let body = json!({ "name": name });
    let (status, body) = org.call(Some(who), "POST", "/api/tokens", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    (
        body["token"]["id"].as_i64().unwrap(),
        body["secret"].as_str().unwrap().to_string(),
    )
}

async fn with_token(
    org: &Org,
    token: &str,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let (status, _, body) = call_with_token(&org.api.app, method, path, token, body).await;
    (status, body)
}

fn names(body: &Value) -> Vec<&str> {
    body["tokens"]
        .as_array()
        .expect("a tokens array")
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn tokens_need_a_sign_in() {
    let org = org().await;
    for (method, path) in [
        ("GET", "/api/tokens"),
        ("POST", "/api/tokens"),
        ("DELETE", "/api/tokens/1"),
    ] {
        let (status, body) = org
            .call(None, method, path, Some(json!({ "name": "ci" })))
            .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {path}");
        assert_eq!(error_code(&body), "unauthenticated");
    }
}

#[tokio::test]
async fn a_token_authenticates_as_its_owner() {
    let org = org().await;
    let lena = org.sign_in("lena").await;
    let (_, secret) = create(&org, &lena, "ci").await;

    let (status, body) = with_token(&org, &secret, "GET", "/api/auth/me", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["user"]["id"], org.lena);
    assert_eq!(body["user"]["email"], "lena@example.com");

    // No cookie, so no CSRF header is needed.
    let second = json!({ "name": "made with a token" });
    let (status, body) = with_token(&org, &secret, "POST", "/api/tokens", Some(second)).await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(body["secret"].as_str().unwrap().starts_with("uf-at-"));

    let (status, body) = with_token(&org, &secret, "GET", "/api/tokens", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(names(&body), ["made with a token", "ci"]);
    assert!(body["tokens"][1]["last_used_at"].is_string());

    // A token is not a virtual key.
    let chat = r#"{"model":"p/m","messages":[{"role":"user","content":"hi"}]}"#;
    let (status, _) = common::post_chat(&org.api.app, Some(&secret), chat).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn creating_a_token() {
    let org = org().await;
    let lena = org.sign_in("lena").await;
    let body = json!({ "name": "  deploy  ", "expires_at": FUTURE });
    let (status, body) = org
        .call(Some(&lena), "POST", "/api/tokens", Some(body))
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body.as_object().unwrap().len(), 2);
    let secret = body["secret"].as_str().unwrap();
    assert!(secret.starts_with("uf-at-"));
    assert_eq!(secret.len(), 6 + 64);
    let token = &body["token"];
    assert_eq!(token["name"], "deploy");
    assert_eq!(token["expires_at"], FUTURE);
    assert_eq!(token["revoked_at"], Value::Null);
    assert_eq!(token["last_used_at"], Value::Null);
    assert!(token["display"]
        .as_str()
        .unwrap()
        .ends_with(&secret[secret.len() - 4..]));
    let mut fields: Vec<&str> = token
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    fields.sort_unstable();
    assert_eq!(
        fields,
        [
            "created_at",
            "display",
            "expires_at",
            "id",
            "last_used_at",
            "name",
            "revoked_at"
        ]
    );
}

#[tokio::test]
async fn token_input_is_validated() {
    let org = org().await;
    let lena = org.sign_in("lena").await;
    let long = "n".repeat(101);
    for (body, field) in [
        (json!({ "name": "" }), "name"),
        (json!({ "name": "  " }), "name"),
        (json!({ "name": long }), "name"),
        (
            json!({ "name": "ci", "expires_at": "2999-02-31 00:00:00" }),
            "expires_at",
        ),
        (
            json!({ "name": "ci", "expires_at": "tomorrow" }),
            "expires_at",
        ),
        (json!({ "name": "ci", "expires_at": PAST }), "expires_at"),
    ] {
        let (status, answer) = org
            .call(Some(&lena), "POST", "/api/tokens", Some(body))
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{field}");
        assert!(answer["error"]["fields"][field].is_string(), "{answer}");
    }
    for body in [json!({}), json!({ "name": "ci", "user_id": org.maya })] {
        let (status, _) = org
            .call(Some(&lena), "POST", "/api/tokens", Some(body))
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
    assert!(org
        .api
        .store
        .list_tokens_of(org.lena)
        .await
        .unwrap()
        .is_empty());
    assert!(audited(&org).await.is_empty());
}

#[tokio::test]
async fn tokens_follow_the_owners_current_role() {
    let org = org().await;
    let noor = seed_user(
        &org.api.store,
        "noor@example.com",
        Role::Admin,
        ORG_PASSWORD,
    )
    .await;
    assert!(noor > 0);
    let maya = org.sign_in("maya").await;
    let (_, secret) = create(&org, &maya, "ci").await;

    let team = json!({ "name": "Before" });
    let (status, _) = with_token(&org, &secret, "POST", "/api/teams", Some(team)).await;
    assert_eq!(status, StatusCode::CREATED);

    let noor = org.sign_in("noor").await;
    let lower = json!({ "role": "member" });
    let path = format!("/api/users/{}", org.maya);
    let (status, _) = org.call(Some(&noor), "PATCH", &path, Some(lower)).await;
    assert_eq!(status, StatusCode::OK);

    let team = json!({ "name": "After" });
    let (status, body) = with_token(&org, &secret, "POST", "/api/teams", Some(team)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error_code(&body), "forbidden");
    let (status, body) = with_token(&org, &secret, "GET", "/api/auth/me", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["user"]["role"], "member");
}

#[tokio::test]
async fn tokens_are_private() {
    let org = org().await;
    let lena = org.sign_in("lena").await;
    let arjun = org.sign_in("arjun").await;
    let maya = org.sign_in("maya").await;
    let (lena_token, lena_secret) = create(&org, &lena, "lena ci").await;
    let (arjun_token, _) = create(&org, &arjun, "arjun ci").await;

    // Not the lead of her team, and not an admin either.
    let (status, body) = org.call(Some(&arjun), "GET", "/api/tokens", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(names(&body), ["arjun ci"]);
    assert_eq!(body["tokens"][0]["id"], arjun_token);
    let (_, body) = org.call(Some(&maya), "GET", "/api/tokens", None).await;
    assert_eq!(names(&body), Vec::<&str>::new());

    for who in [&arjun, &maya] {
        let (status, body) = org
            .call(Some(who), "DELETE", &token_path(lena_token), None)
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(error_code(&body), "not_found");
    }
    let (status, _) = with_token(&org, &lena_secret, "GET", "/api/auth/me", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(audited(&org).await, ["token.create", "token.create"]);
}

#[tokio::test]
async fn another_users_token_looks_like_a_missing_one() {
    let org = org().await;
    let lena = org.sign_in("lena").await;
    let arjun = org.sign_in("arjun").await;
    let (lena_token, _) = create(&org, &lena, "ci").await;

    let app = &org.api.app;
    let hidden = common::call(app, "DELETE", &token_path(lena_token), Some(&arjun), None).await;
    let missing = common::call(app, "DELETE", &token_path(9999), Some(&arjun), None).await;
    let malformed = common::call(app, "DELETE", "/api/tokens/abc", Some(&arjun), None).await;
    assert_eq!(hidden.0, StatusCode::NOT_FOUND);
    assert_eq!(hidden.0, missing.0);
    assert_eq!(hidden.1, missing.1, "headers differ");
    assert_eq!(hidden.2.to_string(), missing.2.to_string());
    assert_eq!(hidden.2.to_string(), malformed.2.to_string());
}

#[tokio::test]
async fn revoked_and_expired_tokens_fail() {
    let org = org().await;
    let lena = org.sign_in("lena").await;
    let (id, secret) = create(&org, &lena, "ci").await;
    let (status, _) = with_token(&org, &secret, "GET", "/api/auth/me", None).await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = org.call(Some(&lena), "DELETE", &token_path(id), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null);
    let (status, body) = with_token(&org, &secret, "GET", "/api/auth/me", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&body), "unauthenticated");

    // Revoking again is quiet.
    let (status, _) = org.call(Some(&lena), "DELETE", &token_path(id), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(audited(&org).await, ["token.create", "token.revoke"]);
    let (_, body) = org.call(Some(&lena), "GET", "/api/tokens", None).await;
    assert!(body["tokens"][0]["revoked_at"].is_string());

    let expired = generate_secret(TOKEN_PREFIX);
    let mut tx = org.api.store.begin().await.unwrap();
    tx.insert_token(org.lena, "old", &expired.hash, &expired.display, Some(PAST))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let (status, body) = with_token(&org, &expired.full, "GET", "/api/auth/me", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&body), "unauthenticated");
}

#[tokio::test]
async fn a_token_can_revoke_itself() {
    let org = org().await;
    let lena = org.sign_in("lena").await;
    let (id, secret) = create(&org, &lena, "ci").await;
    let (status, _) = with_token(&org, &secret, "DELETE", &token_path(id), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = with_token(&org, &secret, "GET", "/api/tokens", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn token_secret_is_shown_once() {
    let org = org().await;
    let lena = org.sign_in("lena").await;
    let (id, secret) = create(&org, &lena, "ci").await;
    let random = &secret["uf-at-".len()..];
    let hash = hash_key(&secret);

    let (_, list) = org.call(Some(&lena), "GET", "/api/tokens", None).await;
    assert_eq!(names(&list), ["ci"]);
    org.call(Some(&lena), "DELETE", &token_path(id), None).await;
    let maya = org.sign_in("maya").await;
    let (status, audit) = org.call(Some(&maya), "GET", "/api/audit", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(audited(&org).await, ["token.create", "token.revoke"]);
    assert!(audit.to_string().contains("token.revoke"));

    for text in [list.to_string(), audit.to_string()] {
        assert!(!text.contains(random), "{text}");
        assert!(!text.contains(&hash), "{text}");
        assert!(!text.contains("secret"), "{text}");
        assert!(!text.contains("hash"), "{text}");
    }
    for row in org.api.store.list_audit(200, None).await.unwrap() {
        assert!(!row.summary.contains(random), "{}", row.summary);
        if !row.action.starts_with("token.") {
            continue;
        }
        assert_eq!(row.target_type, "token");
        assert_eq!(row.target_id, Some(id));
    }
}
