//! Anthropic Messages wire format, as received from and returned to callers.

use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::error::TranslateError;
use crate::types::{ChatRequest, ChatResponse, FinishReason, Message, Role, StreamEvent, Usage};

const ONLY_TEXT: &str = "Only text content is supported.";

#[derive(Deserialize)]
struct WireRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<WireMessage>,
    #[serde(default)]
    system: Option<WireContent>,
    #[serde(default)]
    temperature: Option<f32>,
    #[serde(default)]
    top_p: Option<f32>,
    #[serde(default)]
    stop_sequences: Option<Vec<String>>,
    #[serde(default)]
    stream: bool,
    /// Accepted and not used.
    #[serde(default)]
    #[allow(dead_code)]
    metadata: Option<Value>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

#[derive(Deserialize)]
struct WireMessage {
    role: String,
    content: WireContent,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum WireContent {
    Text(String),
    Blocks(Vec<Value>),
}

fn text_of(content: WireContent) -> Result<String, TranslateError> {
    match content {
        WireContent::Text(t) => Ok(t),
        WireContent::Blocks(blocks) => {
            let mut out = String::new();
            for b in &blocks {
                if b["type"] != "text" {
                    return Err(TranslateError::InvalidRequest(ONLY_TEXT.into()));
                }
                // Citations and cache hints change nothing in the text.
                match b["text"].as_str() {
                    Some(t) => out.push_str(t),
                    None => {
                        return Err(TranslateError::InvalidRequest(
                            "content block 'text' must be a string".into(),
                        ))
                    }
                }
            }
            Ok(out)
        }
    }
}

pub fn parse_request(body: &[u8]) -> Result<ChatRequest, TranslateError> {
    let wire: WireRequest =
        serde_json::from_slice(body).map_err(|e| TranslateError::InvalidRequest(e.to_string()))?;
    if let Some((field, _)) = wire.extra.iter().find(|(_, v)| !v.is_null()) {
        return Err(TranslateError::InvalidRequest(format!(
            "field '{field}' is not supported"
        )));
    }
    if wire.messages.is_empty() {
        return Err(TranslateError::InvalidRequest(
            "messages must not be empty".into(),
        ));
    }
    let mut messages = Vec::with_capacity(wire.messages.len() + 1);
    if let Some(system) = wire.system {
        messages.push(Message {
            role: Role::System,
            content: text_of(system)?,
            name: None,
        });
    }
    for m in wire.messages {
        let role = match m.role.as_str() {
            "user" => Role::User,
            "assistant" => Role::Assistant,
            other => {
                return Err(TranslateError::InvalidRequest(format!(
                    "message role '{other}' is not supported"
                )))
            }
        };
        messages.push(Message {
            role,
            content: text_of(m.content)?,
            name: None,
        });
    }
    Ok(ChatRequest {
        model: wire.model,
        messages,
        max_tokens: Some(wire.max_tokens),
        temperature: wire.temperature,
        top_p: wire.top_p,
        stop: wire.stop_sequences,
        stream: wire.stream,
    })
}

/// `end_turn` for every ordinary stop: the gateway does not learn which stop
/// sequence ended an answer.
fn stop_reason(f: Option<FinishReason>) -> Value {
    match f {
        None => Value::Null,
        Some(FinishReason::Stop) => json!("end_turn"),
        Some(FinishReason::Length) => json!("max_tokens"),
        Some(FinishReason::ToolCalls) => json!("tool_use"),
        Some(FinishReason::ContentFilter) => json!("refusal"),
    }
}

fn usage_json(u: Option<Usage>) -> Value {
    let u = u.unwrap_or(Usage {
        input_tokens: 0,
        output_tokens: 0,
    });
    json!({ "input_tokens": u.input_tokens, "output_tokens": u.output_tokens })
}

pub fn render_response(r: &ChatResponse) -> Value {
    json!({
        "id": r.id,
        "type": "message",
        "role": "assistant",
        "model": r.model,
        "content": [{ "type": "text", "text": r.content }],
        "stop_reason": stop_reason(r.finish_reason),
        "stop_sequence": null,
        "usage": usage_json(r.usage),
    })
}

/// The type Anthropic gives an error of this HTTP status.
pub fn error_type(status: u16) -> &'static str {
    match status {
        400 => "invalid_request_error",
        401 => "authentication_error",
        403 => "permission_error",
        404 => "not_found_error",
        413 => "request_too_large",
        429 => "rate_limit_error",
        503 | 529 => "overloaded_error",
        _ => "api_error",
    }
}

