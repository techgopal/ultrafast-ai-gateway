mod common;

use axum::http::StatusCode;
use common::{compared, error_code, org, raw, Org, Signed};
use serde_json::{json, Value};
use ultrafast_gateway::secrets::{generate_key, generate_secret, TOKEN_PREFIX};

const NEW_PASSWORD: &str = "another horse battery";
const LINK_PREFIX: &str = "/accept-invite?token=";

fn user_path(id: i64) -> String {
    format!("/api/users/{id}")
}

fn emails(body: &Value) -> Vec<&str> {
    body["users"]
        .as_array()
        .expect("a users array")
        .iter()
        .map(|u| u["email"].as_str().unwrap())
        .collect()
}

/// The token of an invite link.
fn token_of(body: &Value) -> String {
    let link = body["invite_link"].as_str().expect("an invite link");
    assert!(link.starts_with("/accept-invite?token=uf-inv-"), "{link}");
    link.strip_prefix(LINK_PREFIX).unwrap().to_string()
}

async fn accept(org: &Org, token: &str) -> StatusCode {
    let body = json!({ "token": token, "password": NEW_PASSWORD });
    org.call(None, "POST", "/api/auth/accept-invite", Some(body))
        .await
        .0
}

async fn invite(org: &Org, who: &Signed, body: Value) -> (StatusCode, Value) {
    org.call(Some(who), "POST", "/api/users", Some(body)).await
}

async fn patch(org: &Org, who: &Signed, id: i64, body: Value) -> (StatusCode, Value) {
    org.call(Some(who), "PATCH", &user_path(id), Some(body))
        .await
}

async fn me_status(org: &Org, who: &Signed) -> StatusCode {
    org.call(Some(who), "GET", "/api/auth/me", None).await.0
}

/// No audit entry may hold any part of an invite link.
async fn assert_audit_has_no_token(org: &Org, token: &str) {
    let secret = token.strip_prefix("uf-inv-").unwrap();
    for row in org.api.store.list_audit(200, None).await.unwrap() {
        assert!(!row.summary.contains(secret), "{}", row.summary);
        assert!(!row.summary.contains("uf-inv-"), "{}", row.summary);
        assert!(!row.summary.contains("accept-invite"), "{}", row.summary);
    }
}

#[tokio::test]
async fn admin_lists_everyone() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let (status, body) = org.call(Some(&maya), "GET", "/api/users", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        emails(&body),
        [
            "arjun@example.com",
            "lena@example.com",
            "maya@example.com",
            "priya@example.com",
            "tomas@example.com"
        ]
    );
    let text = body.to_string();
    assert!(!text.contains("password"));
    assert!(!text.contains("argon2"));
    assert_eq!(body["users"][0].as_object().unwrap().len(), 8);
}

