//! The shared parity fixtures (`clients/fixtures/*.json`): the Python and
//! TypeScript clients run the same files and must agree with this one.

mod common;

use common::*;
use futures::StreamExt;
use serde_json::{json, Value};
use ultrafast_client::types::{Role, StreamEvent};
use ultrafast_client::{ChatRequest, Client, EmbeddingsRequest, Error, Target};

const REQUESTS: &str = include_str!("../../../clients/fixtures/requests.json");
const RESPONSES: &str = include_str!("../../../clients/fixtures/responses.json");
const ERRORS: &str = include_str!("../../../clients/fixtures/errors.json");
const STREAMS: &str = include_str!("../../../clients/fixtures/streams.json");

fn load(text: &str) -> (String, Vec<Value>) {
    let v: Value = serde_json::from_str(text).unwrap();
    (
        v["key"].as_str().unwrap().to_string(),
        v["cases"].as_array().unwrap().clone(),
    )
}

fn target(t: &Value, base: &str, key: &str) -> Target {
    let url = t["base_url"].as_str().unwrap().replace("{base}", base);
    match t["kind"].as_str().unwrap() {
        "gateway" => Target::gateway(url, key),
        "openai" => Target::openai(key).with_base_url(url),
        "openai_compatible" => Target::openai_compatible(url, key),
        "anthropic" => Target::anthropic(key).with_base_url(url),
        "gemini" => Target::gemini(key).with_base_url(url),
        "azure" => {
            let t2 = Target::azure(url, key);
            match t["api_version"].as_str() {
                Some(v) => t2.with_api_version(v),
                None => t2,
            }
        }
        other => panic!("unknown kind {other}"),
    }
}

fn chat_request(r: &Value) -> ChatRequest {
    let mut c = ChatRequest::new(r["model"].as_str().unwrap());
    for m in r["messages"].as_array().unwrap() {
        let role = match m["role"].as_str().unwrap() {
            "system" => Role::System,
            "user" => Role::User,
            _ => Role::Assistant,
        };
        c = c.message(role, m["content"].as_str().unwrap());
    }
    if let Some(v) = r["max_tokens"].as_u64() {
        c = c.max_tokens(v as u32);
    }
    if let Some(v) = r["temperature"].as_f64() {
        c = c.temperature(v as f32);
    }
    if let Some(v) = r["top_p"].as_f64() {
        c = c.top_p(v as f32);
    }
    if let Some(v) = r["stop"].as_array() {
        c = c.stop(v.iter().map(|s| s.as_str().unwrap().to_string()));
    }
    for (k, v) in r["tags"].as_object().into_iter().flatten() {
        c = c.tag(k, v.as_str().unwrap());
    }
    c
}

fn embed_request(r: &Value) -> EmbeddingsRequest {
    let input = r["input"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string());
    let mut e = EmbeddingsRequest::new(r["model"].as_str().unwrap(), input);
    if let Some(v) = r["dimensions"].as_u64() {
        e = e.dimensions(v as u32);
    }
    for (k, v) in r["tags"].as_object().into_iter().flatten() {
        e = e.tag(k, v.as_str().unwrap());
    }
    e
}

fn error_json(e: &Error) -> Value {
    json!({
        "kind": e.kind.as_str(),
        "retryable": e.retryable,
        "status": e.status,
        "retry_after": e.retry_after.map(|d| d.as_secs()),
        "message": e.message,
    })
}

fn event_json(e: &StreamEvent) -> Value {
    match e {
        StreamEvent::Delta { text } => json!({"type": "delta", "text": text}),
        StreamEvent::Done {
            finish_reason,
            usage,
        } => json!({
            "type": "done",
            "finish_reason": finish_reason.map(|f| f.as_openai()),
            "usage": usage.map(|u| json!({"input_tokens": u.input_tokens, "output_tokens": u.output_tokens})),
        }),
    }
}

/// Compares an error to the fixture's; a fixture without `message` leaves it out.
fn assert_error(got: &Value, want: &Value, at: &str) {
    for k in ["kind", "retryable", "status", "retry_after"] {
        assert_eq!(got[k], want[k], "{at}: {k}");
    }
    if want.get("message").is_some() {
        assert_eq!(got["message"], want["message"], "{at}: message");
    }
}

fn split_raw(raw: &str) -> (String, String, Vec<(String, String)>, String) {
    let (head, body) = raw.split_once("\r\n\r\n").unwrap();
    let mut lines = head.lines();
    let mut first = lines.next().unwrap().split(' ');
    let method = first.next().unwrap().to_string();
    let path = first.next().unwrap().to_string();
    let headers = lines
        .map(|l| {
            let (k, v) = l.split_once(':').unwrap();
            (k.to_ascii_lowercase(), v.trim().to_string())
        })
        .collect();
    (method, path, headers, body.to_string())
}

