//! `POST /api/playground/chat`: a signed-in user's call through the shared
//! `/v1` pipeline, as a key they own would make it.

mod common;

use std::sync::Arc;

use axum::http::StatusCode;
use common::{call, org_with_sink, post_to, raw, MemorySink, Org, Signed};
use serde_json::{json, Value};
use time::OffsetDateTime;
use ultrafast_gateway::identity::UserStatus;
use ultrafast_gateway::limits::{LimitScope, RateLimit};
use ultrafast_gateway::secrets::generate_key;
use ultrafast_gateway::store::{Grants, RouteSettings, TargetsInput};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

struct World {
    org: Org,
    sink: Arc<MemorySink>,
    upstream: MockServer,
}

const USERS: [&str; 5] = ["maya", "arjun", "lena", "tomas", "priya"];

fn ok() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "c1", "model": "open",
        "choices": [{ "message": { "role": "assistant", "content": "hello" }, "finish_reason": "stop" }],
        "usage": { "prompt_tokens": 1, "completion_tokens": 2 }
    }))
}

async fn world() -> World {
    let sink = Arc::new(MemorySink::default());
    let org = org_with_sink(Some(sink.clone())).await;
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ok())
        .mount(&upstream)
        .await;
    let store = &org.api.store;
    let provider = store
        .insert_provider("p", "openai", &upstream.uri(), None)
        .await
        .unwrap();
    let mut tx = store.begin().await.unwrap();
    let mut ids = Vec::new();
    // (name, enabled, grants)
    let models = [
        (
            "open",
            true,
            Grants {
                everyone: true,
                ..Grants::default()
            },
        ),
        (
            "research-only",
            true,
            Grants {
                team_ids: vec![org.research],
                ..Grants::default()
            },
        ),
        (
            "tomas-only",
            true,
            Grants {
                user_ids: vec![org.tomas],
                ..Grants::default()
            },
        ),
        (
            "disabled",
            false,
            Grants {
                everyone: true,
                ..Grants::default()
            },
        ),
    ];
    for (name, enabled, grants) in models {
        let id = tx.insert_model(provider, name).await.unwrap();
        assert!(tx.set_model_enabled(id, enabled).await.unwrap());
        tx.replace_grants(id, &grants).await.unwrap();
        ids.push(id);
    }
    let settings = RouteSettings {
        retries: 0,
        first_token_timeout_ms: 30_000,
        total_timeout_ms: 300_000,
        breaker_failures: 5,
        breaker_window_s: 60,
        breaker_open_s: 30,
    };
    let open_route = tx
        .insert_route("open-route", &settings, true)
        .await
        .unwrap();
    tx.replace_targets(
        open_route,
        &TargetsInput {
            primaries: vec![(ids[0], 1)],
            fallbacks: vec![],
        },
    )
    .await
    .unwrap();
    let team_route = tx
        .insert_route("research-route", &settings, false)
        .await
        .unwrap();
    tx.replace_targets(
        team_route,
        &TargetsInput {
            primaries: vec![(ids[1], 1)],
            fallbacks: vec![],
        },
    )
    .await
    .unwrap();
    tx.replace_route_grants(team_route, &[org.research])
        .await
        .unwrap();
    tx.commit().await.unwrap();
    org.api.state.refresh().await.unwrap();
    World {
        org,
        sink,
        upstream,
    }
}

fn chat(model: &str) -> Value {
    json!({ "model": model, "messages": [{ "role": "user", "content": "hi" }] })
}

impl World {
    async fn play(
        &self,
        who: &Signed,
        body: Value,
    ) -> (StatusCode, Vec<(String, String)>, Vec<u8>) {
        raw(&self.org, who, "POST", "/api/playground/chat", Some(body)).await
    }

    /// A key owned by the user, with no team and no allowlist.
    async fn key_of(&self, user: i64) -> String {
        let key = generate_key();
        let mut tx = self.org.api.store.begin().await.unwrap();
        tx.insert_key("mine", &key.hash, &key.display, None, Some(user), None)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        self.org.api.state.refresh().await.unwrap();
        key.full
    }

