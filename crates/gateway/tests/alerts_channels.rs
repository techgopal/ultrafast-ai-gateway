//! Alert channels: the admin API, the signature, the retries, and a dead
//! host that must not hold up the others.

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::http::StatusCode;
use common::{call, error_code, seed_user, Signed, ORG_PASSWORD};
use hmac::{Hmac, KeyInit, Mac};
use serde_json::{json, Value};
use sha2::Sha256;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use ultrafast_gateway::alerts::{Deliverer, DeliveryConfig};
use ultrafast_gateway::app::{router, AppState};
use ultrafast_gateway::identity::password::warm_up;
use ultrafast_gateway::identity::Role;
use ultrafast_gateway::secrets::Cipher;
use ultrafast_gateway::store::{NewAlertEvent, Store};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

struct Env {
    app: axum::Router,
    store: Store,
    state: Arc<AppState>,
    admin: Signed,
    member: Signed,
    stop: watch::Sender<bool>,
    task: Option<JoinHandle<()>>,
}

fn fast() -> DeliveryConfig {
    DeliveryConfig {
        retry_delays: vec![Duration::from_millis(50), Duration::from_millis(100)],
        timeout: Duration::from_secs(5),
        shutdown_cap: Duration::from_millis(300),
    }
}

async fn env(cfg: DeliveryConfig) -> Env {
    warm_up().unwrap();
    let store = Store::open_in_memory().await.unwrap();
    let cipher = Cipher::from_hex(&Cipher::generate_master_hex()).unwrap();
    let mut state = AppState::new(store.clone(), cipher.clone()).await.unwrap();
    state.cookie_secure = false;
    let (stop, stopped) = watch::channel(false);
    let (deliverer, task) = Deliverer::spawn(
        store.clone(),
        cipher,
        state.http.clone(),
        state.metrics.clone(),
        cfg,
        stopped,
    );
    state.alerts = Some(deliverer);
    let state = Arc::new(state);
    let app = router(state.clone());
    let admin_id = seed_user(&store, "maya@example.com", Role::Admin, ORG_PASSWORD).await;
    let member_id = seed_user(&store, "lena@example.com", Role::Member, ORG_PASSWORD).await;
    let session = |id: i64, s: ultrafast_gateway::store::NewSession| Signed {
        cookie: format!("uf_session={}", s.id),
        csrf: s.csrf_token,
        user_id: id,
    };
    let admin = session(admin_id, store.create_session(admin_id).await.unwrap());
    let member = session(member_id, store.create_session(member_id).await.unwrap());
    Env {
        app,
        store,
        state,
        admin,
        member,
        stop,
        task: Some(task),
    }
}

impl Env {
    async fn call(&self, m: &str, p: &str, body: Option<Value>) -> (StatusCode, Value) {
        let (s, _, b) = call(&self.app, m, p, Some(&self.admin), body).await;
        (s, b)
    }

