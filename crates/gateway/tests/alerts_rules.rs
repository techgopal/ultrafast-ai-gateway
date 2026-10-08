//! Alert rules and the engine, from the outside: a provider that fails, a
//! breaker that opens, a budget that fills, a configuration that moves.

mod common;

use std::sync::Arc;
use std::time::Duration;

use axum::http::StatusCode;
use common::{
    call, error_code, harness_with_state, post_chat, seed_user, Harness, Signed, ORG_PASSWORD,
};
use serde_json::{json, Value};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use ultrafast_gateway::alerts::engine;
use ultrafast_gateway::alerts::{Deliverer, DeliveryConfig, EngineConfig};
use ultrafast_gateway::budgets::{self, BudgetAction, Period};
use ultrafast_gateway::identity::Role;
use ultrafast_gateway::limits::LimitScope;
use ultrafast_gateway::store::{RouteSettings, TargetsInput};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PROVIDER_KEY: &str = "provider-secret";
const FAIL_FAST: RouteSettings = RouteSettings {
    retries: 0,
    first_token_timeout_ms: 30_000,
    total_timeout_ms: 60_000,
    // High, so that a failing provider does not open the breaker here.
    breaker_failures: 1000,
    breaker_window_s: 60,
    breaker_open_s: 30,
};

struct World {
    h: Harness,
    admin: Signed,
    member: Signed,
    receiver: MockServer,
    channel: i64,
    secret: String,
    model_id: i64,
    stop: watch::Sender<bool>,
    tasks: Vec<JoinHandle<()>>,
}

fn ok_response() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "c1", "model": "m",
        "choices": [{ "message": { "role": "assistant", "content": "hi" }, "finish_reason": "stop" }],
        "usage": { "prompt_tokens": 1, "completion_tokens": 2 }
    }))
}

fn cfg() -> EngineConfig {
    EngineConfig {
        tick: Duration::from_millis(40),
        bucket: Duration::from_millis(200),
        circuit_quiet: Duration::from_millis(100),
    }
}

