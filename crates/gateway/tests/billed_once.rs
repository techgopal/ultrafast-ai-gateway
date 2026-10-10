//! A call that is paid for once (an image) is never repeated once it was
//! sent: the three ways it can end after the send, each with one upstream
//! connection, a 504 that says so, the attempt recorded as fatal and the
//! fallback as not tried.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::http::StatusCode;
use common::{allow_model, harness_with_state, post_to, Harness};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use ultrafast_gateway::store::{RouteSettings, TargetsInput};
use ultrafast_gateway::telemetry::AttemptOutcome;
use wiremock::matchers::method;
use wiremock::{Mock, ResponseTemplate};

fn slow_floor_off(s: &mut ultrafast_gateway::app::AppState) {
    s.slow_calls = ultrafast_gateway::app::SlowCalls {
        first_byte: Duration::ZERO,
        total: Duration::ZERO,
    };
}

async fn generate(h: &Harness) -> (StatusCode, Value) {
    let bearer = format!("Bearer {}", h.key);
    let (s, _, text) = post_to(
        &h.app,
        "/v1/images/generations",
        &[("authorization", &bearer)],
        r#"{"model":"r","prompt":"x"}"#,
    )
    .await;
    (
        s,
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}: {text}")),
    )
}

/// A route "r" over two models of `provider`.
async fn route_of_two(h: &Harness, provider: &str, first_token_ms: i64, total_ms: i64) {
    let a = allow_model(&h.store, provider, "img-a").await;
    let b = allow_model(&h.store, provider, "img-b").await;
    let mut tx = h.store.begin().await.unwrap();
    let id = tx
        .insert_route(
            "r",
            &RouteSettings {
                retries: 2,
                first_token_timeout_ms: first_token_ms,
                total_timeout_ms: total_ms,
                breaker_failures: 50,
                breaker_window_s: 60,
                breaker_open_s: 30,
            },
            true,
        )
        .await
        .unwrap();
    tx.replace_targets(
        id,
        &TargetsInput {
            primaries: vec![(a, 1)],
            fallbacks: vec![b],
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    h.state.refresh().await.unwrap();
}

/// A provider "q" at a raw socket server; `after_request` runs on every
/// connection once its request has been read.
async fn raw_provider<F, Fut>(h: &Harness, after_request: F) -> Arc<AtomicUsize>
where
    F: Fn(tokio::net::TcpStream) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let uri = format!("http://{}", listener.local_addr().unwrap());
    h.store
        .insert_provider("q", "openai", &uri, None)
        .await
        .unwrap();
    let connections = Arc::new(AtomicUsize::new(0));
    let counted = connections.clone();
    let after_request = Arc::new(after_request);
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            counted.fetch_add(1, Ordering::SeqCst);
            let after_request = after_request.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let mut seen = Vec::new();
                loop {
                    let Ok(n) = sock.read(&mut buf).await else {
                        return;
                    };
                    if n == 0 {
                        return;
                    }
                    seen.extend_from_slice(&buf[..n]);
                    if let Some(end) = seen.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&seen[..end]).to_ascii_lowercase();
                        let len: usize = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .map(|v| v.trim().parse().unwrap())
                            .unwrap_or(0);
                        if seen.len() >= end + 4 + len {
                            break;
                        }
                    }
                }
                after_request(sock).await;
            });
        }
    });
    connections
}

async fn assert_not_repeated(h: &Harness, s: StatusCode, v: &Value, connections: usize) {
    assert_eq!(s, StatusCode::GATEWAY_TIMEOUT, "{v}");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("not repeated"),
        "{v}"
    );
    assert_eq!(connections, 1, "the call was repeated");
    let r = &h.sink.wait_for(1).await[0];
    let seen: Vec<_> = r
        .attempts
        .iter()
        .map(|a| (a.model.as_str(), a.outcome))
        .collect();
    assert_eq!(
        seen,
        [
            ("img-a", AttemptOutcome::Fatal),
            ("img-b", AttemptOutcome::Skipped)
        ]
    );
}

// The route allows the first byte longer than the whole request: the request
// runs out of time after the send. The caller is told it may be billed, not
// that no provider could serve it.
#[tokio::test]
async fn running_out_of_time_after_the_send_is_a_504_and_not_repeated() {
    let h = harness_with_state("openai", slow_floor_off).await;
    route_of_two(&h, "p", 5_000, 500).await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(3)))
        .mount(&h.upstream)
        .await;
    let (s, v) = generate(&h).await;
    let sent = h.upstream.received_requests().await.unwrap().len();
    assert_not_repeated(&h, s, &v, sent).await;
}

// The connection breaks after the provider took the request.
#[tokio::test]
async fn a_connection_that_breaks_after_the_send_is_a_504_and_not_repeated() {
    let h = harness_with_state("openai", slow_floor_off).await;
    let connections = raw_provider(&h, |sock| async move { drop(sock) }).await;
    route_of_two(&h, "q", 5_000, 300_000).await;
    let (s, v) = generate(&h).await;
    assert_not_repeated(&h, s, &v, connections.load(Ordering::SeqCst)).await;
}

// The answer's headers arrive and its body breaks off.
#[tokio::test]
async fn a_body_that_breaks_off_is_a_504_and_not_repeated() {
    let h = harness_with_state("openai", slow_floor_off).await;
    let connections = raw_provider(&h, |mut sock| async move {
        let _ = sock
            .write_all(b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 1000\r\n\r\n{\"data\"")
            .await;
        drop(sock);
    })
    .await;
    route_of_two(&h, "q", 5_000, 300_000).await;
    let (s, v) = generate(&h).await;
    assert_not_repeated(&h, s, &v, connections.load(Ordering::SeqCst)).await;
}

// The answer's headers arrive and its body stalls.
#[tokio::test]
async fn a_body_that_stalls_is_a_504_and_not_repeated() {
    let h = harness_with_state("openai", slow_floor_off).await;
    let connections = raw_provider(&h, |mut sock| async move {
        let _ = sock
            .write_all(b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 1000\r\n\r\n{\"data\"")
            .await;
        tokio::time::sleep(Duration::from_secs(30)).await;
        drop(sock);
    })
    .await;
    // The idle wait of the body is the route's first-token timeout.
    route_of_two(&h, "q", 400, 300_000).await;
    let (s, v) = generate(&h).await;
    assert_not_repeated(&h, s, &v, connections.load(Ordering::SeqCst)).await;
}
