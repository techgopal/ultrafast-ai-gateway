mod common;

use axum::http::StatusCode;
use common::{error_code, org, post_chat, Org, Signed};
use serde_json::{json, Value};
use ultrafast_gateway::secrets::{generate_key, hash_key};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PAST: &str = "2000-01-01 00:00:00";
const FUTURE: &str = "2999-01-01 00:00:00";

/// One key for each of four users, and one from before keys had owners.
struct Keys {
    arjun: i64,
    lena: i64,
    tomas: i64,
    priya: i64,
    legacy: i64,
}

async fn seed_key(
    org: &Org,
    name: &str,
    expires_at: Option<&str>,
    owner: Option<i64>,
    team: Option<i64>,
) -> i64 {
    let key = generate_key();
    let mut tx = org.api.store.begin().await.unwrap();
    let id = tx
        .insert_key(name, &key.hash, &key.display, expires_at, owner, team)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    id
}

async fn seed_keys(org: &Org) -> Keys {
    Keys {
        legacy: seed_key(org, "legacy", None, None, None).await,
        arjun: seed_key(org, "arjun", None, Some(org.arjun), Some(org.platform)).await,
        lena: seed_key(org, "lena", None, Some(org.lena), Some(org.platform)).await,
        tomas: seed_key(org, "tomas", None, Some(org.tomas), Some(org.research)).await,
        priya: seed_key(org, "priya", None, Some(org.priya), None).await,
    }
}

/// The audit actions of this area, oldest first. Signing in is audited
/// too, and is left out.
async fn audited(org: &Org) -> Vec<String> {
    let all = org.audit_actions().await;
    all.into_iter().filter(|a| a.starts_with("key.")).collect()
}

fn key_path(id: i64) -> String {
    format!("/api/keys/{id}")
}

fn names(body: &Value) -> Vec<&str> {
    let mut names: Vec<&str> = body["keys"]
        .as_array()
        .expect("a keys array")
        .iter()
        .map(|k| k["name"].as_str().unwrap())
        .collect();
    names.sort_unstable();
    names
}

async fn create(org: &Org, who: &Signed, body: Value) -> (StatusCode, Value) {
    org.call(Some(who), "POST", "/api/keys", Some(body)).await
}

async fn revoke(org: &Org, who: &Signed, id: i64) -> (StatusCode, Value) {
    org.call(Some(who), "DELETE", &key_path(id), None).await
}

fn assert_invalid(status: StatusCode, body: &Value, field: &str) {
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{field}: {body}");
    assert_eq!(error_code(body), "validation_failed");
    assert!(
        body["error"]["fields"][field].is_string(),
        "{field}: {body}"
    );
}

async fn count_keys(org: &Org) -> usize {
    org.api.store.list_keys().await.unwrap().len()
}

#[tokio::test]
async fn keys_need_a_sign_in() {
    let org = org().await;
    let keys = seed_keys(&org).await;
    for (method, path) in [
        ("GET", "/api/keys".to_string()),
        ("POST", "/api/keys".to_string()),
        ("GET", key_path(keys.lena)),
        ("DELETE", key_path(keys.lena)),
    ] {
        let (status, body) = org
            .call(None, method, &path, Some(json!({ "name": "k" })))
            .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {path}");
        assert_eq!(error_code(&body), "unauthenticated");
    }
    assert_eq!(count_keys(&org).await, 5);
}

#[tokio::test]
async fn admin_lists_all_keys() {
    let org = org().await;
    let keys = seed_keys(&org).await;
    let maya = org.sign_in("maya").await;
    let (status, body) = org.call(Some(&maya), "GET", "/api/keys", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(names(&body), ["arjun", "legacy", "lena", "priya", "tomas"]);

    let list = body["keys"].as_array().unwrap();
    // Newest first.
    assert_eq!(list[0]["id"], keys.priya);
    let legacy = list.iter().find(|k| k["id"] == keys.legacy).unwrap();
    assert_eq!(legacy["owner_id"], Value::Null);
    assert_eq!(legacy["owner_email"], Value::Null);
    assert_eq!(legacy["team_id"], Value::Null);
    assert_eq!(legacy["team_name"], Value::Null);

    let lena = list.iter().find(|k| k["id"] == keys.lena).unwrap();
    assert_eq!(lena["owner_id"], org.lena);
    assert_eq!(lena["owner_email"], "lena@example.com");
    assert_eq!(lena["team_id"], org.platform);
    assert_eq!(lena["team_name"], "Platform");
    assert_eq!(lena["status"], "active");
    let mut fields: Vec<&str> = lena
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
            "name",
            "owner_email",
            "owner_id",
            "revoked_at",
            "status",
            "team_id",
            "team_name"
        ]
    );
}

