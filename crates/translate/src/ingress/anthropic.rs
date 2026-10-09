//! Anthropic Messages wire format, as received from and returned to callers.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::error::TranslateError;
use crate::types::{
    base64_source, image_source, ChatRequest, ChatResponse, FinishReason, ImageSource, Message,
    Part, ResponseFormat, Role, StreamEvent, Tool, ToolCall, ToolChoice, Usage,
};

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
    #[serde(default)]
    tools: Option<Vec<Value>>,
    #[serde(default)]
    tool_choice: Option<Value>,
    #[serde(default)]
    output_config: Option<Value>,
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
                // Blocks are separate texts: a newline keeps the end of one
                // from running into the start of the next.
                if !out.is_empty() {
                    out.push('\n');
                }
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

fn invalid(m: impl Into<String>) -> TranslateError {
    TranslateError::InvalidRequest(m.into())
}

fn parse_tools(tools: Vec<Value>) -> Result<Vec<Tool>, TranslateError> {
    tools
        .into_iter()
        .map(|t| {
            // Server tools (web_search_*, bash_*, ...) run at Anthropic, not here.
            // "custom" is the explicit form of a normal client tool.
            if let Some(kind) = t
                .get("type")
                .filter(|k| !k.is_null() && k.as_str() != Some("custom"))
            {
                return Err(invalid(format!(
                    "server tool '{}' is not supported",
                    kind.as_str().unwrap_or("?")
                )));
            }
            let name = t["name"]
                .as_str()
                .ok_or_else(|| invalid("tool 'name' must be a string"))?;
            // `cache_control` is accepted and ignored.
            Ok(Tool {
                name: name.to_string(),
                description: t["description"].as_str().map(str::to_string),
                parameters: match &t["input_schema"] {
                    Value::Null => json!({ "type": "object" }),
                    v @ Value::Object(_) => v.clone(),
                    _ => return Err(invalid("tool 'input_schema' must be an object")),
                },
                strict: None,
            })
        })
        .collect()
}

fn parse_tool_choice(v: &Value) -> Result<(ToolChoice, Option<bool>), TranslateError> {
    let choice = match v["type"].as_str() {
        Some("auto") => ToolChoice::Auto,
        Some("any") => ToolChoice::Required,
        Some("none") => ToolChoice::None,
        Some("tool") => ToolChoice::Tool(
            v["name"]
                .as_str()
                .ok_or_else(|| invalid("tool_choice 'name' must be a string"))?
                .to_string(),
        ),
        _ => return Err(invalid("tool_choice 'type' is not supported")),
    };
    let parallel = (v["disable_parallel_tool_use"] == true).then_some(false);
    Ok((choice, parallel))
}

fn image_block(b: &Value) -> Result<Part, TranslateError> {
    let s = &b["source"];
    let str_of = |k: &str| {
        s[k].as_str()
            .ok_or_else(|| invalid(format!("image source '{k}' must be a string")))
    };
    let source = match s["type"].as_str() {
        Some("base64") => base64_source(str_of("media_type")?, str_of("data")?)?,
        Some("url") => match image_source(str_of("url")?)? {
            u @ ImageSource::Url(_) => u,
            // A data URL in a url source is not what the format says.
            ImageSource::Base64 { .. } => return Err(invalid("image url must be an http(s) URL")),
        },
        _ => return Err(invalid("image source 'type' must be 'base64' or 'url'")),
    };
    Ok(Part::Image(source))
}

fn text_block(b: &Value) -> Result<String, TranslateError> {
    // Citations and cache hints change nothing in the text.
    b["text"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| invalid("content block 'text' must be a string"))
}

/// The text of a `tool_result` body. The error flag (`is_error`) does not
/// survive into other formats: the text is kept as it is.
fn tool_result_text(content: &Value) -> Result<String, TranslateError> {
    match content {
        Value::Null => Ok(String::new()),
        Value::String(t) => Ok(t.clone()),
        Value::Array(blocks) => {
            let mut out = String::new();
            for b in blocks {
                match b["type"].as_str() {
                    Some("text") => out.push_str(&text_block(b)?),
                    Some("image") => {
                        return Err(TranslateError::Unsupported(
                            "images inside a tool result are not supported".into(),
                        ))
                    }
                    _ => return Err(invalid(ONLY_TEXT)),
                }
            }
            Ok(out)
        }
        _ => Err(invalid("tool_result 'content' must be a string or blocks")),
    }
}

