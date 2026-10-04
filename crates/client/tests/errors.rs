mod common;

use std::time::{Duration, Instant};

use futures::StreamExt;

use common::*;
use ultrafast_client::{ChatRequest, Client, EmbeddingsRequest, Error, ErrorKind, Target};

const KEY: &str = "sk-very-secret-key-123";

fn req() -> ChatRequest {
    ChatRequest::new("m").user("hi")
}

async fn chat_error(script: Script, target: impl Fn(&str) -> Target) -> Error {
    let s = serve(script).await;
    Client::new(target(&s.url)).chat(req()).await.unwrap_err()
}

fn gateway(u: &str) -> Target {
    Target::gateway(u, KEY)
}
fn openai(u: &str) -> Target {
    Target::openai_compatible(u, KEY)
}

#[tokio::test]
async fn a_429_with_retry_after_is_rate_limited_on_every_target() {
    let body = r#"{"error":{"message":"slow down","type":"rate_limit_error"}}"#;
    let anth = r#"{"type":"error","error":{"type":"rate_limit_error","message":"slow down"}}"#;
    type Make = Box<dyn Fn(&str) -> Target>;
    let cases: Vec<(&str, Make)> = vec![
        (body, Box::new(gateway)),
        (body, Box::new(openai)),
        (anth, Box::new(|u| Target::anthropic(KEY).with_base_url(u))),
        (body, Box::new(|u| Target::gemini(KEY).with_base_url(u))),
        (body, Box::new(|u| Target::azure(u, KEY))),
    ];
    for (b, t) in cases {
        let e = chat_error(Script::json(429, b).header("Retry-After", "12"), t).await;
        assert_eq!(e.kind, ErrorKind::RateLimited, "{e:?}");
        assert!(e.retryable);
        assert_eq!(e.status, Some(429));
        assert_eq!(e.retry_after, Some(Duration::from_secs(12)));
        assert!(e.message.contains("slow down"));
    }
}

#[tokio::test]
async fn a_429_without_retry_after_has_none() {
    let e = chat_error(Script::json(429, "{}"), gateway).await;
    assert_eq!(e.kind, ErrorKind::RateLimited);
    assert_eq!(e.retry_after, None);
}

#[tokio::test]
async fn a_gateway_budget_429_is_rate_limited_with_its_message() {
    let b = r#"{"error":{"message":"Budget spent.","type":"rate_limit_error","param":null,"code":"budget_exceeded"}}"#;
    let e = chat_error(Script::json(429, b).header("retry-after", "3600"), gateway).await;
    assert_eq!(e.kind, ErrorKind::RateLimited);
    assert_eq!(e.retry_after, Some(Duration::from_secs(3600)));
    assert!(e.message.contains("Budget spent."));
}

