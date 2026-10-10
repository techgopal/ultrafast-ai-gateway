//! `POST /v1/audio/{transcriptions,translations,speech}`: audio over OpenAI,
//! Azure and OpenAI-compatible providers, through access, guardrails, logs
//! and metrics, with a capped upload.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use bytes::Bytes;
use common::{allow_model, harness, harness_with_metrics_token, harness_with_state, Harness};
use futures::StreamExt;
use serde_json::{json, Value};
use tower::ServiceExt;
use ultrafast_gateway::store::{Grants, NewGuardrail, RouteSettings, TargetsInput};
use ultrafast_gateway::telemetry::AttemptOutcome;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const BOUNDARY: &str = "ufboundary7MA4YWxk";
const MIB: usize = 1024 * 1024;

/// The head of a multipart form: the text fields, then the opening of the
/// file part. The file bytes and `tail()` follow.
fn head(fields: &[(&str, &str)], file_name: &str) -> Vec<u8> {
    let mut out = String::new();
    for (k, v) in fields {
        out.push_str(&format!(
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{v}\r\n"
        ));
    }
    out.push_str(&format!(
        "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\nContent-Type: audio/mpeg\r\n\r\n"
    ));
    out.into_bytes()
}

fn tail() -> Vec<u8> {
    format!("\r\n--{BOUNDARY}--\r\n").into_bytes()
}

fn form(fields: &[(&str, &str)], file: &[u8]) -> Vec<u8> {
    let mut body = head(fields, "talk.mp3");
    body.extend_from_slice(file);
    body.extend(tail());
    body
}

fn request(h: &Harness, uri: &str, body: Body) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("authorization", format!("Bearer {}", h.key))
        .header(
            "content-type",
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(body)
        .unwrap()
}

async fn send(h: &Harness, req: Request<Body>) -> (StatusCode, HeaderMap, Vec<u8>) {
    let resp = h.app.clone().oneshot(req).await.unwrap();
    let (parts, body) = resp.into_parts();
    let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
    (parts.status, parts.headers, bytes.to_vec())
}

async fn transcribe_at(
    h: &Harness,
    uri: &str,
    fields: &[(&str, &str)],
    file: &[u8],
) -> (StatusCode, HeaderMap, Vec<u8>) {
    send(h, request(h, uri, Body::from(form(fields, file)))).await
}

async fn transcribe(
    h: &Harness,
    fields: &[(&str, &str)],
    file: &[u8],
) -> (StatusCode, HeaderMap, Vec<u8>) {
    transcribe_at(h, "/v1/audio/transcriptions", fields, file).await
}

async fn speak(h: &Harness, body: &str) -> (StatusCode, HeaderMap, Vec<u8>) {
    let req = Request::builder()
        .method("POST")
        .uri("/v1/audio/speech")
        .header("authorization", format!("Bearer {}", h.key))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    send(h, req).await
}

fn as_json(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(bytes)))
}

fn tokens_answer() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "text": "hello there",
        "usage": { "type": "tokens", "input_tokens": 40, "output_tokens": 5, "total_tokens": 45 }
    }))
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[tokio::test]
async fn a_transcription_is_forwarded_with_its_fields() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .and(path("/audio/transcriptions"))
        .and(header("authorization", "Bearer provider-secret"))
        .respond_with(tokens_answer())
        .expect(1)
        .mount(&h.upstream)
        .await;
    let (s, headers, body) = transcribe(
        &h,
        &[
            ("model", "p/m"),
            ("language", "en"),
            ("prompt", "names: Ada"),
            ("temperature", "0.2"),
            ("response_format", "json"),
        ],
        b"RIFFaudio-bytes",
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    assert_eq!(headers["content-type"], "application/json");
    let v = as_json(&body);
    assert_eq!(v["text"], "hello there");
    assert_eq!(v["usage"]["input_tokens"], 40);

    let seen = &h.upstream.received_requests().await.unwrap()[0];
    let ct = seen.headers.get("content-type").unwrap().to_str().unwrap();
    assert!(ct.starts_with("multipart/form-data; boundary="), "{ct}");
    let b = &seen.body;
    for part in [
        &b"name=\"model\"\r\n\r\nm\r\n"[..],
        b"name=\"language\"\r\n\r\nen\r\n",
        b"name=\"prompt\"\r\n\r\nnames: Ada\r\n",
        b"name=\"temperature\"\r\n\r\n0.2\r\n",
        b"name=\"response_format\"\r\n\r\njson\r\n",
        b"name=\"file\"; filename=\"talk.mp3\"",
        b"audio/mpeg",
        b"RIFFaudio-bytes",
    ] {
        assert!(
            contains(b, part),
            "missing {}",
            String::from_utf8_lossy(part)
        );
    }
    // The caller's model name is replaced by the target's.
    assert!(!contains(b, b"p/m"));

    let r = &h.sink.wait_for(1).await[0];
    assert_eq!(r.endpoint, "transcriptions");
    let u = r.usage.expect("usage");
    assert_eq!((u.input_tokens, u.output_tokens), (40, 5));
}