#[tokio::test]
async fn listing_needs_a_sign_in() {
    let org = org().await;
    for path in ["/api/users", "/api/teams", "/api/audit", "/api/users/1"] {
        let (status, body) = org.call(None, "GET", path, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{path}");
        assert_eq!(error_code(&body), "unauthenticated");
    }
}

#[tokio::test]
async fn lead_lists_their_teams_members_and_self() {
    let org = org().await;
    let arjun = org.sign_in("arjun").await;
    let (status, body) = org.call(Some(&arjun), "GET", "/api/users", None).await;
    assert_eq!(status, StatusCode::OK);
    // Platform only: arjun is a plain member of Research, so not tomas.
    assert_eq!(emails(&body), ["arjun@example.com", "lena@example.com"]);
}

#[tokio::test]
async fn member_lists_only_self() {
    let org = org().await;
    let lena = org.sign_in("lena").await;
    let (status, body) = org.call(Some(&lena), "GET", "/api/users", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(emails(&body), ["lena@example.com"]);
}

#[tokio::test]
async fn invite_creates_an_invited_user_with_a_link() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let (status, body) = invite(
        &org,
        &maya,
        json!({ "email": " Noor@Example.com ", "name": " Noor ", "role": "member" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body.as_object().unwrap().len(), 2);
    assert_eq!(body["user"]["email"], "noor@example.com");
    assert_eq!(body["user"]["name"], "Noor");
    assert_eq!(body["user"]["role"], "member");
    assert_eq!(body["user"]["status"], "invited");
    let token = token_of(&body);

    assert!(org
        .audit_actions()
        .await
        .contains(&"user.invite".to_string()));
    let summary = org.last_summary("user.invite").await;
    assert!(summary.contains("noor@example.com"), "{summary}");
    assert_audit_has_no_token(&org, &token).await;

    assert_eq!(accept(&org, &token).await, StatusCode::NO_CONTENT);
    let noor = common::sign_in(&org.api.app, "noor@example.com", NEW_PASSWORD).await;
    assert_eq!(me_status(&org, &noor).await, StatusCode::OK);
    // A link works once.
    assert_eq!(accept(&org, &token).await, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn only_admins_invite() {
    let org = org().await;
    for name in ["arjun", "lena"] {
        let who = org.sign_in(name).await;
        for role in ["member", "admin", "owner"] {
            let (status, body) = invite(
                &org,
                &who,
                json!({ "email": "noor@example.com", "name": "Noor", "role": role }),
            )
            .await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{name} {role}");
            assert_eq!(error_code(&body), "forbidden");
        }
        // Not even the validation of the fields is shown to them.
        let (status, _) = invite(
            &org,
            &who,
            json!({ "email": "maya@example.com", "name": "", "role": "member" }),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
    assert_eq!(org.api.store.count_users().await.unwrap(), 5);
    assert!(!org
        .audit_actions()
        .await
        .contains(&"user.invite".to_string()));
}

#[tokio::test]
async fn invite_validates_and_detects_duplicates() {
    let org = org().await;
    let maya = org.sign_in("maya").await;

    let (status, body) = invite(
        &org,
        &maya,
        json!({ "email": "not-an-email", "name": "Noor", "role": "member" }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error_code(&body), "validation_failed");
    assert!(body["error"]["fields"]["email"].is_string());

    let (status, body) = invite(
        &org,
        &maya,
        json!({ "email": "noor@example.com", "name": "Noor", "role": "owner" }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["error"]["fields"]["role"].is_string());

    let (status, body) = invite(
        &org,
        &maya,
        json!({ "email": "x", "name": " ", "role": "" }),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["error"]["fields"].as_object().unwrap().len(), 3);

    let (status, body) = invite(
        &org,
        &maya,
        json!({ "email": "MAYA@example.com", "name": "Maya Again", "role": "member" }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&body), "user_exists");

    let (status, _) = invite(
        &org,
        &maya,
        json!({ "email": "noor@example.com", "name": "Noor", "role": "member", "status": "active" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    assert_eq!(org.api.store.count_users().await.unwrap(), 5);
    assert!(!org
        .audit_actions()
        .await
        .contains(&"user.invite".to_string()));
}

#[tokio::test]
async fn reinvite_replaces_the_old_link() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let (_, body) = invite(
        &org,
        &maya,
        json!({ "email": "noor@example.com", "name": "Noor", "role": "member" }),
    )
    .await;
    let noor = body["user"]["id"].as_i64().unwrap();
    let old = token_of(&body);

    let path = format!("/api/users/{noor}/invite");
    let (status, body) = org.call(Some(&maya), "POST", &path, None).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body.as_object().unwrap().len(), 1);
    let new = token_of(&body);
    assert_ne!(old, new);
    assert_eq!(
        org.last_summary("user.reinvite").await,
        "Sent a new invite to noor@example.com"
    );
    assert_audit_has_no_token(&org, &old).await;
    assert_audit_has_no_token(&org, &new).await;

    assert_eq!(accept(&org, &old).await, StatusCode::NOT_FOUND);
    assert_eq!(accept(&org, &new).await, StatusCode::NO_CONTENT);

    // Now active: no more invites.
    let (status, body) = org.call(Some(&maya), "POST", &path, None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&body), "not_invited");
    let lena_path = format!("/api/users/{}/invite", org.lena);
    let (status, body) = org.call(Some(&maya), "POST", &lena_path, None).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&body), "not_invited");

    let (status, _) = org
        .call(Some(&maya), "POST", "/api/users/999/invite", None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn only_admins_reinvite() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let (_, body) = invite(
        &org,
        &maya,
        json!({ "email": "noor@example.com", "name": "Noor", "role": "member" }),
    )
    .await;
    let noor = body["user"]["id"].as_i64().unwrap();
    let token = token_of(&body);

    let arjun = org.sign_in("arjun").await;
    let path = format!("/api/users/{noor}/invite");
    let (status, _) = org.call(Some(&arjun), "POST", &path, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    // The first link still works: nothing was replaced.
    assert_eq!(accept(&org, &token).await, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn viewing_users_hides_outsiders() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let arjun = org.sign_in("arjun").await;
    let lena = org.sign_in("lena").await;

    let (status, body) = org
        .call(Some(&arjun), "GET", &user_path(org.lena), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["email"], "lena@example.com");
    assert_eq!(body.as_object().unwrap().len(), 8);
    assert!(!body.to_string().contains("password"));

    // tomas shares Research with arjun, but arjun does not lead it.
    let hidden = org
        .call(Some(&arjun), "GET", &user_path(org.tomas), None)
        .await;
    assert_eq!(hidden.0, StatusCode::NOT_FOUND);

    let (status, _) = org
        .call(Some(&lena), "GET", &user_path(org.arjun), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, body) = org
        .call(Some(&lena), "GET", &user_path(org.lena), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], org.lena);

    let missing_for_admin = org.call(Some(&maya), "GET", "/api/users/999", None).await;
    let missing_for_lead = org.call(Some(&arjun), "GET", "/api/users/999", None).await;
    assert_eq!(missing_for_admin.0, StatusCode::NOT_FOUND);
    assert_eq!(missing_for_admin, hidden);
    assert_eq!(missing_for_lead, hidden);
}

/// A hidden user and a missing user get the same bytes, for every method.
#[tokio::test]
async fn hidden_and_missing_users_answer_alike() {
    let org = org().await;
    let arjun = org.sign_in("arjun").await;
    let cases = [
        ("GET", "", None),
        ("PATCH", "", Some(json!({ "name": "X" }))),
        ("PATCH", "", Some(json!({ "role": "admin" }))),
        ("DELETE", "", None),
    ];
    for (method, suffix, body) in cases {
        let mut answers = Vec::new();
        for id in [org.tomas, 999] {
            let path = format!("/api/users/{id}{suffix}");
            let answer = raw(&org, &arjun, method, &path, body.clone()).await;
            assert_eq!(answer.0, StatusCode::NOT_FOUND, "{method} {path}");
            answers.push(answer);
        }
        assert_eq!(compared(&answers[0]), compared(&answers[1]), "{method}");
    }
    assert_eq!(org.api.store.count_users().await.unwrap(), 5);
}

#[tokio::test]
async fn users_edit_their_own_name_only() {
    let org = org().await;
    let lena = org.sign_in("lena").await;

    let (status, body) = patch(&org, &lena, org.lena, json!({ "name": " Lena K " })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "Lena K");
    assert_eq!(body["id"], org.lena);
    assert_eq!(
        org.last_summary("user.update").await,
        "Changed name of lena@example.com from Test User to Lena K"
    );
    // A name change keeps the session.
    assert_eq!(me_status(&org, &lena).await, StatusCode::OK);

    for body in [
        json!({ "role": "admin" }),
        json!({ "role": "member" }),
        json!({ "status": "disabled" }),
        json!({ "name": "Lena", "role": "admin" }),
    ] {
        let (status, answer) = patch(&org, &lena, org.lena, body.clone()).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert_eq!(error_code(&answer), "forbidden");
    }
    let (status, _) = patch(&org, &lena, org.arjun, json!({ "name": "Hacked" })).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // A lead sees lena but may not edit her.
    let arjun = org.sign_in("arjun").await;
    let (status, _) = patch(&org, &arjun, org.lena, json!({ "name": "Hacked" })).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let store = &org.api.store;
    let row = store.user_by_id(org.lena).await.unwrap().unwrap();
    assert_eq!(row.name, "Lena K");
    assert_eq!(row.role.as_str(), "member");
    assert_eq!(
        store.user_by_id(org.arjun).await.unwrap().unwrap().name,
        "Test User"
    );
    let updates = org.audit_actions().await;
    assert_eq!(updates.iter().filter(|a| *a == "user.update").count(), 1);
}

#[tokio::test]
async fn patch_validates_its_fields() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    for (body, field) in [
        (json!({ "name": "" }), "name"),
        (json!({ "name": "n".repeat(101) }), "name"),
        (json!({ "role": "owner" }), "role"),
        (json!({ "status": "invited" }), "status"),
        (json!({ "status": "gone" }), "status"),
    ] {
        let (status, answer) = patch(&org, &maya, org.lena, body.clone()).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert!(answer["error"]["fields"][field].is_string(), "{body}");
    }
    for body in [json!({}), json!({ "email": "x@example.com" })] {
        let (status, _) = patch(&org, &maya, org.lena, body.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    }
    assert!(!org
        .audit_actions()
        .await
        .contains(&"user.update".to_string()));
}

#[tokio::test]
async fn admin_changes_role_and_status() {
    let org = org().await;
    let maya = org.sign_in("maya").await;

    let lena = org.sign_in("lena").await;
    let (status, body) = patch(&org, &maya, org.lena, json!({ "role": "admin" })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["role"], "admin");
    assert_eq!(
        org.last_summary("user.update").await,
        "Changed role of lena@example.com from member to admin"
    );
    assert_eq!(me_status(&org, &lena).await, StatusCode::UNAUTHORIZED);

    // A role change keeps access tokens, disabling revokes them.
    let token = generate_secret(TOKEN_PREFIX);
    let mut tx = org.api.store.begin().await.unwrap();
    tx.insert_token(org.lena, "ci", &token.hash, &token.display, None)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let with_token = |path: &'static str| {
        let app = org.api.app.clone();
        let full = token.full.clone();
        async move {
            common::call_with_token(&app, "GET", path, &full, None)
                .await
                .0
        }
    };
    let (status, _) = patch(&org, &maya, org.lena, json!({ "role": "member" })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(with_token("/api/auth/me").await, StatusCode::OK);

    let lena = org.sign_in("lena").await;
    let (status, body) = patch(&org, &maya, org.lena, json!({ "status": "disabled" })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "disabled");
    assert_eq!(
        org.last_summary("user.update").await,
        "Changed status of lena@example.com from active to disabled"
    );
    assert_eq!(me_status(&org, &lena).await, StatusCode::UNAUTHORIZED);
    assert_eq!(with_token("/api/auth/me").await, StatusCode::UNAUTHORIZED);

    let (status, body) = patch(&org, &maya, org.lena, json!({ "status": "active" })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "active");
    // The revoked token stays revoked.
    assert_eq!(with_token("/api/auth/me").await, StatusCode::UNAUTHORIZED);
    org.sign_in("lena").await;
}

#[tokio::test]
async fn one_patch_can_change_several_fields() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let before = org.audit_actions().await.len();
    let (status, body) = patch(
        &org,
        &maya,
        org.lena,
        json!({ "name": "Lena K", "role": "admin", "status": "disabled" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "Lena K");
    assert_eq!(body["role"], "admin");
    assert_eq!(body["status"], "disabled");
    assert_eq!(org.audit_actions().await.len(), before + 1);
    assert_eq!(
        org.last_summary("user.update").await,
        "Changed name of lena@example.com from Test User to Lena K, \
         role from member to admin, status from active to disabled"
    );
}

#[tokio::test]
async fn a_patch_that_changes_nothing_is_not_recorded() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let lena = org.sign_in("lena").await;
    let before = org.audit_actions().await.len();
    let (status, body) = patch(
        &org,
        &maya,
        org.lena,
        json!({ "name": "Test User", "role": "member", "status": "active" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["role"], "member");
    assert_eq!(org.audit_actions().await.len(), before);
    assert_eq!(me_status(&org, &lena).await, StatusCode::OK);
}

#[tokio::test]
async fn the_last_admin_is_protected() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let before = org.audit_actions().await.len();

    for body in [
        json!({ "role": "member" }),
        json!({ "status": "disabled" }),
        json!({ "name": "Maya R", "role": "member" }),
    ] {
        let (status, answer) = patch(&org, &maya, org.maya, body.clone()).await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(error_code(&answer), "last_admin");
        assert_eq!(
            answer["error"]["message"],
            "At least one active admin is required."
        );
    }
    let (status, answer) = org
        .call(Some(&maya), "DELETE", &user_path(org.maya), None)
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(["last_admin", "cannot_delete_self"].contains(&error_code(&answer)));

    // Nothing was changed, ended or recorded by the refused calls.
    let row = org.api.store.user_by_id(org.maya).await.unwrap().unwrap();
    assert_eq!(row.role.as_str(), "admin");
    assert_eq!(row.status.as_str(), "active");
    assert_eq!(row.name, "Test User");
    assert_eq!(me_status(&org, &maya).await, StatusCode::OK);
    assert_eq!(org.audit_actions().await.len(), before);

    let (status, _) = patch(&org, &maya, org.arjun, json!({ "role": "admin" })).await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = patch(&org, &maya, org.maya, json!({ "role": "member" })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["role"], "member");
    // Her own role changed, so her session ended too.
    assert_eq!(me_status(&org, &maya).await, StatusCode::UNAUTHORIZED);
    assert_eq!(org.api.store.count_active_admins().await.unwrap(), 1);
}

#[tokio::test]
async fn an_invited_admin_does_not_count_as_an_admin() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let (status, body) = invite(
        &org,
        &maya,
        json!({ "email": "noor@example.com", "name": "Noor", "role": "admin" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["user"]["role"], "admin");
    let (status, answer) = patch(&org, &maya, org.maya, json!({ "role": "member" })).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&answer), "last_admin");
}

#[tokio::test]
async fn two_admins_cannot_both_be_removed() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let (status, _) = patch(&org, &maya, org.arjun, json!({ "role": "admin" })).await;
    assert_eq!(status, StatusCode::OK);
    let arjun = org.sign_in("arjun").await;

    let (status, _) = patch(&org, &arjun, org.maya, json!({ "status": "disabled" })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me_status(&org, &maya).await, StatusCode::UNAUTHORIZED);

    let (status, answer) = patch(&org, &arjun, org.arjun, json!({ "status": "disabled" })).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&answer), "last_admin");
    let (status, answer) = patch(&org, &arjun, org.arjun, json!({ "role": "member" })).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&answer), "last_admin");
    assert_eq!(me_status(&org, &arjun).await, StatusCode::OK);
    assert_eq!(org.api.store.count_active_admins().await.unwrap(), 1);
}

#[tokio::test]
async fn activating_a_user_without_password_is_refused() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let (_, body) = invite(
        &org,
        &maya,
        json!({ "email": "noor@example.com", "name": "Noor", "role": "member" }),
    )
    .await;
    let noor = body["user"]["id"].as_i64().unwrap();
    let token = token_of(&body);

    let (status, answer) = patch(&org, &maya, noor, json!({ "status": "active" })).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&answer), "no_password");
    let row = org.api.store.user_by_id(noor).await.unwrap().unwrap();
    assert_eq!(row.status.as_str(), "invited");

    // Disabling an invited user ends their invite.
    let (status, _) = patch(&org, &maya, noor, json!({ "status": "disabled" })).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(accept(&org, &token).await, StatusCode::NOT_FOUND);
    let (status, answer) = patch(&org, &maya, noor, json!({ "status": "active" })).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&answer), "no_password");
}

