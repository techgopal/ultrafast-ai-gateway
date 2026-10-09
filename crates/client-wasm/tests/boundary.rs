//! The JSON boundary on the host: what the TypeScript client sends in and
//! reads out, for every provider kind.

use serde_json::{json, Value};
use ultrafast_client_wasm::api;

fn j(s: &str) -> Value {
    serde_json::from_str(s).unwrap()
}

fn target(kind: &str, base: &str) -> String {
    json!({"kind": kind, "base_url": base, "api_key": "sk-test-key-123"}).to_string()
}

fn chat(model: &str, stream: bool) -> Value {
    json!({
        "model": model,
        "messages": [{"role": "system", "content": "be brief"}, {"role": "user", "content": "hi"}],
        "max_tokens": 16,
        "stream": stream,
    })
}

const OPENAI_CHAT: &str = r#"{"id":"c1","model":"gpt-4o","choices":[{"message":{"role":"assistant","content":"hello"},"finish_reason":"stop"}],"usage":{"prompt_tokens":3,"completion_tokens":2}}"#;

#[test]
fn a_gateway_target_is_openai_format_under_v1_with_tags() {
    let mut req = chat("gpt-4o", false);
    req["tags"] = json!({"team": "search"});
    let out =
        j(&api::build_request(&target("gateway", "http://gw:3900/"), &req.to_string()).unwrap());
    assert_eq!(out["method"], "POST");
    assert_eq!(out["url"], "http://gw:3900/v1/chat/completions");
    assert_eq!(out["headers"]["authorization"], "Bearer sk-test-key-123");
    assert_eq!(out["headers"]["x-uf-tags"], r#"{"team":"search"}"#);
    let body = j(out["body"].as_str().unwrap());
    assert_eq!(body["model"], "gpt-4o");
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["max_tokens"], 16);
}

#[test]
fn tags_never_go_to_a_provider() {
    let mut req = chat("gpt-4o", false);
    req["tags"] = json!({"team": "search"});
    for kind in ["openai", "anthropic", "gemini", "azure"] {
        let base = if kind == "azure" {
            "https://r.openai.azure.com/openai/deployments/d"
        } else {
            "https://p.example/v1"
        };
        let out = j(&api::build_request(&target(kind, base), &req.to_string()).unwrap());
        assert!(out["headers"].get("x-uf-tags").is_none(), "{kind}: {out}");
    }
}

#[test]
fn every_kind_builds_its_own_request() {
    let url = |kind: &str, base: &str, model: &str, stream: bool| {
        let out =
            j(&api::build_request(&target(kind, base), &chat(model, stream).to_string()).unwrap());
        out["url"].as_str().unwrap().to_string()
    };
    assert_eq!(
        url("openai", "https://api.openai.com/v1", "gpt-4o", false),
        "https://api.openai.com/v1/chat/completions"
    );
    assert_eq!(
        url("anthropic", "https://api.anthropic.com", "claude-x", false),
        "https://api.anthropic.com/v1/messages"
    );
    assert!(
        url("gemini", "https://g.example", "gemini-2", false).contains("gemini-2:generateContent")
    );
    assert!(url("gemini", "https://g.example", "gemini-2", true).contains("streamGenerateContent"));
}

#[test]
fn a_stream_request_asks_for_event_stream() {
    let out = j(&api::build_request(
        &target("openai", "https://p/v1"),
        &chat("m", true).to_string(),
    )
    .unwrap());
    assert_eq!(out["headers"]["accept"], "text/event-stream");
    assert_eq!(j(out["body"].as_str().unwrap())["stream"], true);
}

#[test]
fn the_azure_api_version_is_passed() {
    let t = json!({"kind":"azure","base_url":"https://r.openai.azure.com/openai/deployments","api_key":"k","api_version":"2025-01-01"}).to_string();
    let out = j(&api::build_request(&t, &chat("dep", false).to_string()).unwrap());
    assert!(
        out["url"]
            .as_str()
            .unwrap()
            .contains("api-version=2025-01-01"),
        "{out}"
    );
}

#[test]
fn bad_input_is_a_typed_invalid_request_error() {
    for (t, r) in [
        ("not json".to_string(), chat("m", false).to_string()),
        (target("nope", "https://p"), chat("m", false).to_string()),
        (target("openai", "https://p"), "{".to_string()),
        (
            target("openai", "https://p"),
            json!({"model":"m","messages":[{"role":"robot","content":"x"}]}).to_string(),
        ),
    ] {
        let e = j(&api::build_request(&t, &r).unwrap_err());
        assert_eq!(e["kind"], "invalid_request", "{e}");
        assert_eq!(e["retryable"], false);
    }
}