#[tokio::test]
async fn lead_lists_team_and_own_keys() {
    let org = org().await;
    seed_keys(&org).await;
    seed_key(&org, "arjun private", None, Some(org.arjun), None).await;
    let arjun = org.sign_in("arjun").await;
    let (status, body) = org.call(Some(&arjun), "GET", "/api/keys", None).await;
    assert_eq!(status, StatusCode::OK);
    // Platform only: arjun is a plain member of Research, so not tomas's.
    assert_eq!(names(&body), ["arjun", "arjun private", "lena"]);
}

#[tokio::test]
async fn member_lists_own_keys() {
    let org = org().await;
    seed_keys(&org).await;
    let lena = org.sign_in("lena").await;
    let (status, body) = org.call(Some(&lena), "GET", "/api/keys", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(names(&body), ["lena"]);
}

#[tokio::test]
async fn creating_a_key_returns_the_secret_once() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let (status, body) = create(&org, &maya, json!({ "name": "  ci  " })).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body.as_object().unwrap().len(), 2);
    let secret = body["secret"].as_str().unwrap().to_string();
    assert!(secret.starts_with("uf-sk-"));
    assert_eq!(secret.len(), 6 + 64);
    assert_eq!(body["key"]["name"], "ci");
    assert_eq!(body["key"]["owner_id"], org.maya);
    assert_eq!(body["key"]["owner_email"], "maya@example.com");
    assert_eq!(body["key"]["team_id"], Value::Null);
    assert_eq!(body["key"]["status"], "active");
    let display = body["key"]["display"].as_str().unwrap();
    assert!(display.ends_with(&secret[secret.len() - 4..]));
    assert!(!body["key"].to_string().contains(&secret[6..]));
    let id = body["key"]["id"].as_i64().unwrap();

    let hash = hash_key(&secret);
    let (status, seen) = org.call(Some(&maya), "GET", &key_path(id), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(seen, body["key"]);
    let (_, list) = org.call(Some(&maya), "GET", "/api/keys", None).await;
    for text in [seen.to_string(), list.to_string()] {
        assert!(!text.contains("secret"), "{text}");
        assert!(!text.contains("hash"), "{text}");
        assert!(!text.contains(&secret[6..]), "{text}");
        assert!(!text.contains(&hash), "{text}");
    }

    assert_eq!(audited(&org).await, ["key.create"]);
    for row in org.api.store.list_audit(200, None).await.unwrap() {
        assert!(!row.summary.contains(&secret[6..]), "{}", row.summary);
        assert!(!row.summary.contains(&hash), "{}", row.summary);
        if !row.action.starts_with("key.") {
            continue;
        }
        assert_eq!(row.target_id, Some(id));
    }

    // The key works on /v1 at once.
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("authorization", "Bearer provider-secret"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "c1", "model": "gpt-4o",
            "choices": [{
                "message": { "role": "assistant", "content": "hello" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 2 }
        })))
        .expect(1)
        .mount(&upstream)
        .await;
    let provider = json!({
        "name": "p", "kind": "openai", "base_url": upstream.uri(), "api_key": "provider-secret"
    });
    let (status, _) = org
        .call(Some(&maya), "POST", "/api/providers", Some(provider))
        .await;
    assert_eq!(status, StatusCode::CREATED);

    let chat = r#"{"model":"p/gpt-4o","messages":[{"role":"user","content":"hi"}]}"#;
    let (status, answer) = post_chat(&org.api.app, Some(&secret), chat).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert!(answer.contains("hello"));

    assert_eq!(revoke(&org, &maya, id).await.0, StatusCode::NO_CONTENT);
    let (status, _) = post_chat(&org.api.app, Some(&secret), chat).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn members_create_keys_for_themselves_in_their_teams() {
    let org = org().await;
    let lena = org.sign_in("lena").await;

    let (status, body) = create(&org, &lena, json!({ "name": "k", "team_id": org.platform })).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["key"]["owner_id"], org.lena);
    assert_eq!(body["key"]["team_name"], "Platform");

    let (status, body) = create(&org, &lena, json!({ "name": "k" })).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["key"]["team_id"], Value::Null);

    let (status, body) = create(&org, &lena, json!({ "name": "k", "team_id": org.research })).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error_code(&body), "forbidden");

    let (status, _) = create(&org, &lena, json!({ "name": "k", "owner_id": org.arjun })).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let for_arjun = json!({ "name": "k", "owner_id": org.arjun, "team_id": org.platform });
    assert_eq!(
        create(&org, &lena, for_arjun).await.0,
        StatusCode::FORBIDDEN
    );

    assert_eq!(count_keys(&org).await, 2);
    assert_eq!(audited(&org).await, ["key.create", "key.create"]);
}

