//! Rate limits on `/v1`: each limit at each scope, the strictest wins, the
//! refusal in both shapes, and the permit held for the whole call.

mod common;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::{
    allow_model, hanging_upstream, harness, harness_with_rate, post_to, seed_team, seed_user,
    Harness,
};
use futures::StreamExt;
use serde_json::{json, Value};
use tower::ServiceExt;
use ultrafast_gateway::identity::{Role, TeamRole};
use ultrafast_gateway::limits::{
    LimitScope, Limiter, MemoryLimiter, Permit, RateLimit, Refusal, Subjects,
};
use ultrafast_gateway::secrets::generate_key;
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

const PASSWORD: &str = "correct horse battery";

/// A user in a team with a key, on top of the harness.
struct World {
    h: Harness,
    user: i64,
    team: i64,
    key_id: i64,
    key: String,
}

async fn world() -> World {
    world_on(harness("openai").await).await
}

async fn world_on(h: Harness) -> World {
    let user = seed_user(&h.store, "lena@example.com", Role::Member, PASSWORD).await;
    let team = seed_team(&h.store, "Platform", &[(user, TeamRole::Member)]).await;
    let key = generate_key();
    let mut tx = h.store.begin().await.unwrap();
    let key_id = tx
        .insert_key("ci", &key.hash, &key.display, None, Some(user), Some(team))
        .await
        .unwrap();
    tx.commit().await.unwrap();
    h.state.refresh().await.unwrap();
    World {
        h,
        user,
        team,
        key_id,
        key: key.full,
    }
}

impl World {
    async fn limit(
        &self,
        scope: LimitScope,
        rpm: Option<u64>,
        tpm: Option<u64>,
        conc: Option<u64>,
    ) {
        let scope_id = match scope {
            LimitScope::Gateway => None,
            LimitScope::Team => Some(self.team),
            LimitScope::User => Some(self.user),
            LimitScope::Key => Some(self.key_id),
        };
        let mut tx = self.h.store.begin().await.unwrap();
        tx.upsert_limit(
            scope,
            scope_id,
            &RateLimit {
                requests_per_minute: rpm,
                tokens_per_minute: tpm,
                concurrent: conc,
            },
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        self.h.state.refresh().await.unwrap();
    }

    async fn chat(&self, body: &str) -> (StatusCode, axum::http::HeaderMap, String) {
        let bearer = format!("Bearer {}", self.key);
        post_to(
            &self.h.app,
            "/v1/chat/completions",
            &[("authorization", &bearer)],
            body,
        )
        .await
    }
}

const BODY: &str =
    r#"{"model":"p/gpt-4o","max_tokens":10,"messages":[{"role":"user","content":"hi"}]}"#;

fn ok(prompt: u64, completion: u64) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "c1", "model": "gpt-4o",
        "choices": [{ "message": { "role": "assistant", "content": "hello" }, "finish_reason": "stop" }],
        "usage": { "prompt_tokens": prompt, "completion_tokens": completion }
    }))
}

async fn upstream(w: &World, template: ResponseTemplate) {
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(template)
        .mount(&w.h.upstream)
        .await;
}

fn message(body: &str) -> String {
    let v: Value = serde_json::from_str(body).unwrap();
    v["error"]["message"].as_str().unwrap().to_string()
}

const SCOPES: [(LimitScope, &str); 4] = [
    (LimitScope::Gateway, "gateway"),
    (LimitScope::Team, "team 'Platform'"),
    (LimitScope::User, "user 'lena@example.com'"),
    (LimitScope::Key, "key 'ci'"),
];

#[tokio::test]
async fn requests_per_minute_at_every_scope() {
    for (scope, label) in SCOPES {
        let w = world().await;
        upstream(&w, ok(1, 2)).await;
        w.limit(scope, Some(1), None, None).await;
        let (status, _, _) = w.chat(BODY).await;
        assert_eq!(status, StatusCode::OK, "{label}");
        let (status, headers, body) = w.chat(BODY).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{label}");
        assert_eq!(
            message(&body),
            format!("rate limit 'requests per minute' of {label} reached")
        );
        let wait: u64 = headers["retry-after"].to_str().unwrap().parse().unwrap();
        assert!((1..=60).contains(&wait), "{label}: {wait}");
        // The refused call never reached the provider.
        assert_eq!(w.h.upstream.received_requests().await.unwrap().len(), 1);
    }
}