#[tokio::test]
async fn deleting_a_user() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let lena = org.sign_in("lena").await;
    let key = generate_key();
    let mut tx = org.api.store.begin().await.unwrap();
    tx.insert_key(
        "lena's key",
        &key.hash,
        &key.display,
        None,
        Some(org.lena),
        None,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let (status, body) = org
        .call(Some(&maya), "DELETE", &user_path(org.lena), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(body.is_null());
    assert_eq!(
        org.last_summary("user.delete").await,
        "Deleted user lena@example.com, left 1 key working without an owner"
    );

    let store = &org.api.store;
    assert!(store.user_by_id(org.lena).await.unwrap().is_none());
    let kept = store.active_key_by_hash(&key.hash).await.unwrap().unwrap();
    assert_eq!(kept.user_id, None);
    assert_eq!(me_status(&org, &lena).await, StatusCode::UNAUTHORIZED);
    assert!(store.members_of(org.platform).await.unwrap().len() == 1);

    let (status, _) = org
        .call(Some(&maya), "DELETE", &user_path(org.lena), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn only_admins_delete_and_never_themselves() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let (status, _) = patch(&org, &maya, org.arjun, json!({ "role": "admin" })).await;
    assert_eq!(status, StatusCode::OK);
    // With a second admin, only the self-delete rule is in the way.
    let (status, answer) = org
        .call(Some(&maya), "DELETE", &user_path(org.maya), None)
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&answer), "cannot_delete_self");

    let lena = org.sign_in("lena").await;
    let (status, _) = org
        .call(Some(&lena), "DELETE", &user_path(org.lena), None)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = org
        .call(Some(&lena), "DELETE", &user_path(org.tomas), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(org.api.store.count_users().await.unwrap(), 5);
}

#[tokio::test]
async fn bad_ids_are_404() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let missing = org.call(Some(&maya), "GET", "/api/users/999", None).await;
    for id in ["abc", "0", "-1", "1.5", "+1", "99999999999999999999", "%20"] {
        let path = format!("/api/users/{id}");
        for (method, body) in [
            ("GET", None),
            ("PATCH", Some(json!({ "name": "X" }))),
            ("DELETE", None),
        ] {
            let answer = org.call(Some(&maya), method, &path, body).await;
            assert_eq!(answer, missing, "{method} {path}");
        }
        let answer = org
            .call(Some(&maya), "POST", &format!("{path}/invite"), None)
            .await;
        assert_eq!(answer, missing, "POST {path}/invite");
    }
    assert_eq!(org.api.store.count_users().await.unwrap(), 5);
}