#[tokio::test]
async fn leads_create_keys_for_team_members() {
    let org = org().await;
    let arjun = org.sign_in("arjun").await;

    let for_lena = json!({ "name": "k", "owner_id": org.lena, "team_id": org.platform });
    let (status, body) = create(&org, &arjun, for_lena).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["key"]["owner_id"], org.lena);
    assert_eq!(body["key"]["owner_email"], "lena@example.com");
    let summary = org.last_summary("key.create").await;
    assert!(summary.contains("lena@example.com"), "{summary}");
    assert!(summary.contains("Platform"), "{summary}");

    let for_tomas = json!({ "name": "k", "owner_id": org.tomas, "team_id": org.research });
    assert_eq!(
        create(&org, &arjun, for_tomas).await.0,
        StatusCode::FORBIDDEN
    );

    let no_team = json!({ "name": "k", "owner_id": org.lena });
    assert_eq!(create(&org, &arjun, no_team).await.0, StatusCode::FORBIDDEN);
    assert_eq!(count_keys(&org).await, 1);
}

#[tokio::test]
async fn leads_cannot_create_keys_for_users_outside_the_team() {
    let org = org().await;
    let arjun = org.sign_in("arjun").await;
    let for_tomas = json!({ "name": "k", "owner_id": org.tomas, "team_id": org.platform });
    let (status, body) = create(&org, &arjun, for_tomas).await;
    assert_invalid(status, &body, "team_id");
    assert_eq!(
        body["error"]["fields"]["team_id"],
        "owner is not a member of this team"
    );
    assert_eq!(count_keys(&org).await, 0);
    assert!(audited(&org).await.is_empty());
}

#[tokio::test]
async fn a_refusal_does_not_reveal_what_exists() {
    let org = org().await;
    let lena = org.sign_in("lena").await;
    // A team and a user that exist, and ones that do not: one answer.
    let mut answers = Vec::new();
    for body in [
        json!({ "name": "k", "team_id": org.research }),
        json!({ "name": "k", "team_id": 9999 }),
        json!({ "name": "k", "owner_id": org.arjun }),
        json!({ "name": "k", "owner_id": 9999 }),
        json!({ "name": "", "owner_id": 9999, "expires_at": "never" }),
    ] {
        let (status, body) = create(&org, &lena, body).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        answers.push(body.to_string());
    }
    answers.dedup();
    assert_eq!(answers.len(), 1);
}

#[tokio::test]
async fn owner_must_belong_to_the_team() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let for_priya = json!({ "name": "k", "owner_id": org.priya, "team_id": org.platform });
    let (status, body) = create(&org, &maya, for_priya).await;
    assert_invalid(status, &body, "team_id");
    assert_eq!(
        body["error"]["fields"]["team_id"],
        "owner is not a member of this team"
    );
    assert_eq!(count_keys(&org).await, 0);
}