#[tokio::test]
async fn tokens_per_minute_at_every_scope() {
    for (scope, label) in SCOPES {
        let w = world().await;
        // 110 tokens used against a limit of 100.
        upstream(&w, ok(90, 20)).await;
        w.limit(scope, None, Some(100), None).await;
        let (status, _, _) = w.chat(BODY).await;
        assert_eq!(status, StatusCode::OK, "{label}");
        let (status, headers, body) = w.chat(BODY).await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{label}");
        assert_eq!(
            message(&body),
            format!("rate limit 'tokens per minute' of {label} reached")
        );
        assert!(headers.contains_key("retry-after"));
    }
}

#[tokio::test]
async fn concurrent_requests_at_every_scope() {
    for (scope, label) in SCOPES {
        let w = world().await;
        upstream(&w, ok(1, 2).set_delay(Duration::from_millis(400))).await;
        w.limit(scope, None, None, Some(1)).await;
        let slow = w.chat(BODY);
        let refused = async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            w.chat(BODY).await
        };
        let ((first, _, _), (second, _, body)) = tokio::join!(slow, refused);
        assert_eq!(first, StatusCode::OK, "{label}");
        assert_eq!(second, StatusCode::TOO_MANY_REQUESTS, "{label}");
        assert_eq!(
            message(&body),
            format!("rate limit 'concurrent requests' of {label} reached")
        );
        // The slot is free again.
        let (third, _, _) = w.chat(BODY).await;
        assert_eq!(third, StatusCode::OK, "{label}");
    }
}

#[tokio::test]
async fn the_strictest_limit_wins() {
    let w = world().await;
    upstream(&w, ok(1, 2)).await;
    w.limit(LimitScope::Key, Some(5), None, None).await;
    w.limit(LimitScope::User, Some(4), None, None).await;
    w.limit(LimitScope::Team, Some(2), None, None).await;
    w.limit(LimitScope::Gateway, Some(3), None, None).await;
    assert_eq!(w.chat(BODY).await.0, StatusCode::OK);
    assert_eq!(w.chat(BODY).await.0, StatusCode::OK);
    let (status, _, body) = w.chat(BODY).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert!(
        message(&body).ends_with("of team 'Platform' reached"),
        "{body}"
    );
}

#[tokio::test]
async fn the_refusal_is_in_the_shape_of_the_endpoint() {
    let w = world().await;
    upstream(&w, ok(1, 2)).await;
    w.limit(LimitScope::Key, Some(1), None, None).await;
    assert_eq!(w.chat(BODY).await.0, StatusCode::OK);

    let (status, headers, body) = w.chat(BODY).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert!(
        headers["retry-after"]
            .to_str()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            >= 1
    );
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"]["type"], "rate_limit_error");
    assert_eq!(
        v["error"]["message"],
        "rate limit 'requests per minute' of key 'ci' reached"
    );

    let bearer = format!("Bearer {}", w.key);
    let (status, headers, body) = post_to(
        &w.h.app,
        "/v1/messages",
        &[
            ("authorization", &bearer),
            ("anthropic-version", "2023-06-01"),
        ],
        r#"{"model":"p/gpt-4o","max_tokens":10,"messages":[{"role":"user","content":"hi"}]}"#,
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert!(
        headers["retry-after"]
            .to_str()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            >= 1
    );
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["type"], "error");
    assert_eq!(v["error"]["type"], "rate_limit_error");
    assert_eq!(
        v["error"]["message"],
        "rate limit 'requests per minute' of key 'ci' reached"
    );
}