    async fn create(&self, name: &str, kind: &str, url: &str) -> (i64, String) {
        let (status, body) = self
            .call(
                "POST",
                "/api/alerts/channels",
                Some(json!({ "name": name, "kind": kind, "url": url })),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        (
            body["channel"]["id"].as_i64().unwrap(),
            body["secret"].as_str().unwrap().to_string(),
        )
    }

    async fn event(&self, summary: &str) -> i64 {
        let mut tx = self.store.begin().await.unwrap();
        let id = tx
            .insert_alert_event(NewAlertEvent {
                rule_id: None,
                rule_name: "High errors",
                kind: "error_rate",
                subject: "route:chat",
                state: "firing",
                summary,
                details: r#"{"rate":0.4}"#,
                at: "2999-01-01 00:00:00",
            })
            .await
            .unwrap();
        tx.commit().await.unwrap();
        id
    }

    fn deliverer(&self) -> &Deliverer {
        self.state.alerts.as_ref().unwrap()
    }

    /// The deliveries of an event, once every channel finished.
    async fn deliveries(&self, event: i64) -> Vec<Value> {
        for _ in 0..400 {
            let e = self.store.alert_event(event).await.unwrap().unwrap();
            if e.deliveries != "[]" {
                return serde_json::from_str(&e.deliveries).unwrap();
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("the deliveries of event {event} were never recorded");
    }

    fn metrics(&self) -> String {
        self.state.metrics.render(&[])
    }
}

fn counter(text: &str, result: &str) -> u64 {
    let prefix = format!("uf_alert_deliveries_total{{result=\"{result}\"}} ");
    text.lines()
        .find_map(|l| l.strip_prefix(&prefix))
        .unwrap_or_else(|| panic!("no counter for {result} in\n{text}"))
        .parse()
        .unwrap()
}

fn signature_of(request: &Request) -> (i64, String) {
    let header = request
        .headers
        .get("x-uf-signature")
        .expect("the delivery is signed")
        .to_str()
        .unwrap();
    let (t, v1) = header.split_once(",v1=").expect("t=..,v1=..");
    (
        t.strip_prefix("t=").unwrap().parse().unwrap(),
        v1.to_string(),
    )
}

/// Checks the signature the way a receiver would.
fn verify(request: &Request, secret: &str) {
    let (t, v1) = signature_of(request);
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(format!("{t}.").as_bytes());
    mac.update(&request.body);
    assert_eq!(hex::encode(mac.finalize().into_bytes()), v1);
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    assert!((now - t).abs() < 120, "t={t} now={now}");
}

async fn receiver(status: u16) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/hook"))
        .respond_with(ResponseTemplate::new(status))
        .mount(&server)
        .await;
    server
}

fn hook(server: &MockServer) -> String {
    format!("{}/hook?token=abc123", server.uri())
}

#[tokio::test]
async fn channels_are_created_listed_changed_and_deleted() {
    let env = env(fast()).await;
    let server = receiver(200).await;
    let (status, body) = env
        .call(
            "POST",
            "/api/alerts/channels",
            Some(json!({ "name": "ops", "kind": "webhook", "url": hook(&server) })),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let channel = &body["channel"];
    assert_eq!(channel["name"], "ops");
    assert_eq!(channel["kind"], "webhook");
    assert_eq!(channel["url_host"], server.uri());
    assert_eq!(channel["enabled"], true);
    assert_eq!(channel["rules"], json!([]));
    assert!(body["secret"].as_str().unwrap().starts_with("whsec_"));
    let id = channel["id"].as_i64().unwrap();

    let (status, list) = env.call("GET", "/api/alerts/channels", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["channels"].as_array().unwrap().len(), 1);
    assert_eq!(list["channels"][0]["id"], id);

    // A duplicate name.
    let (status, body) = env
        .call(
            "POST",
            "/api/alerts/channels",
            Some(json!({ "name": "ops", "kind": "slack", "url": hook(&server) })),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(error_code(&body), "alert_channel_exists");

    // Bad input is named by field.
    for (bad, field) in [
        (
            json!({ "name": "x", "kind": "webhook", "url": "ftp://h/x" }),
            "url",
        ),
        (
            json!({ "name": "x", "kind": "webhook", "url": "https://u:p@h/x" }),
            "url",
        ),
        (
            json!({ "name": "x", "kind": "webhook", "url": "https://h/x#f" }),
            "url",
        ),
        (
            json!({ "name": "x", "kind": "webhook", "url": "https://h /x" }),
            "url",
        ),
        (
            json!({ "name": "x", "kind": "webhook", "url": "https:///x" }),
            "url",
        ),
        (
            json!({ "name": "x", "kind": "email", "url": "https://h/x" }),
            "kind",
        ),
        (
            json!({ "name": " ", "kind": "slack", "url": "https://h/x" }),
            "name",
        ),
    ] {
        let (status, body) = env.call("POST", "/api/alerts/channels", Some(bad)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert!(
            body["error"]["fields"][field].is_string(),
            "{field}: {body}"
        );
    }

    let (status, body) = env
        .call(
            "PATCH",
            &format!("/api/alerts/channels/{id}"),
            Some(json!({ "name": "ops2", "url": "https://hooks.example.com:8443/T/x", "enabled": false })),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["name"], "ops2");
    assert_eq!(body["url_host"], "https://hooks.example.com:8443");
    assert_eq!(body["enabled"], false);

    let (status, _) = env
        .call(
            "PATCH",
            &format!("/api/alerts/channels/{id}"),
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = env
        .call(
            "PATCH",
            "/api/alerts/channels/999",
            Some(json!({ "name": "z" })),
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = env
        .call("DELETE", &format!("/api/alerts/channels/{id}"), None)
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = env
        .call("DELETE", &format!("/api/alerts/channels/{id}"), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, list) = env.call("GET", "/api/alerts/channels", None).await;
    assert_eq!(list["channels"], json!([]));

    let mut actions: Vec<String> = env
        .store
        .list_audit(50, None)
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.action)
        .filter(|a| a.starts_with("alert_channel."))
        .collect();
    actions.reverse();
    assert_eq!(
        actions,
        [
            "alert_channel.create",
            "alert_channel.update",
            "alert_channel.delete"
        ]
    );
}

#[tokio::test]
async fn only_an_admin_manages_channels() {
    let env = env(fast()).await;
    let (id, _) = env
        .create("ops", "webhook", "https://h.example.com/x")
        .await;
    for (m, p, body) in [
        ("GET", "/api/alerts/channels".to_string(), None),
        (
            "POST",
            "/api/alerts/channels".to_string(),
            Some(json!({ "name": "n", "kind": "slack", "url": "https://h/x" })),
        ),
        (
            "PATCH",
            format!("/api/alerts/channels/{id}"),
            Some(json!({ "enabled": false })),
        ),
        ("DELETE", format!("/api/alerts/channels/{id}"), None),
        (
            "POST",
            format!("/api/alerts/channels/{id}/rotate-secret"),
            None,
        ),
        ("POST", format!("/api/alerts/channels/{id}/test"), None),
    ] {
        let (status, _, b) = call(&env.app, m, &p, Some(&env.member), body.clone()).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{m} {p}: {b}");
        let (status, _, _) = call(&env.app, m, &p, None, body).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{m} {p}");
    }
    assert_eq!(env.store.list_alert_channels().await.unwrap().len(), 1);
}

#[tokio::test]
async fn the_secret_and_the_url_are_shown_once_and_nowhere_else() {
    let env = env(fast()).await;
    let server = receiver(200).await;
    let url = hook(&server);
    let (id, secret) = env.create("ops", "webhook", &url).await;
    let (_, list) = env.call("GET", "/api/alerts/channels", None).await;
    let (_, patched) = env
        .call(
            "PATCH",
            &format!("/api/alerts/channels/{id}"),
            Some(json!({ "name": "ops-renamed" })),
        )
        .await;
    let (status, tested) = env
        .call("POST", &format!("/api/alerts/channels/{id}/test"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    let rotated_status = env
        .call(
            "POST",
            &format!("/api/alerts/channels/{id}/rotate-secret"),
            None,
        )
        .await;
    assert_eq!(rotated_status.0, StatusCode::OK);
    let new_secret = rotated_status.1["secret"].as_str().unwrap().to_string();
    assert!(new_secret.starts_with("whsec_"));
    assert_ne!(new_secret, secret, "rotation makes a new secret");
    let (_, list_after) = env.call("GET", "/api/alerts/channels", None).await;

    let audit = format!("{:?}", env.store.list_audit(200, None).await.unwrap());
    let events = format!("{:?}", env.store.alert_events(50).await.unwrap());
    let rows = format!("{:?}", env.store.list_alert_channels().await.unwrap());
    let token_path = "/hook?token=abc123";
    let everything = [
        ("list", list.to_string()),
        ("patch", patched.to_string()),
        ("test", tested.to_string()),
        ("list after rotate", list_after.to_string()),
        ("audit", audit),
        ("events", events),
        ("rows", rows),
        ("metrics", env.metrics()),
    ];
    for (what, text) in &everything {
        for needle in [
            secret.as_str(),
            new_secret.as_str(),
            token_path,
            "abc123",
            &url,
        ] {
            assert!(!text.contains(needle), "{what} shows {needle}");
        }
    }
    // The received event carries the new signature, never a secret itself.
    let received = server.received_requests().await.unwrap();
    let body = String::from_utf8_lossy(&received[0].body).to_string();
    assert!(!body.contains(&secret) && !body.contains("abc123"));
    // And a rotation took effect: the next test is signed with the new one.
    let (_, tested) = env
        .call("POST", &format!("/api/alerts/channels/{id}/test"), None)
        .await;
    assert_eq!(tested["ok"], true);
    let received = server.received_requests().await.unwrap();
    verify(&received[1], &new_secret);
}

#[tokio::test]
async fn the_test_delivery_is_signed_and_stored_as_a_test_event() {
    let env = env(fast()).await;
    let server = receiver(200).await;
    let (id, secret) = env.create("ops", "webhook", &hook(&server)).await;
    let (status, body) = env
        .call("POST", &format!("/api/alerts/channels/{id}/test"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({ "ok": true, "status": 200, "error": null }));

    let received = server.received_requests().await.unwrap();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].url.path(), "/hook");
    assert_eq!(received[0].url.query(), Some("token=abc123"));
    assert_eq!(
        received[0].headers.get("content-type").unwrap(),
        "application/json"
    );
    verify(&received[0], &secret);
    let payload: Value = serde_json::from_slice(&received[0].body).unwrap();
    assert_eq!(payload["version"], 1);
    assert_eq!(payload["state"], "test");
    assert_eq!(payload["rule"]["kind"], "test");
    assert!(payload["id"].as_i64().unwrap() > 0);
    assert!(payload["summary"].is_string() && payload["subject"].is_string());
    assert!(payload["details"].is_object());
    let at = payload["at"].as_str().unwrap();
    assert!(at.ends_with('Z') && at.contains('T'), "{at}");
    assert!(payload["gateway"]
        .as_str()
        .unwrap()
        .starts_with("ultrafast "));

    let events = env.store.alert_events(10).await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].state, "test");
    assert_eq!(events[0].id, payload["id"].as_i64().unwrap());
    let deliveries: Vec<Value> = serde_json::from_str(&events[0].deliveries).unwrap();
    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0]["channel_id"], id);
    assert_eq!(deliveries[0]["channel_name"], "ops");
    assert_eq!(deliveries[0]["ok"], true);
    assert_eq!(deliveries[0]["status"], 200);
    assert_eq!(deliveries[0]["tries"], 1);
    assert!(env
        .store
        .list_audit(50, None)
        .await
        .unwrap()
        .iter()
        .any(|r| r.action == "alert_channel.test"));
}

#[tokio::test]
async fn a_test_makes_one_try_and_says_why_it_failed_without_the_url() {
    let env = env(fast()).await;
    let server = receiver(500).await;
    let (id, _) = env.create("ops", "webhook", &hook(&server)).await;
    let (status, body) = env
        .call("POST", &format!("/api/alerts/channels/{id}/test"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], false);
    assert_eq!(body["status"], 500);
    assert!(body["error"].is_string());
    assert_eq!(server.received_requests().await.unwrap().len(), 1);

    // A host that is not there.
    let dead = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let (id, _) = env
        .create(
            "gone",
            "webhook",
            &format!("http://127.0.0.1:{dead}/secret-path"),
        )
        .await;
    let (status, body) = env
        .call("POST", &format!("/api/alerts/channels/{id}/test"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["ok"], false);
    assert_eq!(body["status"], Value::Null);
    let error = body["error"].as_str().unwrap();
    assert!(
        !error.contains("secret-path") && !error.contains("127.0.0.1"),
        "{error}"
    );
    let (status, _) = env
        .call("POST", "/api/alerts/channels/999/test", None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_slack_channel_gets_one_line_of_text_signed_too() {
    let env = env(fast()).await;
    let server = receiver(200).await;
    let (id, secret) = env.create("chat", "slack", &hook(&server)).await;
    let event = env.event("Error rate is 40% on route chat").await;
    env.deliverer().offer(event, vec![id]);
    let deliveries = env.deliveries(event).await;
    assert_eq!(deliveries[0]["ok"], true);
    let received = server.received_requests().await.unwrap();
    verify(&received[0], &secret);
    let payload: Value = serde_json::from_slice(&received[0].body).unwrap();
    assert_eq!(payload.as_object().unwrap().len(), 1);
    let text = payload["text"].as_str().unwrap();
    assert!(text.contains("Error rate is 40% on route chat"), "{text}");
    assert!(!text.contains('\n') && text.is_ascii(), "{text}");
}

#[tokio::test]
async fn a_firing_event_is_delivered_with_its_rule_and_details() {
    let env = env(fast()).await;
    let server = receiver(204).await;
    let (id, secret) = env.create("ops", "webhook", &hook(&server)).await;
    let event = env.event("Error rate is 40%").await;
    env.deliverer().offer(event, vec![id]);
    let deliveries = env.deliveries(event).await;
    assert_eq!(deliveries[0]["status"], 204);
    assert_eq!(deliveries[0]["tries"], 1);
    let received = server.received_requests().await.unwrap();
    verify(&received[0], &secret);
    let p: Value = serde_json::from_slice(&received[0].body).unwrap();
    assert_eq!(p["id"], event);
    assert_eq!(p["state"], "firing");
    assert_eq!(
        p["rule"],
        json!({ "id": null, "name": "High errors", "kind": "error_rate" })
    );
    assert_eq!(p["subject"], "route:chat");
    assert_eq!(p["details"], json!({ "rate": 0.4 }));
    assert_eq!(p["at"], "2999-01-01T00:00:00Z");
    assert_eq!(counter(&env.metrics(), "ok"), 1);
    assert_eq!(counter(&env.metrics(), "failed"), 0);
}

#[tokio::test]
async fn a_failing_receiver_is_tried_three_times_and_then_it_is_a_failure() {
    let env = env(fast()).await;
    // First two answers are 500, then it recovers.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .up_to_n_times(2)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    let (id, _) = env.create("flaky", "webhook", &hook(&server)).await;
    let event = env.event("x").await;
    env.deliverer().offer(event, vec![id]);
    let d = env.deliveries(event).await;
    assert_eq!(
        (
            d[0]["ok"].clone(),
            d[0]["tries"].clone(),
            d[0]["status"].clone()
        ),
        (json!(true), json!(3), json!(200))
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 3);
    assert_eq!(counter(&env.metrics(), "ok"), 1);

    // One that never recovers stops after three tries.
    let down = receiver(500).await;
    let (id, _) = env.create("down", "webhook", &hook(&down)).await;
    let event = env.event("y").await;
    env.deliverer().offer(event, vec![id]);
    let d = env.deliveries(event).await;
    assert_eq!(
        (
            d[0]["ok"].clone(),
            d[0]["tries"].clone(),
            d[0]["status"].clone()
        ),
        (json!(false), json!(3), json!(500))
    );
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(down.received_requests().await.unwrap().len(), 3);
    assert_eq!(counter(&env.metrics(), "failed"), 1);

    // Each try is signed afresh and verifies.
    let (_, secret) = (0, {
        let c = env.store.alert_channel_by_id(id).await.unwrap().unwrap();
        String::from_utf8(env.state.cipher.decrypt(&c.secret_enc).unwrap()).unwrap()
    });
    for r in down.received_requests().await.unwrap() {
        verify(&r, &secret);
    }
}

#[tokio::test]
async fn the_retry_schedule_is_now_then_5_then_30_seconds() {
    let d = DeliveryConfig::default();
    assert_eq!(
        d.retry_delays,
        [Duration::from_secs(5), Duration::from_secs(30)]
    );
    assert_eq!(d.timeout, Duration::from_secs(10));
    assert_eq!(d.shutdown_cap, Duration::from_secs(5));
}

#[tokio::test]
async fn a_dead_host_does_not_hold_up_a_healthy_one() {
    let env = env(DeliveryConfig {
        retry_delays: vec![Duration::from_millis(400), Duration::from_millis(400)],
        timeout: Duration::from_millis(400),
        shutdown_cap: Duration::from_millis(300),
    })
    .await;
    // Answers only after the timeout.
    let dead = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(30)))
        .mount(&dead)
        .await;
    let healthy = receiver(200).await;
    let (dead_id, _) = env.create("a-dead", "webhook", &hook(&dead)).await;
    let (ok_id, _) = env.create("b-healthy", "webhook", &hook(&healthy)).await;
    let event = env.event("x").await;
    let started = Instant::now();
    env.deliverer().offer(event, vec![dead_id, ok_id]);

    // The healthy channel gets its delivery within the first timeout of the
    // dead one...
    loop {
        if !healthy.received_requests().await.unwrap().is_empty() {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_millis(350),
            "healthy waited"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    // ... while the dead one is still on its tries.
    let e = env.store.alert_event(event).await.unwrap().unwrap();
    assert_eq!(e.deliveries, "[]");
    // Both are recorded together once the dead one gave up.
    let d = env.deliveries(event).await;
    assert!(started.elapsed() > Duration::from_millis(1200));
    assert_eq!(d.len(), 2);
    assert_eq!(
        (
            d[0]["channel_id"].clone(),
            d[0]["ok"].clone(),
            d[0]["tries"].clone()
        ),
        (json!(dead_id), json!(false), json!(3))
    );
    assert!(d[0]["error"].is_string());
    assert_eq!(
        (
            d[1]["channel_id"].clone(),
            d[1]["ok"].clone(),
            d[1]["tries"].clone()
        ),
        (json!(ok_id), json!(true), json!(1))
    );
    assert_eq!(healthy.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn a_disabled_channel_is_not_called_and_says_so() {
    let env = env(fast()).await;
    let server = receiver(200).await;
    let (id, _) = env.create("off", "webhook", &hook(&server)).await;
    let (status, _) = env
        .call(
            "PATCH",
            &format!("/api/alerts/channels/{id}"),
            Some(json!({ "enabled": false })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let event = env.event("x").await;
    env.deliverer().offer(event, vec![id]);
    let d = env.deliveries(event).await;
    assert_eq!(
        (d[0]["ok"].clone(), d[0]["tries"].clone()),
        (json!(false), json!(0))
    );
    assert_eq!(d[0]["error"], "the channel is disabled");
    assert!(server.received_requests().await.unwrap().is_empty());
    // A test of a disabled channel still goes out.
    let (_, body) = env
        .call("POST", &format!("/api/alerts/channels/{id}/test"), None)
        .await;
    assert_eq!(body["ok"], true);
}

#[tokio::test]
async fn a_full_queue_drops_and_counts_and_never_blocks() {
    let env = env(DeliveryConfig {
        retry_delays: vec![],
        timeout: Duration::from_secs(60),
        shutdown_cap: Duration::from_millis(200),
    })
    .await;
    let hung = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(60)))
        .mount(&hung)
        .await;
    let (id, _) = env.create("hung", "webhook", &hook(&hung)).await;
    let event = env.event("x").await;
    let started = Instant::now();
    for _ in 0..3000 {
        env.deliverer().offer(event, vec![id]);
    }
    assert!(started.elapsed() < Duration::from_secs(2), "offer blocked");
    let dropped = counter(&env.metrics(), "dropped");
    assert!(dropped > 0, "nothing was dropped");
    assert!(env.deliverer().queued() <= ultrafast_gateway::alerts::QUEUE_CAPACITY);
}

#[tokio::test]
async fn shutdown_ends_the_deliverer_even_with_a_delivery_in_flight() {
    let mut env = env(DeliveryConfig {
        retry_delays: vec![],
        timeout: Duration::from_secs(60),
        shutdown_cap: Duration::from_millis(200),
    })
    .await;
    let hung = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(60)))
        .mount(&hung)
        .await;
    let (id, _) = env.create("hung", "webhook", &hook(&hung)).await;
    let event = env.event("x").await;
    env.deliverer().offer(event, vec![id]);
    // Let the request start.
    for _ in 0..100 {
        if !hung.received_requests().await.unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    env.stop.send(true).unwrap();
    let task = env.task.take().unwrap();
    tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .expect("the deliverer ended within its cap")
        .unwrap();
}
