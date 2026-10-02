//! The response cache of a route: hits and misses, whom an answer is kept
//! for (never across teams), what is never kept, expiry, and that a hit
//! still passes limits and budgets and is recorded as cached at no cost.

mod common;

use std::time::Duration;

use axum::http::StatusCode;
use common::{harness, post_to, seed_team, seed_user, Harness};
use serde_json::{json, Value};
use time::OffsetDateTime;
use ultrafast_gateway::budgets::{account, BudgetAction, Period};
use ultrafast_gateway::cache::{CacheScope, RouteCache};
use ultrafast_gateway::identity::{Role, TeamRole};
use ultrafast_gateway::limits::{LimitScope, RateLimit};
use ultrafast_gateway::secrets::generate_key;
use ultrafast_gateway::store::{RouteSettings, TargetsInput};
use ultrafast_gateway::telemetry::{AttemptOutcome, RequestRecord};
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

const PASSWORD: &str = "correct horse battery";

const SETTINGS: RouteSettings = RouteSettings {
    retries: 0,
    first_token_timeout_ms: 30_000,
    total_timeout_ms: 300_000,
    breaker_failures: 5,
    breaker_window_s: 60,
    breaker_open_s: 30,
};

const BODY: &str = r#"{"model":"r","max_tokens":10,"messages":[{"role":"user","content":"hi"}]}"#;

fn ok(content: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "c1", "model": "gpt-4o",
        "choices": [{ "message": { "role": "assistant", "content": content }, "finish_reason": "stop" }],
        "usage": { "prompt_tokens": 90, "completion_tokens": 20 }
    }))
}

struct Who {
    id: i64,
    key: String,
    team: Option<i64>,
    user: Option<i64>,
}

struct World {
    h: Harness,
}

