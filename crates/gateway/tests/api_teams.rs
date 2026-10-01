mod common;

use axum::http::StatusCode;
use common::{compared, error_code, org, raw, Org, Signed};
use serde_json::{json, Value};
use ultrafast_gateway::identity::TeamRole;
use ultrafast_gateway::secrets::generate_key;

fn team_path(id: i64) -> String {
    format!("/api/teams/{id}")
}

fn member_path(team: i64, user: i64) -> String {
    format!("/api/teams/{team}/members/{user}")
}

fn names(body: &Value) -> Vec<&str> {
    body["teams"]
        .as_array()
        .expect("a teams array")
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect()
}

async fn list(org: &Org, who: &Signed) -> Value {
    let (status, body) = org.call(Some(who), "GET", "/api/teams", None).await;
    assert_eq!(status, StatusCode::OK);
    body
}

async fn create(org: &Org, who: &Signed, name: &str) -> (StatusCode, Value) {
    org.call(
        Some(who),
        "POST",
        "/api/teams",
        Some(json!({ "name": name })),
    )
    .await
}

async fn rename(org: &Org, who: &Signed, team: i64, name: &str) -> (StatusCode, Value) {
    let body = Some(json!({ "name": name }));
    org.call(Some(who), "PATCH", &team_path(team), body).await
}

async fn put(org: &Org, who: &Signed, team: i64, user: i64, role: &str) -> (StatusCode, Value) {
    let body = Some(json!({ "role": role }));
    org.call(Some(who), "PUT", &member_path(team, user), body)
        .await
}

async fn remove(org: &Org, who: &Signed, team: i64, user: i64) -> (StatusCode, Value) {
    org.call(Some(who), "DELETE", &member_path(team, user), None)
        .await
}

async fn role_in(org: &Org, team: i64, user: i64) -> Option<TeamRole> {
    let members = org.api.store.members_of(team).await.unwrap();
    members.iter().find(|m| m.user_id == user).map(|m| m.role)
}

#[tokio::test]
async fn admin_lists_all_teams_with_counts() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let body = list(&org, &maya).await;
    assert_eq!(names(&body), ["Growth", "Platform", "Research"]);
    let teams = body["teams"].as_array().unwrap();
    assert_eq!(teams[0]["member_count"], 0);
    assert_eq!(teams[1]["member_count"], 2);
    assert_eq!(teams[2]["member_count"], 2);
    assert_eq!(teams[1]["id"], org.platform);
    assert!(teams[1]["created_at"].is_string());
    assert_eq!(teams[1].as_object().unwrap().len(), 4);
}

#[tokio::test]
async fn users_list_only_their_teams() {
    let org = org().await;
    let arjun = org.sign_in("arjun").await;
    let body = list(&org, &arjun).await;
    assert_eq!(names(&body), ["Platform", "Research"]);
    // The count is of the whole team, not of what the caller may see.
    assert_eq!(body["teams"][1]["member_count"], 2);

    let lena = org.sign_in("lena").await;
    assert_eq!(names(&list(&org, &lena).await), ["Platform"]);
    let priya = org.sign_in("priya").await;
    assert!(names(&list(&org, &priya).await).is_empty());
}