pub fn render_error(status: u16, message: &str) -> Value {
    json!({ "type": "error", "error": { "type": error_type(status), "message": message } })
}

fn event(name: &str, data: Value) -> String {
    format!("event: {name}\ndata: {data}\n\n")
}

pub fn render_stream_error(message: &str) -> String {
    event("error", render_error(500, message))
}

/// Renders a stream as Anthropic events. The message opens with the first
/// event, because the usage is only known at the end it reports 0 input
/// tokens there and gives both counts in the closing `message_delta`.
pub struct StreamRenderer {
    id: String,
    model: String,
    started: bool,
}

impl StreamRenderer {
    pub fn new(id: &str, model: &str) -> Self {
        Self {
            id: id.to_string(),
            model: model.to_string(),
            started: false,
        }
    }

    fn open(&mut self) -> String {
        if self.started {
            return String::new();
        }
        self.started = true;
        let start = event(
            "message_start",
            json!({ "type": "message_start", "message": {
                "id": self.id, "type": "message", "role": "assistant", "model": self.model,
                "content": [], "stop_reason": null, "stop_sequence": null,
                "usage": { "input_tokens": 0, "output_tokens": 0 },
            }}),
        );
        let block = event(
            "content_block_start",
            json!({ "type": "content_block_start", "index": 0,
                    "content_block": { "type": "text", "text": "" } }),
        );
        start + &block
    }