#[tokio::test]
async fn a_translation_goes_to_the_translations_path() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .and(path("/audio/translations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "text": "hi" })))
        .expect(1)
        .mount(&h.upstream)
        .await;
    let (s, _, body) =
        transcribe_at(&h, "/v1/audio/translations", &[("model", "p/m")], b"abc").await;
    assert_eq!(s, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    assert_eq!(as_json(&body)["text"], "hi");
    assert_eq!(h.sink.wait_for(1).await[0].endpoint, "translations");
    // Not priced: no usage given.
    assert!(h.sink.records()[0].usage.is_none());
}

#[tokio::test]
async fn an_azure_provider_transcribes_on_the_deployment() {
    let h = harness("azure").await;
    Mock::given(method("POST"))
        .and(path("/openai/deployments/m/audio/transcriptions"))
        .and(header("api-key", "provider-secret"))
        .respond_with(tokens_answer())
        .expect(1)
        .mount(&h.upstream)
        .await;
    let (s, _, body) = transcribe(&h, &[("model", "p/m")], b"abc").await;
    assert_eq!(s, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let seen = &h.upstream.received_requests().await.unwrap()[0];
    assert!(seen.url.query().unwrap().starts_with("api-version="));
    assert!(!contains(&seen.body, b"name=\"model\""));
}

#[tokio::test]
async fn text_and_subtitle_answers_keep_their_type_and_shape() {
    let h = harness("openai").await;
    let srt = "1\n00:00:00,000 --> 00:00:01,000\nhello\n";
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_string(srt))
        .mount(&h.upstream)
        .await;
    let (s, headers, body) =
        transcribe(&h, &[("model", "p/m"), ("response_format", "srt")], b"abc").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body, srt.as_bytes());
    assert!(headers["content-type"]
        .to_str()
        .unwrap()
        .starts_with("application/x-subrip"));
    let (s, headers, body) =
        transcribe(&h, &[("model", "p/m"), ("response_format", "text")], b"abc").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body, srt.as_bytes());
    assert!(headers["content-type"]
        .to_str()
        .unwrap()
        .starts_with("text/plain"));
}

// Review focus 3: an upload over the cap is refused without being read in
// full.
#[tokio::test]
async fn a_30_mib_upload_is_refused_without_reading_it_all() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(tokens_answer())
        .mount(&h.upstream)
        .await;
    assert_eq!(ultrafast_gateway::app::DEFAULT_MAX_AUDIO_BYTES, 25 * MIB);
    let pulled = Arc::new(AtomicUsize::new(0));
    let chunk = Bytes::from(vec![7u8; 64 * 1024]);
    let counter = pulled.clone();
    let total_chunks = 30 * MIB / chunk.len();
    let mut first = Some(Bytes::from(head(&[("model", "p/m")], "big.mp3")));
    let mut sent = 0usize;
    let stream = futures::stream::poll_fn(move |_| {
        if let Some(f) = first.take() {
            counter.fetch_add(f.len(), Ordering::SeqCst);
            return std::task::Poll::Ready(Some(Ok::<_, std::io::Error>(f)));
        }
        if sent < total_chunks {
            sent += 1;
            counter.fetch_add(chunk.len(), Ordering::SeqCst);
            return std::task::Poll::Ready(Some(Ok(chunk.clone())));
        }
        std::task::Poll::Ready(None)
    });
    let (s, _, body) = send(
        &h,
        request(&h, "/v1/audio/transcriptions", Body::from_stream(stream)),
    )
    .await;
    assert_eq!(
        s,
        StatusCode::PAYLOAD_TOO_LARGE,
        "{}",
        String::from_utf8_lossy(&body)
    );
    assert_eq!(as_json(&body)["error"]["type"], "invalid_request_error");
    let read = pulled.load(Ordering::SeqCst);
    assert!(
        read <= 25 * MIB + 2 * MIB,
        "the reader went on after the cap: {read} bytes"
    );
    assert!(h.upstream.received_requests().await.unwrap().is_empty());
    assert_eq!(h.sink.wait_for(1).await[0].status, 413);
}

