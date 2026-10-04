//! The `x-uf-tags` header and the tags of a key on `/v1`.

mod common;

use std::collections::BTreeMap;

use axum::http::StatusCode;
use common::{harness, post_to, Harness};
use serde_json::{json, Value};
use wiremock::matchers::method;
use wiremock::{Mock, ResponseTemplate};

const BODY: &str = r#"{"model":"p/gpt-4o","messages":[{"role":"user","content":"hi"}]}"#;

fn openai_ok() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "c1", "model": "gpt-4o",
        "choices": [{ "message": { "role": "assistant", "content": "hello" }, "finish_reason": "stop" }],
        "usage": { "prompt_tokens": 1, "completion_tokens": 2 }
    }))
}

fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

async fn chat(h: &Harness, tags: &[&str]) -> (StatusCode, Value) {
    let bearer = format!("Bearer {}", h.key);
    let mut headers = vec![("authorization", bearer.as_str())];
    for t in tags {
        headers.push(("x-uf-tags", t));
    }
    let (status, _, body) = post_to(&h.app, "/v1/chat/completions", &headers, BODY).await;
    (status, serde_json::from_str(&body).unwrap_or(Value::Null))
}

async fn key_tags(h: &Harness, tags: &[(&str, &str)]) {
    let id = h.store.list_keys().await.unwrap()[0].id;
    let mut tx = h.store.begin().await.unwrap();
    tx.set_key_tags(id, &map(tags)).await.unwrap();
    tx.commit().await.unwrap();
    h.state.refresh().await.unwrap();
}

