//! OpenAI Chat Completions wire format, as received from and returned to callers.

use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::error::TranslateError;
use crate::types::{ChatRequest, ChatResponse, Message, Role, StreamEvent, Usage};

#[derive(Deserialize)]
struct WireRequest {
    model: String,
    messages: Vec<WireMessage>,
    #[serde(default)]
    max_tokens: Option<u32>,
    #[serde(default)]
    max_completion_tokens: Option<u32>,
    #[serde(default)]
    temperature: Option<f32>,
    #[serde(default)]
    top_p: Option<f32>,
    #[serde(default)]
    stop: Option<StopField>,
    #[serde(default)]
    stream: bool,
    #[serde(default)]
    tools: Option<Value>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

/// Request fields that change the output and that `ChatRequest` cannot carry.
const UNSUPPORTED_REQUEST_FIELDS: &[&str] = &[
    "tool_choice",
    "functions",
    "function_call",
    "response_format",
    "logit_bias",
    "logprobs",
    "top_logprobs",
    "presence_penalty",
    "frequency_penalty",
    "seed",
    "audio",
    "modalities",
    "prediction",
    "reasoning_effort",
];

/// Message fields that change the output and that `Message` cannot carry.
const UNSUPPORTED_MESSAGE_FIELDS: &[&str] = &["tool_calls", "function_call", "audio"];

fn reject_present(fields: &[&str], extra: &Map<String, Value>) -> Result<(), TranslateError> {
    for field in fields {
        if extra.get(*field).is_some_and(|v| !v.is_null()) {
            return Err(unsupported_field(field));
        }
    }
    Ok(())
}

fn unsupported_field(field: &str) -> TranslateError {
    TranslateError::Unsupported(format!("field '{field}' is not supported yet"))
}

#[derive(Deserialize)]
#[serde(untagged)]
enum StopField {
    One(String),
    Many(Vec<String>),
}

#[derive(Deserialize)]
struct WireMessage {
    role: String,
    #[serde(default)]
    content: Option<WireContent>,
    #[serde(default)]
    name: Option<String>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum WireContent {
    Text(String),
    Parts(Vec<Value>),
}

pub fn parse_request(body: &[u8]) -> Result<ChatRequest, TranslateError> {
    let wire: WireRequest =
        serde_json::from_slice(body).map_err(|e| TranslateError::InvalidRequest(e.to_string()))?;
    if wire.tools.is_some() {
        return Err(TranslateError::Unsupported(
            "tools are not supported yet".into(),
        ));
    }
    reject_present(UNSUPPORTED_REQUEST_FIELDS, &wire.extra)?;
    if let Some(n) = wire.extra.get("n") {
        if !n.is_null() && n.as_u64() != Some(1) {
            return Err(unsupported_field("n"));
        }
    }
    if wire.messages.is_empty() {
        return Err(TranslateError::InvalidRequest(
            "messages must not be empty".into(),
        ));
    }
    let mut messages = Vec::with_capacity(wire.messages.len());
    for m in wire.messages {
        let role = match m.role.as_str() {
            "system" | "developer" => Role::System,
            "user" => Role::User,
            "assistant" => Role::Assistant,
            other => {
                return Err(TranslateError::Unsupported(format!(
                    "message role '{other}' is not supported yet"
                )))
            }
        };
        reject_present(UNSUPPORTED_MESSAGE_FIELDS, &m.extra)?;
        let content = match m.content {
            None => {
                return Err(TranslateError::InvalidRequest(
                    "message content is required".into(),
                ))
            }
            Some(WireContent::Text(t)) => t,
            Some(WireContent::Parts(parts)) => {
                let mut out = String::new();
                for p in parts {
                    match (p["type"].as_str(), p["text"].as_str()) {
                        (Some("text"), Some(t)) => out.push_str(t),
                        (kind, _) => {
                            return Err(TranslateError::Unsupported(format!(
                                "content part '{}' is not supported yet",
                                kind.unwrap_or("unknown")
                            )))
                        }
                    }
                }
                out
            }
        };
        messages.push(Message {
            role,
            content,
            name: m.name,
        });
    }
    Ok(ChatRequest {
        model: wire.model,
        messages,
        max_tokens: wire.max_completion_tokens.or(wire.max_tokens),
        temperature: wire.temperature,
        top_p: wire.top_p,
        stop: wire.stop.map(|s| match s {
            StopField::One(s) => vec![s],
            StopField::Many(v) => v,
        }),
        stream: wire.stream,
    })
}

/// The total is 64-bit: both counts may be saturated at `u32::MAX`.
fn usage_json(u: Usage) -> Value {
    json!({
        "prompt_tokens": u.input_tokens,
        "completion_tokens": u.output_tokens,
        "total_tokens": u64::from(u.input_tokens) + u64::from(u.output_tokens),
    })
}

pub fn render_response(r: &ChatResponse, created: u64) -> Value {
    let mut v = json!({
        "id": r.id,
        "object": "chat.completion",
        "created": created,
        "model": r.model,
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": r.content },
            "finish_reason": r.finish_reason.map(|f| f.as_openai()),
        }],
    });
    if let Some(u) = r.usage {
        v["usage"] = usage_json(u);
    }
    v
}

