mod common;

use axum::http::StatusCode;
use common::{harness, post_chat};
use futures::StreamExt;
use serde_json::Value;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, ResponseTemplate};

const BODY: &str = r#"{"model":"p/m","stream":true,"messages":[{"role":"user","content":"hi"}]}"#;

/// Returns the JSON payload of every `data:` line except `[DONE]`.
fn payloads(body: &str) -> Vec<Value> {
    body.lines()
        .filter_map(|l| l.strip_prefix("data: "))
        .filter(|d| *d != "[DONE]")
        .map(|d| serde_json::from_str(d).expect("every data line must be JSON"))
        .collect()
}

fn text(body: &str) -> String {
    payloads(body)
        .iter()
        .filter_map(|v| {
            v["choices"][0]["delta"]["content"]
                .as_str()
                .map(str::to_string)
        })
        .collect()
}

fn sse(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(body.to_string(), "text/event-stream")
}

#[tokio::test]
async fn streams_openai_provider() {
    let h = harness("openai").await;
    let upstream = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"h\u{e9}l\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"lo \u{1f600}\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":3}}\n\n",
        "data: [DONE]\n\n",
    );
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_partial_json(serde_json::json!({ "stream": true })))
        .respond_with(sse(upstream))
        .mount(&h.upstream)
        .await;
    let (status, body) = post_chat(&h.app, Some(&h.key), BODY).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(text(&body), "h\u{e9}llo \u{1f600}");
    let all = payloads(&body);
    let last = all.last().unwrap();
    assert_eq!(last["choices"][0]["finish_reason"], "stop");
    assert_eq!(last["usage"]["total_tokens"], 5);
    assert!(body.ends_with("data: [DONE]\n\n"));
    assert!(all.iter().all(|v| v["model"] == "m"));
    let first_id = all[0]["id"].as_str().unwrap();
    assert!(first_id.starts_with("chatcmpl-"));
    assert!(all.iter().all(|v| v["id"] == first_id));
}

#[tokio::test]
async fn streams_anthropic_provider_in_openai_shape() {
    let h = harness("anthropic").await;
    let upstream = concat!(
        "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":3,\"output_tokens\":0}}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"sal\"}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"ut\"}}\n\n",
        "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"max_tokens\"},\"usage\":{\"output_tokens\":4}}\n\n",
        "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
    );
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(sse(upstream))
        .mount(&h.upstream)
        .await;
    let (status, body) = post_chat(&h.app, Some(&h.key), BODY).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(text(&body), "salut");
    let all = payloads(&body);
    assert_eq!(all.last().unwrap()["choices"][0]["finish_reason"], "length");
    assert_eq!(all.last().unwrap()["usage"]["total_tokens"], 7);
    assert!(body.ends_with("data: [DONE]\n\n"));
}

#[tokio::test]
async fn response_has_event_stream_content_type() {
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(sse("data: [DONE]\n\n"))
        .mount(&h.upstream)
        .await;
    let resp = h
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("authorization", format!("Bearer {}", h.key))
                .body(Body::from(BODY))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.headers()["content-type"], "text/event-stream");
    assert_eq!(resp.headers()["cache-control"], "no-cache");
}

#[tokio::test]
async fn stream_that_ends_early_sends_an_error_event_and_no_done() {
    let h = harness("openai").await;
    let upstream =
        "data: {\"choices\":[{\"delta\":{\"content\":\"par\"},\"finish_reason\":null}]}\n\n";
    Mock::given(method("POST"))
        .respond_with(sse(upstream))
        .mount(&h.upstream)
        .await;
    let (status, body) = post_chat(&h.app, Some(&h.key), BODY).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(text(&body), "par");
    let last = payloads(&body).pop().unwrap();
    assert!(last["error"]["message"]
        .as_str()
        .unwrap()
        .contains("ended before"));
    assert!(!body.contains("[DONE]"));
}