#[tokio::test]
async fn the_header_is_checked_rule_by_rule() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(openai_ok())
        .expect(0)
        .mount(&h.upstream)
        .await;
    let long_value = format!(r#"{{"a":"{}"}}"#, "x".repeat(65));
    let long_name = format!(r#"{{"{}":"v"}}"#, "n".repeat(65));
    let over_kib = format!(r#"{{"a":"{}"}}"#, "x".repeat(1024));
    let twenty_one = {
        let pairs: Vec<String> = (0..21).map(|i| format!(r#""k{i}":"v""#)).collect();
        format!("{{{}}}", pairs.join(","))
    };
    let cases: Vec<(String, &str)> = vec![
        ("not json".into(), "it is not JSON"),
        ("".into(), "it is not JSON"),
        (r#"["a"]"#.into(), "it must be a JSON object of strings"),
        (r#""a""#.into(), "it must be a JSON object of strings"),
        (r#"{"a":1}"#.into(), "it must be a JSON object of strings"),
        (
            r#"{"a":null}"#.into(),
            "it must be a JSON object of strings",
        ),
        (over_kib, "it is longer than 1024 bytes"),
        (twenty_one, "it has more than 20 entries"),
        (r#"{"":"v"}"#.into(), "a name or value is empty"),
        (r#"{"a":""}"#.into(), "a name or value is empty"),
        (long_name, "a name or value is longer than 64 characters"),
        (long_value, "a name or value is longer than 64 characters"),
        (
            r#"{"a b":"v"}"#.into(),
            "a name may use only A-Z a-z 0-9 _ . -",
        ),
        (
            r#"{"a:b":"v"}"#.into(),
            "a name may use only A-Z a-z 0-9 _ . -",
        ),
        (
            r#"{"é":"v"}"#.into(),
            "a name may use only A-Z a-z 0-9 _ . -",
        ),
    ];
    for (header, reason) in &cases {
        let (status, body) = chat(&h, &[header]).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{header}");
        assert_eq!(body["error"]["type"], "invalid_request_error", "{header}");
        assert_eq!(
            body["error"]["message"],
            format!("The x-uf-tags header is not valid: {reason}."),
            "{header}"
        );
    }
    let (status, body) = chat(&h, &[r#"{"a":"b"}"#, r#"{"c":"d"}"#]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        body["error"]["message"],
        "The x-uf-tags header is not valid: it was sent more than once."
    );
}

#[tokio::test]
async fn the_limits_are_inclusive() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(openai_ok())
        .mount(&h.upstream)
        .await;
    let twenty = {
        let pairs: Vec<String> = (0..20).map(|i| format!(r#""k{i}":"v""#)).collect();
        format!("{{{}}}", pairs.join(","))
    };
    let edge = format!(r#"{{"{}":"{}"}}"#, "n".repeat(64), "v".repeat(64));
    let charset = r#"{"Az09_.-":"any value, with spaces é"}"#;
    for header in [twenty.as_str(), edge.as_str(), charset, "{}"] {
        let (status, body) = chat(&h, &[header]).await;
        assert_eq!(status, StatusCode::OK, "{header}: {body}");
    }
}

#[tokio::test]
async fn the_other_calls_check_it_too_in_their_own_shape() {
    let h = harness("openai").await;
    let bearer = format!("Bearer {}", h.key);
    let headers = [("authorization", bearer.as_str()), ("x-uf-tags", "nope")];
    let (status, _, body) = post_to(&h.app, "/v1/embeddings", &headers, "{}").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(
        v["error"]["message"],
        "The x-uf-tags header is not valid: it is not JSON."
    );
    let (status, _, body) = post_to(&h.app, "/v1/messages", &headers, "{}").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["type"], "error");
    assert_eq!(v["error"]["type"], "invalid_request_error");
    assert_eq!(
        v["error"]["message"],
        "The x-uf-tags header is not valid: it is not JSON."
    );
}

#[tokio::test]
async fn an_unauthenticated_call_is_refused_before_its_tags_are_read() {
    let h = harness("openai").await;
    let (status, _, _) = post_to(
        &h.app,
        "/v1/chat/completions",
        &[("x-uf-tags", "nope")],
        BODY,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_key_wins_on_the_same_name() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(openai_ok())
        .mount(&h.upstream)
        .await;
    key_tags(&h, &[("team", "platform"), ("env", "prod")]).await;
    let (status, _) = chat(&h, &[r#"{"env":"dev","job":"nightly","team":"hacker"}"#]).await;
    assert_eq!(status, StatusCode::OK);
    let records = h.sink.wait_for(1).await;
    assert_eq!(
        records[0].tags,
        map(&[("env", "prod"), ("job", "nightly"), ("team", "platform")])
    );
}

#[tokio::test]
async fn a_key_without_tags_and_a_call_without_a_header_have_none() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(openai_ok())
        .mount(&h.upstream)
        .await;
    chat(&h, &[]).await;
    let only_call = chat(&h, &[r#"{"job":"x"}"#]).await;
    assert_eq!(only_call.0, StatusCode::OK);
    key_tags(&h, &[("env", "prod")]).await;
    chat(&h, &[]).await;
    let records = h.sink.wait_for(3).await;
    assert!(records[0].tags.is_empty());
    assert_eq!(records[1].tags, map(&[("job", "x")]));
    assert_eq!(records[2].tags, map(&[("env", "prod")]));
}

#[tokio::test]
async fn a_refused_header_is_recorded_as_a_400_with_the_key_tags_only() {
    let h = harness("openai").await;
    key_tags(&h, &[("env", "prod")]).await;
    let (status, _) = chat(&h, &["nope"]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let records = h.sink.wait_for(1).await;
    assert_eq!(records[0].status, 400);
    assert_eq!(records[0].tags, map(&[("env", "prod")]));
}

#[tokio::test]
async fn the_header_is_never_forwarded_to_a_provider() {
    for kind in ["openai", "anthropic"] {
        let h = harness(kind).await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "c1", "model": "gpt-4o", "type": "message", "role": "assistant",
                "content": [{ "type": "text", "text": "hi" }],
                "stop_reason": "end_turn",
                "choices": [{ "message": { "role": "assistant", "content": "hello" }, "finish_reason": "stop" }],
                "usage": { "prompt_tokens": 1, "completion_tokens": 2, "input_tokens": 1, "output_tokens": 2 }
            })))
            .mount(&h.upstream)
            .await;
        let (status, _) = chat(&h, &[r#"{"secret-label":"internal"}"#]).await;
        assert_eq!(status, StatusCode::OK, "{kind}");
        let seen = h.upstream.received_requests().await.unwrap();
        assert_eq!(seen.len(), 1, "{kind}");
        for (name, value) in seen[0].headers.iter() {
            assert!(
                !name.as_str().contains("tags")
                    && !value.to_str().unwrap_or("").contains("secret-label"),
                "{kind}: {name:?}"
            );
        }
        assert!(!String::from_utf8_lossy(&seen[0].body).contains("secret-label"));
    }
}

/// What the provider was sent for a call with a tags header on each endpoint.
async fn forwarded(h: &Harness, path: &str, key_header: (&str, &str), body: &str) -> Vec<Value> {
    let headers = [
        key_header,
        ("x-uf-tags", r#"{"secret-label":"internal"}"#),
        ("anthropic-version", "2023-06-01"),
    ];
    let (status, _, text) = post_to(&h.app, path, &headers, body).await;
    assert_eq!(status, StatusCode::OK, "{path}: {text}");
    h.upstream
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| {
            json!({
                "headers": r.headers.iter().map(|(n, v)| format!("{}={}", n, v.to_str().unwrap_or(""))).collect::<Vec<_>>(),
                "body": String::from_utf8_lossy(&r.body),
            })
        })
        .collect()
}

fn assert_clean(seen: &[Value], what: &str) {
    assert_eq!(seen.len(), 1, "{what}");
    let text = seen[0].to_string();
    assert!(
        !text.contains("x-uf-tags") && !text.contains("secret-label"),
        "{what}: {text}"
    );
}

#[tokio::test]
async fn the_header_is_not_forwarded_on_messages_embeddings_or_a_stream() {
    let anthropic_ok = json!({
        "id": "m1", "type": "message", "role": "assistant", "model": "claude-sonnet-5",
        "content": [{ "type": "text", "text": "hi" }], "stop_reason": "end_turn",
        "usage": { "input_tokens": 1, "output_tokens": 2 }
    });
    let h = harness("anthropic").await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(anthropic_ok))
        .mount(&h.upstream)
        .await;
    let bearer = format!("Bearer {}", h.key);
    let _ = bearer;
    let body = r#"{"model":"p/claude-sonnet-5","max_tokens":8,"messages":[{"role":"user","content":"hi"}]}"#;
    let seen = forwarded(&h, "/v1/messages", ("x-api-key", &h.key), body).await;
    assert_clean(&seen, "messages");

    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list", "model": "m",
            "data": [{ "object": "embedding", "index": 0, "embedding": [0.1, 0.2] }],
            "usage": { "prompt_tokens": 1, "total_tokens": 1 }
        })))
        .mount(&h.upstream)
        .await;
    let bearer = format!("Bearer {}", h.key);
    let seen = forwarded(
        &h,
        "/v1/embeddings",
        ("authorization", &bearer),
        r#"{"model":"p/m","input":"hi"}"#,
    )
    .await;
    assert_clean(&seen, "embeddings");

    let h = harness("openai").await;
    let sse = "data: {\"id\":\"c1\",\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"hi\"},\"finish_reason\":null}]}\n\n\
data: {\"id\":\"c1\",\"model\":\"gpt-4o\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1}}\n\ndata: [DONE]\n\n";
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse),
        )
        .mount(&h.upstream)
        .await;
    let bearer = format!("Bearer {}", h.key);
    let seen = forwarded(
        &h,
        "/v1/chat/completions",
        ("authorization", &bearer),
        r#"{"model":"p/gpt-4o","stream":true,"messages":[{"role":"user","content":"hi"}]}"#,
    )
    .await;
    assert_clean(&seen, "stream");
}