fn parse_message(
    role: Role,
    content: WireContent,
    out: &mut Vec<Message>,
) -> Result<(), TranslateError> {
    let blocks = match content {
        WireContent::Text(t) => {
            out.push(Message::text(role, t));
            return Ok(());
        }
        WireContent::Blocks(b) => b,
    };
    let mut parts = Vec::new();
    let mut calls = Vec::new();
    let mut results = Vec::new();
    for b in &blocks {
        match (b["type"].as_str(), role) {
            (Some("text"), _) => parts.push(Part::Text(text_block(b)?)),
            (Some("image"), Role::User) => parts.push(image_block(b)?),
            (Some("tool_use"), Role::Assistant) => {
                let field = |k: &str| {
                    b[k].as_str()
                        .ok_or_else(|| invalid(format!("tool_use '{k}' must be a string")))
                };
                if !b["input"].is_object() {
                    return Err(invalid("tool_use 'input' must be an object"));
                }
                calls.push(ToolCall {
                    id: field("id")?.to_string(),
                    name: field("name")?.to_string(),
                    arguments: b["input"].to_string(),
                });
            }
            (Some("tool_result"), Role::User) => {
                let id = b["tool_use_id"]
                    .as_str()
                    .ok_or_else(|| invalid("tool_result 'tool_use_id' must be a string"))?;
                let mut m = Message::text(Role::Tool, tool_result_text(&b["content"])?);
                m.tool_call_id = Some(id.to_string());
                results.push(m);
            }
            (other, _) => {
                let role = match role {
                    Role::Assistant => "an assistant",
                    _ => "a user",
                };
                return Err(invalid(format!(
                    "content block '{}' is not supported in {role} message",
                    other.unwrap_or("(no type)")
                )));
            }
        }
    }
    // Tool results come first: they answer the assistant message before this one.
    out.extend(results);
    if !parts.is_empty() || !calls.is_empty() {
        let mut m = Message::text(role, "");
        m.content = parts;
        m.tool_calls = calls;
        out.push(m);
    }
    Ok(())
}