#[tokio::test]
async fn only_admins_create_and_delete_teams() {
    let org = org().await;
    let arjun = org.sign_in("arjun").await;
    let (status, body) = create(&org, &arjun, "Design").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error_code(&body), "forbidden");
    // Refused before the name is looked at.
    assert_eq!(create(&org, &arjun, "").await.0, StatusCode::FORBIDDEN);
    assert_eq!(
        create(&org, &arjun, "Growth").await.0,
        StatusCode::FORBIDDEN
    );

    let (status, _) = org
        .call(Some(&arjun), "DELETE", &team_path(org.platform), None)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = org
        .call(Some(&arjun), "DELETE", &team_path(org.research), None)
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = org
        .call(Some(&arjun), "DELETE", &team_path(org.growth), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(org.api.store.list_teams().await.unwrap().len(), 3);

    let maya = org.sign_in("maya").await;
    let (status, body) = create(&org, &maya, "Design").await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["name"], "Design");
    assert_eq!(body["member_count"], 0);
    assert!(body["created_at"].is_string());
    assert_eq!(body.as_object().unwrap().len(), 4);
    assert_eq!(org.last_summary("team.create").await, "Created team Design");

    let id = body["id"].as_i64().unwrap();
    let (status, body) = org.call(Some(&maya), "DELETE", &team_path(id), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(body.is_null());
    assert_eq!(org.last_summary("team.delete").await, "Deleted team Design");
    let (status, _) = org.call(Some(&maya), "DELETE", &team_path(id), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn team_names_are_validated_and_unique() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    for name in ["", "   ", &"n".repeat(61), "a\nb"] {
        let (status, body) = create(&org, &maya, name).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{name:?}");
        assert!(body["error"]["fields"]["name"].is_string());
        let (status, body) = rename(&org, &maya, org.growth, name).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{name:?}");
        assert!(body["error"]["fields"]["name"].is_string());
    }

    let (status, body) = create(&org, &maya, "Platform").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&body), "team_exists");
    let (status, body) = create(&org, &maya, "  Platform ").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&body), "team_exists");
    let (status, body) = rename(&org, &maya, org.growth, "Platform").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&body), "team_exists");
    assert_eq!(org.api.store.list_teams().await.unwrap().len(), 3);

    let longest = "é".repeat(60);
    let (status, body) = create(&org, &maya, &format!("  {longest}  ")).await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(body["name"], longest);
    let (status, body) = rename(&org, &maya, org.growth, "  Growth Lab ").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "Growth Lab");

    let (status, _) = org
        .call(
            Some(&maya),
            "POST",
            "/api/teams",
            Some(json!({ "name": "X", "id": 9 })),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn team_detail_lists_members() {
    let org = org().await;
    let lena = org.sign_in("lena").await;
    let (status, body) = org
        .call(Some(&lena), "GET", &team_path(org.platform), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_object().unwrap().len(), 2);
    assert_eq!(body["team"]["id"], org.platform);
    assert_eq!(body["team"]["name"], "Platform");
    assert_eq!(body["team"]["member_count"], 2);
    assert_eq!(
        body["members"],
        json!([
            { "user_id": org.arjun, "email": "arjun@example.com",
              "name": "Test User", "role": "lead" },
            { "user_id": org.lena, "email": "lena@example.com",
              "name": "Test User", "role": "member" },
        ])
    );

    let (status, _) = org
        .call(Some(&lena), "GET", &team_path(org.research), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// A hidden team and a missing team get the same bytes, for every method.
#[tokio::test]
async fn hidden_and_missing_teams_answer_alike() {
    let org = org().await;
    let lena = org.sign_in("lena").await;
    let maya = org.sign_in("maya").await;
    let cases = [
        ("GET", "", None),
        ("PATCH", "", Some(json!({ "name": "X" }))),
        ("DELETE", "", None),
        ("PUT", "/members/1", Some(json!({ "role": "member" }))),
        ("PUT", "/members/1", Some(json!({ "role": "lead" }))),
        ("PUT", "/members/999", Some(json!({ "role": "member" }))),
        ("DELETE", "/members/1", None),
        ("DELETE", "/members/4", None),
    ];
    for (method, suffix, body) in cases {
        let mut answers = Vec::new();
        for (who, id) in [(&lena, org.research), (&lena, 999), (&maya, 999)] {
            let path = format!("/api/teams/{id}{suffix}");
            let answer = raw(&org, who, method, &path, body.clone()).await;
            assert_eq!(answer.0, StatusCode::NOT_FOUND, "{method} {path}");
            answers.push(answer);
        }
        for answer in &answers[1..] {
            assert_eq!(compared(answer), compared(&answers[0]), "{method} {suffix}");
        }
    }
    assert_eq!(org.api.store.list_teams().await.unwrap().len(), 3);
    assert_eq!(
        org.api.store.members_of(org.research).await.unwrap().len(),
        2
    );
}

#[tokio::test]
async fn leads_rename_their_team() {
    let org = org().await;
    let arjun = org.sign_in("arjun").await;
    let (status, body) = rename(&org, &arjun, org.platform, "Platform Core").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "Platform Core");
    assert_eq!(body["id"], org.platform);
    assert_eq!(body["member_count"], 2);
    assert_eq!(
        org.last_summary("team.rename").await,
        "Renamed team Platform to Platform Core"
    );

    let (status, _) = rename(&org, &arjun, org.research, "Mine").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let lena = org.sign_in("lena").await;
    let (status, _) = rename(&org, &lena, org.platform, "Mine").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let priya = org.sign_in("priya").await;
    let (status, _) = rename(&org, &priya, org.platform, "Mine").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let store = &org.api.store;
    let name = |id| async move { store.team_by_id(id).await.unwrap().unwrap().name };
    assert_eq!(name(org.platform).await, "Platform Core");
    assert_eq!(name(org.research).await, "Research");
    let renames = org.audit_actions().await;
    assert_eq!(renames.iter().filter(|a| *a == "team.rename").count(), 1);
}

#[tokio::test]
async fn leads_add_members_but_not_leads() {
    let org = org().await;
    let arjun = org.sign_in("arjun").await;
    let (status, body) = put(&org, &arjun, org.platform, org.priya, "member").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(body.is_null());
    assert_eq!(
        role_in(&org, org.platform, org.priya).await,
        Some(TeamRole::Member)
    );
    assert_eq!(
        org.last_summary("team.member_put").await,
        "Added priya@example.com to team Platform as member"
    );

    let (status, _) = put(&org, &arjun, org.platform, org.priya, "lead").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = put(&org, &arjun, org.platform, org.tomas, "lead").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    // A role that does not exist is checked as the strictest one.
    let (status, _) = put(&org, &arjun, org.platform, org.tomas, "owner").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = put(&org, &arjun, org.research, org.priya, "member").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = put(&org, &arjun, org.growth, org.priya, "member").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let lena = org.sign_in("lena").await;
    let (status, _) = put(&org, &lena, org.platform, org.tomas, "member").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = put(&org, &lena, org.platform, org.lena, "lead").await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    assert_eq!(
        role_in(&org, org.platform, org.priya).await,
        Some(TeamRole::Member)
    );
    assert_eq!(
        role_in(&org, org.platform, org.lena).await,
        Some(TeamRole::Member)
    );
    assert_eq!(role_in(&org, org.platform, org.tomas).await, None);
    assert_eq!(role_in(&org, org.research, org.priya).await, None);
    assert_eq!(role_in(&org, org.growth, org.priya).await, None);
    let puts = org.audit_actions().await;
    assert_eq!(puts.iter().filter(|a| *a == "team.member_put").count(), 1);
}

#[tokio::test]
async fn member_roles_are_validated() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let (status, body) = put(&org, &maya, org.platform, org.priya, "owner").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body["error"]["fields"]["role"].is_string());
    for user in ["abc", "0", "-1"] {
        let path = format!("/api/teams/{}/members/{user}", org.platform);
        let body = Some(json!({ "role": "member" }));
        let (status, _) = org.call(Some(&maya), "PUT", &path, body).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
        let (status, _) = org.call(Some(&maya), "DELETE", &path, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
    }
    for team in ["abc", "0", "-1"] {
        let path = format!("/api/teams/{team}");
        let (status, _) = org.call(Some(&maya), "GET", &path, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
    }
    assert_eq!(role_in(&org, org.platform, org.priya).await, None);
}

#[tokio::test]
async fn leads_remove_members() {
    let org = org().await;
    let arjun = org.sign_in("arjun").await;
    let (status, body) = remove(&org, &arjun, org.platform, org.lena).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(body.is_null());
    assert_eq!(role_in(&org, org.platform, org.lena).await, None);
    assert_eq!(
        org.last_summary("team.member_remove").await,
        "Removed lena@example.com from team Platform"
    );
    let (status, _) = remove(&org, &arjun, org.platform, org.lena).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Not from a team he does not lead, and not by a plain member.
    let (status, _) = remove(&org, &arjun, org.research, org.tomas).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let tomas = org.sign_in("tomas").await;
    let (status, _) = remove(&org, &tomas, org.research, org.arjun).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = remove(&org, &tomas, org.platform, org.arjun).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        org.api.store.members_of(org.research).await.unwrap().len(),
        2
    );
    assert_eq!(
        role_in(&org, org.platform, org.arjun).await,
        Some(TeamRole::Lead)
    );
    let removes = org.audit_actions().await;
    let count = removes.iter().filter(|a| *a == "team.member_remove");
    assert_eq!(count.count(), 1);
}

#[tokio::test]
async fn a_team_may_lose_its_last_lead() {
    let org = org().await;
    let arjun = org.sign_in("arjun").await;
    let (status, _) = remove(&org, &arjun, org.platform, org.arjun).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    // He is out, so the team is hidden from him now.
    let (status, _) = put(&org, &arjun, org.platform, org.priya, "member").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        org.api.store.members_of(org.platform).await.unwrap().len(),
        1
    );
}