#[test]
fn a_response_parses_to_json_with_openai_finish_names() {
    let r = j(&api::parse_response("openai", 200, OPENAI_CHAT.as_bytes(), None).unwrap());
    assert_eq!(r["id"], "c1");
    assert_eq!(r["content"], "hello");
    assert_eq!(r["finish_reason"], "stop");
    assert_eq!(r["usage"], json!({"input_tokens": 3, "output_tokens": 2}));
}

#[test]
fn an_error_answer_throws_the_classified_json() {
    let e = j(&api::parse_response(
        "openai",
        429,
        br#"{"error":{"message":"slow down"}}"#,
        Some("7"),
    )
    .unwrap_err());
    assert_eq!(e["kind"], "rate_limited");
    assert_eq!(e["retryable"], true);
    assert_eq!(e["status"], 429);
    assert_eq!(e["message"], "slow down");
    assert_eq!(e["retry_after_secs"], 7);
    let e = j(&api::parse_response("openai", 200, b"<html>", None).unwrap_err());
    assert_eq!(e["kind"], "malformed");
    assert_eq!(e["status"], Value::Null);
}

#[test]
fn classify_error_matches_the_rust_client_mapping() {
    for (status, kind, retry) in [
        (401, "auth", false),
        (403, "permission", false),
        (404, "not_found", false),
        (408, "timeout", true),
        (400, "invalid_request", false),
        (429, "rate_limited", true),
        (500, "upstream", true),
        (302, "invalid_request", false),
    ] {
        let e = j(&api::classify_error(status, b"{}", None));
        assert_eq!(
            (
                e["kind"].as_str().unwrap(),
                e["retryable"].as_bool().unwrap()
            ),
            (kind, retry),
            "{status}"
        );
    }
    assert_eq!(
        j(&api::classify_error(429, b"{}", Some("99999999999")))["retry_after_secs"],
        86400
    );
    assert_eq!(
        j(&api::classify_error(429, b"{}", Some("Wed, 21 Oct")))["retry_after_secs"],
        Value::Null
    );
}

#[test]
fn embeddings_build_and_parse() {
    let req =
        json!({"model":"text-embedding-3-small","input":["a","b"],"dimensions":8}).to_string();
    let out = j(&api::build_embeddings_request(&target("openai", "https://p/v1"), &req).unwrap());
    assert_eq!(out["url"], "https://p/v1/embeddings");
    let body = j(out["body"].as_str().unwrap());
    assert_eq!(body["input"], json!(["a", "b"]));
    assert_eq!(body["dimensions"], 8);
    let resp = br#"{"model":"m","data":[{"index":1,"embedding":[0.5]},{"index":0,"embedding":[0.25]}],"usage":{"prompt_tokens":4}}"#;
    let r = j(&api::parse_embeddings("openai", 200, resp, "fallback", None).unwrap());
    assert_eq!(r["vectors"], json!([[0.25], [0.5]]));
    assert_eq!(r["prompt_tokens"], 4);
    let e = j(&api::build_embeddings_request(&target("anthropic", "https://a"), &req).unwrap_err());
    assert_eq!(e["kind"], "invalid_request");
}