    pub fn render(&mut self, ev: &StreamEvent) -> String {
        let mut out = self.open();
        match ev {
            StreamEvent::Delta { text } => out.push_str(&event(
                "content_block_delta",
                json!({ "type": "content_block_delta", "index": 0,
                        "delta": { "type": "text_delta", "text": text } }),
            )),
            StreamEvent::Done {
                finish_reason,
                usage,
            } => {
                out.push_str(&event(
                    "content_block_stop",
                    json!({ "type": "content_block_stop", "index": 0 }),
                ));
                out.push_str(&event(
                    "message_delta",
                    json!({ "type": "message_delta",
                            "delta": { "stop_reason": stop_reason(*finish_reason), "stop_sequence": null },
                            "usage": usage_json(*usage) }),
                ));
                out.push_str(&event("message_stop", json!({ "type": "message_stop" })));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invalid(body: &[u8]) -> String {
        match parse_request(body) {
            Err(TranslateError::InvalidRequest(m)) => m,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_a_request_with_system_blocks() {
        let body = br#"{"model":"p/m","max_tokens":9,"system":[{"type":"text","text":"a"},{"type":"text","text":"b"}],
            "messages":[{"role":"user","content":"hi"},{"role":"assistant","content":[{"type":"text","text":"yo"}]}],
            "temperature":0.5,"top_p":0.9,"stop_sequences":["x"],"stream":true,"metadata":{"user_id":"u"}}"#;
        let r = parse_request(body).unwrap();
        assert_eq!(r.max_tokens, Some(9));
        assert_eq!(r.messages.len(), 3);
        assert_eq!(r.messages[0].role, Role::System);
        assert_eq!(r.messages[0].content, "ab");
        assert_eq!(r.messages[2].role, Role::Assistant);
        assert_eq!(r.messages[2].content, "yo");
        assert_eq!(r.stop, Some(vec!["x".into()]));
        assert!(r.stream);
        let r = parse_request(
            br#"{"model":"m","max_tokens":1,"system":"s","messages":[{"role":"user","content":"x"}]}"#,
        )
        .unwrap();
        assert_eq!(r.messages[0].content, "s");
    }

    #[test]
    fn max_tokens_is_required_and_unknown_fields_are_refused() {
        assert!(matches!(
            parse_request(br#"{"model":"m","messages":[{"role":"user","content":"x"}]}"#),
            Err(TranslateError::InvalidRequest(_))
        ));
        let m = invalid(
            br#"{"model":"m","max_tokens":1,"tools":[{}],"messages":[{"role":"user","content":"x"}]}"#,
        );
        assert!(m.contains("tools"), "{m}");
        invalid(br#"{"model":"m","max_tokens":1,"messages":[]}"#);
        invalid(br#"{"model":"m","max_tokens":1,"messages":[{"role":"system","content":"x"}]}"#);
    }

    #[test]
    fn non_text_blocks_are_refused() {
        let m = invalid(
            br#"{"model":"m","max_tokens":1,"messages":[{"role":"user","content":[{"type":"image","source":{}}]}]}"#,
        );
        assert_eq!(m, "Only text content is supported.");
        let m = invalid(
            br#"{"model":"m","max_tokens":1,"system":[{"type":"tool_use"}],"messages":[{"role":"user","content":"x"}]}"#,
        );
        assert_eq!(m, "Only text content is supported.");
    }

    #[test]
    fn renders_a_message() {
        let r = ChatResponse {
            id: "msg_1".into(),
            model: "m".into(),
            content: "hello".into(),
            finish_reason: Some(FinishReason::Length),
            usage: Some(Usage {
                input_tokens: 3,
                output_tokens: 2,
            }),
        };
        let v = render_response(&r);
        assert_eq!(v["type"], "message");
        assert_eq!(v["role"], "assistant");
        assert_eq!(v["content"][0], json!({"type":"text","text":"hello"}));
        assert_eq!(v["stop_reason"], "max_tokens");
        assert_eq!(v["usage"], json!({"input_tokens":3,"output_tokens":2}));
        let mut r = r;
        r.finish_reason = Some(FinishReason::Stop);
        assert_eq!(render_response(&r)["stop_reason"], "end_turn");
    }

    #[test]
    fn every_finish_reason_has_its_stop_reason() {
        for (reason, want) in [
            (Some(FinishReason::Stop), json!("end_turn")),
            (Some(FinishReason::Length), json!("max_tokens")),
            (Some(FinishReason::ContentFilter), json!("refusal")),
            (Some(FinishReason::ToolCalls), json!("tool_use")),
            (None, Value::Null),
        ] {
            let r = ChatResponse {
                id: "i".into(),
                model: "m".into(),
                content: String::new(),
                finish_reason: reason,
                usage: None,
            };
            assert_eq!(render_response(&r)["stop_reason"], want, "{reason:?}");
            let mut s = StreamRenderer::new("i", "m");
            let done = s.render(&StreamEvent::Done {
                finish_reason: reason,
                usage: None,
            });
            assert!(
                done.contains(&format!(r#""stop_reason":{want}"#)),
                "{reason:?}: {done}"
            );
        }
    }

    #[test]
    fn error_types_follow_the_status() {
        for (status, kind) in [
            (400, "invalid_request_error"),
            (401, "authentication_error"),
            (403, "permission_error"),
            (404, "not_found_error"),
            (429, "rate_limit_error"),
            (500, "api_error"),
            (502, "api_error"),
            (503, "overloaded_error"),
        ] {
            let v = render_error(status, "m");
            assert_eq!(v["type"], "error");
            assert_eq!(v["error"]["type"], kind, "{status}");
            assert_eq!(v["error"]["message"], "m");
        }
        assert!(render_stream_error("x").starts_with("event: error\ndata: {"));
    }

    #[test]
    fn renders_the_event_sequence() {
        let mut r = StreamRenderer::new("msg_1", "m");
        let a = r.render(&StreamEvent::Delta { text: "a".into() });
        let b = r.render(&StreamEvent::Delta { text: "b".into() });
        let c = r.render(&StreamEvent::Done {
            finish_reason: Some(FinishReason::Stop),
            usage: Some(Usage {
                input_tokens: 4,
                output_tokens: 2,
            }),
        });
        let all = format!("{a}{b}{c}");
        let names: Vec<&str> = all
            .lines()
            .filter_map(|l| l.strip_prefix("event: "))
            .collect();
        assert_eq!(
            names,
            [
                "message_start",
                "content_block_start",
                "content_block_delta",
                "content_block_delta",
                "content_block_stop",
                "message_delta",
                "message_stop"
            ]
        );
        assert!(c.contains(r#""stop_reason":"end_turn""#), "{c}");
        assert!(c.contains(r#""input_tokens":4"#), "{c}");
        // A stream that ends before any text still opens the message.
        let mut r = StreamRenderer::new("i", "m");
        let c = r.render(&StreamEvent::Done {
            finish_reason: None,
            usage: None,
        });
        assert!(c.starts_with("event: message_start"), "{c}");
        assert!(c.contains("event: message_stop"));
    }
}