#[tokio::test]
async fn adding_a_missing_or_disabled_user() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let (status, _) = put(&org, &maya, org.platform, 999, "member").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let body = Some(json!({ "status": "disabled" }));
    let path = format!("/api/users/{}", org.priya);
    let (status, _) = org.call(Some(&maya), "PATCH", &path, body).await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = put(&org, &maya, org.platform, org.priya, "member").await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&body), "user_disabled");
    assert_eq!(role_in(&org, org.platform, org.priya).await, None);
    assert!(!org
        .audit_actions()
        .await
        .contains(&"team.member_put".to_string()));
}

#[tokio::test]
async fn admin_appoints_leads() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let lena = org.sign_in("lena").await;
    let (status, _) = put(&org, &maya, org.platform, org.lena, "lead").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(
        org.last_summary("team.member_put").await,
        "Changed role of lena@example.com in team Platform from member to lead"
    );
    // Her session goes on, with the new role.
    let (status, body) = rename(&org, &lena, org.platform, "Platform Two").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "Platform Two");

    // Putting the same role again changes and records nothing.
    let before = org.audit_actions().await.len();
    let (status, _) = put(&org, &maya, org.platform, org.lena, "lead").await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(org.audit_actions().await.len(), before);
}

#[tokio::test]
async fn deleting_a_team_keeps_its_keys() {
    let org = org().await;
    let maya = org.sign_in("maya").await;
    let key = generate_key();
    let mut tx = org.api.store.begin().await.unwrap();
    tx.insert_key(
        "platform key",
        &key.hash,
        &key.display,
        None,
        Some(org.arjun),
        Some(org.platform),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let (status, _) = org
        .call(Some(&maya), "DELETE", &team_path(org.platform), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let store = &org.api.store;
    assert!(store.team_by_id(org.platform).await.unwrap().is_none());
    let kept = store.active_key_by_hash(&key.hash).await.unwrap().unwrap();
    assert_eq!(kept.team_id, None);
    assert_eq!(kept.user_id, Some(org.arjun));
    assert!(store.user_by_id(org.lena).await.unwrap().is_some());
}

#[tokio::test]
async fn audit_is_admin_only_and_paged() {
    let org = org().await;
    let arjun = org.sign_in("arjun").await;
    let (status, body) = org.call(Some(&arjun), "GET", "/api/audit", None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(error_code(&body), "forbidden");

    let maya = org.sign_in("maya").await;
    for name in ["One", "Two", "Three"] {
        assert_eq!(create(&org, &maya, name).await.0, StatusCode::CREATED);
    }
    let (status, body) = org.call(Some(&maya), "GET", "/api/audit", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_object().unwrap().len(), 1);
    let entries = body["entries"].as_array().unwrap();
    // Two sign-ins and three teams.
    assert_eq!(entries.len(), 5);
    assert_eq!(entries[0]["action"], "team.create");
    assert_eq!(entries[0]["summary"], "Created team Three");
    assert_eq!(entries[0]["actor_email"], "maya@example.com");
    assert_eq!(entries[0]["target_type"], "team");
    assert!(entries[0]["target_id"].is_i64());
    assert!(entries[0]["at"].is_string());
    assert_eq!(entries[4]["action"], "auth.login");
    let ids: Vec<i64> = entries.iter().map(|e| e["id"].as_i64().unwrap()).collect();
    assert!(ids.windows(2).all(|w| w[0] > w[1]), "{ids:?}");

    let (status, body) = org
        .call(Some(&maya), "GET", "/api/audit?limit=2", None)
        .await;
    assert_eq!(status, StatusCode::OK);
    let page = body["entries"].as_array().unwrap();
    assert_eq!(page.len(), 2);
    assert_eq!(page[0]["id"], ids[0]);
    assert_eq!(page[1]["id"], ids[1]);

    let path = format!("/api/audit?limit=2&before={}", ids[1]);
    let (status, body) = org.call(Some(&maya), "GET", &path, None).await;
    assert_eq!(status, StatusCode::OK);
    let page = body["entries"].as_array().unwrap();
    assert_eq!(page.len(), 2);
    assert_eq!(page[0]["id"], ids[2]);
    assert_eq!(page[1]["id"], ids[3]);

    let path = format!("/api/audit?before={}", ids[4]);
    let (_, body) = org.call(Some(&maya), "GET", &path, None).await;
    assert!(body["entries"].as_array().unwrap().is_empty());

    for query in ["limit=abc", "before=abc", "limit=0", "limit=-1", "before=0"] {
        let path = format!("/api/audit?{query}");
        let (status, body) = org.call(Some(&maya), "GET", &path, None).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{query}");
        assert_eq!(error_code(&body), "validation_failed");
    }
    let (status, body) = org
        .call(Some(&maya), "GET", "/api/audit?limit=100000", None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["entries"].as_array().unwrap().len(), 5);
}