    async fn with_key(
        &self,
        key: &str,
        body: &Value,
    ) -> (StatusCode, Vec<(String, String)>, Vec<u8>) {
        let bearer = format!("Bearer {key}");
        let (status, headers, text) = post_to(
            &self.org.api.app,
            "/v1/chat/completions",
            &[("authorization", &bearer)],
            &body.to_string(),
        )
        .await;
        let headers = headers
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap().to_string()))
            .collect();
        (status, headers, text.into_bytes())
    }
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

#[tokio::test]
async fn a_playground_call_is_answered_or_refused_as_the_users_key_would_be() {
    let w = world().await;
    let names = [
        "p/open",
        "p/research-only",
        "p/tomas-only",
        "p/disabled",
        "p/nothing",
        "open-route",
        "research-route",
        "no-such-route",
    ];
    let mut seen = std::collections::BTreeSet::new();
    for who in USERS {
        let signed = w.org.sign_in(who).await;
        let key = w.key_of(signed.user_id).await;
        for name in names {
            let body = chat(name);
            let (via_key, _, key_body) = w.with_key(&key, &body).await;
            let (via_playground, _, play_body) = w.play(&signed, body).await;
            assert_eq!(via_playground, via_key, "{who} {name}");
            let kind = |bytes: &[u8]| {
                serde_json::from_slice::<Value>(bytes)
                    .ok()
                    .and_then(|v| v["error"]["type"].as_str().map(str::to_string))
            };
            assert_eq!(kind(&play_body), kind(&key_body), "{who} {name}");
            seen.insert(via_key.as_u16());
        }
    }
    // Answered, refused and unknown all happened.
    assert_eq!(seen, [200, 403, 404].into_iter().collect());

    // Spot checks of the matrix itself.
    let lena = w.org.sign_in("lena").await;
    assert_eq!(w.play(&lena, chat("p/open")).await.0, StatusCode::OK);
    assert_eq!(
        w.play(&lena, chat("p/research-only")).await.0,
        StatusCode::FORBIDDEN
    );
    let tomas = w.org.sign_in("tomas").await;
    assert_eq!(
        w.play(&tomas, chat("p/research-only")).await.0,
        StatusCode::OK
    );
    assert_eq!(w.play(&tomas, chat("p/tomas-only")).await.0, StatusCode::OK);
    let maya = w.org.sign_in("maya").await;
    assert_eq!(
        w.play(&maya, chat("research-route")).await.0,
        StatusCode::OK
    );
    assert_eq!(
        w.play(&maya, chat("p/disabled")).await.0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn a_call_is_logged_to_the_user_with_no_key_and_the_playground_endpoint() {
    let w = world().await;
    let lena = w.org.sign_in("lena").await;
    let (status, _, _) = w.play(&lena, chat("p/open")).await;
    assert_eq!(status, StatusCode::OK);
    let records = w.sink.wait_for(1).await;
    let r = &records[0];
    assert_eq!(r.endpoint, "playground");
    assert_eq!(r.key_id, None);
    assert_eq!(r.user_id, Some(w.org.lena));
    assert_eq!(r.team_id, None);
    assert_eq!(r.requested, "p/open");
    assert_eq!(r.status, 200);
    assert!(!r.stream);
    // A refused call is recorded too.
    let (status, _, _) = w.play(&lena, chat("p/research-only")).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let records = w.sink.wait_for(2).await;
    assert_eq!(records[1].status, 403);
    assert_eq!(records[1].key_id, None);
}

#[tokio::test]
async fn limits_of_the_user_and_their_teams_apply() {
    let w = world().await;
    let mut tx = w.org.api.store.begin().await.unwrap();
    // The user's own limit, and the gateway's.
    tx.upsert_limit(
        LimitScope::User,
        Some(w.org.lena),
        &RateLimit {
            requests_per_minute: Some(1),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    w.org.api.state.refresh().await.unwrap();
    let lena = w.org.sign_in("lena").await;
    assert_eq!(w.play(&lena, chat("p/open")).await.0, StatusCode::OK);
    let (status, headers, body) = w.play(&lena, chat("p/open")).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert!(header(&headers, "retry-after").is_some());
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["error"]["type"], "rate_limit_error");
    // Someone else is not limited by it.
    let tomas = w.org.sign_in("tomas").await;
    assert_eq!(w.play(&tomas, chat("p/open")).await.0, StatusCode::OK);

    // A team limit counts the playground calls of its members (the call
    // has no key team, so all of the owner's teams count).
    let mut tx = w.org.api.store.begin().await.unwrap();
    tx.upsert_limit(
        LimitScope::Team,
        Some(w.org.research),
        &RateLimit {
            requests_per_minute: Some(1),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    w.org.api.state.refresh().await.unwrap();
    let arjun = w.org.sign_in("arjun").await;
    assert_eq!(w.play(&arjun, chat("p/open")).await.0, StatusCode::OK);
    assert_eq!(
        w.play(&arjun, chat("p/open")).await.0,
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[tokio::test]
async fn a_spent_budget_refuses_the_call_as_it_would_a_key() {
    use ultrafast_gateway::budgets::{BudgetAction, Period};
    let w = world().await;
    let mut tx = w.org.api.store.begin().await.unwrap();
    tx.upsert_budget(
        LimitScope::User,
        Some(w.org.lena),
        1_000_000,
        Period::Monthly,
        BudgetAction::Block,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    w.org.api.state.refresh().await.unwrap();
    let lena = w.org.sign_in("lena").await;
    assert_eq!(w.play(&lena, chat("p/open")).await.0, StatusCode::OK);
    // What the log writer does when it prices a call.
    let budgets = w
        .org
        .api
        .state
        .snapshot
        .load()
        .budgets_of(None, Some(w.org.lena), None);
    assert_eq!(budgets.len(), 1);
    w.org
        .api
        .state
        .budgets
        .spend(&budgets, 2_000_000, OffsetDateTime::now_utc());
    let (status, _, body) = w.play(&lena, chat("p/open")).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["error"]["code"], "budget_exceeded");
    // Another user's budget is untouched.
    let tomas = w.org.sign_in("tomas").await;
    assert_eq!(w.play(&tomas, chat("p/open")).await.0, StatusCode::OK);
}

#[tokio::test]
async fn a_stream_is_sent_as_server_sent_events() {
    let w = world().await;
    w.upstream.reset().await;
    let upstream = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"he\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"llo\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":3}}\n\n",
        "data: [DONE]\n\n",
    );
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(upstream, "text/event-stream"))
        .mount(&w.upstream)
        .await;
    let lena = w.org.sign_in("lena").await;
    let mut body = chat("p/open");
    body["stream"] = json!(true);
    body["max_tokens"] = json!(50);
    body["temperature"] = json!(0.2);
    body["top_p"] = json!(0.9);
    body["stop"] = json!(["END"]);
    let (status, headers, bytes) = w.play(&lena, body).await;
    assert_eq!(status, StatusCode::OK);
    assert!(header(&headers, "content-type")
        .unwrap()
        .starts_with("text/event-stream"));
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.contains("\"content\":\"he\""), "{text}");
    assert!(text.contains("\"content\":\"llo\""), "{text}");
    assert!(text.contains("\"total_tokens\":5"), "{text}");
    assert!(text.ends_with("data: [DONE]\n\n"));
    let records = w.sink.wait_for(1).await;
    assert_eq!(records[0].endpoint, "playground");
    assert!(records[0].stream);
    assert_eq!(records[0].key_id, None);
}

#[tokio::test]
async fn a_session_with_the_csrf_token_is_needed() {
    let w = world().await;
    let lena = w.org.sign_in("lena").await;
    let (status, _, body) = call(
        &w.org.api.app,
        "POST",
        "/api/playground/chat",
        Some(&Signed {
            cookie: lena.cookie.clone(),
            csrf: "wrong".into(),
            user_id: lena.user_id,
        }),
        Some(chat("p/open")),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["error"]["code"], "csrf_failed");
    let (status, _, body) = call(
        &w.org.api.app,
        "POST",
        "/api/playground/chat",
        None,
        Some(chat("p/open")),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"]["code"], "unauthenticated");
    // Nothing reached a provider or a record.
    assert!(w.upstream.received_requests().await.unwrap().is_empty());
    assert!(w.sink.records().is_empty());
}

#[tokio::test]
async fn a_disabled_user_is_refused() {
    let w = world().await;
    let lena = w.org.sign_in("lena").await;
    assert_eq!(w.play(&lena, chat("p/open")).await.0, StatusCode::OK);
    let mut tx = w.org.api.store.begin().await.unwrap();
    assert!(tx
        .set_user_status(w.org.lena, UserStatus::Disabled)
        .await
        .unwrap());
    tx.commit().await.unwrap();
    w.org.api.state.refresh().await.unwrap();
    let (status, _, body) = w.play(&lena, chat("p/open")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["error"]["code"], "unauthenticated");
    assert_eq!(w.upstream.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn a_body_that_is_not_a_chat_request_is_refused_in_the_openai_shape() {
    let w = world().await;
    let lena = w.org.sign_in("lena").await;
    let (status, _, body) = w.play(&lena, json!({ "messages": [] })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let body: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["error"]["type"], "invalid_request_error");
}

#[tokio::test]
async fn a_cache_kept_per_key_is_kept_per_user_for_a_call_without_a_key() {
    use ultrafast_gateway::cache::{CacheScope, RouteCache};
    let w = world().await;
    let mut tx = w.org.api.store.begin().await.unwrap();
    let route = tx
        .route_by_id(1)
        .await
        .unwrap()
        .expect("the first route exists");
    assert!(tx
        .set_route_cache(
            route.id,
            &RouteCache {
                enabled: true,
                ttl_s: 60,
                scope: CacheScope::Key,
            },
        )
        .await
        .unwrap());
    tx.commit().await.unwrap();
    w.org.api.state.refresh().await.unwrap();
    let sent = || async { w.upstream.received_requests().await.unwrap().len() };
    let lena = w.org.sign_in("lena").await;
    let tomas = w.org.sign_in("tomas").await;
    assert_eq!(w.play(&lena, chat("open-route")).await.0, StatusCode::OK);
    assert_eq!(w.play(&lena, chat("open-route")).await.0, StatusCode::OK);
    assert_eq!(sent().await, 1, "the same user is answered from the cache");
    assert_eq!(w.play(&tomas, chat("open-route")).await.0, StatusCode::OK);
    assert_eq!(sent().await, 2, "another user is not given lena's answer");
}

/// A call is made with a signed-in user's access token: refused, and
/// nothing reaches a provider or a record.
#[tokio::test]
async fn an_access_token_cannot_call_the_playground() {
    let w = world().await;
    let lena = w.org.sign_in("lena").await;
    let (status, _, body) = call(
        &w.org.api.app,
        "POST",
        "/api/tokens",
        Some(&lena),
        Some(json!({ "name": "ci" })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let secret = body["secret"].as_str().unwrap().to_string();
    let (status, _, body) = common::call_with_token(
        &w.org.api.app,
        "POST",
        "/api/playground/chat",
        &secret,
        Some(chat("p/open")),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert_eq!(body["error"]["code"], "forbidden");
    assert!(w.upstream.received_requests().await.unwrap().is_empty());
    assert!(w.sink.records().is_empty());
    // The same token still works on the routes it is for.
    let (status, _, _) =
        common::call_with_token(&w.org.api.app, "GET", "/api/auth/me", &secret, None).await;
    assert_eq!(status, StatusCode::OK);
}

/// The cost of a playground call reaches the budget of the user, and of
/// their team, through the real log writer.
async fn spend_reaches(scope: LimitScope, who: &str) {
    use std::time::Duration;
    use tokio::sync::watch;
    use ultrafast_gateway::budgets::{self, BudgetAction, Period};
    use ultrafast_gateway::logs::writer::{spawn_accounted, WriterConfig};
    use ultrafast_gateway::logs::{snapshot_prices, LogSink};

    let (sink, rx) = LogSink::channel(64);
    let stats = sink.stats();
    let org = org_with_sink(Some(Arc::new(sink))).await;
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ok())
        .mount(&upstream)
        .await;
    let store = &org.api.store;
    let provider = store
        .insert_provider("p", "openai", &upstream.uri(), None)
        .await
        .unwrap();
    let mut tx = store.begin().await.unwrap();
    let model = tx.insert_model(provider, "open").await.unwrap();
    assert!(tx.set_model_enabled(model, true).await.unwrap());
    tx.replace_grants(
        model,
        &Grants {
            everyone: true,
            ..Grants::default()
        },
    )
    .await
    .unwrap();
    // 1 prompt and 2 completion tokens cost 3 000 micro-dollars.
    tx.set_model_input_price(model, Some(1_000_000_000))
        .await
        .unwrap();
    tx.set_model_output_price(model, Some(1_000_000_000))
        .await
        .unwrap();
    let (scope_id, user) = match scope {
        LimitScope::User => (org.arjun, org.arjun),
        _ => (org.research, org.arjun),
    };
    tx.upsert_budget(
        scope,
        Some(scope_id),
        2_000,
        Period::Monthly,
        BudgetAction::Block,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    org.api.state.refresh().await.unwrap();
    let (stop, stopped) = watch::channel(false);
    let writer = spawn_accounted(
        store.clone(),
        rx,
        snapshot_prices(org.api.state.clone()),
        stats,
        WriterConfig {
            max_batch: 10,
            max_wait: Duration::from_millis(20),
            retry_delay: Duration::from_millis(10),
        },
        stopped,
        budgets::accountant(org.api.state.clone()),
    );
    let signed = org.sign_in(who).await;
    assert_eq!(signed.user_id, user);
    let play = || async {
        raw(
            &org,
            &signed,
            "POST",
            "/api/playground/chat",
            Some(chat("p/open")),
        )
        .await
        .0
    };
    assert_eq!(play().await, StatusCode::OK);
    let mut next = StatusCode::OK;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(30)).await;
        next = play().await;
        if next == StatusCode::TOO_MANY_REQUESTS {
            break;
        }
    }
    assert_eq!(next, StatusCode::TOO_MANY_REQUESTS, "{scope:?} budget");
    // A user outside the scope is not charged.
    if matches!(scope, LimitScope::User) {
        let tomas = org.sign_in("tomas").await;
        let status = raw(
            &org,
            &tomas,
            "POST",
            "/api/playground/chat",
            Some(chat("p/open")),
        )
        .await
        .0;
        assert_eq!(status, StatusCode::OK);
    }
    stop.send(true).unwrap();
    writer.await.unwrap();
}

#[tokio::test]
async fn a_playground_calls_cost_reaches_the_users_budget_through_the_log_writer() {
    spend_reaches(LimitScope::User, "arjun").await;
}

#[tokio::test]
async fn a_playground_calls_cost_reaches_the_teams_budget_through_the_log_writer() {
    spend_reaches(LimitScope::Team, "arjun").await;
}

#[tokio::test]
async fn playground_accepts_tools_and_images() {
    let w = world().await;
    let lena = w.org.sign_in("lena").await;
    let body = json!({
        "model": "p/open",
        "messages": [
            { "role": "user", "content": [
                { "type": "text", "text": "what is this" },
                { "type": "image_url", "image_url": { "url": "data:image/png;base64,iVBORw0KGgo=" } }
            ]},
            { "role": "assistant", "content": null, "tool_calls": [
                { "id": "call_1", "type": "function",
                  "function": { "name": "look", "arguments": "{}" } }
            ]},
            { "role": "tool", "tool_call_id": "call_1", "content": "a cat" }
        ],
        "tools": [{ "type": "function", "function": {
            "name": "look", "parameters": { "type": "object" } } }],
        "tool_choice": "auto",
        "parallel_tool_calls": false
    });
    let (status, _, out) = w.play(&lena, body).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&out));
    let sent = w.upstream.received_requests().await.unwrap();
    assert_eq!(sent.len(), 1);
    let sent: Value = serde_json::from_slice(&sent[0].body).unwrap();
    assert_eq!(sent["tools"][0]["function"]["name"], "look");
    assert_eq!(sent["messages"][0]["content"][1]["type"], "image_url");
    assert_eq!(sent["messages"][2]["tool_call_id"], "call_1");
    // Without the CSRF token the same call is refused before any upstream call.
    let (status, _, _) = post_to(
        &w.org.api.app,
        "/api/playground/chat",
        &[("cookie", &lena.cookie)],
        "{}",
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(w.upstream.received_requests().await.unwrap().len(), 1);
}