/// A harness with the route `r` on `p/gpt-4o`, its cache on for `scope`.
async fn world(scope: CacheScope) -> World {
    let h = harness("openai").await;
    let gpt = h.store.list_models().await.unwrap();
    let model = gpt.iter().find(|m| m.name == "gpt-4o").unwrap().id;
    let mut tx = h.store.begin().await.unwrap();
    let id = tx.insert_route("r", &SETTINGS, true).await.unwrap();
    tx.replace_targets(
        id,
        &TargetsInput {
            primaries: vec![(model, 1)],
            fallbacks: vec![],
        },
    )
    .await
    .unwrap();
    tx.set_route_cache(
        id,
        &RouteCache {
            enabled: true,
            ttl_s: 300,
            scope,
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    h.state.refresh().await.unwrap();
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ok("hello"))
        .mount(&h.upstream)
        .await;
    World { h }
}

impl World {
    async fn key(&self, name: &str, user: Option<i64>, team: Option<i64>) -> Who {
        let key = generate_key();
        let mut tx = self.h.store.begin().await.unwrap();
        let id = tx
            .insert_key(name, &key.hash, &key.display, None, user, team)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        self.h.state.refresh().await.unwrap();
        Who {
            id,
            key: key.full,
            team,
            user,
        }
    }

    async fn chat_as(&self, who: &Who, body: &str) -> (StatusCode, axum::http::HeaderMap, String) {
        let bearer = format!("Bearer {}", who.key);
        post_to(
            &self.h.app,
            "/v1/chat/completions",
            &[("authorization", &bearer)],
            body,
        )
        .await
    }

    async fn chat(&self, who: &Who, body: &str) -> StatusCode {
        self.chat_as(who, body).await.0
    }

    async fn provider_calls(&self) -> usize {
        self.h.upstream.received_requests().await.unwrap().len()
    }

    /// Two teams of one member each, and a key of each.
    async fn two_teams(&self) -> (Who, Who) {
        let a_user = seed_user(&self.h.store, "a@example.com", Role::Member, PASSWORD).await;
        let b_user = seed_user(&self.h.store, "b@example.com", Role::Member, PASSWORD).await;
        let a = seed_team(&self.h.store, "A", &[(a_user, TeamRole::Member)]).await;
        let b = seed_team(&self.h.store, "B", &[(b_user, TeamRole::Member)]).await;
        (
            self.key("a", Some(a_user), Some(a)).await,
            self.key("b", Some(b_user), Some(b)).await,
        )
    }
}

fn content_of(body: &str) -> String {
    let v: Value = serde_json::from_str(body).unwrap();
    v["choices"][0]["message"]["content"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn the_second_identical_call_is_answered_without_a_provider() {
    let w = world(CacheScope::Team).await;
    let (a, _) = w.two_teams().await;
    let (s1, _, first) = w.chat_as(&a, BODY).await;
    let (s2, _, second) = w.chat_as(&a, BODY).await;
    assert_eq!((s1, s2), (StatusCode::OK, StatusCode::OK));
    assert_eq!(w.provider_calls().await, 1);
    assert_eq!(content_of(&first), "hello");
    assert_eq!(content_of(&second), "hello");
    let v: Value = serde_json::from_str(&second).unwrap();
    assert_eq!(v["usage"]["prompt_tokens"], 90);
    assert_eq!(v["usage"]["completion_tokens"], 20);
    // A different call is a miss.
    let other = BODY.replace("hi", "ho");
    assert_eq!(w.chat(&a, &other).await, StatusCode::OK);
    assert_eq!(w.provider_calls().await, 2);
}

#[tokio::test]
async fn a_hit_is_recorded_as_cached_with_the_cached_usage_and_one_cached_attempt() {
    let w = world(CacheScope::Team).await;
    let (a, _) = w.two_teams().await;
    w.chat(&a, BODY).await;
    w.chat(&a, BODY).await;
    let records = w.h.sink.records();
    assert_eq!(records.len(), 2);
    assert!(!records[0].cached);
    assert_eq!(records[0].attempts[0].outcome, AttemptOutcome::Ok);
    let hit = &records[1];
    assert!(hit.cached);
    assert_eq!(hit.status, 200);
    assert_eq!(hit.requested, "r");
    assert_eq!(hit.endpoint, "chat");
    let usage = hit.usage.unwrap();
    assert_eq!((usage.input_tokens, usage.output_tokens), (90, 20));
    assert_eq!(hit.attempts.len(), 1);
    let attempt = &hit.attempts[0];
    assert_eq!(attempt.outcome, AttemptOutcome::Cached);
    // The target that gave the answer, and no provider status.
    assert_eq!(
        (attempt.provider.as_str(), attempt.model.as_str()),
        ("p", "gpt-4o")
    );
    assert_eq!(attempt.status, None);
}

#[tokio::test]
async fn two_teams_never_share_an_answer() {
    for scope in [CacheScope::Team, CacheScope::User, CacheScope::Key] {
        let w = world(scope).await;
        let (a, b) = w.two_teams().await;
        // The identical request from the other team goes to the provider.
        assert_eq!(w.chat(&a, BODY).await, StatusCode::OK);
        assert_eq!(w.chat(&b, BODY).await, StatusCode::OK);
        assert_eq!(w.provider_calls().await, 2, "{scope:?}");
        // Each has its own answer afterwards.
        assert_eq!(w.chat(&a, BODY).await, StatusCode::OK);
        assert_eq!(w.chat(&b, BODY).await, StatusCode::OK);
        assert_eq!(w.provider_calls().await, 2, "{scope:?}");
    }
}

#[tokio::test]
async fn scope_team_shares_within_a_team_and_not_between_keys_of_scope_key() {
    let w = world(CacheScope::Team).await;
    let user = seed_user(&w.h.store, "a@example.com", Role::Member, PASSWORD).await;
    let team = seed_team(&w.h.store, "A", &[(user, TeamRole::Member)]).await;
    let k1 = w.key("k1", Some(user), Some(team)).await;
    // Another key of the team, by another owner, or by none.
    let k2 = w.key("k2", None, Some(team)).await;
    w.chat(&k1, BODY).await;
    w.chat(&k2, BODY).await;
    assert_eq!(w.provider_calls().await, 1);

    let w = world(CacheScope::Key).await;
    let user = seed_user(&w.h.store, "a@example.com", Role::Member, PASSWORD).await;
    let team = seed_team(&w.h.store, "A", &[(user, TeamRole::Member)]).await;
    let k1 = w.key("k1", Some(user), Some(team)).await;
    let k2 = w.key("k2", Some(user), Some(team)).await;
    w.chat(&k1, BODY).await;
    w.chat(&k2, BODY).await;
    assert_eq!(w.provider_calls().await, 2);
    w.chat(&k1, BODY).await;
    w.chat(&k2, BODY).await;
    assert_eq!(w.provider_calls().await, 2);
}

#[tokio::test]
async fn scope_user_shares_between_the_keys_of_one_user_only() {
    let w = world(CacheScope::User).await;
    let lena = seed_user(&w.h.store, "lena@example.com", Role::Member, PASSWORD).await;
    let tomas = seed_user(&w.h.store, "tomas@example.com", Role::Member, PASSWORD).await;
    let team = seed_team(
        &w.h.store,
        "A",
        &[(lena, TeamRole::Member), (tomas, TeamRole::Member)],
    )
    .await;
    let l1 = w.key("l1", Some(lena), Some(team)).await;
    let l2 = w.key("l2", Some(lena), None).await;
    let t1 = w.key("t1", Some(tomas), Some(team)).await;
    w.chat(&l1, BODY).await;
    w.chat(&l2, BODY).await;
    assert_eq!(w.provider_calls().await, 1);
    w.chat(&t1, BODY).await;
    assert_eq!(w.provider_calls().await, 2);
}

#[tokio::test]
async fn keys_without_a_team_or_an_owner_never_share_an_answer_with_each_other() {
    // Scope team, no team, no user: each key is its own scope.
    let w = world(CacheScope::Team).await;
    let k1 = w.key("k1", None, None).await;
    let k2 = w.key("k2", None, None).await;
    w.chat(&k1, BODY).await;
    w.chat(&k2, BODY).await;
    assert_eq!(w.provider_calls().await, 2);
    w.chat(&k1, BODY).await;
    assert_eq!(w.provider_calls().await, 2);
    // Scope team, no team: the owner's answers, which no team's key shares.
    let user = seed_user(&w.h.store, "a@example.com", Role::Member, PASSWORD).await;
    let team = seed_team(&w.h.store, "A", &[(user, TeamRole::Member)]).await;
    let owned = w.key("owned", Some(user), None).await;
    let teamed = w.key("teamed", Some(user), Some(team)).await;
    w.chat(&owned, BODY).await;
    w.chat(&teamed, BODY).await;
    assert_eq!(w.provider_calls().await, 4);
}

#[tokio::test]
async fn streams_and_warm_temperatures_are_never_kept() {
    let w = world(CacheScope::Team).await;
    let (a, _) = w.two_teams().await;
    let warm = BODY.replace("\"max_tokens\"", "\"temperature\":0.6,\"max_tokens\"");
    w.chat(&a, &warm).await;
    w.chat(&a, &warm).await;
    assert_eq!(w.provider_calls().await, 2, "temperature 0.6");
    // Up to 0.5 is kept, and so is none and 0.
    for t in ["0.5", "0", "0.25"] {
        let body = BODY.replace(
            "\"max_tokens\"",
            &format!("\"temperature\":{t},\"max_tokens\""),
        );
        let before = w.provider_calls().await;
        w.chat(&a, &body).await;
        w.chat(&a, &body).await;
        assert_eq!(w.provider_calls().await, before + 1, "temperature {t}");
    }
    // A stream is neither kept nor answered from the cache, even when the
    // same call without `stream` is kept.
    w.chat(&a, BODY).await;
    let stream = BODY.replace("\"max_tokens\"", "\"stream\":true,\"max_tokens\"");
    let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
    w.h.upstream.reset().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"))
        .mount(&w.h.upstream)
        .await;
    for _ in 0..2 {
        let (status, _, body) = w.chat_as(&a, &stream).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("data: [DONE]"));
    }
    assert_eq!(w.provider_calls().await, 2, "streams reach the provider");
    assert!(w.h.sink.records().iter().all(|r| !(r.stream && r.cached)));
}

#[tokio::test]
async fn a_failed_answer_is_not_kept() {
    let w = world(CacheScope::Team).await;
    let (a, _) = w.two_teams().await;
    w.h.upstream.reset().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({"error": {"message": "no"}})))
        .up_to_n_times(1)
        .mount(&w.h.upstream)
        .await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ok("fine"))
        .mount(&w.h.upstream)
        .await;
    assert_eq!(w.chat(&a, BODY).await, StatusCode::BAD_REQUEST);
    assert_eq!(w.chat(&a, BODY).await, StatusCode::OK);
    assert_eq!(w.provider_calls().await, 2);
    assert_eq!(w.chat(&a, BODY).await, StatusCode::OK);
    assert_eq!(w.provider_calls().await, 2);
}