#[tokio::test]
async fn changes_need_the_csrf_header() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let path = user_path(org.lena);
    let headers = [("cookie", maya.cookie.as_str())];
    let (status, _, body) = common::send(&org.api.app, "DELETE", &path, &headers, None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error_code(&body), "csrf_failed");
    assert!(org.api.store.user_by_id(org.lena).await.unwrap().is_some());
}

/// Whoever may not invite learns nothing about an id from asking.
#[tokio::test]
async fn reinvite_hides_existence_from_non_admins() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let (_, body) = invite(
        &org,
        &maya,
        json!({ "email": "noor@example.com", "name": "Noor", "role": "admin" }),
    )
    .await;
    let noor = body["user"]["id"].as_i64().unwrap();
    let token = token_of(&body);
    let paths = [
        format!("/api/users/{noor}/invite"),
        format!("/api/users/{}/invite", org.tomas),
        "/api/users/999/invite".to_string(),
        "/api/users/abc/invite".to_string(),
    ];

    for name in ["lena", "arjun"] {
        let who = org.sign_in(name).await;
        let mut answers = Vec::new();
        for path in &paths {
            let answer = raw(&org, &who, "POST", path, None).await;
            assert_eq!(answer.0, StatusCode::FORBIDDEN, "{name} {path}");
            answers.push(answer);
        }
        for answer in &answers[1..] {
            assert_eq!(compared(answer), compared(&answers[0]), "{name}");
        }
    }
    assert!(!org
        .audit_actions()
        .await
        .contains(&"user.reinvite".to_string()));

    let status = |path: &str| {
        let path = path.to_string();
        let (org, maya) = (&org, &maya);
        async move { org.call(Some(maya), "POST", &path, None).await }
    };
    let (code, body) = status(&paths[1]).await;
    assert_eq!(code, StatusCode::CONFLICT);
    assert_eq!(error_code(&body), "not_invited");
    assert_eq!(status(&paths[2]).await.0, StatusCode::NOT_FOUND);
    assert_eq!(status(&paths[3]).await.0, StatusCode::NOT_FOUND);
    // The refused calls replaced nothing.
    assert_eq!(status(&paths[0]).await.0, StatusCode::CREATED);
    assert_eq!(accept(&org, &token).await, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn user_view_lists_teams() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let arjun_teams = json!([
        { "team_id": org.platform, "name": "Platform", "role": "lead" },
        { "team_id": org.research, "name": "Research", "role": "member" },
    ]);

    // The list: every user with their own teams, ordered by name.
    let (status, body) = org.call(Some(&maya), "GET", "/api/users", None).await;
    assert_eq!(status, StatusCode::OK);
    let teams_of = |email: &str| {
        let users = body["users"].as_array().unwrap();
        users.iter().find(|u| u["email"] == email).unwrap()["teams"].clone()
    };
    assert_eq!(teams_of("arjun@example.com"), arjun_teams);
    assert_eq!(
        teams_of("lena@example.com"),
        json!([{ "team_id": org.platform, "name": "Platform", "role": "member" }])
    );
    assert_eq!(teams_of("priya@example.com"), json!([]));
    assert_eq!(teams_of("maya@example.com"), json!([]));

    let (_, body) = org
        .call(Some(&maya), "GET", &user_path(org.arjun), None)
        .await;
    assert_eq!(body["teams"], arjun_teams);

    // The update answers with them too.
    let path = user_path(org.arjun);
    let patch = Some(json!({ "name": "Arjun K" }));
    let (_, body) = org.call(Some(&maya), "PATCH", &path, patch).await;
    assert_eq!(body["teams"], arjun_teams);

    let arjun = org.sign_in("arjun").await;
    let (_, body) = org.call(Some(&arjun), "GET", "/api/auth/me", None).await;
    assert_eq!(body["user"]["teams"], arjun_teams);
}