async fn world() -> World {
    let (stop, stopped) = watch::channel(false);
    let tasks = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (tasks_in, stopped_in) = (tasks.clone(), stopped.clone());
    let h = harness_with_state("openai", move |state| {
        let (deliverer, task) = Deliverer::spawn(
            state.store.clone(),
            state.cipher.clone(),
            state.http.clone(),
            state.metrics.clone(),
            DeliveryConfig {
                retry_delays: vec![Duration::from_millis(50), Duration::from_millis(100)],
                timeout: Duration::from_secs(5),
                shutdown_cap: Duration::from_millis(300),
                queue_capacity: ultrafast_gateway::alerts::QUEUE_CAPACITY,
                max_pending: ultrafast_gateway::alerts::MAX_PENDING,
            },
            stopped_in.clone(),
        );
        let (handle, engine_task) = engine::spawn(
            state.store.clone(),
            Some(deliverer.clone()),
            Some(state.health.clone()),
            cfg(),
            stopped_in,
        );
        state.health.watch(handle.health_sender());
        state.alerts = Some(deliverer);
        state.alert_engine = Some(handle);
        tasks_in.lock().unwrap().extend([task, engine_task]);
    })
    .await;
    let tasks = std::mem::take(&mut *tasks.lock().unwrap());
    let admin_id = seed_user(&h.store, "maya@example.com", Role::Admin, ORG_PASSWORD).await;
    let member_id = seed_user(&h.store, "lena@example.com", Role::Member, ORG_PASSWORD).await;
    let session = |id: i64, s: ultrafast_gateway::store::NewSession| Signed {
        cookie: format!("uf_session={}", s.id),
        csrf: s.csrf_token,
        user_id: id,
    };
    let admin = session(admin_id, h.store.create_session(admin_id).await.unwrap());
    let member = session(member_id, h.store.create_session(member_id).await.unwrap());
    let receiver = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/hook"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&receiver)
        .await;
    let mut w = World {
        model_id: h
            .store
            .list_models()
            .await
            .unwrap()
            .into_iter()
            .find(|m| m.name == "m")
            .expect("the harness has model m")
            .id,
        h,
        admin,
        member,
        receiver,
        channel: 0,
        secret: String::new(),
        stop,
        tasks,
    };
    let (status, body) = w
        .api(
            "POST",
            "/api/alerts/channels",
            Some(json!({ "name": "ops", "kind": "webhook",
                          "url": format!("{}/hook?token=hook-token-123", w.receiver.uri()) })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    w.channel = body["channel"]["id"].as_i64().unwrap();
    w.secret = body["secret"].as_str().unwrap().to_string();
    // The provider fails until a test says otherwise.
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&w.h.upstream)
        .await;
    w
}

impl World {
    async fn api(&self, m: &str, p: &str, body: Option<Value>) -> (StatusCode, Value) {
        let (s, _, b) = call(&self.h.app, m, p, Some(&self.admin), body).await;
        (s, b)
    }

    async fn rule(&self, name: &str, kind: &str, params: Value) -> i64 {
        let (status, body) = self
            .api(
                "POST",
                "/api/alerts/rules",
                Some(json!({ "name": name, "kind": kind, "params": params,
                              "channel_ids": [self.channel] })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        body["id"].as_i64().unwrap()
    }

    /// A route "r" over model m of provider p.
    async fn route(&self, settings: RouteSettings) {
        let mut tx = self.h.store.begin().await.unwrap();
        let id = tx.insert_route("r", &settings, true).await.unwrap();
        tx.replace_targets(
            id,
            &TargetsInput {
                primaries: vec![(self.model_id, 1)],
                fallbacks: vec![],
            },
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        self.h.state.refresh().await.unwrap();
    }

    async fn chat(&self) -> StatusCode {
        self.chat_model("r").await
    }

    async fn chat_model(&self, model: &str) -> StatusCode {
        let body = json!({ "model": model, "messages": [{ "role": "user", "content": "hi" }] });
        post_chat(&self.h.app, Some(&self.h.key), &body.to_string())
            .await
            .0
    }

    async fn events(&self, query: &str) -> Vec<Value> {
        let (status, body) = self
            .api("GET", &format!("/api/alerts/events{query}"), None)
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body["events"].as_array().unwrap().clone()
    }

    /// Waits until an event of this state exists for the rule.
    async fn wait_event(&self, rule: i64, state: &str) -> Value {
        for _ in 0..600 {
            let events = self.events(&format!("?rule_id={rule}&state={state}")).await;
            if let Some(e) = events.first() {
                return e.clone();
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!(
            "no {state} event for rule {rule}: {:?}",
            self.events("").await
        );
    }

    /// Waits until the receiver got `n` deliveries; returns their bodies.
    async fn delivered(&self, n: usize) -> Vec<Value> {
        for _ in 0..600 {
            let got = self.receiver.received_requests().await.unwrap();
            if got.len() >= n {
                return got
                    .iter()
                    .map(|r| serde_json::from_slice(&r.body).unwrap())
                    .collect();
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("fewer than {n} deliveries");
    }

    async fn shutdown(self) {
        let _ = self.stop.send(true);
        for t in self.tasks {
            let _ = t.await;
        }
    }
}

#[tokio::test]
async fn a_failing_route_fires_an_error_rate_alert_and_recovers() {
    let w = world().await;
    w.route(FAIL_FAST).await;
    let rule = w
        .rule(
            "r errors",
            "error_rate",
            // 6 = the calls made: the engine ticks while they are made, and
            // would fire at the fifth under load.
            json!({ "scope": "route", "subject": "r", "percent": 50,
                    "window_minutes": 5, "min_requests": 6 }),
        )
        .await;
    for _ in 0..6 {
        assert!(w.chat().await.as_u16() >= 500);
    }
    let firing = w.wait_event(rule, "firing").await;
    assert_eq!(firing["subject"], "route:r");
    assert_eq!(firing["kind"], "error_rate");
    assert_eq!(firing["details"]["scope"], "route");
    assert_eq!(firing["details"]["requests"], 6);
    assert_eq!(firing["details"]["errors"], 6);
    let sent = w.delivered(1).await;
    assert_eq!(sent[0]["state"], "firing");
    assert_eq!(sent[0]["rule"]["name"], "r errors");
    assert_eq!(sent[0]["subject"], "route:r");
    // The rule shows what it is firing for.
    let (_, rules) = w.api("GET", "/api/alerts/rules", None).await;
    assert_eq!(rules["rules"][0]["firing"][0]["subject"], "route:r");

    // No repeat while it goes on.
    for _ in 0..3 {
        w.chat().await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(w.events("?state=firing").await.len(), 1);

    // The provider recovers; calls go on until the window is quiet.
    w.h.upstream.reset().await;
    Mock::given(method("POST"))
        .respond_with(ok_response())
        .mount(&w.h.upstream)
        .await;
    let mut resolved = None;
    for _ in 0..160 {
        assert_eq!(w.chat().await, StatusCode::OK);
        let events = w.events(&format!("?rule_id={rule}&state=resolved")).await;
        if let Some(e) = events.first() {
            resolved = Some(e.clone());
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let resolved = resolved.expect("the alert resolves after a quiet window");
    assert_eq!(resolved["subject"], "route:r");
    let sent = w.delivered(2).await;
    assert_eq!(sent[1]["state"], "resolved");
    assert_eq!(w.events("?state=firing").await.len(), 1, "one episode");
    let (_, rules) = w.api("GET", "/api/alerts/rules", None).await;
    assert_eq!(rules["rules"][0]["firing"], json!([]));
    w.shutdown().await;
}

#[tokio::test]
async fn a_call_the_gateway_refused_is_not_an_error_but_a_provider_429_is() {
    let w = world().await;
    w.route(FAIL_FAST).await;
    // The provider answers 429: that is an error of the provider.
    w.h.upstream.reset().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429))
        .mount(&w.h.upstream)
        .await;
    let rule = w
        .rule(
            "limited",
            "error_rate",
            json!({ "scope": "provider", "subject": "p", "percent": 50, "min_requests": 3 }),
        )
        .await;
    for _ in 0..3 {
        assert_eq!(w.chat().await, StatusCode::TOO_MANY_REQUESTS);
    }
    let firing = w.wait_event(rule, "firing").await;
    assert_eq!(firing["subject"], "provider:p");
    w.shutdown().await;
}

#[tokio::test]
async fn an_opening_breaker_fires_circuit_open_and_closing_resolves_it() {
    let w = world().await;
    w.route(RouteSettings {
        breaker_failures: 3,
        breaker_open_s: 1,
        ..FAIL_FAST
    })
    .await;
    let rule = w
        .rule("circuit", "circuit_open", json!({ "provider": "p" }))
        .await;
    for _ in 0..3 {
        w.chat().await;
    }
    let firing = w.wait_event(rule, "firing").await;
    assert_eq!(firing["subject"], "target:p/m");
    assert_eq!(firing["details"], json!({ "provider": "p", "model": "m" }));
    assert_eq!(w.delivered(1).await[0]["state"], "firing");
    // After the open time one trial call goes through; it succeeds.
    w.h.upstream.reset().await;
    Mock::given(method("POST"))
        .respond_with(ok_response())
        .mount(&w.h.upstream)
        .await;
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(w.chat().await, StatusCode::OK);
    w.wait_event(rule, "resolved").await;
    assert_eq!(w.events(&format!("?rule_id={rule}")).await.len(), 2);
    w.shutdown().await;
}

#[tokio::test]
async fn a_circuit_episode_whose_target_left_the_catalog_is_resolved() {
    let w = world().await;
    w.route(RouteSettings {
        breaker_failures: 3,
        breaker_open_s: 60,
        ..FAIL_FAST
    })
    .await;
    let rule = w.rule("circuit", "circuit_open", json!({})).await;
    for _ in 0..3 {
        w.chat().await;
    }
    w.wait_event(rule, "firing").await;
    // Still in the catalog: a tick leaves the episode alone.
    w.h.state.alert_engine.as_ref().unwrap().tick().await;
    assert!(w
        .events(&format!("?rule_id={rule}&state=resolved"))
        .await
        .is_empty());

    // The model goes (with the route that used it); its breaker never closes.
    let mut tx = w.h.store.begin().await.unwrap();
    assert!(tx.delete_model(w.model_id).await.unwrap());
    tx.commit().await.unwrap();
    w.h.state.refresh().await.unwrap();
    w.h.state.alert_engine.as_ref().unwrap().tick().await;
    let resolved = w.wait_event(rule, "resolved").await;
    assert_eq!(resolved["summary"], "target removed");
    assert_eq!(resolved["subject"], "target:p/m");
    // An episode is resolved once, however many ticks follow; the state is
    // gone. (The engine's own ticker, every 40 ms here, may run between the
    // delete and the refresh above, while the breaker is still held: it then
    // sees the target open again, opens a second episode, and the tick
    // resolves that one too. Each firing still has exactly one resolved.)
    for _ in 0..3 {
        w.h.state.alert_engine.as_ref().unwrap().tick().await;
    }
    let count = |state: &'static str| {
        let w = &w;
        async move {
            w.events(&format!("?rule_id={rule}&state={state}"))
                .await
                .len()
        }
    };
    let (firing, resolved) = (count("firing").await, count("resolved").await);
    assert!(
        firing >= 1 && resolved == firing,
        "{firing} firing, {resolved} resolved"
    );
    for _ in 0..3 {
        w.h.state.alert_engine.as_ref().unwrap().tick().await;
    }
    assert_eq!(
        (count("firing").await, count("resolved").await),
        (firing, resolved),
        "ticks went on to make events"
    );
    assert!(w.h.store.alert_states().await.unwrap().is_empty());
    w.shutdown().await;
}

#[tokio::test]
async fn a_budget_at_80_percent_fires_a_75_percent_rule_once() {
    let w = world().await;
    let mut tx = w.h.store.begin().await.unwrap();
    tx.upsert_budget(
        LimitScope::Gateway,
        None,
        1_000_000,
        Period::Monthly,
        BudgetAction::Block,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    w.h.state.refresh().await.unwrap();
    let rule = w
        .rule(
            "budget 75",
            "budget",
            json!({ "budget_id": null, "percent": 75 }),
        )
        .await;
    let budgets_of = || w.h.state.snapshot.load().all_budgets();
    let now = time::OffsetDateTime::now_utc();
    w.h.state.budgets.spend(&budgets_of(), 700_000, now);
    budgets::flush(&w.h.state).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(w.events("").await.is_empty(), "70% is under 75%");
    w.h.state.budgets.spend(&budgets_of(), 100_000, now);
    budgets::flush(&w.h.state).await;
    let firing = w.wait_event(rule, "firing").await;
    assert_eq!(firing["details"]["spent_micros"], 800_000);
    assert_eq!(firing["details"]["percent"], 75);
    assert_eq!(
        firing["details"]["period_start"],
        Period::Monthly.start_string(now)
    );
    // The next flush does not say it again.
    w.h.state.budgets.spend(&budgets_of(), 50_000, now);
    budgets::flush(&w.h.state).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(w.events("").await.len(), 1);
    // Nor does the rules being read again (what a restart does).
    w.h.state.alert_engine.as_ref().unwrap().reload().await;
    w.h.state.budgets.spend(&budgets_of(), 10_000, now);
    budgets::flush(&w.h.state).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(w.events("").await.len(), 1);
    assert_eq!(w.delivered(1).await.len(), 1);
    w.shutdown().await;
}

#[tokio::test]
async fn rules_are_validated_listed_changed_and_deleted_and_audited() {
    let w = world().await;
    let (status, body) = w
        .api(
            "POST",
            "/api/alerts/rules",
            Some(
                json!({ "name": "x", "kind": "budget", "params": { "percent": 0 },
                          "channel_ids": [] }),
            ),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["error"]["fields"]["params.percent"],
        "must be from 1 to 100"
    );
    for (bad, field) in [
        (
            json!({ "name": "x", "kind": "latency", "params": {}, "channel_ids": [] }),
            "kind",
        ),
        (
            json!({ "name": "x", "kind": "budget", "params": { "percent": 5, "extra": 1 }, "channel_ids": [] }),
            "params.extra",
        ),
        (
            json!({ "name": "x", "kind": "budget", "params": {}, "channel_ids": [] }),
            "params.percent",
        ),
        (
            json!({ "name": "x", "kind": "error_rate", "params": { "scope": "route", "percent": 5, "window_minutes": 1 }, "channel_ids": [] }),
            "params.window_minutes",
        ),
        (
            json!({ "name": " ", "kind": "budget", "params": { "percent": 5 }, "channel_ids": [] }),
            "name",
        ),
        (
            json!({ "name": "x", "kind": "budget", "params": { "percent": 5 }, "channel_ids": [9999] }),
            "channel_ids",
        ),
        (
            json!({ "name": "x", "kind": "budget", "params": { "percent": 5, "budget_id": 4242 }, "channel_ids": [] }),
            "params.budget_id",
        ),
    ] {
        let (status, body) = w.api("POST", "/api/alerts/rules", Some(bad.clone())).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}: {body}");
        assert!(body["error"]["fields"][field].is_string(), "{bad}: {body}");
    }
    let (status, body) = w
        .api(
            "POST",
            "/api/alerts/rules",
            Some(json!({ "name": "rate", "kind": "error_rate",
                          "params": { "scope": "gateway", "percent": 20 },
                          "channel_ids": [w.channel], "enabled": false })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let id = body["id"].as_i64().unwrap();
    assert_eq!(body["enabled"], false);
    assert_eq!(
        body["params"],
        json!({ "scope": "gateway", "subject": null, "percent": 20,
                "window_minutes": 5, "min_requests": 20 })
    );
    assert_eq!(
        body["channels"],
        json!([{ "id": w.channel, "name": "ops" }])
    );
    assert_eq!(body["firing"], json!([]));
    let (status, dup) = w
        .api(
            "POST",
            "/api/alerts/rules",
            Some(
                json!({ "name": "rate", "kind": "circuit_open", "params": {}, "channel_ids": [] }),
            ),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&dup), "alert_rule_exists");

    let (status, body) = w
        .api(
            "PATCH",
            &format!("/api/alerts/rules/{id}"),
            Some(
                json!({ "name": "rate 2", "enabled": true, "channel_ids": [],
                          "params": { "scope": "route", "percent": 30, "window_minutes": 10 } }),
            ),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["name"], "rate 2");
    assert_eq!(body["enabled"], true);
    assert_eq!(body["channels"], json!([]));
    assert_eq!(body["params"]["window_minutes"], 10);
    for bad in [
        json!({}),
        json!({ "kind": "budget" }),
        json!({ "params": { "scope": "nope", "percent": 3 } }),
    ] {
        let (status, body) = w
            .api(
                "PATCH",
                &format!("/api/alerts/rules/{id}"),
                Some(bad.clone()),
            )
            .await;
        assert!(
            status == StatusCode::BAD_REQUEST || status == StatusCode::UNPROCESSABLE_ENTITY,
            "{bad}: {status} {body}"
        );
    }
    // The channel list shows the rule that uses it.
    let (status, _) = w
        .api(
            "PATCH",
            &format!("/api/alerts/rules/{id}"),
            Some(json!({ "channel_ids": [w.channel] })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (_, channels) = w.api("GET", "/api/alerts/channels", None).await;
    assert_eq!(channels["channels"][0]["rules"][0]["name"], "rate 2");
    let (status, _) = w
        .api(
            "PATCH",
            "/api/alerts/rules/99999",
            Some(json!({ "enabled": true })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = w
        .api("DELETE", &format!("/api/alerts/rules/{id}"), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = w
        .api("DELETE", &format!("/api/alerts/rules/{id}"), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, rules) = w.api("GET", "/api/alerts/rules", None).await;
    assert_eq!(rules["rules"], json!([]));
    let actions: Vec<String> = sqlx_actions(&w.h.store).await;
    for a in [
        "alert_rule.create",
        "alert_rule.update",
        "alert_rule.delete",
    ] {
        assert!(actions.iter().any(|x| x == a), "{a} in {actions:?}");
    }
    w.shutdown().await;
}

async fn sqlx_actions(store: &ultrafast_gateway::store::Store) -> Vec<String> {
    store
        .list_audit(200, None)
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.action)
        .collect()
}

#[tokio::test]
async fn the_events_list_filters_pages_and_refuses_bad_queries() {
    let w = world().await;
    let rule = w.rule("c", "circuit_open", json!({})).await;
    let other = w.rule("d", "circuit_open", json!({})).await;
    let mut tx = w.h.store.begin().await.unwrap();
    for i in 0..5 {
        for (rule_id, name) in [(rule, "c"), (other, "d")] {
            tx.insert_alert_event(ultrafast_gateway::store::NewAlertEvent {
                rule_id: Some(rule_id),
                rule_name: name,
                kind: "circuit_open",
                subject: &format!("target:p/m{i}"),
                state: if i % 2 == 0 { "firing" } else { "resolved" },
                summary: "s",
                details: "{}",
                at: "2999-01-01 00:00:00",
            })
            .await
            .unwrap();
        }
    }
    tx.commit().await.unwrap();
    let all = w.events("").await;
    assert_eq!(all.len(), 10);
    let ids: Vec<i64> = all.iter().map(|e| e["id"].as_i64().unwrap()).collect();
    assert!(ids.windows(2).all(|p| p[0] > p[1]), "newest first");
    assert_eq!(w.events(&format!("?rule_id={rule}")).await.len(), 5);
    assert_eq!(w.events("?state=resolved").await.len(), 4);
    assert_eq!(w.events("?limit=3").await.len(), 3);
    let page2 = w.events(&format!("?limit=4&before_id={}", ids[3])).await;
    assert_eq!(
        page2
            .iter()
            .map(|e| e["id"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        ids[4..8]
    );
    let first = &all[0];
    for key in [
        "id",
        "rule_id",
        "rule_name",
        "kind",
        "subject",
        "state",
        "summary",
        "details",
        "at",
        "deliveries",
    ] {
        assert!(first.get(key).is_some(), "{key}");
    }
    for bad in [
        "?limit=201",
        "?limit=0",
        "?limit=x",
        "?state=bogus",
        "?rule_id=x",
        "?before_id=x",
    ] {
        let (status, _) = w
            .api("GET", &format!("/api/alerts/events{bad}"), None)
            .await;
        assert!(
            status == StatusCode::UNPROCESSABLE_ENTITY || status == StatusCode::BAD_REQUEST,
            "{bad}: {status}"
        );
    }
    w.shutdown().await;
}

#[tokio::test]
async fn only_admins_use_the_rules_and_events_api() {
    let w = world().await;
    let id = w.rule("c", "circuit_open", json!({})).await;
    for (m, p, body) in [
        ("GET", "/api/alerts/rules".to_string(), None),
        (
            "POST",
            "/api/alerts/rules".to_string(),
            Some(json!({ "name": "n", "kind": "circuit_open", "params": {}, "channel_ids": [] })),
        ),
        (
            "PATCH",
            format!("/api/alerts/rules/{id}"),
            Some(json!({ "enabled": false })),
        ),
        ("DELETE", format!("/api/alerts/rules/{id}"), None),
        ("GET", "/api/alerts/events".to_string(), None),
    ] {
        let (status, _, b) = call(&w.h.app, m, &p, Some(&w.member), body.clone()).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{m} {p}: {b}");
        let (status, _, _) = call(&w.h.app, m, &p, None, body).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{m} {p}");
    }
    let (_, rules) = w.api("GET", "/api/alerts/rules", None).await;
    assert_eq!(rules["rules"].as_array().unwrap().len(), 1);
    w.shutdown().await;
}

#[tokio::test]
async fn no_event_delivery_or_audit_row_holds_a_secret() {
    let w = world().await;
    w.route(RouteSettings {
        breaker_failures: 3,
        ..FAIL_FAST
    })
    .await;
    let rule = w.rule("circuit", "circuit_open", json!({})).await;
    let budget_rule = w
        .rule(
            "rate",
            "error_rate",
            json!({ "scope": "key", "percent": 10, "min_requests": 3 }),
        )
        .await;
    for _ in 0..4 {
        w.chat().await;
    }
    w.wait_event(rule, "firing").await;
    w.wait_event(budget_rule, "firing").await;
    let sent = w.delivered(2).await;
    let (_, rules) = w.api("GET", "/api/alerts/rules", None).await;
    let events = w.events("").await;
    let audit = sqlx_actions_all(&w.h.store).await;
    let mut everything = format!("{rules} {sent:?} {audit}");
    for e in &events {
        everything.push_str(&e.to_string());
    }
    // Wait for the deliveries to be recorded on the events too.
    tokio::time::sleep(Duration::from_millis(200)).await;
    for e in w.events("").await {
        everything.push_str(&e.to_string());
    }
    for secret in [
        PROVIDER_KEY,
        w.h.key.as_str(),
        w.secret.as_str(),
        "hook-token-123",
        "/hook?",
    ] {
        assert!(!everything.contains(secret), "{secret} leaked");
    }
    w.shutdown().await;
}

async fn sqlx_actions_all(store: &ultrafast_gateway::store::Store) -> String {
    let rows = store.list_audit(200, None).await.unwrap();
    rows.iter()
        .map(|r| format!("{} {}", r.action, r.summary))
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn the_configuration_carries_rules_and_channel_names_but_no_url() {
    let w = world().await;
    let mut tx = w.h.store.begin().await.unwrap();
    tx.upsert_budget(
        LimitScope::Gateway,
        None,
        5_000_000,
        Period::Weekly,
        BudgetAction::Alert,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    w.h.state.refresh().await.unwrap();
    let budget_id = w.h.store.list_budgets().await.unwrap()[0].id;
    w.rule(
        "budget weekly",
        "budget",
        json!({ "budget_id": budget_id, "percent": 90 }),
    )
    .await;
    w.rule("all budgets", "budget", json!({ "percent": 50 }))
        .await;
    w.rule(
        "errors",
        "error_rate",
        json!({ "scope": "route", "subject": "r", "percent": 5, "min_requests": 2 }),
    )
    .await;
    let (status, export) = w.api("GET", "/api/config/export", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        export["alert_channels"],
        json!([{ "name": "ops", "kind": "webhook" }])
    );
    let rules = export["alert_rules"].as_array().unwrap();
    assert_eq!(rules.len(), 3);
    let weekly = rules.iter().find(|r| r["name"] == "budget weekly").unwrap();
    assert_eq!(
        weekly["params"],
        json!({ "budget": { "scope": "gateway", "name": null, "period": "weekly" }, "percent": 90 })
    );
    assert_eq!(weekly["channels"], json!(["ops"]));
    assert_eq!(weekly["enabled"], true);
    let all = rules.iter().find(|r| r["name"] == "all budgets").unwrap();
    assert_eq!(all["params"], json!({ "budget": null, "percent": 50 }));
    let text = export.to_string();
    let receiver_port = format!("127.0.0.1:{}", w.receiver.address().port());
    for secret in [
        "hook-token-123",
        "/hook",
        w.secret.as_str(),
        "whsec_",
        receiver_port.as_str(),
    ] {
        assert!(!text.contains(secret), "{secret} in the export");
    }

    // A fresh gateway takes the file; its channel has no URL and is off.
    let fresh = world_without_channel().await;
    let mut file = export.clone();
    file["alert_channels"] = json!([{ "name": "ops", "kind": "webhook" }]);
    let (status, _, report) = call(
        &fresh.h.app,
        "POST",
        "/api/config/import?dry_run=false",
        Some(&fresh.admin),
        Some(file.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert!(
        report["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["message"] == "channel 'ops' needs a URL"),
        "{report}"
    );
    let (_, channels) = fresh.api("GET", "/api/alerts/channels", None).await;
    let ops = &channels["channels"][0];
    assert_eq!(ops["name"], "ops");
    assert_eq!(ops["enabled"], false);
    assert_eq!(ops["url_host"], "");
    assert_eq!(ops["rules"].as_array().unwrap().len(), 3);
    let (_, _, second) = call(
        &fresh.h.app,
        "GET",
        "/api/config/export",
        Some(&fresh.admin),
        None,
    )
    .await;
    assert_eq!(second, export, "the round trip is exact");
    // The rule runs against the budget of the new gateway.
    let (_, rules) = fresh.api("GET", "/api/alerts/rules", None).await;
    let new_budget = fresh.h.store.list_budgets().await.unwrap()[0].id;
    let imported = rules["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "budget weekly")
        .unwrap();
    assert_eq!(imported["params"]["budget_id"], new_budget);
    // A second import changes nothing.
    let (_, _, again) = call(
        &fresh.h.app,
        "POST",
        "/api/config/import?dry_run=false",
        Some(&fresh.admin),
        Some(file),
    )
    .await;
    assert_eq!(again["created"], json!([]), "{again}");
    assert_eq!(again["updated"], json!([]), "{again}");
    // A rule naming a channel that is nowhere is refused, with nothing written.
    let mut broken = export.clone();
    broken["alert_rules"][0]["channels"] = json!(["nowhere"]);
    let (status, _, report) = call(
        &fresh.h.app,
        "POST",
        "/api/config/import?dry_run=false",
        Some(&fresh.admin),
        Some(broken),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{report}");
    fresh.shutdown().await;
    w.shutdown().await;
}

#[tokio::test]
async fn an_import_that_disables_a_firing_rule_forgets_its_episodes() {
    let w = world().await;
    let rule = w
        .rule(
            "errors",
            "error_rate",
            json!({ "scope": "route", "subject": "r", "percent": 5, "window_minutes": 60,
                    "min_requests": 2 }),
        )
        .await;
    let mut tx = w.h.store.begin().await.unwrap();
    assert!(tx
        .upsert_alert_state(rule, "route:r", "2999-01-01 00:00:00")
        .await
        .unwrap());
    tx.commit().await.unwrap();
    let (_, mut file) = w.api("GET", "/api/config/export", None).await;
    assert_eq!(file["alert_rules"][0]["enabled"], true);
    // Imported unchanged and enabled, the episode stays.
    let (status, _, report) = call(
        &w.h.app,
        "POST",
        "/api/config/import?dry_run=false",
        Some(&w.admin),
        Some(file.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(w.h.store.alert_states().await.unwrap().len(), 1);
    // Imported with enabled:false, it is forgotten.
    file["alert_rules"][0]["enabled"] = json!(false);
    let (status, _, report) = call(
        &w.h.app,
        "POST",
        "/api/config/import?dry_run=false",
        Some(&w.admin),
        Some(file),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert!(w.h.store.alert_states().await.unwrap().is_empty());
    w.shutdown().await;
}

async fn world_without_channel() -> World {
    let w = world().await;
    let (status, _) = w
        .api(
            "DELETE",
            &format!("/api/alerts/channels/{}", w.channel),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    w
}

#[tokio::test]
async fn an_import_with_bad_alert_entries_is_refused_whole_and_a_dry_run_writes_nothing() {
    let w = world_without_channel().await;
    let import = |file: Value, dry: bool| {
        let app = w.h.app.clone();
        let admin = Signed {
            cookie: w.admin.cookie.clone(),
            csrf: w.admin.csrf.clone(),
            user_id: w.admin.user_id,
        };
        async move {
            let (status, _, body) = call(
                &app,
                "POST",
                &format!("/api/config/import?dry_run={dry}"),
                Some(&admin),
                Some(file),
            )
            .await;
            (status, body)
        }
    };
    let base = |channels: Value, rules: Value| {
        json!({ "format": "ultrafast-config", "version": 1,
                "alert_channels": channels, "alert_rules": rules })
    };
    let rule = |kind: &str, params: Value, channels: Value| json!({ "name": "r", "kind": kind, "params": params, "channels": channels });
    let cases = [
        (
            base(
                json!([]),
                json!([rule("circuit_open", json!({}), json!(["ghost"]))]),
            ),
            "alert_rules[0].channels[0]",
        ),
        (
            base(
                json!([]),
                json!([rule(
                    "budget",
                    json!({ "budget": null, "percent": 0 }),
                    json!([])
                )]),
            ),
            "alert_rules[0].params.percent",
        ),
        (
            base(
                json!([]),
                json!([rule(
                    "budget",
                    json!({ "percent": 5, "budget_id": 1 }),
                    json!([])
                )]),
            ),
            "alert_rules[0].params",
        ),
        (
            base(
                json!([]),
                json!([rule(
                    "budget",
                    json!({ "budget": { "scope": "gateway", "name": null, "period": "daily" }, "percent": 5 }),
                    json!([])
                )]),
            ),
            "alert_rules[0].params.budget",
        ),
        (
            base(
                json!([]),
                json!([rule(
                    "budget",
                    json!({ "budget": { "scope": "key", "name": "k", "period": "daily" }, "percent": 5 }),
                    json!([])
                )]),
            ),
            "alert_rules[0].params.budget.scope",
        ),
        (
            base(json!([]), json!([rule("latency", json!({}), json!([]))])),
            "alert_rules[0].params",
        ),
        (
            base(json!([{ "name": "x", "kind": "email" }]), json!([])),
            "alert_channels[0].kind",
        ),
        (
            base(
                json!([{ "name": "x", "kind": "slack" }, { "name": "x", "kind": "slack" }]),
                json!([]),
            ),
            "alert_channels[1]",
        ),
        (
            base(
                json!([]),
                json!([
                    rule("circuit_open", json!({}), json!([])),
                    rule("circuit_open", json!({}), json!([]))
                ]),
            ),
            "alert_rules[1]",
        ),
    ];
    for (file, at) in cases {
        let (status, report) = import(file.clone(), false).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{file}: {report}");
        let ats: Vec<&str> = report["errors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["at"].as_str().unwrap())
            .collect();
        assert!(ats.contains(&at), "{at} in {ats:?} for {file}");
        assert_eq!(report["created"], json!([]));
    }
    // A good file in a dry run says what it would do, with the warning, and writes nothing.
    let good = base(
        json!([{ "name": "pager", "kind": "slack" }]),
        json!([rule(
            "circuit_open",
            json!({ "provider": "p" }),
            json!(["pager"])
        )]),
    );
    let (status, report) = import(good.clone(), true).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["created"].as_array().unwrap().len(), 2);
    assert_eq!(
        report["warnings"][0]["message"],
        "channel 'pager' needs a URL"
    );
    assert_eq!(report["warnings"][0]["at"], "alert_channels[0]");
    let (_, channels) = w.api("GET", "/api/alerts/channels", None).await;
    assert_eq!(channels["channels"], json!([]));
    // Written, the channel cannot be switched on before it has a URL.
    let (status, _) = import(good, false).await;
    assert_eq!(status, StatusCode::OK);
    let (_, channels) = w.api("GET", "/api/alerts/channels", None).await;
    let id = channels["channels"][0]["id"].as_i64().unwrap();
    let (status, body) = w
        .api(
            "PATCH",
            &format!("/api/alerts/channels/{id}"),
            Some(json!({ "enabled": true })),
        )
        .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["error"]["fields"]["enabled"].is_string());
    let (status, _) = w
        .api(
            "PATCH",
            &format!("/api/alerts/channels/{id}"),
            Some(json!({ "enabled": true, "url": "https://hooks.example.com/x" })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    // A rule of the same name with another kind is refused.
    let (status, report) = import(
        base(
            json!([]),
            json!([rule(
                "error_rate",
                json!({ "scope": "gateway", "percent": 5 }),
                json!([])
            )]),
        ),
        false,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{report}");
    assert_eq!(report["errors"][0]["at"], "alert_rules[0].kind");
    w.shutdown().await;
}

fn windows(w: &World) -> &Arc<ultrafast_gateway::alerts::errors_window::ErrorWindows> {
    w.h.state.alert_engine.as_ref().unwrap().windows()
}

#[tokio::test]
async fn names_a_caller_makes_up_are_never_subjects_of_an_error_window() {
    use ultrafast_gateway::alerts::errors_window::Scope;
    let w = world().await;
    w.route(FAIL_FAST).await;
    w.rule(
        "any route",
        "error_rate",
        json!({ "scope": "route", "percent": 50, "min_requests": 2 }),
    )
    .await;
    for i in 0..300 {
        let status = w.chat_model(&format!("junk-{i}")).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
    let _ = w.chat_model("nope/model").await;
    // A direct call counts for the gateway, its provider and its key, not for a route.
    let _ = w.chat_model("p/m").await;
    // A real route is tracked alongside.
    for _ in 0..3 {
        assert!(w.chat().await.as_u16() >= 500);
    }
    let windows = windows(&w);
    let now = windows.current();
    assert_eq!(windows.tracked_in(Scope::Route), 1, "only the route r");
    assert_eq!(windows.total_of(Scope::Route, "r", now, 5).requests, 3);
    assert_eq!(windows.tracked_in(Scope::Provider), 1);
    assert_eq!(windows.total_of(Scope::Provider, "p", now, 5).requests, 4);
    assert_eq!(windows.total_of(Scope::Gateway, "", now, 5).requests, 4);
    assert_eq!(windows.tracked_in(Scope::Key), 1);
    w.shutdown().await;
}

#[tokio::test]
async fn calls_are_counted_only_while_an_enabled_error_rate_rule_exists() {
    use ultrafast_gateway::alerts::errors_window::Scope;
    let w = world().await;
    // Counting starts on and the engine turns it off once it has read the
    // rules (no error-rate rule yet); a slower database takes longer to answer.
    for _ in 0..200 {
        if !windows(&w).is_active() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    w.route(FAIL_FAST).await;
    for _ in 0..3 {
        w.chat().await;
    }
    let windows = windows(&w);
    assert!(!windows.is_active());
    assert_eq!(
        windows
            .total_of(Scope::Gateway, "", windows.current(), 5)
            .requests,
        0
    );
    assert_eq!(windows.tracked(), 0);
    // Other kinds of rule do not ask for it.
    w.rule("circuit", "circuit_open", json!({})).await;
    assert!(!windows.is_active());
    let rule = w
        .rule(
            "rate",
            "error_rate",
            json!({ "scope": "gateway", "percent": 50 }),
        )
        .await;
    assert!(windows.is_active());
    w.chat().await;
    assert_eq!(
        windows
            .total_of(Scope::Gateway, "", windows.current(), 5)
            .requests,
        1
    );
    // Switched off, it is forgotten.
    let (status, _) = w
        .api(
            "PATCH",
            &format!("/api/alerts/rules/{rule}"),
            Some(json!({ "enabled": false })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!windows.is_active());
    assert_eq!(windows.tracked_in(Scope::Gateway), 0);
    w.shutdown().await;
}

#[tokio::test]
async fn disabling_a_rule_forgets_its_episodes_and_enabling_it_starts_fresh() {
    let w = world().await;
    w.route(RouteSettings {
        breaker_failures: 3,
        breaker_open_s: 1,
        ..FAIL_FAST
    })
    .await;
    let rule = w.rule("circuit", "circuit_open", json!({})).await;
    for _ in 0..3 {
        w.chat().await;
    }
    w.wait_event(rule, "firing").await;
    let (_, rules) = w.api("GET", "/api/alerts/rules", None).await;
    assert_eq!(rules["rules"][0]["firing"].as_array().unwrap().len(), 1);
    let path = format!("/api/alerts/rules/{rule}");
    let (status, body) = w
        .api("PATCH", &path, Some(json!({ "enabled": false })))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["firing"], json!([]), "forgotten at once");
    // Silently: no resolved notice.
    assert_eq!(w.events(&format!("?rule_id={rule}")).await.len(), 1);
    // The breaker closes while the rule is off; nobody hears of it.
    w.h.upstream.reset().await;
    Mock::given(method("POST"))
        .respond_with(ok_response())
        .mount(&w.h.upstream)
        .await;
    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert_eq!(w.chat().await, StatusCode::OK);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(w.events(&format!("?rule_id={rule}")).await.len(), 1);
    // Switched on, the next opening fires.
    let (status, _) = w
        .api("PATCH", &path, Some(json!({ "enabled": true })))
        .await;
    assert_eq!(status, StatusCode::OK);
    w.h.upstream.reset().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&w.h.upstream)
        .await;
    for _ in 0..3 {
        w.chat().await;
    }
    for _ in 0..200 {
        if w.events(&format!("?rule_id={rule}&state=firing"))
            .await
            .len()
            == 2
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(
        w.events(&format!("?rule_id={rule}&state=firing"))
            .await
            .len(),
        2
    );
    w.shutdown().await;
}
