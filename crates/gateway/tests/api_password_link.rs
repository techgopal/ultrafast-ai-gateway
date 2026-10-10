//! A password link: how an admin gives a user made by single sign-on a
//! password. It is an invite of kind `set_password`, taken by the same
//! endpoint as an invite.

mod common;

use axum::http::StatusCode;
use common::{error_code, org, sign_in, Org, Signed};
use serde_json::{json, Value};
use ultrafast_gateway::identity::{Role, UserStatus};
use ultrafast_gateway::secrets::{generate_secret, INVITE_PREFIX};
use ultrafast_gateway::store::{after, NewUser};

const PASSWORD: &str = "a long enough phrase 7";
const PAGE: &str = "/accept-invite#token=";

/// A user the identity provider made: active, linked, no password.
async fn sso_user(org: &Org, name: &str) -> i64 {
    let mut tx = org.api.store.begin().await.unwrap();
    let id = tx
        .insert_user(NewUser {
            email: &format!("{name}@example.com"),
            name,
            role: Role::Member,
            status: UserStatus::Active,
            password_hash: None,
        })
        .await
        .unwrap();
    tx.link_external(id, "oidc", &format!("https://idp.example.com|sub-{name}"))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    id
}

async fn link_for(org: &Org, who: &Signed, id: i64) -> (StatusCode, Value) {
    org.call(
        Some(who),
        "POST",
        &format!("/api/users/{id}/password-link"),
        None,
    )
    .await
}

fn token_of(body: &Value) -> String {
    let url = body["url"].as_str().expect("a url");
    assert!(url.starts_with("/accept-invite#token=uf-pwl-"), "{url}");
    url.strip_prefix(PAGE).unwrap().to_string()
}

async fn accept(org: &Org, token: &str) -> StatusCode {
    org.call(
        None,
        "POST",
        "/api/auth/accept-invite",
        Some(json!({ "token": token, "password": PASSWORD })),
    )
    .await
    .0
}