#[tokio::test]
async fn provider_error_inside_stream_is_forwarded_as_error_event() {
    let h = harness("openai").await;
    let upstream = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"a\"},\"finish_reason\":null}]}\n\n",
        "data: {\"error\":{\"message\":\"over \\\"loaded\\\"\"}}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"never\"},\"finish_reason\":null}]}\n\n",
    );
    Mock::given(method("POST"))
        .respond_with(sse(upstream))
        .mount(&h.upstream)
        .await;
    let (_, body) = post_chat(&h.app, Some(&h.key), BODY).await;
    assert_eq!(text(&body), "a");
    let last = payloads(&body).pop().unwrap();
    assert!(last["error"]["message"]
        .as_str()
        .unwrap()
        .contains("over \"loaded\""));
    assert!(!body.contains("[DONE]"));
}

/// The same stream as the test above, with every line ending replaced.
async fn error_inside_stream_with_line_ending(ending: &str) {
    let h = harness("openai").await;
    let upstream = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"a\"},\"finish_reason\":null}]}\n\n",
        "data: {\"error\":{\"message\":\"over \\\"loaded\\\"\"}}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"never\"},\"finish_reason\":null}]}\n\n",
    )
    .replace('\n', ending);
    Mock::given(method("POST"))
        .respond_with(sse(&upstream))
        .mount(&h.upstream)
        .await;
    let (_, body) = post_chat(&h.app, Some(&h.key), BODY).await;
    assert_eq!(text(&body), "a");
    let last = payloads(&body).pop().unwrap();
    assert!(last["error"]["message"]
        .as_str()
        .unwrap()
        .contains("over \"loaded\""));
    assert!(!body.contains("[DONE]"));
}

#[tokio::test]
async fn provider_error_inside_crlf_stream_is_forwarded_as_error_event() {
    error_inside_stream_with_line_ending("\r\n").await;
}

#[tokio::test]
async fn provider_error_inside_cr_stream_is_forwarded_as_error_event() {
    error_inside_stream_with_line_ending("\r").await;
}

#[tokio::test]
async fn provider_error_before_stream_starts_is_a_normal_json_error() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(503).set_body_string("unavailable"))
        .mount(&h.upstream)
        .await;
    let (status, body) = post_chat(&h.app, Some(&h.key), BODY).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["error"]["message"], "unavailable");
}

/// An upstream that sends one chunk and then holds the connection open.
/// Reports on the channel once the gateway closes its side.
async fn hanging_upstream() -> (String, tokio::sync::oneshot::Receiver<()>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let uri = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 8192];
        let mut seen = Vec::new();
        // Read the whole request: headers, then the announced body length.
        loop {
            let n = sock.read(&mut buf).await.unwrap();
            assert!(n > 0, "gateway closed before sending the request");
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
        let chunk =
            "data: {\"choices\":[{\"delta\":{\"content\":\"one\"},\"finish_reason\":null}]}\n\n";
        let head = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\n\r\n";
        let out = format!("{head}{:x}\r\n{chunk}\r\n", chunk.len());
        sock.write_all(out.as_bytes()).await.unwrap();
        // The stream is never finished; only the gateway closing ends this read.
        loop {
            match sock.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
        }
        let _ = tx.send(());
    });
    (uri, rx)
}

#[tokio::test]
async fn caller_disconnect_drops_the_upstream_request() {
    use axum::body::Body;
    use axum::http::Request;
    use std::time::Duration;
    use tower::ServiceExt;
    let h = harness("openai").await;
    let (uri, closed) = hanging_upstream().await;
    h.store
        .insert_provider("hang", "openai", &uri, None)
        .await
        .unwrap();
    let body = r#"{"model":"hang/m","stream":true,"messages":[{"role":"user","content":"hi"}]}"#;
    let resp = h
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("authorization", format!("Bearer {}", h.key))
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let mut stream = resp.into_body().into_data_stream();
    let first = tokio::time::timeout(Duration::from_secs(5), stream.next())
        .await
        .expect("the first chunk must arrive while upstream is still open")
        .unwrap()
        .unwrap();
    assert_eq!(text(std::str::from_utf8(&first).unwrap()), "one");
    // The caller goes away.
    drop(stream);
    tokio::time::timeout(Duration::from_secs(5), closed)
        .await
        .expect("the upstream connection must be closed when the caller disconnects")
        .unwrap();
}