#[tokio::test]
async fn key_input_is_validated() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let disable = json!({ "status": "disabled" });
    let (status, _) = org
        .call(
            Some(&maya),
            "PATCH",
            &format!("/api/users/{}", org.priya),
            Some(disable),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    let long = "n".repeat(101);
    for (body, field) in [
        (json!({ "name": "" }), "name"),
        (json!({ "name": "   " }), "name"),
        (json!({ "name": long }), "name"),
        (
            json!({ "name": "k", "expires_at": "2999-02-31 00:00:00" }),
            "expires_at",
        ),
        (
            json!({ "name": "k", "expires_at": "2999-01-01T00:00:00Z" }),
            "expires_at",
        ),
        (json!({ "name": "k", "expires_at": PAST }), "expires_at"),
        (json!({ "name": "k", "team_id": 9999 }), "team_id"),
        (json!({ "name": "k", "owner_id": org.priya }), "owner_id"),
        (json!({ "name": "k", "owner_id": 9999 }), "owner_id"),
    ] {
        let (status, answer) = create(&org, &maya, body.clone()).await;
        assert_invalid(status, &answer, field);
        assert_eq!(
            answer["error"]["fields"].as_object().unwrap().len(),
            1,
            "{body}"
        );
    }

    for body in [
        json!({}),
        json!({ "name": "k", "hash": "x" }),
        json!({ "name": 1 }),
    ] {
        let (status, answer) = create(&org, &maya, body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(error_code(&answer), "bad_request");
    }
    assert_eq!(count_keys(&org).await, 0);
    assert!(!org
        .audit_actions()
        .await
        .contains(&"key.create".to_string()));

    let (status, body) = create(&org, &maya, json!({ "name": "k", "expires_at": FUTURE })).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["key"]["expires_at"], FUTURE);
}

#[tokio::test]
async fn an_invited_owner_is_refused() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let invite = json!({ "email": "noor@example.com", "name": "Noor", "role": "member" });
    let (status, body) = org
        .call(Some(&maya), "POST", "/api/users", Some(invite))
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let noor = body["user"]["id"].as_i64().unwrap();
    let (status, body) = create(&org, &maya, json!({ "name": "k", "owner_id": noor })).await;
    assert_invalid(status, &body, "owner_id");
}