#[tokio::test]
async fn requests_on_the_wire() {
    let (key, cases) = load(REQUESTS);
    for c in cases {
        let at = c["name"].as_str().unwrap().to_string();
        let op = c["op"].as_str().unwrap();
        let script = if op == "chat_stream" {
            Script::sse(vec![b"data: [DONE]\n\n".to_vec()])
        } else {
            Script::json(200, "{}")
        };
        let s = serve(script).await;
        let client = Client::new(target(&c["target"], &s.url, &key));
        let sent = match op {
            "chat" => client.chat(chat_request(&c["request"])).await.err(),
            "embed" => client.embed(embed_request(&c["request"])).await.err(),
            _ => match client.chat_stream(chat_request(&c["request"])).await {
                Ok(st) => Box::pin(st)
                    .collect::<Vec<_>>()
                    .await
                    .into_iter()
                    .find_map(Result::err),
                Err(e) => Some(e),
            },
        };
        let want = &c["expect"];
        if !want["sent"].as_bool().unwrap() {
            assert!(s.requests().is_empty(), "{at}: nothing may be sent");
            assert_error(&error_json(&sent.expect(&at)), &want["error"], &at);
            continue;
        }
        let (method, path, headers, body) = split_raw(&s.only_raw());
        assert_eq!(method, want["method"].as_str().unwrap(), "{at}");
        assert_eq!(path, want["path"].as_str().unwrap(), "{at}");
        let header = |name: &str| {
            headers
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
        };
        for (k, v) in want["headers"].as_object().unwrap() {
            assert_eq!(header(k).as_deref(), v.as_str(), "{at}: header {k}");
        }
        if want["headers"].get("x-uf-tags").is_none() {
            assert_eq!(header("x-uf-tags"), None, "{at}: no tags header");
        }
        let auth = header(want["auth_header"].as_str().unwrap()).expect(&at);
        assert!(auth.contains(&key), "{at}: the key is in the auth header");
        let body: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(body, want["body"], "{at}: body");
    }
}

async fn answer(c: &Value, key: &str) -> Result<Value, Error> {
    let mut script = Script::json(
        c["status"].as_u64().unwrap() as u16,
        c["body"].as_str().unwrap(),
    );
    script.headers = c["headers"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string()))
        .collect();
    let s = serve(script).await;
    let client = Client::new(target(&c["target"], &s.url, key));
    let hi = json!({"model": c["model"].as_str().unwrap_or("m"), "messages": [{"role": "user", "content": "hi"}], "input": ["a"]});
    if c["op"] == "embed" {
        let r = client.embed(embed_request(&hi)).await?;
        Ok(json!({"model": r.model, "vectors": r.vectors, "prompt_tokens": r.prompt_tokens}))
    } else {
        let r = client.chat(chat_request(&hi)).await?;
        Ok(json!({
            "id": r.id, "model": r.model, "content": r.content,
            "finish_reason": r.finish_reason.map(|f| f.as_openai()),
            "usage": r.usage.map(|u| json!({"input_tokens": u.input_tokens, "output_tokens": u.output_tokens})),
        }))
    }
}

#[tokio::test]
async fn parsed_responses_and_their_errors() {
    let (key, cases) = load(RESPONSES);
    for c in cases {
        let at = c["name"].as_str().unwrap();
        match (answer(&c, &key).await, &c["expect"]) {
            (Ok(got), want) if want.get("ok").is_some() => assert_eq!(got, want["ok"], "{at}"),
            (Err(e), want) if want.get("error").is_some() => {
                assert_error(&error_json(&e), &want["error"], at)
            }
            (got, want) => panic!("{at}: got {got:?}, wanted {want}"),
        }
    }
}

#[tokio::test]
async fn http_errors() {
    let (key, cases) = load(ERRORS);
    for c in cases {
        let at = c["name"].as_str().unwrap();
        let e = answer(&c, &key).await.expect_err(at);
        assert_error(&error_json(&e), &c["expect"]["error"], at);
    }
}

async fn stream_of(c: &Value, chunks: Vec<Vec<u8>>, key: &str) -> (Vec<Value>, Option<Value>) {
    let s = serve(Script::sse(chunks)).await;
    let client = Client::new(target(&c["target"], &s.url, key));
    let hi = json!({"model": "m", "messages": [{"role": "user", "content": "hi"}]});
    let mut st = Box::pin(client.chat_stream(chat_request(&hi)).await.unwrap());
    let (mut events, mut error) = (Vec::new(), None);
    while let Some(item) = st.next().await {
        match item {
            Ok(e) => events.push(event_json(&e)),
            Err(e) => {
                assert!(st.next().await.is_none(), "nothing follows an error");
                error = Some(error_json(&e));
            }
        }
    }
    (events, error)
}

#[tokio::test]
async fn streams_in_pieces_and_whole() {
    let (key, cases) = load(STREAMS);
    for c in cases {
        let at = c["name"].as_str().unwrap();
        let pieces: Vec<Vec<u8>> = c["chunks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p.as_str().unwrap().as_bytes().to_vec())
            .filter(|p| !p.is_empty())
            .collect();
        let whole: Vec<u8> = pieces.concat();
        let whole = if whole.is_empty() {
            vec![]
        } else {
            vec![whole]
        };
        for (label, chunks) in [("pieces", pieces), ("whole", whole)] {
            let (events, error) = stream_of(&c, chunks, &key).await;
            let at = format!("{at} ({label})");
            assert_eq!(Value::Array(events), c["expect"]["events"], "{at}");
            match (&error, &c["expect"]["error"]) {
                (None, Value::Null) => {}
                (Some(got), want) if !want.is_null() => assert_error(got, want, &at),
                (got, want) => panic!("{at}: error {got:?}, wanted {want}"),
            }
        }
    }
}