/// Anthropic's `output_config.format` (`{type:"json_schema", schema}`) as a
/// response format. Other `output_config` settings are not supported.
fn parse_output_config(oc: Option<&Value>) -> Result<Option<ResponseFormat>, TranslateError> {
    let Some(oc) = oc.filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let fields = oc
        .as_object()
        .ok_or_else(|| invalid("output_config must be an object"))?;
    if let Some(other) = fields.keys().find(|k| *k != "format") {
        return Err(invalid(format!(
            "output_config field '{other}' is not supported"
        )));
    }
    let Some(format) = fields.get("format").filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    if format["type"] != "json_schema" {
        return Err(invalid("output_config format 'type' must be json_schema"));
    }
    match &format["schema"] {
        s @ Value::Object(_) => Ok(Some(ResponseFormat::JsonSchema {
            name: "response".into(),
            schema: s.clone(),
            strict: None,
            description: None,
        })),
        _ => Err(invalid("output_config format 'schema' must be an object")),
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
    let tools = parse_tools(wire.tools.unwrap_or_default())?;
    let (tool_choice, parallel_tool_calls) =
        match wire.tool_choice.as_ref().filter(|v| !v.is_null()) {
            Some(v) => {
                let (c, p) = parse_tool_choice(v)?;
                (Some(c), p)
            }
            None => (None, None),
        };
    let mut messages = Vec::with_capacity(wire.messages.len() + 1);
    if let Some(system) = wire.system {
        messages.push(Message::text(Role::System, text_of(system)?));
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
        parse_message(role, m.content, &mut messages)?;
    }
    Ok(ChatRequest {
        model: wire.model,
        messages,
        max_tokens: Some(wire.max_tokens),
        temperature: wire.temperature,
        top_p: wire.top_p,
        stop: wire.stop_sequences,
        stream: wire.stream,
        tools,
        tool_choice,
        parallel_tool_calls,
        response_format: parse_output_config(wire.output_config.as_ref())?,
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
    let mut content = Vec::new();
    if !r.content.is_empty() || r.tool_calls.is_empty() {
        content.push(json!({ "type": "text", "text": r.content }));
    }
    for c in &r.tool_calls {
        // Anthropic clients need an object. Anything else is a provider bug;
        // the arguments are not logged.
        let input = match serde_json::from_str::<Value>(&c.arguments) {
            Ok(v @ Value::Object(_)) => v,
            _ => {
                tracing::warn!("tool call arguments are not a JSON object");
                json!({})
            }
        };
        content.push(json!({ "type": "tool_use", "id": c.id, "name": c.name, "input": input }));
    }
    json!({
        "id": r.id,
        "type": "message",
        "role": "assistant",
        "model": r.model,
        "content": content,
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
///
/// Blocks are never interleaved: only one block is open at a time and every
/// delta lands on it. Text streams live until the first tool call; that call
/// then streams live. Whatever comes later (more tool calls, text after a
/// call) is held back and written out whole at `Done`: the held tool calls in
/// the order of their call index, each as one start, one `input_json_delta`
/// with all its arguments (`{}` when it had none) and one stop, then the held
/// text as one text block.
pub struct StreamRenderer {
    id: String,
    model: String,
    started: bool,
    /// The index the next block gets.
    next_block: u32,
    /// The open block, and whether it is text.
    open_block: Option<(u32, bool)>,
    /// The call index and block of the tool call that streams live.
    live_tool: Option<(u32, u32)>,
    /// Later tool calls, by the call's own index: id, name, arguments so far.
    held_tools: BTreeMap<u32, (String, String, String)>,
    /// Text that came after the first tool call.
    held_text: String,
}

impl StreamRenderer {
    pub fn new(id: &str, model: &str) -> Self {
        Self {
            id: id.to_string(),
            model: model.to_string(),
            started: false,
            next_block: 0,
            open_block: None,
            live_tool: None,
            held_tools: BTreeMap::new(),
            held_text: String::new(),
        }
    }

    fn open(&mut self) -> String {
        if self.started {
            return String::new();
        }
        self.started = true;
        event(
            "message_start",
            json!({ "type": "message_start", "message": {
                "id": self.id, "type": "message", "role": "assistant", "model": self.model,
                "content": [], "stop_reason": null, "stop_sequence": null,
                "usage": { "input_tokens": 0, "output_tokens": 0 },
            }}),
        )
    }

    fn close_block(&mut self) -> String {
        match self.open_block.take() {
            Some((index, _)) => event(
                "content_block_stop",
                json!({ "type": "content_block_stop", "index": index }),
            ),
            None => String::new(),
        }
    }

    fn start_block(&mut self, block: Value, is_text: bool) -> (u32, String) {
        let mut out = self.close_block();
        let index = self.next_block;
        self.next_block += 1;
        self.open_block = Some((index, is_text));
        out.push_str(&event(
            "content_block_start",
            json!({ "type": "content_block_start", "index": index, "content_block": block }),
        ));
        (index, out)
    }

    fn delta(index: u32, delta: Value) -> String {
        event(
            "content_block_delta",
            json!({ "type": "content_block_delta", "index": index, "delta": delta }),
        )
    }

    pub fn render(&mut self, ev: &StreamEvent) -> String {
        let mut out = self.open();
        match ev {
            StreamEvent::Delta { text } => {
                if self.live_tool.is_some() {
                    self.held_text.push_str(text);
                    return out;
                }
                let index = match self.open_block {
                    Some((i, true)) => i,
                    _ => {
                        let (i, start) =
                            self.start_block(json!({ "type": "text", "text": "" }), true);
                        out.push_str(&start);
                        i
                    }
                };
                out.push_str(&Self::delta(
                    index,
                    json!({ "type": "text_delta", "text": text }),
                ));
            }
            StreamEvent::ToolCallStart { index, id, name } => {
                if self.live_tool.is_some() {
                    self.held_tools
                        .insert(*index, (id.clone(), name.clone(), String::new()));
                    return out;
                }
                let (block, start) = self.start_block(
                    json!({ "type": "tool_use", "id": id, "name": name, "input": {} }),
                    false,
                );
                self.live_tool = Some((*index, block));
                out.push_str(&start);
            }
            StreamEvent::ToolCallDelta { index, arguments } => {
                // The raw partial JSON is passed on; it can only be checked at the end.
                match self.live_tool {
                    Some((t, block)) if t == *index => {
                        out.push_str(&Self::delta(
                            block,
                            json!({ "type": "input_json_delta", "partial_json": arguments }),
                        ));
                    }
                    _ => match self.held_tools.get_mut(index) {
                        Some((_, _, buf)) => buf.push_str(arguments),
                        // Every decoder refuses a delta for an unknown call
                        // before it gets here.
                        None => debug_assert!(false, "tool call delta for an unknown call"),
                    },
                }
            }
            StreamEvent::Done {
                finish_reason,
                usage,
            } => {
                if self.next_block == 0 {
                    // An answer with no content is still one empty text block.
                    let (_, start) = self.start_block(json!({ "type": "text", "text": "" }), true);
                    out.push_str(&start);
                }
                out.push_str(&self.close_block());
                for (_, (id, name, arguments)) in std::mem::take(&mut self.held_tools) {
                    let arguments = if arguments.is_empty() {
                        "{}".to_string()
                    } else {
                        arguments
                    };
                    let (block, start) = self.start_block(
                        json!({ "type": "tool_use", "id": id, "name": name, "input": {} }),
                        false,
                    );
                    out.push_str(&start);
                    out.push_str(&Self::delta(
                        block,
                        json!({ "type": "input_json_delta", "partial_json": arguments }),
                    ));
                    out.push_str(&self.close_block());
                }
                if !self.held_text.is_empty() {
                    let (block, start) =
                        self.start_block(json!({ "type": "text", "text": "" }), true);
                    out.push_str(&start);
                    out.push_str(&Self::delta(
                        block,
                        json!({ "type": "text_delta", "text": std::mem::take(&mut self.held_text) }),
                    ));
                    out.push_str(&self.close_block());
                }
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
    use crate::types::{ImageSource, Part, ToolCall, ToolChoice};

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
        assert_eq!(r.messages[0].joined_text(), "a\nb");
        assert_eq!(r.messages[2].role, Role::Assistant);
        assert_eq!(r.messages[2].joined_text(), "yo");
        assert_eq!(r.stop, Some(vec!["x".into()]));
        assert!(r.stream);
        let r = parse_request(
            br#"{"model":"m","max_tokens":1,"system":"s","messages":[{"role":"user","content":"x"}]}"#,
        )
        .unwrap();
        assert_eq!(r.messages[0].joined_text(), "s");
    }

    #[test]
    fn max_tokens_is_required_and_unknown_fields_are_refused() {
        assert!(matches!(
            parse_request(br#"{"model":"m","messages":[{"role":"user","content":"x"}]}"#),
            Err(TranslateError::InvalidRequest(_))
        ));
        let m = invalid(
            br#"{"model":"m","max_tokens":1,"top_k":1,"messages":[{"role":"user","content":"x"}]}"#,
        );
        assert!(m.contains("top_k"), "{m}");
        invalid(br#"{"model":"m","max_tokens":1,"messages":[]}"#);
        invalid(br#"{"model":"m","max_tokens":1,"messages":[{"role":"system","content":"x"}]}"#);
    }

    #[test]
    fn non_text_blocks_are_refused() {
        let m = invalid(
            br#"{"model":"m","max_tokens":1,"messages":[{"role":"user","content":[{"type":"thinking","thinking":"t"}]}]}"#,
        );
        assert_eq!(
            m,
            "content block 'thinking' is not supported in a user message"
        );
        let m = invalid(
            br#"{"model":"m","max_tokens":1,"system":[{"type":"tool_use"}],"messages":[{"role":"user","content":"x"}]}"#,
        );
        assert_eq!(m, "Only text content is supported.");
    }

    #[test]
    fn refusal_names_the_block_type_and_role() {
        let m = invalid(
            br#"{"model":"m","max_tokens":1,"messages":[{"role":"user","content":[{"type":"tool_use","id":"a","name":"n","input":{}}]}]}"#,
        );
        assert_eq!(
            m,
            "content block 'tool_use' is not supported in a user message"
        );
        let m = invalid(
            br#"{"model":"m","max_tokens":1,"messages":[{"role":"assistant","content":[{"type":"image","source":{}}]}]}"#,
        );
        assert_eq!(
            m,
            "content block 'image' is not supported in an assistant message"
        );
        let m = invalid(
            br#"{"model":"m","max_tokens":1,"messages":[{"role":"user","content":[{"type":"document"}]}]}"#,
        );
        assert!(m.contains("'document'"), "{m}");
    }

    #[test]
    fn renders_a_message() {
        let r = ChatResponse {
            id: "msg_1".into(),
            model: "m".into(),
            content: "hello".into(),
            tool_calls: Vec::new(),
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
                tool_calls: Vec::new(),
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
        // An answer with no content is still one empty text block.
        let start = c.find("event: content_block_start").expect(&c);
        let stop = c.find("event: content_block_stop").expect(&c);
        assert!(start < stop, "{c}");
        assert!(
            c.contains(r#""content_block":{"text":"","type":"text"}"#),
            "{c}"
        );
        assert_eq!(c.matches("event: content_block_start").count(), 1, "{c}");
    }

    fn req(messages: &str, extra: &str) -> Result<ChatRequest, TranslateError> {
        parse_request(
            format!(r#"{{"model":"m","max_tokens":1,{extra}"messages":{messages}}}"#).as_bytes(),
        )
    }

    #[test]
    fn parses_tools_and_each_tool_choice() {
        let tools = r#""tools":[{"name":"w","description":"d","input_schema":{"type":"object","properties":{"a":{"type":"string"}}},"cache_control":{"type":"ephemeral"}},{"name":"n","input_schema":{"type":"object"}}],"#;
        let r = req(r#"[{"role":"user","content":"x"}]"#, tools).unwrap();
        assert_eq!(r.tools.len(), 2);
        assert_eq!(r.tools[0].name, "w");
        assert_eq!(r.tools[0].description.as_deref(), Some("d"));
        assert_eq!(r.tools[0].parameters["properties"]["a"]["type"], "string");
        assert_eq!(r.tools[1].description, None);
        assert_eq!(r.tool_choice, None);
        assert_eq!(r.parallel_tool_calls, None);
        for (choice, want) in [
            (r#"{"type":"auto"}"#, ToolChoice::Auto),
            (r#"{"type":"any"}"#, ToolChoice::Required),
            (r#"{"type":"none"}"#, ToolChoice::None),
            (
                r#"{"type":"tool","name":"w"}"#,
                ToolChoice::Tool("w".into()),
            ),
        ] {
            let r = req(
                r#"[{"role":"user","content":"x"}]"#,
                &format!(r#"{tools}"tool_choice":{choice},"#),
            )
            .unwrap();
            assert_eq!(r.tool_choice, Some(want), "{choice}");
            assert_eq!(r.parallel_tool_calls, None);
        }
        let r = req(
            r#"[{"role":"user","content":"x"}]"#,
            &format!(r#"{tools}"tool_choice":{{"type":"auto","disable_parallel_tool_use":true}},"#),
        )
        .unwrap();
        assert_eq!(r.parallel_tool_calls, Some(false));
        let m = invalid(
            format!(
                r#"{{"model":"m","max_tokens":1,{tools}"tool_choice":{{"type":"x"}},"messages":[{{"role":"user","content":"x"}}]}}"#
            )
            .as_bytes(),
        );
        assert!(m.contains("tool_choice"), "{m}");
    }

    #[test]
    fn custom_tool_type_is_a_normal_tool() {
        let r = req(
            r#"[{"role":"user","content":"x"}]"#,
            r#""tools":[{"type":"custom","name":"w","input_schema":{"type":"object"}}],"#,
        )
        .unwrap();
        assert_eq!(r.tools[0].name, "w");
    }

    #[test]
    fn server_tools_are_invalid() {
        let m = invalid(
            br#"{"model":"m","max_tokens":1,"tools":[{"type":"web_search_20250305","name":"web_search"}],"messages":[{"role":"user","content":"x"}]}"#,
        );
        assert_eq!(m, "server tool 'web_search_20250305' is not supported");
    }

    #[test]
    fn parses_tool_use_and_tool_result_blocks() {
        let r = req(
            r#"[{"role":"user","content":"go"},
            {"role":"assistant","content":[{"type":"text","text":"a"},{"type":"tool_use","id":"t1","name":"w","input":{"q":"x"}},{"type":"text","text":"b"}]},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"r1"},
              {"type":"tool_result","tool_use_id":"t2","content":[{"type":"text","text":"r"},{"type":"text","text":"2"}],"is_error":true},
              {"type":"text","text":"thanks"}]}]"#,
            "",
        )
        .unwrap();
        let roles: Vec<Role> = r.messages.iter().map(|m| m.role).collect();
        assert_eq!(
            roles,
            [
                Role::User,
                Role::Assistant,
                Role::Tool,
                Role::Tool,
                Role::User
            ]
        );
        let a = &r.messages[1];
        assert_eq!(a.joined_text(), "ab");
        assert_eq!(a.tool_calls.len(), 1);
        assert_eq!(a.tool_calls[0].id, "t1");
        assert_eq!(a.tool_calls[0].name, "w");
        assert_eq!(a.tool_calls[0].arguments, r#"{"q":"x"}"#);
        assert_eq!(r.messages[2].tool_call_id.as_deref(), Some("t1"));
        assert_eq!(r.messages[2].joined_text(), "r1");
        assert_eq!(r.messages[3].tool_call_id.as_deref(), Some("t2"));
        assert_eq!(r.messages[3].joined_text(), "r2");
        assert_eq!(r.messages[4].joined_text(), "thanks");
        assert!(r.messages[4].tool_calls.is_empty());
        // Only tool results: no empty user message follows.
        let r = req(
            r#"[{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"r"}]}]"#,
            "",
        )
        .unwrap();
        assert_eq!(r.messages.len(), 1);
        assert_eq!(r.messages[0].role, Role::Tool);
        // Missing id and tool_use on a user message are refused.
        invalid(
            br#"{"model":"m","max_tokens":1,"messages":[{"role":"user","content":[{"type":"tool_result","content":"r"}]}]}"#,
        );
        invalid(
            br#"{"model":"m","max_tokens":1,"messages":[{"role":"user","content":[{"type":"tool_use","id":"a","name":"n","input":{}}]}]}"#,
        );
    }

    #[test]
    fn parses_image_blocks_both_sources() {
        let r = req(
            r#"[{"role":"user","content":[{"type":"text","text":"look"},
              {"type":"image","source":{"type":"base64","media_type":"image/png","data":"QUJD"}},
              {"type":"image","source":{"type":"url","url":"https://x.test/a.png"}}]}]"#,
            "",
        )
        .unwrap();
        assert_eq!(
            r.messages[0].content,
            [
                Part::Text("look".into()),
                Part::Image(ImageSource::Base64 {
                    media_type: "image/png".into(),
                    data: "QUJD".into()
                }),
                Part::Image(ImageSource::Url("https://x.test/a.png".into())),
            ]
        );
        for bad in [
            r#"{"type":"base64","media_type":"image/svg+xml","data":"QUJD"}"#,
            r#"{"type":"base64","media_type":"image/png","data":"@@"}"#,
            r#"{"type":"url","url":"ftp://x/a.png"}"#,
            r#"{"type":"file","file_id":"f"}"#,
        ] {
            invalid(
                format!(
                    r#"{{"model":"m","max_tokens":1,"messages":[{{"role":"user","content":[{{"type":"image","source":{bad}}}]}}]}}"#
                )
                .as_bytes(),
            );
        }
        // Images on an assistant message are refused.
        invalid(
            br#"{"model":"m","max_tokens":1,"messages":[{"role":"assistant","content":[{"type":"image","source":{"type":"url","url":"https://x/a.png"}}]}]}"#,
        );
    }

    #[test]
    fn tool_result_with_image_is_unsupported() {
        let e = req(
            r#"[{"role":"user","content":[{"type":"tool_result","tool_use_id":"t","content":[{"type":"image","source":{"type":"url","url":"https://x/a.png"}}]}]}]"#,
            "",
        )
        .unwrap_err();
        assert!(matches!(e, TranslateError::Unsupported(_)), "{e:?}");
    }

    fn call(id: &str, name: &str, arguments: &str) -> ToolCall {
        ToolCall {
            id: id.into(),
            name: name.into(),
            arguments: arguments.into(),
        }
    }

    #[test]
    fn renders_tool_use_blocks() {
        let mut r = ChatResponse {
            id: "msg_1".into(),
            model: "m".into(),
            content: "let me".into(),
            tool_calls: vec![call("t1", "w", r#"{"q":"x"}"#), call("t2", "n", "oops")],
            finish_reason: Some(FinishReason::ToolCalls),
            usage: None,
        };
        let v = render_response(&r);
        assert_eq!(v["content"][0], json!({"type":"text","text":"let me"}));
        assert_eq!(
            v["content"][1],
            json!({"type":"tool_use","id":"t1","name":"w","input":{"q":"x"}})
        );
        // Not a JSON object: an empty object, never the raw text.
        assert_eq!(
            v["content"][2],
            json!({"type":"tool_use","id":"t2","name":"n","input":{}})
        );
        assert_eq!(v["stop_reason"], "tool_use");
        r.content.clear();
        let v = render_response(&r);
        assert_eq!(v["content"].as_array().unwrap().len(), 2);
        assert_eq!(v["content"][0]["type"], "tool_use");
        r.tool_calls.clear();
        let v = render_response(&r);
        assert_eq!(v["content"], json!([{"type":"text","text":""}]));
    }

    fn events(s: &str) -> Vec<(String, Value)> {
        let mut out = Vec::new();
        let mut name = String::new();
        for l in s.lines() {
            if let Some(n) = l.strip_prefix("event: ") {
                name = n.to_string();
            } else if let Some(d) = l.strip_prefix("data: ") {
                out.push((name.clone(), serde_json::from_str(d).unwrap()));
            }
        }
        out
    }

    #[test]
    fn stream_renderer_interleaves_text_and_two_tool_calls() {
        let mut r = StreamRenderer::new("m1", "m");
        let mut all = String::new();
        for ev in [
            StreamEvent::Delta { text: "hi".into() },
            StreamEvent::ToolCallStart {
                index: 0,
                id: "a".into(),
                name: "w".into(),
            },
            StreamEvent::ToolCallDelta {
                index: 0,
                arguments: r#"{"q":"#.into(),
            },
            StreamEvent::ToolCallDelta {
                index: 0,
                arguments: r#""x"}"#.into(),
            },
            StreamEvent::ToolCallStart {
                index: 1,
                id: "b".into(),
                name: "n".into(),
            },
            StreamEvent::ToolCallDelta {
                index: 1,
                arguments: "{}".into(),
            },
            StreamEvent::Delta { text: "bye".into() },
            StreamEvent::Done {
                finish_reason: Some(FinishReason::ToolCalls),
                usage: None,
            },
        ] {
            all.push_str(&r.render(&ev));
        }
        let evs = events(&all);
        let seq: Vec<(&str, Option<u64>)> = evs
            .iter()
            .map(|(n, d)| (n.as_str(), d["index"].as_u64()))
            .collect();
        assert_eq!(
            seq,
            [
                ("message_start", None),
                ("content_block_start", Some(0)),
                ("content_block_delta", Some(0)),
                ("content_block_stop", Some(0)),
                ("content_block_start", Some(1)),
                ("content_block_delta", Some(1)),
                ("content_block_delta", Some(1)),
                ("content_block_stop", Some(1)),
                ("content_block_start", Some(2)),
                ("content_block_delta", Some(2)),
                ("content_block_stop", Some(2)),
                ("content_block_start", Some(3)),
                ("content_block_delta", Some(3)),
                ("content_block_stop", Some(3)),
                ("message_delta", None),
                ("message_stop", None),
            ]
        );
        assert_eq!(evs[1].1["content_block"], json!({"type":"text","text":""}));
        assert_eq!(
            evs[4].1["content_block"],
            json!({"type":"tool_use","id":"a","name":"w","input":{}})
        );
        assert_eq!(
            evs[5].1["delta"],
            json!({"type":"input_json_delta","partial_json":"{\"q\":"})
        );
        assert_eq!(
            evs[8].1["content_block"],
            json!({"type":"tool_use","id":"b","name":"n","input":{}})
        );
        assert_eq!(evs[11].1["content_block"]["type"], "text");
        assert_eq!(evs[12].1["delta"]["text"], "bye");
        assert_eq!(evs[14].1["delta"]["stop_reason"], "tool_use");
    }

    fn render_all(evs: Vec<StreamEvent>) -> Vec<(String, Value)> {
        let mut r = StreamRenderer::new("m1", "m");
        let mut all = String::new();
        for ev in &evs {
            all.push_str(&r.render(ev));
        }
        events(&all)
    }

    fn start(index: u32, id: &str, name: &str) -> StreamEvent {
        StreamEvent::ToolCallStart {
            index,
            id: id.into(),
            name: name.into(),
        }
    }

    fn args(index: u32, a: &str) -> StreamEvent {
        StreamEvent::ToolCallDelta {
            index,
            arguments: a.into(),
        }
    }

    fn done() -> StreamEvent {
        StreamEvent::Done {
            finish_reason: Some(FinishReason::ToolCalls),
            usage: None,
        }
    }

    #[test]
    fn stream_renderer_never_interleaves_parallel_tool_calls() {
        // The shape of tests/fixtures/openai/stream_tool_calls.txt.
        let evs = render_all(vec![
            start(0, "call_a", "get_weather"),
            start(1, "call_b", "get_time"),
            args(0, r#"{"city":"#),
            args(1, r#"{"tz":"UTC"}"#),
            args(0, r#""Paris"}"#),
            done(),
        ]);
        let seq: Vec<(&str, Option<u64>)> = evs
            .iter()
            .map(|(n, d)| (n.as_str(), d["index"].as_u64()))
            .collect();
        assert_eq!(
            seq,
            [
                ("message_start", None),
                ("content_block_start", Some(0)),
                ("content_block_delta", Some(0)),
                ("content_block_delta", Some(0)),
                ("content_block_stop", Some(0)),
                ("content_block_start", Some(1)),
                ("content_block_delta", Some(1)),
                ("content_block_stop", Some(1)),
                ("message_delta", None),
                ("message_stop", None),
            ]
        );
        assert_eq!(evs[1].1["content_block"]["id"], "call_a");
        assert_eq!(evs[2].1["delta"]["partial_json"], r#"{"city":"#);
        assert_eq!(evs[3].1["delta"]["partial_json"], r#""Paris"}"#);
        assert_eq!(evs[5].1["content_block"]["id"], "call_b");
        assert_eq!(evs[6].1["delta"]["partial_json"], r#"{"tz":"UTC"}"#);
    }

    #[test]
    fn stream_renderer_buffers_later_calls_without_arguments_and_text_after_a_call() {
        let evs = render_all(vec![
            start(0, "a", "w"),
            args(0, "{}"),
            start(2, "c", "z"),
            StreamEvent::Delta { text: "he".into() },
            start(1, "b", "n"),
            StreamEvent::Delta { text: "llo".into() },
            args(1, "{\"k\":1}"),
            done(),
        ]);
        let seq: Vec<(&str, Option<u64>)> = evs
            .iter()
            .map(|(n, d)| (n.as_str(), d["index"].as_u64()))
            .collect();
        assert_eq!(
            seq,
            [
                ("message_start", None),
                ("content_block_start", Some(0)),
                ("content_block_delta", Some(0)),
                ("content_block_stop", Some(0)),
                // Buffered calls, in tool-index order: 1 then 2.
                ("content_block_start", Some(1)),
                ("content_block_delta", Some(1)),
                ("content_block_stop", Some(1)),
                ("content_block_start", Some(2)),
                ("content_block_delta", Some(2)),
                ("content_block_stop", Some(2)),
                // Then the text that came after the first call.
                ("content_block_start", Some(3)),
                ("content_block_delta", Some(3)),
                ("content_block_stop", Some(3)),
                ("message_delta", None),
                ("message_stop", None),
            ]
        );
        assert_eq!(evs[4].1["content_block"]["id"], "b");
        assert_eq!(evs[5].1["delta"]["partial_json"], "{\"k\":1}");
        assert_eq!(evs[7].1["content_block"]["id"], "c");
        assert_eq!(evs[8].1["delta"]["partial_json"], "{}");
        assert_eq!(evs[10].1["content_block"]["type"], "text");
        assert_eq!(evs[11].1["delta"]["text"], "hello");
    }

    #[test]
    fn stream_renderer_tool_call_first_has_no_empty_text_block() {
        let mut r = StreamRenderer::new("m1", "m");
        let mut all = String::new();
        for ev in [
            StreamEvent::ToolCallStart {
                index: 0,
                id: "a".into(),
                name: "w".into(),
            },
            StreamEvent::ToolCallDelta {
                index: 0,
                arguments: "{}".into(),
            },
            StreamEvent::Done {
                finish_reason: Some(FinishReason::ToolCalls),
                usage: None,
            },
        ] {
            all.push_str(&r.render(&ev));
        }
        let evs = events(&all);
        let names: Vec<&str> = evs.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                "message_start",
                "content_block_start",
                "content_block_delta",
                "content_block_stop",
                "message_delta",
                "message_stop"
            ]
        );
        assert_eq!(evs[1].1["index"], 0);
        assert_eq!(evs[1].1["content_block"]["type"], "tool_use");
    }

    #[test]
    fn output_config_format_is_a_json_schema_response_format() {
        let body = |oc: &str| {
            format!(
                r#"{{"model":"m","max_tokens":5,"messages":[{{"role":"user","content":"x"}}],"output_config":{oc}}}"#
            )
        };
        let r = parse_request(
            body(r#"{"format":{"type":"json_schema","schema":{"type":"object"}}}"#).as_bytes(),
        )
        .unwrap();
        assert_eq!(
            r.response_format,
            Some(ResponseFormat::JsonSchema {
                name: "response".into(),
                schema: json!({"type":"object"}),
                strict: None,
                description: None,
            })
        );
        assert_eq!(
            parse_request(body("null").as_bytes())
                .unwrap()
                .response_format,
            None
        );
        assert_eq!(
            parse_request(body("{}").as_bytes())
                .unwrap()
                .response_format,
            None
        );
        for bad in [
            r#"{"format":{"type":"text"}}"#,
            r#"{"format":{"type":"json_schema"}}"#,
            r#"{"format":{"type":"json_schema","schema":[]}}"#,
            r#"{"effort":"high"}"#,
            r#"[]"#,
        ] {
            assert!(
                matches!(
                    parse_request(body(bad).as_bytes()),
                    Err(TranslateError::InvalidRequest(_))
                ),
                "{bad}"
            );
        }
    }
}