#[tokio::test]
async fn statuses_map_to_kinds() {
    for (status, kind, retryable) in [
        (400, ErrorKind::InvalidRequest, false),
        (422, ErrorKind::InvalidRequest, false),
        (401, ErrorKind::Auth, false),
        (403, ErrorKind::Permission, false),
        (404, ErrorKind::NotFound, false),
        (408, ErrorKind::Timeout, true),
        (302, ErrorKind::InvalidRequest, false),
        (500, ErrorKind::Upstream, true),
        (502, ErrorKind::Upstream, true),
        (503, ErrorKind::Upstream, true),
    ] {
        let e = chat_error(
            Script::json(status, r#"{"error":{"message":"nope","type":"x"}}"#),
            gateway,
        )
        .await;
        assert_eq!(e.kind, kind, "status {status}");
        assert_eq!(e.retryable, retryable, "status {status}");
        assert_eq!(e.status, Some(status));
    }
}

#[tokio::test]
async fn a_non_json_error_body_still_gives_a_typed_error() {
    let e = chat_error(Script::json(502, "<html>bad gateway</html>"), openai).await;
    assert_eq!(e.kind, ErrorKind::Upstream);
    assert!(e.retryable);
}

#[tokio::test]
async fn an_unreadable_success_is_malformed() {
    let e = chat_error(Script::json(200, "not json"), openai).await;
    assert_eq!(e.kind, ErrorKind::Malformed);
    assert!(!e.retryable);
}

#[tokio::test]
async fn a_refused_connection_is_a_retryable_network_error() {
    let url = dead_url().await;
    let e = Client::new(gateway(&url)).chat(req()).await.unwrap_err();
    assert_eq!(e.kind, ErrorKind::Network);
    assert!(e.retryable);
    assert_eq!(e.status, None);
    let shown = format!("{e} {e:?}");
    assert!(!shown.contains(KEY), "{shown}");
}

#[tokio::test]
async fn a_server_that_never_answers_times_out() {
    let mut script = Script::json(200, "{}");
    script.hang = true;
    let s = serve(script).await;
    let e = Client::new(gateway(&s.url))
        .with_timeout(Duration::from_millis(150))
        .chat(req())
        .await
        .unwrap_err();
    assert_eq!(e.kind, ErrorKind::Timeout);
    assert!(e.retryable);
}

#[tokio::test]
async fn the_key_is_in_no_error_no_debug_and_no_message() {
    let mut errors: Vec<Error> = Vec::new();
    // Provider messages that echo the credential are scrubbed.
    let echo = format!(r#"{{"error":{{"message":"bad key {KEY}","type":"x"}}}}"#);
    for status in [400, 401, 403, 404, 429, 500] {
        errors.push(chat_error(Script::json(status, &echo), gateway).await);
        errors.push(chat_error(Script::json(status, &echo), openai).await);
    }
    errors.push(chat_error(Script::json(200, &format!("junk {KEY}")), openai).await);
    // Connection failures, with the key in the URL's userinfo-free form.
    let dead = dead_url().await;
    errors.push(Client::new(gateway(&dead)).chat(req()).await.unwrap_err());
    errors.push(
        Client::new(Target::gemini(KEY).with_base_url(&dead))
            .chat(req())
            .await
            .unwrap_err(),
    );
    errors.push(
        Client::new(Target::anthropic(KEY).with_base_url("http://127.0.0.1:1"))
            .embed(EmbeddingsRequest::new("m", ["x"]))
            .await
            .unwrap_err(),
    );
    errors.push(
        Client::new(gateway(&dead))
            .chat(req().tag("k", "x".repeat(2000)))
            .await
            .unwrap_err(),
    );
    let mut script = Script::json(200, "{}");
    script.hang = true;
    let s = serve(script).await;
    errors.push(
        Client::new(gateway(&s.url))
            .with_timeout(Duration::from_millis(100))
            .chat(req())
            .await
            .unwrap_err(),
    );
    assert!(errors.len() > 15);
    for e in &errors {
        let shown = format!("{e} | {e:?} | {e:#?} | {}", e.message);
        assert!(!shown.contains(KEY), "{shown}");
    }
    // The client and its target print no key either.
    let c = Client::new(gateway(&dead));
    assert!(!format!("{c:?}").contains(KEY));
    for t in [
        gateway("http://x"),
        openai("http://x"),
        Target::anthropic(KEY),
        Target::gemini(KEY),
        Target::azure("http://x", KEY),
        Target::openai(KEY),
    ] {
        assert!(!format!("{t:?}").contains(KEY), "{t:?}");
    }
}

#[test]
fn error_displays_kind_and_message() {
    let e = Error::new(ErrorKind::Auth, "bad key");
    assert_eq!(e.to_string(), "auth: bad key");
    assert!(std::error::Error::source(&e).is_none());
}

#[tokio::test]
async fn a_body_over_the_cap_is_a_malformed_error_not_a_read_to_the_end() {
    // Chunked, so no Content-Length warns first: the cap must stop the read.
    let big = format!(r#"{{"pad":"{}"}}"#, "x".repeat(4000));
    for status in [200u16, 502] {
        let s = serve(Script::json(status, &big)).await;
        let c = Client::new(gateway(&s.url)).with_max_response_bytes(1000);
        let e = c.chat(req()).await.unwrap_err();
        assert_eq!(e.kind, ErrorKind::Malformed, "{status}: {e}");
        assert!(!e.retryable);
        assert!(e.message.contains("1000"), "{e}");
    }
}

#[tokio::test]
async fn a_body_at_the_cap_is_read() {
    let s = serve(Script::json(200, OPENAI_CHAT)).await;
    let c = Client::new(gateway(&s.url)).with_max_response_bytes(OPENAI_CHAT.len());
    assert_eq!(c.chat(req()).await.unwrap().content, "hello");
}

#[test]
fn the_default_cap_is_32_mib() {
    assert_eq!(
        ultrafast_client::DEFAULT_MAX_RESPONSE_BYTES,
        32 * 1024 * 1024
    );
}

#[tokio::test]
async fn a_stream_error_body_that_stalls_times_out() {
    // A 500 that promises 100 bytes, sends 3 and goes quiet.
    let s = serve(Script::json(500, "abc").declared_len(100)).await;
    let c = Client::new(gateway(&s.url)).with_timeout(Duration::from_millis(300));
    let started = Instant::now();
    let e = c.chat_stream(req()).await.err().expect("an error");
    assert_eq!(e.kind, ErrorKind::Timeout, "{e:?}");
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn a_redirect_is_refused_and_never_followed() {
    let other = serve(Script::json(200, OPENAI_CHAT)).await;
    let first = serve(
        Script::json(302, "").header("location", &format!("{}/v1/chat/completions", other.url)),
    )
    .await;
    let c = Client::new(gateway(&first.url));
    let e = c.chat(req()).await.unwrap_err();
    assert_eq!(
        (e.kind, e.retryable, e.status),
        (ErrorKind::InvalidRequest, false, Some(302))
    );
    let e = c.chat_stream(req()).await.err().expect("refused");
    assert_eq!(
        (e.kind, e.retryable, e.status),
        (ErrorKind::InvalidRequest, false, Some(302))
    );
    let e = c
        .embed(EmbeddingsRequest::new("m", ["x"]))
        .await
        .unwrap_err();
    assert_eq!(e.status, Some(302));
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        other.requests().is_empty(),
        "the second server was contacted"
    );
    assert_eq!(first.requests().len(), 3);
}

#[tokio::test]
async fn a_stream_that_stalls_after_text_times_out_after_the_text() {
    let part = "data: {\"choices\":[{\"delta\":{\"content\":\"par\"},\"finish_reason\":null}]}\n\n";
    let s = serve(Script::sse(vec![part.as_bytes().to_vec()]).stall()).await;
    let c = Client::new(gateway(&s.url)).with_timeout(Duration::from_millis(300));
    let mut st = Box::pin(c.chat_stream(req()).await.unwrap());
    let first = st.next().await.unwrap().unwrap();
    assert_eq!(
        first,
        ultrafast_client::types::StreamEvent::Delta { text: "par".into() }
    );
    let e = st.next().await.unwrap().unwrap_err();
    assert_eq!(e.kind, ErrorKind::Timeout);
    assert!(e.retryable);
    assert!(st.next().await.is_none());
}

#[tokio::test]
async fn nothing_is_retried() {
    for status in [500, 503, 429] {
        let s = serve(Script::json(status, "{}")).await;
        let c = Client::new(gateway(&s.url));
        c.chat(req()).await.unwrap_err();
        let _ = c.chat_stream(req()).await.err();
        assert_eq!(
            s.requests().len(),
            2,
            "status {status}: one request per call"
        );
    }
}

#[tokio::test]
async fn a_short_key_is_scrubbed_too() {
    let s = serve(Script::json(
        401,
        r#"{"error":{"message":"bad key abc","type":"x"}}"#,
    ))
    .await;
    let e = Client::new(Target::gateway(&s.url, "abc"))
        .chat(req())
        .await
        .unwrap_err();
    assert_eq!(e.message, "bad key [redacted]");
}

#[tokio::test]
async fn a_provider_503_with_retry_after_carries_it() {
    let e = chat_error(Script::json(503, "{}").header("retry-after", "5"), openai).await;
    assert_eq!(e.kind, ErrorKind::Upstream);
    assert!(e.retryable);
    assert_eq!(e.retry_after, Some(Duration::from_secs(5)));
}

#[tokio::test]
async fn the_cap_applies_to_embed_and_to_the_stream_error_body() {
    let big = format!(r#"{{"pad":"{}"}}"#, "x".repeat(4000));
    let s = serve(Script::json(200, &big)).await;
    let c = Client::new(gateway(&s.url)).with_max_response_bytes(1000);
    let e = c
        .embed(EmbeddingsRequest::new("m", ["x"]))
        .await
        .unwrap_err();
    assert_eq!(e.kind, ErrorKind::Malformed, "{e}");

    let s = serve(Script::json(502, &big)).await;
    let c = Client::new(gateway(&s.url)).with_max_response_bytes(1000);
    let e = c.chat_stream(req()).await.err().expect("an error");
    assert_eq!(e.kind, ErrorKind::Malformed, "{e}");
    assert!(e.message.contains("1000"), "{e}");
}

#[tokio::test]
async fn a_declared_length_over_the_cap_is_refused_without_reading() {
    // 3 bytes sent, 5000 promised, then silence: only the early check can
    // answer before the timeout.
    for status in [200u16, 500] {
        let s = serve(Script::json(status, "abc").declared_len(5000)).await;
        let c = Client::new(gateway(&s.url))
            .with_max_response_bytes(1000)
            .with_timeout(Duration::from_secs(20));
        let started = Instant::now();
        let e = c.chat(req()).await.unwrap_err();
        assert_eq!(e.kind, ErrorKind::Malformed, "{status}: {e}");
        assert!(started.elapsed() < Duration::from_secs(5), "{status}");
    }
}