#[test]
fn tags_header_is_shared_with_the_rust_client() {
    assert_eq!(api::tags_header("{}").unwrap(), None);
    assert_eq!(
        api::tags_header(r#"{"a":"é"}"#).unwrap().as_deref(),
        Some("{\"a\":\"\\u00e9\"}")
    );
    let e = j(&api::tags_header(&json!({"k":"x".repeat(2000)}).to_string()).unwrap_err());
    assert_eq!(e["kind"], "invalid_request");
}

#[test]
fn the_stream_decoder_survives_chunks_split_anywhere() {
    let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"he\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"llo\"}}]}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2}}\n\ndata: [DONE]\n\n";
    let mut d = api::Decoder::new("openai").unwrap();
    let mut events = Vec::new();
    for b in sse.as_bytes().chunks(1) {
        events.extend(j(&d.feed(b).unwrap()).as_array().unwrap().clone());
    }
    events.extend(j(&d.finish()).as_array().unwrap().clone());
    assert!(d.take_error().is_none());
    let text: String = events
        .iter()
        .filter(|e| e["type"] == "delta")
        .map(|e| e["text"].as_str().unwrap())
        .collect();
    assert_eq!(text, "hello");
    let last = events.last().unwrap();
    assert_eq!(last["type"], "done");
    assert_eq!(last["finish_reason"], "stop");
    assert_eq!(
        last["usage"],
        json!({"input_tokens": 3, "output_tokens": 2})
    );
}

#[test]
fn a_decoder_error_after_text_is_text_then_take_error() {
    let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\ndata: {not json}\n\n";
    let mut d = api::Decoder::new("openai").unwrap();
    let events = j(&d.feed(sse.as_bytes()).unwrap());
    assert_eq!(events[0]["text"], "hi");
    let e = j(&d.take_error().unwrap());
    assert_eq!(e["kind"], "malformed");
    assert!(d.take_error().is_none(), "given once");
}

#[test]
fn a_decoder_error_with_no_events_is_thrown_from_feed() {
    let mut d = api::Decoder::new("openai").unwrap();
    let e = j(&d.feed(b"data: {not json}\n\n").unwrap_err());
    assert_eq!(e["kind"], "malformed");
}

#[test]
fn an_unknown_kind_is_refused() {
    assert!(api::Decoder::new("nope").is_err());
    assert!(api::parse_response("nope", 200, b"{}", None).is_err());
}

#[test]
fn host_errors_use_the_shared_kinds() {
    let e = j(&api::host_error("network", "could not connect").unwrap());
    assert_eq!(
        (e["kind"].clone(), e["retryable"].clone()),
        (json!("network"), json!(true))
    );
    let e = j(&api::host_error("malformed", "cut").unwrap());
    assert_eq!(e["retryable"], false);
    assert!(api::host_error("bogus", "x").is_err());
}

#[test]
fn scrub_is_shared_and_retry_after_is_read_on_429_and_503() {
    assert_eq!(api::scrub("bad key-1", "key-1"), "bad [redacted]");
    assert_eq!(api::scrub("untouched", ""), "untouched");
    assert_eq!(
        j(&api::classify_error(503, b"{}", Some("5")))["retry_after_secs"],
        5
    );
    assert!(j(&api::classify_error(500, b"{}", Some("5")))["retry_after_secs"].is_null());
}

#[test]
fn a_gateway_base_ending_in_v1_is_not_doubled() {
    for base in ["http://gw:3900/v1", "http://gw:3900/v1/", "http://gw:3900"] {
        let out = j(
            &api::build_request(&target("gateway", base), &chat("m", false).to_string()).unwrap(),
        );
        assert_eq!(out["url"], "http://gw:3900/v1/chat/completions", "{base}");
    }
}

#[test]
fn a_gateway_builds_embeddings_under_v1_with_tags_and_never_for_a_provider() {
    let req = json!({"model": "te3", "input": ["a", "b"], "dimensions": 8, "tags": {"team": "x"}});
    let out = j(&api::build_embeddings_request(
        &target("gateway", "http://gw:3900"),
        &req.to_string(),
    )
    .unwrap());
    assert_eq!(out["url"], "http://gw:3900/v1/embeddings");
    assert_eq!(out["headers"]["authorization"], "Bearer sk-test-key-123");
    assert_eq!(out["headers"]["x-uf-tags"], r#"{"team":"x"}"#);
    let body = j(out["body"].as_str().unwrap());
    assert_eq!(body["model"], "te3");
    assert_eq!(body["input"], json!(["a", "b"]));
    assert_eq!(body["dimensions"], 8);
    let out = j(&api::build_embeddings_request(
        &target("openai", "https://p.example/v1"),
        &req.to_string(),
    )
    .unwrap());
    assert!(out["headers"].get("x-uf-tags").is_none());
}

#[test]
fn scrub_error_removes_the_key_from_thrown_json() {
    let key = "sk-test-key-123";
    let body = format!(r#"{{"error":{{"message":"bad key {key} ({key})"}}}}"#);
    let thrown = api::classify_error(401, body.as_bytes(), None);
    assert!(thrown.contains(key));
    let clean = api::scrub_error(&thrown, key);
    assert!(!clean.contains(key), "{clean}");
    let e = j(&clean);
    assert_eq!(e["message"], "bad key [redacted] ([redacted])");
    assert_eq!(e["kind"], "auth");
    assert_eq!(e["status"], 401);
    // A key with characters JSON escapes is still removed.
    let odd = "k\"ey\\1";
    let thrown = api::host_error("network", &format!("saw {odd} here")).unwrap();
    assert_eq!(
        j(&api::scrub_error(&thrown, odd))["message"],
        "saw [redacted] here"
    );
    // Anything that is not an error JSON is scrubbed as text.
    assert_eq!(
        api::scrub_error("oops sk-test-key-123", key),
        "oops [redacted]"
    );
    assert_eq!(api::scrub_error(&thrown, ""), thrown);
}

#[test]
fn the_decoder_says_whether_the_stream_completed() {
    let mut d = api::Decoder::new("openai").unwrap();
    assert!(!d.is_done());
    d.feed(b"data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n")
        .unwrap();
    d.finish();
    assert!(!d.is_done(), "text alone is not a complete stream");
    d.feed(b"data: [DONE]\n\n").unwrap();
    assert!(d.is_done());
    // Gemini's Done comes from finish().
    let mut g = api::Decoder::new("gemini").unwrap();
    g.feed(b"data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hi\"}]},\"finishReason\":\"STOP\"}]}\n\n").unwrap();
    assert!(!g.is_done());
    assert!(j(&g.finish())
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["type"] == "done"));
    assert!(g.is_done());
}

#[test]
fn a_redirect_answer_says_the_same_thing_everywhere() {
    let e = j(&api::classify_error(302, b"<html></html>", Some("5")));
    assert_eq!(
        e["message"],
        ultrafast_translate::classify::REDIRECT_MESSAGE
    );
    assert_eq!(e["kind"], "invalid_request");
    let e = j(&api::parse_response("openai", 301, b"", None).unwrap_err());
    assert_eq!(
        e["message"],
        ultrafast_translate::classify::REDIRECT_MESSAGE
    );
}

fn tool_req() -> Value {
    json!({
        "model": "gpt-4o",
        "messages": [
            {"role": "user", "content": [
                {"type": "text", "text": "what is this"},
                {"type": "image", "url": "data:image/png;base64,AAAA"},
            ]},
            {"role": "assistant", "content": null,
             "tool_calls": [{"id": "call_1", "name": "weather", "arguments": "{\"city\":\"Paris\"}"}]},
            {"role": "tool", "content": "sunny", "tool_call_id": "call_1"},
        ],
        "tools": [{"name": "weather", "description": "Current weather",
                   "parameters": {"type": "object", "properties": {"city": {"type": "string"}}}}],
        "tool_choice": {"name": "weather"},
        "parallel_tool_calls": false,
    })
}

#[test]
fn tools_images_and_tool_messages_build_the_openai_body() {
    let out = j(&api::build_request(
        &target("gateway", "http://gw:3900"),
        &tool_req().to_string(),
    )
    .unwrap());
    let body = j(out["body"].as_str().unwrap());
    assert_eq!(body["messages"][0]["content"][1]["type"], "image_url");
    assert_eq!(
        body["messages"][0]["content"][1]["image_url"]["url"],
        "data:image/png;base64,AAAA"
    );
    assert_eq!(body["messages"][1]["tool_calls"][0]["id"], "call_1");
    assert_eq!(body["messages"][2]["role"], "tool");
    assert_eq!(body["messages"][2]["tool_call_id"], "call_1");
    assert_eq!(body["tools"][0]["function"]["name"], "weather");
    assert_eq!(body["tool_choice"]["function"]["name"], "weather");
    assert_eq!(body["parallel_tool_calls"], false);
}

#[test]
fn the_same_request_builds_for_a_direct_provider() {
    let out = j(&api::build_request(
        &target("anthropic", "https://api.anthropic.com"),
        &tool_req().to_string(),
    )
    .unwrap());
    let body = j(out["body"].as_str().unwrap());
    assert_eq!(body["tools"][0]["name"], "weather");
    assert_eq!(body["messages"][0]["content"][1]["type"], "image");
}

#[test]
fn a_tool_message_without_a_tool_call_id_is_invalid() {
    let mut req = tool_req();
    req["messages"][2]
        .as_object_mut()
        .unwrap()
        .remove("tool_call_id");
    let e = j(
        &api::build_request(&target("openai", "https://p.example/v1"), &req.to_string())
            .unwrap_err(),
    );
    assert_eq!(e["kind"], "invalid_request");
    assert_eq!(e["retryable"], false);
    assert!(
        e["message"].as_str().unwrap().contains("tool_call_id"),
        "{e}"
    );
}

#[test]
fn bad_images_tools_and_choices_are_invalid_requests() {
    let build = |edit: &dyn Fn(&mut Value)| {
        let mut req = tool_req();
        edit(&mut req);
        j(
            &api::build_request(&target("openai", "https://p.example/v1"), &req.to_string())
                .unwrap_err(),
        )
    };
    for e in [
        build(&|r| r["messages"][0]["content"][1]["url"] = json!("ftp://x/a.png")),
        build(&|r| r["messages"][0]["content"][1]["type"] = json!("audio")),
        build(&|r| r["tool_choice"] = json!("sometimes")),
        build(&|r| r["tools"][0]["name"] = json!("")),
        build(&|r| {
            r["messages"][0]["tool_calls"] = json!([{"id":"x","name":"n","arguments":"{}"}])
        }),
    ] {
        assert_eq!(e["kind"], "invalid_request", "{e}");
    }
    // string forms of tool_choice
    for c in ["auto", "none", "required"] {
        let mut req = tool_req();
        req["tool_choice"] = json!(c);
        api::build_request(&target("openai", "https://p.example/v1"), &req.to_string()).unwrap();
    }
}

#[test]
fn a_text_message_in_the_old_shape_still_builds() {
    let req = json!({"model": "m", "messages": [{"role": "user", "content": "hi", "name": "bob"}]});
    let out = j(
        &api::build_request(&target("openai", "https://p.example/v1"), &req.to_string()).unwrap(),
    );
    let body = j(out["body"].as_str().unwrap());
    assert_eq!(body["messages"][0]["content"], "hi");
    assert_eq!(body["messages"][0]["name"], "bob");
}

#[test]
fn a_tool_call_answer_and_stream_come_out_as_json() {
    let answer = r#"{"id":"c2","model":"gpt-4o","choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":"call_1","type":"function","function":{"name":"weather","arguments":"{\"city\":\"Paris\"}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":9,"completion_tokens":4}}"#;
    let r = j(&api::parse_response("openai", 200, answer.as_bytes(), None).unwrap());
    assert_eq!(r["finish_reason"], "tool_calls");
    assert_eq!(
        r["tool_calls"],
        json!([{"id": "call_1", "name": "weather", "arguments": "{\"city\":\"Paris\"}"}])
    );

    let sse = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"weather\",\"arguments\":\"\"}}]}}]}\n\ndata: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"a\\\":1}\"}}]}}]}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\ndata: [DONE]\n\n";
    let mut d = api::Decoder::new("openai").unwrap();
    let mut events = j(&d.feed(sse.as_bytes()).unwrap())
        .as_array()
        .unwrap()
        .clone();
    events.extend(j(&d.finish()).as_array().unwrap().clone());
    assert_eq!(
        events[0],
        json!({"type": "tool_call_start", "index": 0, "id": "call_1", "name": "weather"})
    );
    assert_eq!(
        events[1],
        json!({"type": "tool_call_delta", "index": 0, "arguments": "{\"a\":1}"})
    );
    assert_eq!(events.last().unwrap()["type"], "done");
}

#[test]
fn response_format_builds_for_a_gateway_and_for_a_direct_provider() {
    let mut req = tool_req();
    req["tools"] = json!([]);
    req["tool_choice"] = Value::Null;
    req["parallel_tool_calls"] = Value::Null;
    req["response_format"] = json!({"type": "json_schema", "json_schema": {
        "name": "n", "schema": {"type": "object"}, "strict": true}});
    let out =
        j(&api::build_request(&target("gateway", "http://gw:3900"), &req.to_string()).unwrap());
    let body = j(out["body"].as_str().unwrap());
    assert_eq!(body["response_format"], req["response_format"]);
    let out = j(&api::build_request(
        &target("anthropic", "https://api.anthropic.com"),
        &req.to_string(),
    )
    .unwrap());
    let body = j(out["body"].as_str().unwrap());
    assert_eq!(
        body["output_config"],
        json!({"format": {"type": "json_schema", "schema": {"type": "object"}}})
    );
    req["response_format"] = json!({"type": "xml"});
    assert!(api::build_request(&target("gateway", "http://gw:3900"), &req.to_string()).is_err());
}