#[tokio::test]
async fn embeddings_are_limited_too() {
    let w = world().await;
    Mock::given(method("POST"))
        .and(path("/embeddings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list", "model": "m",
            "data": [{ "object": "embedding", "index": 0, "embedding": [0.5] }],
            "usage": { "prompt_tokens": 1, "total_tokens": 1 }
        })))
        .mount(&w.h.upstream)
        .await;
    w.limit(LimitScope::Key, Some(1), None, None).await;
    let bearer = format!("Bearer {}", w.key);
    let headers = [("authorization", bearer.as_str())];
    let send = || {
        post_to(
            &w.h.app,
            "/v1/embeddings",
            &headers,
            r#"{"model":"p/m","input":["a"]}"#,
        )
    };
    assert_eq!(send().await.0, StatusCode::OK);
    let (status, headers, body) = send().await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert!(headers.contains_key("retry-after"));
    assert!(body.contains("rate_limit_error"));
}

#[tokio::test]
async fn a_refused_call_is_recorded_and_costs_nothing() {
    let w = world().await;
    upstream(&w, ok(1, 2)).await;
    w.limit(LimitScope::Key, Some(1), None, None).await;
    w.chat(BODY).await;
    w.chat(BODY).await;
    let records = w.h.sink.wait_for(2).await;
    assert_eq!(records[0].status, 200);
    assert_eq!(records[1].status, 429);
    assert!(records[1].usage.is_none());
    assert!(records[1].attempts.is_empty());
}

/// Review Focus 2.
#[tokio::test]
async fn a_burst_against_a_concurrency_limit_of_two() {
    let w = world().await;
    upstream(&w, ok(1, 2).set_delay(Duration::from_millis(800))).await;
    w.limit(LimitScope::Key, None, None, Some(2)).await;
    let started = Instant::now();
    let handles: Vec<_> = (0..20)
        .map(|_| {
            let app = w.h.app.clone();
            let bearer = format!("Bearer {}", w.key);
            tokio::spawn(async move {
                let (status, _, _) = post_to(
                    &app,
                    "/v1/chat/completions",
                    &[("authorization", &bearer)],
                    BODY,
                )
                .await;
                (status, started.elapsed())
            })
        })
        .collect();
    let mut results = Vec::new();
    for h in handles {
        results.push(h.await.unwrap());
    }
    let ran = results.iter().filter(|(s, _)| *s == StatusCode::OK).count();
    let refused: Vec<_> = results
        .iter()
        .filter(|(s, _)| *s == StatusCode::TOO_MANY_REQUESTS)
        .collect();
    assert_eq!(ran, 2);
    assert_eq!(refused.len(), 18);
    // At once: well before the slow upstream answers.
    assert!(
        refused
            .iter()
            .all(|(_, took)| *took < Duration::from_millis(700)),
        "{refused:?}"
    );
    assert_eq!(w.h.upstream.received_requests().await.unwrap().len(), 2);
    // Both slots are free again.
    let (a, b) = tokio::join!(w.chat(BODY), w.chat(BODY));
    assert_eq!((a.0, b.0), (StatusCode::OK, StatusCode::OK));
}