#[tokio::test]
async fn only_a_route_with_the_cache_on_is_cached() {
    let w = world(CacheScope::Team).await;
    let (a, _) = w.two_teams().await;
    // A model called directly.
    let direct = BODY.replace("\"r\"", "\"p/gpt-4o\"");
    w.chat(&a, &direct).await;
    w.chat(&a, &direct).await;
    assert_eq!(w.provider_calls().await, 2);
    // The route with the cache switched off.
    let route = w.h.store.list_routes().await.unwrap().remove(0);
    let mut tx = w.h.store.begin().await.unwrap();
    tx.set_route_cache(
        route.id,
        &RouteCache {
            enabled: false,
            ttl_s: 300,
            scope: CacheScope::Team,
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    w.h.state.refresh().await.unwrap();
    w.chat(&a, BODY).await;
    w.chat(&a, BODY).await;
    assert_eq!(w.provider_calls().await, 4);
}

#[tokio::test]
async fn the_expiry_is_the_ttl_of_the_route() {
    let w = world(CacheScope::Team).await;
    let route = w.h.store.list_routes().await.unwrap().remove(0);
    let mut tx = w.h.store.begin().await.unwrap();
    tx.set_route_cache(
        route.id,
        &RouteCache {
            enabled: true,
            ttl_s: 60,
            scope: CacheScope::Team,
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    w.h.state.refresh().await.unwrap();
    let (a, _) = w.two_teams().await;
    w.chat(&a, BODY).await;
    // The clock is stopped while a hit is answered, which touches no
    // network, and runs while a provider is called.
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(59)).await;
    w.chat(&a, BODY).await;
    assert_eq!(w.provider_calls().await, 1, "59 s: still kept");
    tokio::time::advance(Duration::from_secs(2)).await;
    tokio::time::resume();
    w.chat(&a, BODY).await;
    assert_eq!(w.provider_calls().await, 2, "61 s: expired");
    // The new answer is kept for the full time again.
    w.chat(&a, BODY).await;
    assert_eq!(w.provider_calls().await, 2);
}

#[tokio::test]
async fn a_hit_still_passes_the_limits() {
    let w = world(CacheScope::Team).await;
    let (a, _) = w.two_teams().await;
    let set = |scope: LimitScope, id: Option<i64>, limit: RateLimit| {
        let w = &w;
        async move {
            let mut tx = w.h.store.begin().await.unwrap();
            tx.upsert_limit(scope, id, &limit).await.unwrap();
            tx.commit().await.unwrap();
            w.h.state.refresh().await.unwrap();
        }
    };
    set(
        LimitScope::Key,
        Some(a.id),
        RateLimit {
            requests_per_minute: Some(2),
            tokens_per_minute: None,
            concurrent: None,
        },
    )
    .await;
    assert_eq!(w.chat(&a, BODY).await, StatusCode::OK);
    assert_eq!(w.chat(&a, BODY).await, StatusCode::OK);
    // The third is a hit that is refused.
    let (status, _, _) = w.chat_as(&a, BODY).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(w.provider_calls().await, 1);
}

#[tokio::test]
async fn a_hit_uses_no_tokens_of_the_window() {
    let w = world(CacheScope::Team).await;
    let (a, _) = w.two_teams().await;
    let mut tx = w.h.store.begin().await.unwrap();
    // The call is charged 6 (5 to answer, 1 to read); the provider says 110.
    tx.upsert_limit(
        LimitScope::Key,
        Some(a.id),
        &RateLimit {
            requests_per_minute: None,
            tokens_per_minute: Some(120),
            concurrent: None,
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    w.h.state.refresh().await.unwrap();
    let small = r#"{"model":"r","max_tokens":5,"messages":[{"role":"user","content":"hi"}]}"#;
    assert_eq!(w.chat(&a, small).await, StatusCode::OK);
    // The hit is charged its estimate and gives it back at once: the window
    // still holds 110, so another call of 6 fits under 120.
    assert_eq!(w.chat(&a, small).await, StatusCode::OK);
    assert_eq!(w.chat(&a, small).await, StatusCode::OK);
    assert_eq!(w.provider_calls().await, 1);
    let other = small.replace("hi", "ho");
    assert_eq!(w.chat(&a, &other).await, StatusCode::OK);
    assert_eq!(w.provider_calls().await, 2);
}

#[tokio::test]
async fn a_hit_still_passes_the_budgets() {
    let w = world(CacheScope::Team).await;
    let (a, _) = w.two_teams().await;
    let mut tx = w.h.store.begin().await.unwrap();
    tx.upsert_budget(
        LimitScope::Key,
        Some(a.id),
        1_000_000,
        Period::Monthly,
        BudgetAction::Block,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    w.h.state.refresh().await.unwrap();
    assert_eq!(w.chat(&a, BODY).await, StatusCode::OK);
    assert_eq!(w.chat(&a, BODY).await, StatusCode::OK);
    // The log writer prices a call; here the key's budget is spent.
    let spent = RequestRecord {
        key_id: a.id,
        user_id: a.user,
        team_id: a.team,
        requested: "r".into(),
        endpoint: "chat",
        stream: false,
        status: 200,
        usage: None,
        attempts: Vec::new(),
        cached: false,
        started_at: ultrafast_gateway::store::now(),
        duration_ms: 1,
    };
    account(&w.h.state, &spent, 1_000_000, OffsetDateTime::now_utc());
    let (status, _, body) = w.chat_as(&a, BODY).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert!(body.contains("budget_exceeded"));
    assert_eq!(w.provider_calls().await, 1);
}

#[tokio::test]
async fn an_openai_and_an_anthropic_call_of_the_same_request_share_one_answer() {
    let w = world(CacheScope::Team).await;
    let (a, _) = w.two_teams().await;
    // The same request, as the two shapes say it.
    let openai = r#"{"model":"r","max_tokens":10,"messages":[{"role":"user","content":"hi"}]}"#;
    let anthropic = r#"{"model":"r","max_tokens":10,"messages":[{"role":"user","content":"hi"}]}"#;
    assert_eq!(w.chat(&a, openai).await, StatusCode::OK);
    let (status, headers, body) = post_to(
        &w.h.app,
        "/v1/messages",
        &[("x-api-key", &a.key)],
        anthropic,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(w.provider_calls().await, 1);
    assert!(headers["content-type"].to_str().unwrap().contains("json"));
    // Rendered as an Anthropic message, not as a chat completion.
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["type"], "message");
    assert_eq!(v["content"][0]["text"], "hello");
    assert_eq!(v["usage"]["input_tokens"], 90);
    assert_eq!(v["usage"]["output_tokens"], 20);
    let hit = &w.h.sink.records()[1];
    assert_eq!(hit.endpoint, "messages");
    assert!(hit.cached);
    // And the other way round: a message kept first, a chat call hits it.
    let w = world(CacheScope::Team).await;
    let (a, _) = w.two_teams().await;
    post_to(
        &w.h.app,
        "/v1/messages",
        &[("x-api-key", &a.key)],
        anthropic,
    )
    .await;
    let (_, _, body) = w.chat_as(&a, openai).await;
    assert_eq!(w.provider_calls().await, 1);
    assert_eq!(content_of(&body), "hello");
}

#[tokio::test]
async fn embeddings_are_kept_too() {
    let w = world(CacheScope::Team).await;
    let (a, _) = w.two_teams().await;
    w.h.upstream.reset().await;
    Mock::given(method("POST"))
        .and(path("/embeddings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list", "model": "emb-v1",
            "data": [{ "object": "embedding", "index": 0, "embedding": [0.5, 0.25] }],
            "usage": { "prompt_tokens": 6, "total_tokens": 6 }
        })))
        .mount(&w.h.upstream)
        .await;
    let embed = |body: &'static str| {
        let w = &w;
        let a = &a;
        async move {
            post_to(
                &w.h.app,
                "/v1/embeddings",
                &[("authorization", &format!("Bearer {}", a.key))],
                body,
            )
            .await
        }
    };
    let body = r#"{"model":"r","input":["a"]}"#;
    let (s1, _, first) = embed(body).await;
    let (s2, _, second) = embed(body).await;
    assert_eq!((s1, s2), (StatusCode::OK, StatusCode::OK));
    assert_eq!(first, second);
    assert_eq!(w.provider_calls().await, 1);
    let v: Value = serde_json::from_str(&second).unwrap();
    assert_eq!(v["data"][0]["embedding"], json!([0.5, 0.25]));
    assert_eq!(v["usage"]["prompt_tokens"], 6);
    // Another input or another dimension is another call.
    embed(r#"{"model":"r","input":["b"]}"#).await;
    embed(r#"{"model":"r","input":["a"],"dimensions":2}"#).await;
    assert_eq!(w.provider_calls().await, 3);
    let records = w.h.sink.records();
    assert!(!records[0].cached && records[1].cached);
    assert_eq!(records[1].usage.unwrap().input_tokens, 6);
    assert_eq!(records[1].endpoint, "embeddings");
}

// ---- ids that are reused, and configuration that changes ------------------

/// The reviewer's probe: team A (id 1) has an answer cached; A is deleted;
/// team B is created and gets id 1; B's identical request must reach the
/// provider.
#[tokio::test]
async fn a_new_team_with_the_id_of_a_deleted_team_is_not_given_its_answers() {
    let w = world(CacheScope::Team).await;
    let a = seed_team(&w.h.store, "A", &[]).await;
    let ka = w.key("a", None, Some(a)).await;
    w.chat(&ka, BODY).await;
    assert_eq!(w.provider_calls().await, 1);
    let mut tx = w.h.store.begin().await.unwrap();
    assert!(tx.delete_team(a).await.unwrap());
    tx.commit().await.unwrap();
    let b = seed_team(&w.h.store, "B", &[]).await;
    assert_eq!(a, b, "the database hands the id out again");
    let kb = w.key("b", None, Some(b)).await;
    assert_eq!(w.chat(&kb, BODY).await, StatusCode::OK);
    assert_eq!(w.provider_calls().await, 2, "B was given A's answer");
    // B's own answer is kept.
    w.chat(&kb, BODY).await;
    assert_eq!(w.provider_calls().await, 2);
}

#[tokio::test]
async fn a_new_user_with_the_id_of_a_deleted_user_is_not_given_its_answers() {
    let w = world(CacheScope::User).await;
    let a = seed_user(&w.h.store, "a@example.com", Role::Member, PASSWORD).await;
    let ka = w.key("a", Some(a), None).await;
    w.chat(&ka, BODY).await;
    let mut tx = w.h.store.begin().await.unwrap();
    assert!(tx.delete_user(a).await.unwrap());
    tx.commit().await.unwrap();
    let b = seed_user(&w.h.store, "b@example.com", Role::Member, PASSWORD).await;
    assert_eq!(a, b);
    let kb = w.key("b", Some(b), None).await;
    w.chat(&kb, BODY).await;
    assert_eq!(w.provider_calls().await, 2, "B was given A's answer");
}

#[tokio::test]
async fn a_revoked_key_does_not_hand_its_answers_to_a_new_key() {
    let w = world(CacheScope::Key).await;
    let a = w.key("a", None, None).await;
    w.chat(&a, BODY).await;
    let mut tx = w.h.store.begin().await.unwrap();
    tx.revoke_key(a.id).await.unwrap();
    tx.commit().await.unwrap();
    let b = w.key("b", None, None).await;
    w.chat(&b, BODY).await;
    assert_eq!(w.provider_calls().await, 2);
}

async fn set_cache(w: &World, cache: RouteCache) {
    let route = w.h.store.list_routes().await.unwrap().remove(0);
    let mut tx = w.h.store.begin().await.unwrap();
    tx.set_route_cache(route.id, &cache).await.unwrap();
    tx.commit().await.unwrap();
    w.h.state.refresh().await.unwrap();
}

#[tokio::test]
async fn switching_the_cache_off_and_on_again_does_not_bring_old_answers_back() {
    let w = world(CacheScope::Team).await;
    let (a, _) = w.two_teams().await;
    w.chat(&a, BODY).await;
    let on = RouteCache {
        enabled: true,
        ttl_s: 300,
        scope: CacheScope::Team,
    };
    set_cache(
        &w,
        RouteCache {
            enabled: false,
            ..on
        },
    )
    .await;
    set_cache(&w, on).await;
    w.chat(&a, BODY).await;
    assert_eq!(w.provider_calls().await, 2);
}

#[tokio::test]
async fn a_provider_that_changes_its_address_or_credential_loses_its_answers() {
    for change in ["base_url", "credential"] {
        let w = world(CacheScope::Team).await;
        let (a, _) = w.two_teams().await;
        w.chat(&a, BODY).await;
        let provider = w.h.store.provider_by_name("p").await.unwrap().unwrap();
        let mut tx = w.h.store.begin().await.unwrap();
        if change == "base_url" {
            let url = format!("{}/", w.h.upstream.uri());
            tx.update_provider(provider.id, Some(&url), None)
                .await
                .unwrap();
        } else {
            let cipher = &w.h.state.cipher;
            let c = cipher.encrypt(b"another-secret");
            tx.update_provider(provider.id, None, Some(Some(&c)))
                .await
                .unwrap();
        }
        tx.commit().await.unwrap();
        w.h.state.refresh().await.unwrap();
        let before = w.provider_calls().await;
        w.chat(&a, BODY).await;
        assert_eq!(w.provider_calls().await, before + 1, "{change}");
    }
}

#[tokio::test]
async fn a_refresh_that_changes_nothing_keeps_the_answers() {
    let w = world(CacheScope::Team).await;
    let (a, _) = w.two_teams().await;
    w.chat(&a, BODY).await;
    for _ in 0..3 {
        w.h.state.refresh().await.unwrap();
    }
    w.chat(&a, BODY).await;
    assert_eq!(w.provider_calls().await, 1);
}