#[tokio::test]
async fn viewing_and_revoking_follow_the_policy() {
    let org = org().await;
    let keys = seed_keys(&org).await;
    let arjun = org.sign_in("arjun").await;
    let lena = org.sign_in("lena").await;

    let (status, _) = org
        .call(Some(&lena), "GET", &key_path(keys.arjun), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, body) = org
        .call(Some(&lena), "GET", &key_path(keys.lena), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "lena");
    let (status, _) = org
        .call(Some(&arjun), "GET", &key_path(keys.lena), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    for hidden in [keys.tomas, keys.priya, keys.legacy] {
        let (status, _) = org.call(Some(&arjun), "GET", &key_path(hidden), None).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    assert_eq!(
        revoke(&org, &arjun, keys.lena).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        revoke(&org, &arjun, keys.tomas).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        revoke(&org, &lena, keys.arjun).await.0,
        StatusCode::NOT_FOUND
    );

    let own = seed_key(&org, "lena 2", None, Some(org.lena), None).await;
    assert_eq!(revoke(&org, &lena, own).await.0, StatusCode::NO_CONTENT);

    let store = &org.api.store;
    assert!(store
        .key_by_id(keys.lena)
        .await
        .unwrap()
        .unwrap()
        .revoked_at
        .is_some());
    assert!(store
        .key_by_id(own)
        .await
        .unwrap()
        .unwrap()
        .revoked_at
        .is_some());
    for live in [keys.arjun, keys.tomas, keys.priya, keys.legacy] {
        assert_eq!(
            store.key_by_id(live).await.unwrap().unwrap().revoked_at,
            None
        );
    }
    assert_eq!(audited(&org).await, ["key.revoke", "key.revoke"]);

    let maya = org.sign_in("maya").await;
    assert_eq!(
        revoke(&org, &maya, keys.legacy).await.0,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn a_hidden_key_looks_like_a_missing_one() {
    let org = org().await;
    let keys = seed_keys(&org).await;
    let lena = org.sign_in("lena").await;
    for method in ["GET", "DELETE"] {
        let hidden = common::raw(&org, &lena, method, &key_path(keys.arjun), None).await;
        let missing = common::raw(&org, &lena, method, &key_path(9999), None).await;
        let malformed = common::raw(&org, &lena, method, "/api/keys/abc", None).await;
        assert_eq!(hidden.0, StatusCode::NOT_FOUND);
        assert_eq!(common::compared(&hidden), common::compared(&missing));
        assert_eq!(common::compared(&hidden), common::compared(&malformed));
    }
    assert!(audited(&org).await.is_empty());
}

#[tokio::test]
async fn revoking_twice_is_quiet() {
    let org = org().await;
    let keys = seed_keys(&org).await;
    let maya = org.sign_in("maya").await;
    assert_eq!(
        revoke(&org, &maya, keys.lena).await.0,
        StatusCode::NO_CONTENT
    );
    let first = org.api.store.key_by_id(keys.lena).await.unwrap().unwrap();
    assert_eq!(
        revoke(&org, &maya, keys.lena).await.0,
        StatusCode::NO_CONTENT
    );
    let second = org.api.store.key_by_id(keys.lena).await.unwrap().unwrap();
    assert_eq!(first.revoked_at, second.revoked_at);
    assert_eq!(audited(&org).await, ["key.revoke"]);
    let summary = org.last_summary("key.revoke").await;
    assert!(summary.contains("lena"), "{summary}");
}

#[tokio::test]
async fn status_reflects_expiry_and_revocation() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let live = seed_key(&org, "live", Some(FUTURE), Some(org.maya), None).await;
    let expired = seed_key(&org, "expired", Some(PAST), Some(org.maya), None).await;
    let revoked = seed_key(&org, "revoked", None, Some(org.maya), None).await;
    let both = seed_key(&org, "both", Some(PAST), Some(org.maya), None).await;
    for id in [revoked, both] {
        assert_eq!(revoke(&org, &maya, id).await.0, StatusCode::NO_CONTENT);
    }
    for (id, status) in [
        (live, "active"),
        (expired, "expired"),
        (revoked, "revoked"),
        (both, "revoked"),
    ] {
        let (code, body) = org.call(Some(&maya), "GET", &key_path(id), None).await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(body["status"], status, "{body}");
    }
    let (_, body) = org.call(Some(&maya), "GET", "/api/keys", None).await;
    let of = |id: i64| {
        let list = body["keys"].as_array().unwrap();
        list.iter().find(|k| k["id"] == id).unwrap()["status"].clone()
    };
    assert_eq!(of(expired), "expired");
    assert_eq!(of(both), "revoked");
}

fn status_of(list: &Value, id: i64) -> &str {
    let keys = list["keys"].as_array().expect("a keys array");
    let key = keys.iter().find(|k| k["id"] == id).expect("the key");
    key["status"].as_str().unwrap()
}

#[tokio::test]
async fn the_key_of_a_disabled_owner_is_suspended() {
    let org = org().await;
    let keys = seed_keys(&org).await;
    let expired = seed_key(&org, "old", Some(PAST), Some(org.lena), None).await;
    let revoked = seed_key(&org, "gone", None, Some(org.lena), None).await;
    let maya = org.sign_in("maya").await;
    let (status, _) = revoke(&org, &maya, revoked).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let lena = format!("/api/users/{}", org.lena);

    let disable = json!({ "status": "disabled" });
    let (status, body) = org.call(Some(&maya), "PATCH", &lena, Some(disable)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, list) = org.call(Some(&maya), "GET", "/api/keys", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(status_of(&list, keys.lena), "suspended");
    // Revoked and expired say more, and win.
    assert_eq!(status_of(&list, revoked), "revoked");
    assert_eq!(status_of(&list, expired), "expired");
    // The keys of others, and keys without an owner, are as before.
    assert_eq!(status_of(&list, keys.arjun), "active");
    assert_eq!(status_of(&list, keys.legacy), "active");
    let (status, seen) = org
        .call(Some(&maya), "GET", &key_path(keys.lena), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(seen["status"], "suspended");

    let enable = json!({ "status": "active" });
    let (status, body) = org.call(Some(&maya), "PATCH", &lena, Some(enable)).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (_, list) = org.call(Some(&maya), "GET", "/api/keys", None).await;
    assert_eq!(status_of(&list, keys.lena), "active");
    assert_eq!(status_of(&list, revoked), "revoked");
    assert_eq!(status_of(&list, expired), "expired");
    let (_, seen) = org
        .call(Some(&maya), "GET", &key_path(keys.lena), None)
        .await;
    assert_eq!(seen["status"], "active");
}