#[tokio::test]
async fn the_slot_is_released_after_an_upstream_failure() {
    let w = world().await;
    upstream(&w, ResponseTemplate::new(500)).await;
    w.limit(LimitScope::Key, None, None, Some(1)).await;
    for _ in 0..3 {
        let (status, _, _) = w.chat(BODY).await;
        assert_ne!(status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    }
}

#[tokio::test]
async fn the_slot_is_released_when_the_caller_drops_a_whole_call() {
    let w = world().await;
    upstream(&w, ok(1, 2).set_delay(Duration::from_millis(500))).await;
    w.limit(LimitScope::Key, None, None, Some(1)).await;
    // The caller gives up while the provider is thinking.
    let gone = tokio::time::timeout(Duration::from_millis(100), w.chat(BODY)).await;
    assert!(gone.is_err(), "the call should still have been running");
    let (status, _, _) = w.chat(BODY).await;
    assert_eq!(status, StatusCode::OK);
}

fn stream_request(uri_key: &str, model: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("authorization", format!("Bearer {uri_key}"))
        .body(Body::from(format!(
            r#"{{"model":"{model}","stream":true,"messages":[{{"role":"user","content":"hi"}}]}}"#
        )))
        .unwrap()
}

#[tokio::test]
async fn a_stream_holds_its_slot_until_it_ends_or_the_caller_drops() {
    let w = world().await;
    let (uri, closed) = hanging_upstream().await;
    w.h.store
        .insert_provider("hang", "openai", &uri, None)
        .await
        .unwrap();
    allow_model(&w.h.store, "hang", "m").await;
    w.limit(LimitScope::Key, None, None, Some(1)).await;
    upstream(&w, ok(1, 2)).await;

    let resp =
        w.h.app
            .clone()
            .oneshot(stream_request(&w.key, "hang/m"))
            .await
            .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let mut stream = resp.into_body().into_data_stream();
    tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    // While the stream runs the slot is taken.
    let (status, _, _) = w.chat(BODY).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);

    drop(stream);
    tokio::time::timeout(Duration::from_secs(5), closed)
        .await
        .unwrap()
        .unwrap();
    let (status, _, _) = w.chat(BODY).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn a_finished_stream_releases_its_slot() {
    let w = world().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            concat!(
                "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"},\"finish_reason\":null}]}\n\n",
                "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":3}}\n\n",
                "data: [DONE]\n\n",
            ),
            "text/event-stream",
        ))
        .mount(&w.h.upstream)
        .await;
    w.limit(LimitScope::Key, None, None, Some(1)).await;
    for _ in 0..3 {
        let resp =
            w.h.app
                .clone()
                .oneshot(stream_request(&w.key, "p/gpt-4o"))
                .await
                .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&bytes).contains("[DONE]"));
    }
}

#[tokio::test]
async fn tokens_are_corrected_from_the_usage_of_the_call() {
    let w = world().await;
    // Each call is estimated at about 501 tokens and uses 3.
    upstream(&w, ok(1, 2)).await;
    w.limit(LimitScope::Key, None, Some(1000), None).await;
    let big =
        r#"{"model":"p/gpt-4o","max_tokens":500,"messages":[{"role":"user","content":"hi"}]}"#;
    for _ in 0..5 {
        assert_eq!(w.chat(big).await.0, StatusCode::OK);
    }
}

#[tokio::test]
async fn a_failed_call_gives_its_estimate_back() {
    let w = world().await;
    upstream(&w, ResponseTemplate::new(500)).await;
    w.limit(LimitScope::Key, None, Some(600), None).await;
    let big =
        r#"{"model":"p/gpt-4o","max_tokens":500,"messages":[{"role":"user","content":"hi"}]}"#;
    for _ in 0..3 {
        assert_eq!(w.chat(big).await.0, StatusCode::SERVICE_UNAVAILABLE);
    }
}

#[tokio::test]
async fn a_missing_max_tokens_is_estimated_at_a_thousand() {
    let w = world().await;
    upstream(&w, ok(1, 2).set_delay(Duration::from_millis(400))).await;
    // Room for one estimate of 1 000 + the input, not for two.
    w.limit(LimitScope::Key, None, Some(1100), None).await;
    let body = r#"{"model":"p/gpt-4o","messages":[{"role":"user","content":"hi"}]}"#;
    let first = w.chat(body);
    let second = async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        w.chat(body).await
    };
    let (a, b) = tokio::join!(first, second);
    assert_eq!(a.0, StatusCode::OK);
    assert_eq!(b.0, StatusCode::TOO_MANY_REQUESTS);
    assert!(b.2.contains("tokens per minute"));
    // Settled at 3 tokens, the next call fits.
    assert_eq!(w.chat(body).await.0, StatusCode::OK);
}

#[tokio::test]
async fn no_limit_means_no_change() {
    let w = world().await;
    upstream(&w, ok(1, 2)).await;
    for _ in 0..5 {
        assert_eq!(w.chat(BODY).await.0, StatusCode::OK);
    }
}