pub fn render_stream_event(ev: &StreamEvent, id: &str, model: &str, created: u64) -> String {
    match ev {
        StreamEvent::Delta { text } => {
            let v = json!({
                "id": id, "object": "chat.completion.chunk", "created": created, "model": model,
                "choices": [{ "index": 0, "delta": { "content": text }, "finish_reason": null }],
            });
            format!("data: {v}\n\n")
        }
        StreamEvent::Done {
            finish_reason,
            usage,
        } => {
            let mut v = json!({
                "id": id, "object": "chat.completion.chunk", "created": created, "model": model,
                "choices": [{ "index": 0, "delta": {}, "finish_reason": finish_reason.map(|f| f.as_openai()) }],
            });
            if let Some(u) = usage {
                v["usage"] = usage_json(*u);
            }
            format!("data: {v}\n\ndata: [DONE]\n\n")
        }
    }
}

pub fn render_error(kind: &str, message: &str) -> Value {
    json!({ "error": { "message": message, "type": kind, "param": null, "code": null } })
}

pub fn render_stream_error(message: &str) -> String {
    format!("data: {}\n\n", render_error("upstream_error", message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::TranslateError;
    use crate::types::*;

    #[test]
    fn parses_minimal_request() {
        let body = br#"{"model":"openai/gpt-4o","messages":[{"role":"user","content":"hi"}]}"#;
        let req = parse_request(body).unwrap();
        assert_eq!(req.model, "openai/gpt-4o");
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.messages[0].role, Role::User);
        assert_eq!(req.messages[0].content, "hi");
        assert!(!req.stream);
    }

    #[test]
    fn parses_text_parts_and_single_stop() {
        let body = br#"{"model":"m","stream":true,"stop":"END","max_completion_tokens":9,
            "messages":[{"role":"developer","content":[{"type":"text","text":"a"},{"type":"text","text":"b"}]}]}"#;
        let req = parse_request(body).unwrap();
        assert_eq!(req.messages[0].role, Role::System);
        assert_eq!(req.messages[0].content, "ab");
        assert_eq!(req.stop, Some(vec!["END".to_string()]));
        assert_eq!(req.max_tokens, Some(9));
        assert!(req.stream);
    }

    #[test]
    fn rejects_tools_and_images_instead_of_dropping_them() {
        let tools = br#"{"model":"m","messages":[{"role":"user","content":"x"}],"tools":[{"type":"function"}]}"#;
        assert!(matches!(
            parse_request(tools),
            Err(TranslateError::Unsupported(_))
        ));
        let image = br#"{"model":"m","messages":[{"role":"user","content":[{"type":"image_url","image_url":{"url":"u"}}]}]}"#;
        assert!(matches!(
            parse_request(image),
            Err(TranslateError::Unsupported(_))
        ));
        let tool_msg = br#"{"model":"m","messages":[{"role":"tool","content":"x"}]}"#;
        assert!(matches!(
            parse_request(tool_msg),
            Err(TranslateError::Unsupported(_))
        ));
    }

    #[test]
    fn rejects_bad_json_and_empty_messages() {
        assert!(matches!(
            parse_request(b"{"),
            Err(TranslateError::InvalidRequest(_))
        ));
        let empty = br#"{"model":"m","messages":[]}"#;
        assert!(matches!(
            parse_request(empty),
            Err(TranslateError::InvalidRequest(_))
        ));
    }

    #[test]
    fn renders_response() {
        let r = ChatResponse {
            id: "id1".into(),
            model: "gpt-4o".into(),
            content: "hello".into(),
            finish_reason: Some(FinishReason::Stop),
            usage: Some(Usage {
                input_tokens: 3,
                output_tokens: 2,
            }),
        };
        let v = render_response(&r, 100);
        assert_eq!(v["object"], "chat.completion");
        assert_eq!(v["created"], 100);
        assert_eq!(v["choices"][0]["message"]["content"], "hello");
        assert_eq!(v["choices"][0]["finish_reason"], "stop");
        assert_eq!(v["usage"]["prompt_tokens"], 3);
        assert_eq!(v["usage"]["completion_tokens"], 2);
        assert_eq!(v["usage"]["total_tokens"], 5);
    }

    #[test]
    fn renders_stream_events() {
        let d = render_stream_event(
            &StreamEvent::Delta {
                text: "a\"b".into(),
            },
            "id1",
            "m",
            1,
        );
        assert!(d.starts_with("data: "));
        assert!(d.ends_with("\n\n"));
        let v: serde_json::Value = serde_json::from_str(d["data: ".len()..].trim()).unwrap();
        assert_eq!(v["object"], "chat.completion.chunk");
        assert_eq!(v["choices"][0]["delta"]["content"], "a\"b");

        let done = render_stream_event(
            &StreamEvent::Done {
                finish_reason: Some(FinishReason::Length),
                usage: None,
            },
            "id1",
            "m",
            1,
        );
        assert!(done.contains("\"finish_reason\":\"length\""));
        assert!(done.ends_with("data: [DONE]\n\n"));
    }

    #[test]
    fn error_text_is_escaped() {
        let v = render_error("invalid_request_error", "bad \"quote\"");
        assert_eq!(v["error"]["message"], "bad \"quote\"");
        assert_eq!(v["error"]["type"], "invalid_request_error");
        let s = render_stream_error("x\ny");
        let v: serde_json::Value = serde_json::from_str(s["data: ".len()..].trim()).unwrap();
        assert_eq!(v["error"]["message"], "x\ny");
    }

    fn unsupported_message(body: &str) -> String {
        match parse_request(body.as_bytes()) {
            Err(TranslateError::Unsupported(m)) => m,
            other => panic!("expected Unsupported for {body}, got {other:?}"),
        }
    }

    #[test]
    fn rejects_every_unsupported_top_level_field() {
        let cases = [
            ("tool_choice", r#""auto""#),
            ("functions", r#"[{"name":"f"}]"#),
            ("function_call", r#""auto""#),
            ("response_format", r#"{"type":"json_object"}"#),
            ("logit_bias", r#"{"50256":-100}"#),
            ("logprobs", "true"),
            ("top_logprobs", "2"),
            ("presence_penalty", "0.5"),
            ("frequency_penalty", "0.5"),
            ("seed", "7"),
            ("audio", r#"{"voice":"alloy","format":"wav"}"#),
            ("modalities", r#"["text","audio"]"#),
            ("prediction", r#"{"type":"content","content":"x"}"#),
            ("reasoning_effort", r#""low""#),
        ];
        for (field, value) in cases {
            let body = format!(
                r#"{{"model":"m","messages":[{{"role":"user","content":"x"}}],"{field}":{value}}}"#
            );
            let msg = unsupported_message(&body);
            assert!(msg.contains(&format!("'{field}'")), "{field}: {msg}");

            let null_body = format!(
                r#"{{"model":"m","messages":[{{"role":"user","content":"x"}}],"{field}":null}}"#
            );
            assert!(parse_request(null_body.as_bytes()).is_ok(), "{field} null");
        }
    }

    #[test]
    fn rejects_n_other_than_one() {
        let body = |n: &str| {
            format!(r#"{{"model":"m","messages":[{{"role":"user","content":"x"}}],"n":{n}}}"#)
        };
        assert!(parse_request(body("1").as_bytes()).is_ok());
        assert!(parse_request(body("null").as_bytes()).is_ok());
        for n in ["2", "0", "\"1\"", "1.5"] {
            let msg = unsupported_message(&body(n));
            assert!(msg.contains("'n'"), "{n}: {msg}");
        }
    }

    #[test]
    fn rejects_unsupported_message_fields() {
        let cases = [
            (
                "tool_calls",
                r#"[{"id":"c","type":"function","function":{"name":"f","arguments":"{}"}}]"#,
            ),
            ("function_call", r#"{"name":"f","arguments":"{}"}"#),
            ("audio", r#"{"id":"a1"}"#),
        ];
        for (field, value) in cases {
            let body = format!(
                r#"{{"model":"m","messages":[{{"role":"assistant","content":"x","{field}":{value}}}]}}"#
            );
            let msg = unsupported_message(&body);
            assert!(msg.contains(&format!("'{field}'")), "{field}: {msg}");

            let null_body = format!(
                r#"{{"model":"m","messages":[{{"role":"assistant","content":"x","{field}":null}}]}}"#
            );
            assert!(parse_request(null_body.as_bytes()).is_ok(), "{field} null");
        }
    }

    #[test]
    fn rejects_missing_or_null_content() {
        for body in [
            r#"{"model":"m","messages":[{"role":"assistant"}]}"#,
            r#"{"model":"m","messages":[{"role":"assistant","content":null}]}"#,
            r#"{"model":"m","messages":[{"role":"user","content":null}]}"#,
        ] {
            match parse_request(body.as_bytes()) {
                Err(TranslateError::InvalidRequest(m)) => {
                    assert_eq!(m, "message content is required")
                }
                other => panic!("expected InvalidRequest for {body}, got {other:?}"),
            }
        }
    }

    #[test]
    fn accepts_and_ignores_fields_that_do_not_change_output() {
        let body = br#"{"model":"m","messages":[{"role":"user","content":"x"}],
            "user":"u1","metadata":{"k":"v"},"store":true,"stream_options":{"include_usage":true},
            "service_tier":"auto","parallel_tool_calls":false,"some_future_field":123}"#;
        let req = parse_request(body).unwrap();
        assert_eq!(req.messages[0].content, "x");
    }

    #[test]
    fn saturated_token_counts_do_not_overflow_the_total() {
        let usage = Some(Usage {
            input_tokens: u32::MAX,
            output_tokens: u32::MAX,
        });
        let r = ChatResponse {
            id: "id1".into(),
            model: "m".into(),
            content: "x".into(),
            finish_reason: Some(FinishReason::Stop),
            usage,
        };
        let v = render_response(&r, 1);
        assert_eq!(v["usage"]["prompt_tokens"], u32::MAX);
        assert_eq!(v["usage"]["completion_tokens"], u32::MAX);
        assert_eq!(v["usage"]["total_tokens"], 8589934590u64);

        let done = render_stream_event(
            &StreamEvent::Done {
                finish_reason: Some(FinishReason::Stop),
                usage,
            },
            "id1",
            "m",
            1,
        );
        let first = done.lines().next().unwrap();
        let v: serde_json::Value = serde_json::from_str(&first["data: ".len()..]).unwrap();
        assert_eq!(v["usage"]["total_tokens"], 8589934590u64);
    }
}