#[tokio::test]
async fn a_declared_size_over_the_cap_is_refused_before_reading() {
    let h = harness_with_state("openai", |s| s.max_audio_bytes = 1024).await;
    let (s, _, _) = transcribe(&h, &[("model", "p/m")], &vec![1u8; 2 * MIB]).await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
    assert!(h.upstream.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_20_mib_upload_is_streamed_to_the_provider() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(tokens_answer())
        .expect(1)
        .mount(&h.upstream)
        .await;
    let file = vec![9u8; 20 * MIB];
    let (s, _, body) = transcribe(&h, &[("model", "p/m")], &file).await;
    assert_eq!(s, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let seen = &h.upstream.received_requests().await.unwrap()[0];
    assert!(seen.body.len() > 20 * MIB);
    assert!(contains(
        &seen.body[..4096.min(seen.body.len())],
        b"filename=\"talk.mp3\""
    ));
}

#[tokio::test]
async fn the_upload_must_be_a_file_with_known_fields() {
    let h = harness("openai").await;
    for (fields, expect) in [
        (vec![], StatusCode::BAD_REQUEST),
        (
            vec![("model", "p/m"), ("response_format", "diarized_json")],
            StatusCode::BAD_REQUEST,
        ),
        (
            vec![("model", "p/m"), ("stream", "true")],
            StatusCode::BAD_REQUEST,
        ),
        (
            vec![("model", "p/m"), ("chunking_strategy", "auto")],
            StatusCode::BAD_REQUEST,
        ),
        (
            vec![("model", "p/m"), ("timestamp_granularities[]", "word")],
            StatusCode::BAD_REQUEST,
        ),
        (
            vec![("model", "p/m"), ("temperature", "7")],
            StatusCode::BAD_REQUEST,
        ),
        (vec![("model", "p/nope")], StatusCode::NOT_FOUND),
    ] {
        let (s, _, body) = transcribe(&h, &fields, b"abc").await;
        assert_eq!(s, expect, "{fields:?}: {}", String::from_utf8_lossy(&body));
        assert!(as_json(&body)["error"]["type"].is_string());
    }
    // No file part at all, an empty file, and not multipart.
    let no_file = format!(
        "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\np/m\r\n--{BOUNDARY}--\r\n"
    );
    let (s, _, _) = send(
        &h,
        request(&h, "/v1/audio/transcriptions", Body::from(no_file)),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _, _) = transcribe(&h, &[("model", "p/m")], b"").await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let json_req = Request::builder()
        .method("POST")
        .uri("/v1/audio/transcriptions")
        .header("authorization", format!("Bearer {}", h.key))
        .header("content-type", "application/json")
        .body(Body::from(r#"{"model":"p/m"}"#))
        .unwrap();
    assert_eq!(send(&h, json_req).await.0, StatusCode::BAD_REQUEST);
    // No key: refused before the body is read.
    let anon = Request::builder()
        .method("POST")
        .uri("/v1/audio/transcriptions")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(Body::from(form(&[("model", "p/m")], b"abc")))
        .unwrap();
    assert_eq!(send(&h, anon).await.0, StatusCode::UNAUTHORIZED);
    assert!(h.upstream.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn speech_bytes_pass_through_with_the_providers_content_type() {
    let h = harness("openai").await;
    let audio: Vec<u8> = (0..=255u8).cycle().take(100_000).collect();
    Mock::given(method("POST"))
        .and(path("/audio/speech"))
        .and(header("authorization", "Bearer provider-secret"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(audio.clone(), "audio/wav"))
        .expect(1)
        .mount(&h.upstream)
        .await;
    let (s, headers, body) = speak(
        &h,
        r#"{"model":"p/m","input":"hello","voice":"alloy","response_format":"wav","speed":1.25,"instructions":"calm"}"#,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(headers["content-type"], "audio/wav");
    assert_eq!(body, audio);
    let sent: Value =
        serde_json::from_slice(&h.upstream.received_requests().await.unwrap()[0].body).unwrap();
    assert_eq!(
        sent,
        json!({"model":"m","input":"hello","voice":"alloy","response_format":"wav","speed":1.25,"instructions":"calm"})
    );
    let r = &h.sink.wait_for(1).await[0];
    assert_eq!(r.endpoint, "speech");
    assert_eq!(r.status, 200);
    assert!(r.usage.is_none());
    assert_eq!(r.attempts[0].outcome, AttemptOutcome::Ok);
}

#[tokio::test]
async fn speech_refuses_what_it_cannot_do() {
    let h = harness("openai").await;
    for bad in [
        r#"{"model":"p/m","voice":"a"}"#,
        r#"{"model":"p/m","input":"x"}"#,
        r#"{"model":"p/m","input":"x","voice":"a","speed":9}"#,
        r#"{"model":"p/m","input":"x","voice":"a","stream_format":"sse"}"#,
        r#"{"model":"p/m","input":"x","voice":"a","stream":true}"#,
        r#"{"model":"p/m","input":"x","voice":"a","response_format":"ogg"}"#,
    ] {
        let (s, _, body) = speak(&h, bad).await;
        assert_eq!(
            s,
            StatusCode::BAD_REQUEST,
            "{bad}: {}",
            String::from_utf8_lossy(&body)
        );
    }
    let long = format!(
        r#"{{"model":"p/m","input":"{}","voice":"a"}}"#,
        "a".repeat(4097)
    );
    assert_eq!(speak(&h, &long).await.0, StatusCode::BAD_REQUEST);
    assert!(h.upstream.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_speech_error_from_the_provider_is_an_error_body() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(400)
                .set_body_json(json!({ "error": { "message": "Invalid voice." } })),
        )
        .mount(&h.upstream)
        .await;
    let (s, headers, body) = speak(&h, r#"{"model":"p/m","input":"x","voice":"zzz"}"#).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(headers["content-type"], "application/json");
    assert_eq!(as_json(&body)["error"]["message"], "Invalid voice.");
}

#[tokio::test]
async fn anthropic_and_gemini_are_refused_before_any_upstream_call() {
    for kind in ["anthropic", "gemini"] {
        let h = harness(kind).await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&h.upstream)
            .await;
        for uri in ["/v1/audio/transcriptions", "/v1/audio/translations"] {
            let (s, _, body) = transcribe_at(&h, uri, &[("model", "p/m")], b"abc").await;
            assert_eq!(s, StatusCode::BAD_REQUEST, "{kind} {uri}");
            let v = as_json(&body);
            assert_eq!(v["error"]["type"], "invalid_request_error");
            assert_eq!(v["error"]["message"], "This model does not support audio.");
        }
        let (s, _, body) = speak(&h, r#"{"model":"p/m","input":"x","voice":"a"}"#).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{kind}");
        assert_eq!(
            as_json(&body)["error"]["message"],
            "This model does not support audio."
        );
        assert!(h.upstream.received_requests().await.unwrap().is_empty());
        let records = h.sink.wait_for(3).await;
        assert_eq!(records[0].endpoint, "transcriptions");
        assert_eq!(records[0].attempts[0].outcome, AttemptOutcome::Skipped);
    }
}

// Review focus 5.
#[tokio::test]
async fn a_model_the_key_may_not_call_is_refused_and_a_route_skips_it() {
    const SETTINGS: RouteSettings = RouteSettings {
        retries: 0,
        first_token_timeout_ms: 30_000,
        total_timeout_ms: 300_000,
        breaker_failures: 5,
        breaker_window_s: 60,
        breaker_open_s: 30,
    };
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(tokens_answer())
        .mount(&h.upstream)
        .await;
    Mock::given(method("POST"))
        .and(path("/audio/speech"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(b"abc".to_vec(), "audio/mpeg"))
        .mount(&h.upstream)
        .await;
    let hidden = allow_model(&h.store, "p", "hidden").await;
    let mut tx = h.store.begin().await.unwrap();
    tx.replace_grants(hidden, &Grants::default()).await.unwrap();
    let visible = tx.insert_model(1, "visible").await.unwrap();
    assert!(tx.set_model_enabled(visible, true).await.unwrap());
    tx.replace_grants(
        visible,
        &Grants {
            everyone: true,
            ..Grants::default()
        },
    )
    .await
    .unwrap();
    let route = tx.insert_route("r", &SETTINGS, true).await.unwrap();
    tx.replace_targets(
        route,
        &TargetsInput {
            primaries: vec![(hidden, 1)],
            fallbacks: vec![visible],
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    h.state.refresh().await.unwrap();

    let (s, _, body) = transcribe(&h, &[("model", "p/hidden")], b"abc").await;
    assert_eq!(
        s,
        StatusCode::FORBIDDEN,
        "{}",
        String::from_utf8_lossy(&body)
    );
    assert_eq!(as_json(&body)["error"]["type"], "permission_error");
    let (s, _, _) = speak(&h, r#"{"model":"p/hidden","input":"x","voice":"a"}"#).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert!(h.upstream.received_requests().await.unwrap().is_empty());

    let (s, _, _) = transcribe(&h, &[("model", "r")], b"abc").await;
    assert_eq!(s, StatusCode::OK);
    let seen: Vec<_> = h
        .sink
        .records()
        .last()
        .unwrap()
        .attempts
        .iter()
        .map(|a| (a.model.clone(), a.outcome))
        .collect();
    assert_eq!(
        seen,
        [
            ("hidden".to_string(), AttemptOutcome::Skipped),
            ("visible".to_string(), AttemptOutcome::Ok)
        ]
    );
    let (s, _, _) = speak(&h, r#"{"model":"r","input":"x","voice":"a"}"#).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(h.upstream.received_requests().await.unwrap().len(), 2);
}

async fn guardrail(h: &Harness, rules: Value) {
    let rules = rules.to_string();
    let mut tx = h.store.begin().await.unwrap();
    tx.insert_guardrail(NewGuardrail {
        name: "g",
        description: "",
        kind: "rules",
        rules: &rules,
        url: None,
        secret_enc: None,
        timeout_ms: 3000,
        fail_mode: "open",
        directions: "both",
        enabled: true,
        is_default: true,
    })
    .await
    .unwrap();
    tx.commit().await.unwrap();
    h.state.refresh().await.unwrap();
}

fn rules() -> Value {
    json!([
        { "id": "w", "matcher": { "keywords": { "words": ["swordfish"] } },
          "action": "block", "directions": "both" },
        { "id": "e", "matcher": { "pii": ["EMAIL"] },
          "action": "redact", "directions": "both" }
    ])
}

#[tokio::test]
async fn speech_input_is_checked_and_the_audio_never_is() {
    let h = harness("openai").await;
    // The audio holds the forbidden word's bytes: never inspected.
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(b"swordfish".to_vec(), "audio/mpeg"))
        .mount(&h.upstream)
        .await;
    guardrail(&h, rules()).await;
    let (s, _, body) = speak(
        &h,
        r#"{"model":"p/m","input":"the word is swordfish","voice":"a"}"#,
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(as_json(&body)["error"]["code"], "guardrail_blocked");
    assert!(h.upstream.received_requests().await.unwrap().is_empty());
    let (s, _, body) = speak(
        &h,
        r#"{"model":"p/m","input":"mail ada@example.com","voice":"a"}"#,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body, b"swordfish");
    let sent: Value =
        serde_json::from_slice(&h.upstream.received_requests().await.unwrap()[0].body).unwrap();
    assert!(!sent["input"].as_str().unwrap().contains("ada@example.com"));
}

#[tokio::test]
async fn transcripts_are_checked_in_every_format_and_the_audio_never_is() {
    let h = harness("openai").await;
    guardrail(&h, rules()).await;
    // The upload holds a forbidden word: not inspected.
    let file = b"swordfish audio";
    let verbose = json!({
        "task": "transcribe", "language": "en", "duration": 2.0,
        "text": "mail ada@example.com now",
        "segments": [{"id": 0, "start": 0.0, "end": 2.0, "text": "mail ada@example.com now"}],
        "words": [{"word": "mail", "start": 0.0, "end": 0.4}]
    });
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(verbose))
        .up_to_n_times(1)
        .mount(&h.upstream)
        .await;
    let (s, _, body) = transcribe(
        &h,
        &[("model", "p/m"), ("response_format", "verbose_json")],
        file,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let text = String::from_utf8(body.clone()).unwrap();
    assert!(!text.contains("ada@example.com"), "{text}");
    let v = as_json(&body);
    assert_eq!(v["segments"][0]["start"], 0.0);
    assert!(v.get("words").is_none());

    let srt = "1\n00:00:00,000 --> 00:00:01,000\nmail ada@example.com\n\n2\n00:00:01,000 --> 00:00:02,000\nfine\n";
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_string(srt))
        .up_to_n_times(1)
        .mount(&h.upstream)
        .await;
    let (s, _, body) = transcribe(&h, &[("model", "p/m"), ("response_format", "srt")], file).await;
    assert_eq!(s, StatusCode::OK);
    let text = String::from_utf8(body).unwrap();
    assert!(!text.contains("ada@example.com"), "{text}");
    assert!(text.contains("00:00:00,000 --> 00:00:01,000\n"));
    assert!(text.starts_with("1\n"));
    assert!(text.contains("\nfine\n"));

    // A blocked transcript is not given.
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"text": "swordfish"})))
        .mount(&h.upstream)
        .await;
    let (s, _, body) = transcribe(&h, &[("model", "p/m")], file).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let b = String::from_utf8(body).unwrap();
    assert!(
        b.contains("guardrail_blocked") && !b.contains("swordfish\""),
        "{b}"
    );
    let records = h.sink.wait_for(3).await;
    assert!(records[2].guardrails.is_some());
}

#[tokio::test]
async fn audio_calls_are_not_cached() {
    use ultrafast_gateway::cache::{CacheScope, RouteCache};
    let h = harness("openai").await;
    let model = allow_model(&h.store, "p", "stt").await;
    let mut tx = h.store.begin().await.unwrap();
    let id = tx
        .insert_route(
            "r",
            &RouteSettings {
                retries: 0,
                first_token_timeout_ms: 30_000,
                total_timeout_ms: 300_000,
                breaker_failures: 5,
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
            scope: CacheScope::Key,
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    h.state.refresh().await.unwrap();
    Mock::given(method("POST"))
        .and(path("/audio/transcriptions"))
        .respond_with(tokens_answer())
        .mount(&h.upstream)
        .await;
    Mock::given(method("POST"))
        .and(path("/audio/speech"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(b"abc".to_vec(), "audio/mpeg"))
        .mount(&h.upstream)
        .await;
    for _ in 0..2 {
        assert_eq!(
            transcribe(&h, &[("model", "r")], b"abc").await.0,
            StatusCode::OK
        );
        assert_eq!(
            speak(&h, r#"{"model":"r","input":"x","voice":"a"}"#)
                .await
                .0,
            StatusCode::OK
        );
    }
    assert_eq!(h.upstream.received_requests().await.unwrap().len(), 4);
    assert!(h.sink.wait_for(4).await.iter().all(|r| !r.cached));
}

#[tokio::test]
async fn the_calls_are_counted_by_endpoint() {
    let h = harness_with_metrics_token("openai", Some("scrape-token-0123456789")).await;
    Mock::given(method("POST"))
        .and(path("/audio/transcriptions"))
        .respond_with(tokens_answer())
        .mount(&h.upstream)
        .await;
    Mock::given(method("POST"))
        .and(path("/audio/translations"))
        .respond_with(tokens_answer())
        .mount(&h.upstream)
        .await;
    Mock::given(method("POST"))
        .and(path("/audio/speech"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(b"abc".to_vec(), "audio/mpeg"))
        .mount(&h.upstream)
        .await;
    assert_eq!(
        transcribe(&h, &[("model", "p/m")], b"abc").await.0,
        StatusCode::OK
    );
    assert_eq!(
        transcribe_at(&h, "/v1/audio/translations", &[("model", "p/m")], b"abc")
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        speak(&h, r#"{"model":"p/m","input":"x","voice":"a"}"#)
            .await
            .0,
        StatusCode::OK
    );
    h.sink.wait_for(3).await;
    let resp = h
        .app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .header("authorization", "Bearer scrape-token-0123456789")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    for e in ["transcriptions", "translations", "speech"] {
        assert!(
            text.contains(&format!(
                r#"uf_requests_total{{endpoint="{e}",status_class="2xx"}} 1"#
            )),
            "{e}: {text}"
        );
    }
}

fn slow_floor_off(s: &mut ultrafast_gateway::app::AppState) {
    s.slow_calls = ultrafast_gateway::app::SlowCalls {
        first_byte: Duration::ZERO,
        total: Duration::ZERO,
    };
}

async fn route_of_two(h: &Harness) {
    let a = allow_model(&h.store, "p", "a").await;
    let b = allow_model(&h.store, "p", "b").await;
    let mut tx = h.store.begin().await.unwrap();
    let id = tx
        .insert_route(
            "r",
            &RouteSettings {
                retries: 2,
                first_token_timeout_ms: 300,
                total_timeout_ms: 300_000,
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

// Billed once: after a send that got no answer in time there is no repeat
// and no fallback, for transcription and for speech.
#[tokio::test]
async fn a_timed_out_audio_call_is_not_repeated_or_sent_to_a_fallback() {
    let h = harness_with_state("openai", slow_floor_off).await;
    route_of_two(&h).await;
    Mock::given(method("POST"))
        .respond_with(tokens_answer().set_delay(Duration::from_secs(2)))
        .mount(&h.upstream)
        .await;
    let (s, _, body) = transcribe(&h, &[("model", "r")], b"abc").await;
    assert_eq!(
        s,
        StatusCode::GATEWAY_TIMEOUT,
        "{}",
        String::from_utf8_lossy(&body)
    );
    assert!(as_json(&body)["error"]["message"]
        .as_str()
        .unwrap()
        .contains("not repeated"));
    assert_eq!(h.upstream.received_requests().await.unwrap().len(), 1);
    let (s, _, _) = speak(&h, r#"{"model":"r","input":"x","voice":"a"}"#).await;
    assert_eq!(s, StatusCode::GATEWAY_TIMEOUT);
    assert_eq!(h.upstream.received_requests().await.unwrap().len(), 2);
    let r = &h.sink.wait_for(2).await[0];
    let seen: Vec<_> = r
        .attempts
        .iter()
        .map(|a| (a.model.as_str(), a.outcome))
        .collect();
    assert_eq!(
        seen,
        [("a", AttemptOutcome::Fatal), ("b", AttemptOutcome::Skipped)]
    );
}

#[tokio::test]
async fn an_unreadable_transcript_is_not_repeated() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
        .mount(&h.upstream)
        .await;
    let (s, _, _) = transcribe(&h, &[("model", "p/m")], b"abc").await;
    assert_eq!(s, StatusCode::BAD_GATEWAY);
    assert_eq!(h.upstream.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn audio_calls_get_the_slow_timeouts() {
    // Same property as images: a route's short timeout is raised.
    let h = harness("openai").await;
    route_of_two(&h).await; // first byte 300 ms
    Mock::given(method("POST"))
        .respond_with(tokens_answer().set_delay(Duration::from_millis(900)))
        .mount(&h.upstream)
        .await;
    let (s, _, body) = transcribe(&h, &[("model", "r")], b"abc").await;
    assert_eq!(s, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
}

#[tokio::test]
async fn a_speech_answer_over_its_cap_is_cut_and_recorded() {
    let h = harness_with_state("openai", |s| s.max_speech_response_bytes = 1024).await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(vec![1u8; 4096], "audio/mpeg"))
        .mount(&h.upstream)
        .await;
    let resp = h
        .app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/audio/speech")
                .header("authorization", format!("Bearer {}", h.key))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"model":"p/m","input":"x","voice":"a"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    // The body ends in an error, not in 4096 bytes.
    assert!(axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .is_err());
    let r = &h.sink.wait_for(1).await[0];
    assert_ne!(r.attempts[0].outcome, AttemptOutcome::Ok);
}

#[tokio::test]
async fn an_openai_compatible_provider_with_a_path_prefix_serves_audio() {
    let h = harness("openai").await;
    let other = MockServer::start().await;
    h.store
        .insert_provider("o", "openai", &format!("{}/v1", other.uri()), None)
        .await
        .unwrap();
    allow_model(&h.store, "o", "whisper").await;
    h.state.refresh().await.unwrap();
    Mock::given(method("POST"))
        .and(path("/v1/audio/transcriptions"))
        .respond_with(tokens_answer())
        .expect(1)
        .mount(&other)
        .await;
    let (s, _, body) = transcribe(&h, &[("model", "o/whisper")], b"abc").await;
    assert_eq!(s, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    assert!(h.upstream.received_requests().await.unwrap().is_empty());
}

/// A body of a form that sends `head` at once, then `file_len` bytes of file
/// in 64 KiB chunks, counting what was pulled from it.
fn counted_upload(fields: &[(&str, &str)], file_len: usize) -> (Body, Arc<AtomicUsize>) {
    let pulled = Arc::new(AtomicUsize::new(0));
    let counter = pulled.clone();
    let chunk = Bytes::from(vec![7u8; 64 * 1024]);
    let mut first = Some(Bytes::from(head(fields, "big.mp3")));
    let mut remaining = file_len;
    let mut closing = Some(Bytes::from(tail()));
    let mut ready = false;
    let stream = futures::stream::poll_fn(move |cx| {
        // A network body is not always ready: after each chunk the reader
        // must come back for the next, as it does for a socket.
        if !ready {
            ready = true;
            cx.waker().wake_by_ref();
            return std::task::Poll::Pending;
        }
        ready = false;
        let next = if let Some(f) = first.take() {
            Some(f)
        } else if remaining > 0 {
            let n = remaining.min(chunk.len());
            remaining -= n;
            Some(chunk.slice(..n))
        } else {
            closing.take()
        };
        std::task::Poll::Ready(next.map(|b| {
            counter.fetch_add(b.len(), Ordering::SeqCst);
            Ok::<_, std::io::Error>(b)
        }))
    });
    (Body::from_stream(stream), pulled)
}

// Fix round 1, U1: a refusal that depends on the model alone is answered
// before the file is read when the model comes first.
#[tokio::test]
async fn an_unknown_or_forbidden_model_is_refused_before_the_file_is_read() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(tokens_answer())
        .mount(&h.upstream)
        .await;
    let hidden = allow_model(&h.store, "p", "hidden").await;
    let mut tx = h.store.begin().await.unwrap();
    tx.replace_grants(hidden, &Grants::default()).await.unwrap();
    tx.commit().await.unwrap();
    h.state.refresh().await.unwrap();
    for (model, expect) in [
        ("p/nope", StatusCode::NOT_FOUND),
        ("p/hidden", StatusCode::FORBIDDEN),
    ] {
        let (body, pulled) = counted_upload(&[("model", model)], 20 * MIB);
        let (s, _, text) = send(&h, request(&h, "/v1/audio/transcriptions", body)).await;
        assert_eq!(s, expect, "{model}: {}", String::from_utf8_lossy(&text));
        let read = pulled.load(Ordering::SeqCst);
        assert!(read < MIB, "{model}: the file was read: {read} bytes");
    }
    assert!(h.upstream.received_requests().await.unwrap().is_empty());
    // The refusals are recorded as calls of the endpoint.
    let records = h.sink.wait_for(2).await;
    assert!(records.iter().all(|r| r.endpoint == "transcriptions"));
    assert_eq!(records[0].requested, "p/nope");
}

#[tokio::test]
async fn a_rate_limited_key_is_refused_before_the_file_is_read_and_the_permit_is_kept_once() {
    use ultrafast_gateway::limits::{LimitScope, RateLimit};
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(tokens_answer())
        .mount(&h.upstream)
        .await;
    let mut tx = h.store.begin().await.unwrap();
    tx.upsert_limit(
        LimitScope::Gateway,
        None,
        &RateLimit {
            requests_per_minute: Some(1),
            tokens_per_minute: None,
            concurrent: None,
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    h.state.refresh().await.unwrap();
    // The first call is counted once, not twice (early and again after the read).
    assert_eq!(
        transcribe(&h, &[("model", "p/m")], b"abc").await.0,
        StatusCode::OK
    );
    let (body, pulled) = counted_upload(&[("model", "p/m")], 20 * MIB);
    let (s, _, text) = send(&h, request(&h, "/v1/audio/transcriptions", body)).await;
    assert_eq!(
        s,
        StatusCode::TOO_MANY_REQUESTS,
        "{}",
        String::from_utf8_lossy(&text)
    );
    assert!(pulled.load(Ordering::SeqCst) < MIB);
    assert_eq!(h.upstream.received_requests().await.unwrap().len(), 1);
}

// When the model follows the file the whole file is read first (stated in the
// docs); the refusal still comes, and is bounded by the cap.
#[tokio::test]
async fn a_model_after_the_file_is_still_refused_after_the_read() {
    let h = harness("openai").await;
    let mut body = head(&[], "a.mp3");
    body.extend_from_slice(&[1u8; 1000]);
    body.extend_from_slice(
        format!("\r\n--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\np/nope\r\n--{BOUNDARY}--\r\n").as_bytes(),
    );
    let (s, _, _) = send(
        &h,
        request(&h, "/v1/audio/transcriptions", Body::from(body)),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

// U2: a client that stops sending, or trickles, is cut off with 408.
#[tokio::test]
async fn a_slow_upload_is_cut_off_with_408() {
    let h = harness_with_state("openai", |s| {
        s.upload_idle = Duration::from_millis(300);
        s.upload_total = Duration::from_millis(1000);
    })
    .await;
    // Silence after the head.
    let stalled = futures::stream::once(async {
        Ok::<_, std::io::Error>(Bytes::from(head(&[("model", "p/m")], "a.mp3")))
    })
    .chain(futures::stream::pending::<Result<Bytes, std::io::Error>>());
    let (s, _, text) = send(
        &h,
        request(&h, "/v1/audio/transcriptions", Body::from_stream(stalled)),
    )
    .await;
    assert_eq!(
        s,
        StatusCode::REQUEST_TIMEOUT,
        "{}",
        String::from_utf8_lossy(&text)
    );
    // A trickle that never stops is cut by the total deadline.
    let trickle = futures::stream::unfold(0u8, |i| async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let chunk = if i == 0 {
            Bytes::from(head(&[("model", "p/m")], "a.mp3"))
        } else {
            Bytes::from_static(b"xxxxxxxx")
        };
        Some((Ok::<_, std::io::Error>(chunk), 1))
    });
    let started = std::time::Instant::now();
    let (s, _, _) = send(
        &h,
        request(&h, "/v1/audio/transcriptions", Body::from_stream(trickle)),
    )
    .await;
    assert_eq!(s, StatusCode::REQUEST_TIMEOUT);
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(h.upstream.received_requests().await.unwrap().is_empty());
    assert!(h.sink.wait_for(2).await.iter().all(|r| r.status == 408));
}

// Global bound on uploads being received.
#[tokio::test]
async fn too_many_uploads_at_once_are_refused_with_503_before_the_body_is_read() {
    let h = harness_with_state("openai", |s| {
        s.audio_uploads = Arc::new(tokio::sync::Semaphore::new(1));
    })
    .await;
    Mock::given(method("POST"))
        .respond_with(tokens_answer())
        .mount(&h.upstream)
        .await;
    let stalled = futures::stream::once(async {
        Ok::<_, std::io::Error>(Bytes::from(head(&[("model", "p/m")], "a.mp3")))
    })
    .chain(futures::stream::pending::<Result<Bytes, std::io::Error>>());
    let first = {
        let app = h.app.clone();
        let req = request(&h, "/v1/audio/transcriptions", Body::from_stream(stalled));
        tokio::spawn(async move { app.oneshot(req).await })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    let (body, pulled) = counted_upload(&[("model", "p/m")], MIB);
    let (s, headers, text) = send(&h, request(&h, "/v1/audio/transcriptions", body)).await;
    assert_eq!(
        s,
        StatusCode::SERVICE_UNAVAILABLE,
        "{}",
        String::from_utf8_lossy(&text)
    );
    assert_eq!(pulled.load(Ordering::SeqCst), 0);
    assert!(headers.contains_key("retry-after"));
    // Giving the slot back lets the next one in.
    first.abort();
    let _ = first.await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        transcribe(&h, &[("model", "p/m")], b"abc").await.0,
        StatusCode::OK
    );
}

// U5.
#[tokio::test]
async fn an_oversize_text_field_is_named() {
    let h = harness("openai").await;
    let big = "x".repeat(70 * 1024);
    let (s, _, text) = transcribe(&h, &[("model", "p/m"), ("prompt", &big)], b"abc").await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
    let message = as_json(&text)["error"]["message"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        message.contains("prompt") && !message.contains("audio file"),
        "{message}"
    );
}

// U6.
#[tokio::test]
async fn the_prompt_of_a_transcription_is_checked_as_input() {
    let h = harness("openai").await;
    Mock::given(method("POST"))
        .respond_with(tokens_answer())
        .mount(&h.upstream)
        .await;
    guardrail(&h, rules()).await;
    let (s, _, text) = transcribe(
        &h,
        &[("model", "p/m"), ("prompt", "the word is swordfish")],
        b"abc",
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(as_json(&text)["error"]["code"], "guardrail_blocked");
    assert!(h.upstream.received_requests().await.unwrap().is_empty());
    let (s, _, _) = transcribe(
        &h,
        &[("model", "p/m"), ("prompt", "names: ada@example.com")],
        b"abc",
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let sent = &h.upstream.received_requests().await.unwrap()[0].body;
    assert!(!contains(sent, b"ada@example.com"));
    assert!(contains(sent, b"name=\"prompt\""));
}

// The bound on uploads is on receiving them: the place is given back when
// the body has arrived, not held while the provider answers.
#[tokio::test]
async fn an_upload_place_is_given_back_once_the_body_has_arrived() {
    let h = harness_with_state("openai", |s| {
        s.audio_uploads = Arc::new(tokio::sync::Semaphore::new(1));
    })
    .await;
    Mock::given(method("POST"))
        .respond_with(tokens_answer().set_delay(Duration::from_millis(1500)))
        .mount(&h.upstream)
        .await;
    let first = {
        let app = h.app.clone();
        let req = request(
            &h,
            "/v1/audio/transcriptions",
            Body::from(form(&[("model", "p/m")], b"abc")),
        );
        tokio::spawn(async move { app.oneshot(req).await })
    };
    // The provider has the first call and is slow to answer.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(h.upstream.received_requests().await.unwrap().len(), 1);
    assert_eq!(h.state.audio_uploads.available_permits(), 1);
    let (s, _, text) = transcribe(&h, &[("model", "p/m")], b"abc").await;
    assert_eq!(s, StatusCode::OK, "{}", String::from_utf8_lossy(&text));
    assert_eq!(first.await.unwrap().unwrap().status(), StatusCode::OK);
}

// A body with no declared size that names the model first is charged an
// estimate of what it turned out to be, so a token limit counts it.
#[tokio::test]
async fn a_chunked_upload_that_names_the_model_first_is_charged_tokens() {
    use ultrafast_gateway::limits::{LimitScope, RateLimit};
    let h = harness("openai").await;
    // No usage in the answer: the estimate stays what the call was charged.
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "text": "hi" })))
        .mount(&h.upstream)
        .await;
    let mut tx = h.store.begin().await.unwrap();
    tx.upsert_limit(
        LimitScope::Gateway,
        None,
        &RateLimit {
            requests_per_minute: None,
            tokens_per_minute: Some(2000),
            concurrent: None,
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    h.state.refresh().await.unwrap();
    // 1 MiB is about 1300 tokens at the gateway's estimate.
    let (body, _) = counted_upload(&[("model", "p/m")], MIB);
    let (s, _, text) = send(&h, request(&h, "/v1/audio/transcriptions", body)).await;
    assert_eq!(s, StatusCode::OK, "{}", String::from_utf8_lossy(&text));
    let (body, _) = counted_upload(&[("model", "p/m")], MIB);
    let (s, _, text) = send(&h, request(&h, "/v1/audio/transcriptions", body)).await;
    assert_eq!(
        s,
        StatusCode::TOO_MANY_REQUESTS,
        "{}",
        String::from_utf8_lossy(&text)
    );
    assert_eq!(h.upstream.received_requests().await.unwrap().len(), 1);
    // The refused one gave its tokens back: a small one still fits.
    let (body, _) = counted_upload(&[("model", "p/m")], 1000);
    let (s, _, text) = send(&h, request(&h, "/v1/audio/transcriptions", body)).await;
    assert_eq!(s, StatusCode::OK, "{}", String::from_utf8_lossy(&text));
}