#[tokio::test]
async fn a_limit_written_to_the_store_applies_after_the_snapshot_refresh() {
    let w = world().await;
    upstream(&w, ok(1, 2)).await;
    assert_eq!(w.chat(BODY).await.0, StatusCode::OK);
    let mut tx = w.h.store.begin().await.unwrap();
    tx.upsert_limit(
        LimitScope::Key,
        Some(w.key_id),
        &RateLimit {
            requests_per_minute: Some(1),
            tokens_per_minute: None,
            concurrent: None,
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    // `/v1` reads the snapshot: no refresh, no limit.
    assert_eq!(w.chat(BODY).await.0, StatusCode::OK);
    w.h.state.refresh().await.unwrap();
    // The two calls above were not counted: the limit was not there yet.
    assert_eq!(w.chat(BODY).await.0, StatusCode::OK);
    assert_eq!(w.chat(BODY).await.0, StatusCode::TOO_MANY_REQUESTS);
}

/// A limiter whose clock the test sets: every call is counted at
/// `base + at` seconds, whatever the time is.
struct SetClock {
    inner: MemoryLimiter,
    base: Instant,
    at: AtomicU64,
}

impl Limiter for SetClock {
    fn acquire(&self, who: &Subjects, estimate: u64, _now: Instant) -> Result<Permit, Refusal> {
        let at = Duration::from_secs(self.at.load(Ordering::SeqCst));
        self.inner.acquire(who, estimate, self.base + at)
    }
}

#[tokio::test]
async fn retry_after_is_the_exact_wait_for_the_window_to_empty() {
    let inner = MemoryLimiter::new();
    let clock = Arc::new(SetClock {
        inner,
        base: Instant::now() + Duration::from_secs(1),
        at: AtomicU64::new(0),
    });
    let w = world_on(harness_with_rate("openai", clock.clone()).await).await;
    upstream(&w, ok(1, 2)).await;
    w.limit(LimitScope::Key, Some(1), None, None).await;
    // The one request is counted at second 0; at second 20 it leaves the
    // window at second 60: 40 seconds to wait.
    assert_eq!(w.chat(BODY).await.0, StatusCode::OK);
    clock.at.store(20, Ordering::SeqCst);
    let (status, headers, _) = w.chat(BODY).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(headers["retry-after"], "40");
    // At second 59 one second is left; at 60 the call goes through.
    clock.at.store(59, Ordering::SeqCst);
    let (_, headers, _) = w.chat(BODY).await;
    assert_eq!(headers["retry-after"], "1");
    clock.at.store(60, Ordering::SeqCst);
    assert_eq!(w.chat(BODY).await.0, StatusCode::OK);
}

/// A key of a team throttles by the team's limit whoever owns it: its owner
/// need not be a member, and it need not have an owner.
#[tokio::test]
async fn a_team_key_is_throttled_by_its_teams_limit_without_membership() {
    for owned in [true, false] {
        let w = world().await;
        let outsider = seed_user(&w.h.store, "omar@example.com", Role::Member, PASSWORD).await;
        let key = generate_key();
        let mut tx = w.h.store.begin().await.unwrap();
        tx.insert_key(
            "team-key",
            &key.hash,
            &key.display,
            None,
            owned.then_some(outsider),
            Some(w.team),
        )
        .await
        .unwrap();
        tx.commit().await.unwrap();
        w.limit(LimitScope::Team, Some(1), None, None).await;
        upstream(&w, ok(1, 2)).await;
        let bearer = format!("Bearer {}", key.full);
        let headers = [("authorization", bearer.as_str())];
        let call = || post_to(&w.h.app, "/v1/chat/completions", &headers, BODY);
        assert_eq!(call().await.0, StatusCode::OK, "owned: {owned}");
        let (status, _, body) = call().await;
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "owned: {owned}");
        assert_eq!(
            message(&body),
            "rate limit 'requests per minute' of team 'Platform' reached"
        );
    }
}