#[tokio::test]
async fn a_link_sets_the_password_once_and_keeps_the_sso_identity() {
    let org = org().await;
    let ann = sso_user(&org, "ann").await;
    let maya = org.sign_in("maya").await;
    let users = org.api.store.count_users().await.unwrap();

    let (status, body) = link_for(&org, &maya, ann).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body.as_object().unwrap().len(), 2, "{body}");
    let token = token_of(&body);
    // 24 hours, give or take the time the test takes.
    let expires = body["expires_at"].as_str().unwrap();
    assert!(expires > after(24 * 3600 - 120).as_str(), "{expires}");
    assert!(expires < after(24 * 3600 + 120).as_str(), "{expires}");

    assert_eq!(accept(&org, &token).await, StatusCode::NO_CONTENT);
    let ann_row = org.api.store.user_by_id(ann).await.unwrap().unwrap();
    assert!(ann_row.password_hash.is_some());
    assert_eq!(ann_row.auth_provider, "oidc");
    assert_eq!(
        ann_row.external_id.as_deref(),
        Some("https://idp.example.com|sub-ann")
    );
    assert_eq!(ann_row.role, Role::Member);
    assert_eq!(ann_row.status, UserStatus::Active);
    assert_eq!(org.api.store.count_users().await.unwrap(), users);
    assert_eq!(
        sign_in(&org.api.app, "ann@example.com", PASSWORD)
            .await
            .user_id,
        ann
    );

    // Single use.
    assert_eq!(accept(&org, &token).await, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_token_reaches_no_audit_entry() {
    let org = org().await;
    let ann = sso_user(&org, "ann").await;
    let maya = org.sign_in("maya").await;
    let (_, body) = link_for(&org, &maya, ann).await;
    let token = token_of(&body);
    assert_eq!(accept(&org, &token).await, StatusCode::NO_CONTENT);
    let actions = org.audit_actions().await;
    assert!(
        actions.contains(&"user.password_link".to_string()),
        "{actions:?}"
    );
    assert!(
        actions.contains(&"user.set_password".to_string()),
        "{actions:?}"
    );
    let secret = token.strip_prefix("uf-pwl-").unwrap();
    for row in org.api.store.list_audit(200, None).await.unwrap() {
        for text in [&row.summary, &row.action] {
            assert!(!text.contains(secret), "{text}");
            assert!(!text.contains("uf-pwl-"), "{text}");
            assert!(!text.contains("accept-invite"), "{text}");
        }
    }
    let issued = org.last_summary("user.password_link").await;
    assert!(issued.contains("ann@example.com"), "{issued}");
}

#[tokio::test]
async fn an_expired_link_is_refused() {
    let org = org().await;
    let ann = sso_user(&org, "ann").await;
    let token = generate_secret("uf-pwl-");
    let mut tx = org.api.store.begin().await.unwrap();
    tx.insert_invite_of_kind(ann, &token.hash, "2000-01-01 00:00:00", "set_password")
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(accept(&org, &token.full).await, StatusCode::NOT_FOUND);
    assert!(org
        .api
        .store
        .user_by_id(ann)
        .await
        .unwrap()
        .unwrap()
        .password_hash
        .is_none());
}

#[tokio::test]
async fn a_link_changes_the_password_of_its_user_only() {
    let org = org().await;
    let ann = sso_user(&org, "ann").await;
    let bob = sso_user(&org, "bob").await;
    let maya = org.sign_in("maya").await;
    let (_, body) = link_for(&org, &maya, ann).await;
    assert_eq!(accept(&org, &token_of(&body)).await, StatusCode::NO_CONTENT);
    assert!(org
        .api
        .store
        .user_by_id(bob)
        .await
        .unwrap()
        .unwrap()
        .password_hash
        .is_none());

    // An ordinary invite is for a user who has not signed up: it is no
    // way to set the password of an active user.
    let plain = generate_secret(INVITE_PREFIX);
    let mut tx = org.api.store.begin().await.unwrap();
    tx.insert_invite(bob, &plain.hash, &after(3600))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(accept(&org, &plain.full).await, StatusCode::NOT_FOUND);

    // A password link is no way to activate an invited or disabled user.
    let mut tx = org.api.store.begin().await.unwrap();
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
    let forged = generate_secret("uf-pwl-");
    tx.insert_invite_of_kind(sam, &forged.hash, &after(3600), "set_password")
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(accept(&org, &forged.full).await, StatusCode::NOT_FOUND);
    let sam = org.api.store.user_by_id(sam).await.unwrap().unwrap();
    assert_eq!(sam.status, UserStatus::Invited);

    // A user disabled after the link was made cannot use it.
    let cy = sso_user(&org, "cy").await;
    let (_, body) = link_for(&org, &maya, cy).await;
    let (status, _) = org
        .call(
            Some(&maya),
            "PATCH",
            &format!("/api/users/{cy}"),
            Some(json!({ "status": "disabled" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(accept(&org, &token_of(&body)).await, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_new_link_ends_the_earlier_one() {
    let org = org().await;
    let ann = sso_user(&org, "ann").await;
    let maya = org.sign_in("maya").await;
    let (_, first) = link_for(&org, &maya, ann).await;
    let (_, second) = link_for(&org, &maya, ann).await;
    assert_eq!(accept(&org, &token_of(&first)).await, StatusCode::NOT_FOUND);
    assert_eq!(
        accept(&org, &token_of(&second)).await,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn only_an_admin_issues_a_link_and_only_for_the_right_user() {
    let org = org().await;
    let ann = sso_user(&org, "ann").await;
    for name in ["arjun", "lena", "priya"] {
        let who = org.sign_in(name).await;
        let (status, body) = link_for(&org, &who, ann).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{name}: {body}");
        assert_eq!(error_code(&body), "forbidden");
    }
    let (status, _) = org
        .call(
            None,
            "POST",
            &format!("/api/users/{ann}/password-link"),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // A refusal leaves no trace of an issuance.
    assert!(!org
        .audit_actions()
        .await
        .contains(&"user.password_link".to_string()));

    let maya = org.sign_in("maya").await;
    let (status, body) = link_for(&org, &maya, 9999).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    // A user who signs in with a password gets a password change, not a link.
    let (status, body) = link_for(&org, &maya, org.lena).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(error_code(&body), "not_sso_user");
    // A user made by single sign-on who has a password already.
    let ann_hash = ultrafast_gateway::identity::password::hash_password(PASSWORD).unwrap();
    let mut tx = org.api.store.begin().await.unwrap();
    assert!(tx.set_user_password(ann, &ann_hash).await.unwrap());
    tx.commit().await.unwrap();
    let (status, body) = link_for(&org, &maya, ann).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(error_code(&body), "has_password");
    // A disabled user.
    let bob = sso_user(&org, "bob").await;
    let mut tx = org.api.store.begin().await.unwrap();
    tx.set_user_status(bob, UserStatus::Disabled).await.unwrap();
    tx.commit().await.unwrap();
    let (status, body) = link_for(&org, &maya, bob).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(error_code(&body), "not_active");
}

#[tokio::test]
async fn a_link_is_refused_for_an_admin_unless_it_is_the_caller() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    // Another admin made by single sign-on, without a password.
    let mut tx = org.api.store.begin().await.unwrap();
    let boss = tx
        .insert_user(NewUser {
            email: "boss@example.com",
            name: "boss",
            role: Role::Admin,
            status: UserStatus::Active,
            password_hash: None,
        })
        .await
        .unwrap();
    tx.link_external(boss, "oidc", "https://idp.example.com|sub-boss")
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let (status, body) = link_for(&org, &maya, boss).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(error_code(&body), "admin_target");
    assert_eq!(
        body["error"]["message"],
        "Admins get a password through their own account, not a link."
    );
    assert!(!org
        .audit_actions()
        .await
        .contains(&"user.password_link".to_string()));

    // The caller themself is not refused for being an admin: here maya has a
    // password, so the next check answers.
    let (status, body) = link_for(&org, &maya, org.maya).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_ne!(error_code(&body), "admin_target");
}
