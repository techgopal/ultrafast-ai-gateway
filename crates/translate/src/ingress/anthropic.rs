//! Anthropic Messages wire format, as received from and returned to callers.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::error::TranslateError;
use crate::ingress::openai::REASONING_EFFORTS;
use crate::types::{
    base64_source, image_source, ChatRequest, ChatResponse, FinishReason, ImageSource, Message,
    Part, ResponseFormat, Role, StreamEvent, Tool, ToolCall, ToolChoice, Usage,
};

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
    #[serde(default)]
    thinking: Option<Value>,
    /// Accepted and not used.
    #[serde(default)]
    #[allow(dead_code)]
    metadata: Option<Value>,
    /// Every field that is not named above: dropped, and named.
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

/// What translation leaves out, in the order it is met. `cache_control` and
/// `defer_loading` are named once however often they occur.
#[derive(Default)]
struct Dropped(Vec<String>);

impl Dropped {
    fn once(&mut self, name: &str) {
        if !self.0.iter().any(|d| d == name) {
            self.0.push(name.to_string());
        }
    }

    fn add(&mut self, name: String) {
        self.0.push(name);
    }

    /// `cache_control` on a block or a tool changes nothing in the text.
    fn cache_control(&mut self, block: &Value) {
        if block.get("cache_control").is_some_and(|c| !c.is_null()) {
            self.once("cache_control");
        }
    }
}

/// The text blocks of `blocks` joined by newlines (separate texts must not
/// run together); any other block is dropped and named `{at}[i]:{type}`.
fn blocks_text(
    blocks: &[Value],
    at: &str,
    dropped: &mut Dropped,
) -> Result<String, TranslateError> {
    let mut out = String::new();
    for (i, b) in blocks.iter().enumerate() {
        dropped.cache_control(b);
        if b["type"] != "text" {
            dropped.add(format!("{at}[{i}]:{}", b["type"].as_str().unwrap_or("?")));
            continue;
        }
        let Some(t) = b["text"].as_str() else {
            return Err(invalid("content block 'text' must be a string"));
        };
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(t);
    }
    Ok(out)
}

fn text_of(
    content: WireContent,
    at: &str,
    dropped: &mut Dropped,
) -> Result<String, TranslateError> {
    match content {
        WireContent::Text(t) => Ok(t),
        WireContent::Blocks(blocks) => blocks_text(&blocks, at, dropped),
    }
}

fn invalid(m: impl Into<String>) -> TranslateError {
    TranslateError::InvalidRequest(m.into())
}

fn parse_tools(tools: Vec<Value>, dropped: &mut Dropped) -> Result<Vec<Tool>, TranslateError> {
    let mut out = Vec::with_capacity(tools.len());
    for (i, t) in tools.into_iter().enumerate() {
        // Server tools (web_search_*, bash_*, tool_search_*, ...) run at
        // Anthropic, not here. "custom" is the explicit form of a normal
        // client tool.
        if let Some(kind) = t
            .get("type")
            .filter(|k| !k.is_null() && k.as_str() != Some("custom"))
        {
            dropped.add(format!("tools[{i}]:{}", kind.as_str().unwrap_or("?")));
            continue;
        }
        let name = t["name"]
            .as_str()
            .ok_or_else(|| invalid("tool 'name' must be a string"))?;
        dropped.cache_control(&t);
        if t.get("defer_loading").is_some() {
            dropped.once("defer_loading");
        }
        out.push(Tool {
            name: name.to_string(),
            description: t["description"].as_str().map(str::to_string),
            parameters: match &t["input_schema"] {
                Value::Null => json!({ "type": "object" }),
                v @ Value::Object(_) => v.clone(),
                _ => return Err(invalid("tool 'input_schema' must be an object")),
            },
            strict: None,
            freeform: false,
        });
    }
    Ok(out)
}

fn parse_tool_choice(
    v: &Value,
    dropped: &mut Dropped,
) -> Result<(ToolChoice, Option<bool>), TranslateError> {
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
        other => {
            // A kind the gateway does not know leaves the model to choose.
            dropped.add(format!("tool_choice:{}", other.unwrap_or("?")));
            ToolChoice::Auto
        }
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

/// The text of a `tool_result` body, and the images in it (which the target
/// receives as a user message after the result). The error flag (`is_error`)
/// does not survive into other formats: the text is kept as it is. Other
/// blocks (`tool_reference`, documents) are dropped and named.
fn tool_result(
    content: &Value,
    at: &str,
    dropped: &mut Dropped,
) -> Result<(String, Vec<Part>), TranslateError> {
    match content {
        Value::Null => Ok((String::new(), Vec::new())),
        Value::String(t) => Ok((t.clone(), Vec::new())),
        Value::Array(blocks) => {
            let mut text = String::new();
            let mut images = Vec::new();
            for (i, b) in blocks.iter().enumerate() {
                dropped.cache_control(b);
                match b["type"].as_str() {
                    Some("text") => text.push_str(&text_block(b)?),
                    Some("image") => images.push(image_block(b)?),
                    other => dropped.add(format!("{at}.content[{i}]:{}", other.unwrap_or("?"))),
                }
            }
            Ok((text, images))
        }
        _ => Err(invalid("tool_result 'content' must be a string or blocks")),
    }
}

fn parse_message(
    role: Role,
    content: WireContent,
    at: usize,
    out: &mut Vec<Message>,
    dropped: &mut Dropped,
) -> Result<(), TranslateError> {
    let blocks = match content {
        WireContent::Text(t) => {
            out.push(Message::text(role, t));
            return Ok(());
        }
        WireContent::Blocks(b) => b,
    };
    if role == Role::System {
        let at = format!("messages[{at}].content");
        let text = blocks_text(&blocks, &at, dropped)?;
        if !text.is_empty() {
            out.push(Message::text(role, text));
        }
        return Ok(());
    }
    let mut parts = Vec::new();
    let mut result_images = Vec::new();
    let mut calls = Vec::new();
    let mut results = Vec::new();
    for (i, b) in blocks.iter().enumerate() {
        dropped.cache_control(b);
        let here = format!("messages[{at}].content[{i}]");
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
                let (text, images) = tool_result(&b["content"], &here, dropped)?;
                let mut m = Message::text(Role::Tool, text);
                m.tool_call_id = Some(id.to_string());
                results.push(m);
                result_images.extend(images);
            }
            // Known blocks in the wrong place are malformed.
            (Some(kind @ ("image" | "tool_use" | "tool_result")), _) => {
                let role = match role {
                    Role::Assistant => "an assistant",
                    _ => "a user",
                };
                return Err(invalid(format!(
                    "content block '{kind}' is not supported in {role} message"
                )));
            }
            // Thinking, documents, server tool blocks and whatever comes
            // next: not carried, and named.
            (other, _) => dropped.add(format!("{here}:{}", other.unwrap_or("?"))),
        }
    }
    // Tool results come first: they answer the assistant message before this one.
    out.extend(results);
    if !result_images.is_empty() {
        result_images.append(&mut parts);
        parts = result_images;
    }
    if !parts.is_empty() || !calls.is_empty() {
        let mut m = Message::text(role, "");
        m.content = parts;
        m.tool_calls = calls;
        out.push(m);
    }
    Ok(())
}

/// Anthropic's `output_config`: `format` (`{type:"json_schema", schema}`) is a
/// response format, `effort` the reasoning effort. Other settings are
/// dropped and named.
fn parse_output_config(
    oc: Option<&Value>,
    dropped: &mut Dropped,
) -> Result<(Option<ResponseFormat>, Option<String>), TranslateError> {
    let Some(oc) = oc.filter(|v| !v.is_null()) else {
        return Ok((None, None));
    };
    let fields = oc
        .as_object()
        .ok_or_else(|| invalid("output_config must be an object"))?;
    let mut effort = None;
    for (k, v) in fields {
        match k.as_str() {
            "format" => {}
            "effort" => match v.as_str() {
                Some(e) if REASONING_EFFORTS.contains(&e) => effort = Some(e.to_string()),
                _ => dropped.add("output_config.effort".into()),
            },
            other => dropped.add(format!("output_config.{other}")),
        }
    }
    let Some(format) = fields.get("format").filter(|v| !v.is_null()) else {
        return Ok((None, effort));
    };
    if format["type"] != "json_schema" {
        return Err(invalid("output_config format 'type' must be json_schema"));
    }
    match &format["schema"] {
        s @ Value::Object(_) => Ok((
            Some(ResponseFormat::JsonSchema {
                name: "response".into(),
                schema: s.clone(),
                // Anthropic always enforces the schema; OpenAI and Azure treat a
                // schema without `strict` as best effort, so a caller of this
                // format keeps its enforcement on any provider.
                strict: Some(true),
                description: None,
            }),
            effort,
        )),
        _ => Err(invalid("output_config format 'schema' must be an object")),
    }
}

pub fn parse_request(body: &[u8]) -> Result<ChatRequest, TranslateError> {
    let wire: WireRequest =
        serde_json::from_slice(body).map_err(|e| TranslateError::InvalidRequest(e.to_string()))?;
    if wire.messages.is_empty() {
        return Err(TranslateError::InvalidRequest(
            "messages must not be empty".into(),
        ));
    }
    let mut dropped = Dropped::default();
    // Unknown top-level fields (context_management, top_k, ...): named.
    for (field, _) in wire.extra.iter().filter(|(_, v)| !v.is_null()) {
        dropped.add(field.clone());
    }
    // `thinking` has no setting of its own here: the effort comes from
    // `output_config.effort`. It must still be well formed.
    if let Some(t) = wire.thinking.as_ref().filter(|t| !t.is_null()) {
        if !t.is_object() {
            return Err(invalid("thinking must be an object"));
        }
    }
    let tools = parse_tools(wire.tools.unwrap_or_default(), &mut dropped)?;
    let (mut tool_choice, mut parallel_tool_calls) =
        match wire.tool_choice.as_ref().filter(|v| !v.is_null()) {
            Some(v) => {
                let (c, p) = parse_tool_choice(v, &mut dropped)?;
                (Some(c), p)
            }
            None => (None, None),
        };
    if tools.is_empty() && tool_choice.is_some() {
        // Every tool was one the gateway cannot carry: nothing to choose from.
        tool_choice = None;
        parallel_tool_calls = None;
        dropped.once("tool_choice");
    }
    let mut messages = Vec::with_capacity(wire.messages.len() + 1);
    if let Some(system) = wire.system {
        let text = text_of(system, "system", &mut dropped)?;
        messages.push(Message::text(Role::System, text));
    }
    for (at, m) in wire.messages.into_iter().enumerate() {
        let role = match m.role.as_str() {
            "user" => Role::User,
            "assistant" => Role::Assistant,
            // Claude Code puts system messages among the others.
            "system" => Role::System,
            other => {
                return Err(TranslateError::InvalidRequest(format!(
                    "message role '{other}' is not supported"
                )))
            }
        };
        parse_message(role, m.content, at, &mut messages, &mut dropped)?;
    }
    let (response_format, reasoning_effort) =
        parse_output_config(wire.output_config.as_ref(), &mut dropped)?;
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
        response_format,
        reasoning_effort,
        dropped: dropped.0,
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
        ..Default::default()
    });
    json!({
        "input_tokens": u.input_tokens,
        "output_tokens": u.output_tokens,
        "cache_read_input_tokens": u.cache_read_input_tokens,
        "cache_creation_input_tokens": u.cache_write_input_tokens,
    })
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
    fn max_tokens_is_required() {
        assert!(matches!(
            parse_request(br#"{"model":"m","messages":[{"role":"user","content":"x"}]}"#),
            Err(TranslateError::InvalidRequest(_))
        ));
        invalid(br#"{"model":"m","max_tokens":1,"messages":[]}"#);
        invalid(br#"{"model":"m","max_tokens":1,"messages":[{"role":"tool","content":"x"}]}"#);
    }

    #[test]
    fn unknown_top_level_fields_are_dropped_not_refused() {
        let r = parse_request(
            br#"{"model":"m","max_tokens":1,"top_k":1,"service_tier":null,"container":"c","messages":[{"role":"user","content":"x"}]}"#,
        )
        .unwrap();
        assert_eq!(
            r.dropped,
            vec!["container".to_string(), "top_k".to_string()]
        );
        // A known field that is malformed is still refused.
        invalid(br#"{"model":"m","max_tokens":1,"thinking":"yes","messages":[{"role":"user","content":"x"}]}"#);
    }

    #[test]
    fn unknown_blocks_are_dropped_and_named() {
        let r = parse_request(
            br#"{"model":"m","max_tokens":1,"system":[{"type":"text","text":"s"},{"type":"tool_use"}],
                "messages":[{"role":"user","content":[{"type":"thinking","thinking":"t"},{"type":"document","source":{}},{"type":"text","text":"x"}]}]}"#,
        )
        .unwrap();
        assert_eq!(r.messages.len(), 2);
        assert_eq!(r.messages[0].joined_text(), "s");
        assert_eq!(r.messages[1].joined_text(), "x");
        assert_eq!(
            r.dropped,
            vec![
                "system[1]:tool_use".to_string(),
                "messages[0].content[0]:thinking".to_string(),
                "messages[0].content[1]:document".to_string(),
            ]
        );
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
    }

    const FIXTURE: &str =
        include_str!("../../../gateway/tests/fixtures/agents/claude-code-2.1.305.json");

    #[test]
    fn claude_code_request_parses() {
        let r = parse_request(FIXTURE.as_bytes()).unwrap();
        assert!(r.dropped.contains(&"context_management".to_string()));
        assert!(r.dropped.contains(&"cache_control".to_string()));
        assert!(r.dropped.contains(&"defer_loading".to_string()));
        // `metadata` is a known field and is not listed.
        assert!(!r.dropped.iter().any(|d| d == "metadata"));
        assert_eq!(r.reasoning_effort.as_deref(), Some("medium"));
        assert!(r.stream);
        assert!(r.tools.len() >= 10);
        // the system message of the first request stays at its position
        assert!(r.messages.iter().any(|m| m.role == Role::System));
    }

    #[test]
    fn system_role_messages_stay_in_place() {
        let r = req(
            r#"[{"role":"user","content":"a"},{"role":"system","content":"b"},{"role":"user","content":[{"type":"text","text":"c"}]}]"#,
            "",
        )
        .unwrap();
        let roles: Vec<_> = r
            .messages
            .iter()
            .map(|m| (m.role, m.joined_text()))
            .collect();
        assert_eq!(
            roles,
            vec![
                (Role::User, "a".to_string()),
                (Role::System, "b".to_string()),
                (Role::User, "c".to_string())
            ]
        );
    }

    #[test]
    fn thinking_blocks_in_history_are_dropped() {
        let r = req(
            r#"[{"role":"user","content":"a"},
                {"role":"assistant","content":[{"type":"thinking","thinking":"t","signature":"s"},{"type":"redacted_thinking","data":"d"},{"type":"text","text":"b"}]}]"#,
            "",
        )
        .unwrap();
        assert_eq!(r.messages.len(), 2);
        assert_eq!(r.messages[1].joined_text(), "b");
        assert_eq!(
            r.dropped,
            vec![
                "messages[1].content[0]:thinking".to_string(),
                "messages[1].content[1]:redacted_thinking".to_string()
            ]
        );
    }

    #[test]
    fn server_tools_are_dropped() {
        let r = req(
            r#"[{"role":"user","content":"x"}]"#,
            r#""tools":[{"name":"f","input_schema":{"type":"object"}},{"type":"web_search_20250305","name":"web_search"}],"tool_choice":{"type":"auto"},"#,
        )
        .unwrap();
        assert_eq!(r.tools.len(), 1);
        assert_eq!(r.tools[0].name, "f");
        assert_eq!(r.dropped, vec!["tools[1]:web_search_20250305".to_string()]);
        // Nothing left to choose from.
        let r = req(
            r#"[{"role":"user","content":"x"}]"#,
            r#""tools":[{"type":"web_search_20250305","name":"web_search"}],"tool_choice":{"type":"any"},"#,
        )
        .unwrap();
        assert!(r.tools.is_empty());
        assert_eq!(r.tool_choice, None);
    }

    #[test]
    fn effort_comes_from_output_config() {
        let eff = |oc: &str, thinking: &str| {
            req(
                r#"[{"role":"user","content":"x"}]"#,
                &format!(r#""output_config":{oc},"thinking":{thinking},"#),
            )
            .unwrap()
        };
        assert_eq!(
            eff(r#"{"effort":"high"}"#, r#"{"type":"adaptive"}"#)
                .reasoning_effort
                .as_deref(),
            Some("high")
        );
        // adaptive thinking without an effort leaves the provider default
        let r = eff("{}", r#"{"type":"adaptive"}"#);
        assert_eq!(r.reasoning_effort, None);
        assert!(r.dropped.is_empty());
        // enabled thinking with a budget and no effort: no effort
        assert_eq!(
            eff("null", r#"{"type":"enabled","budget_tokens":2000}"#).reasoning_effort,
            None
        );
        // an effort the gateway does not know is dropped and named
        let r = eff(r#"{"effort":"turbo"}"#, "null");
        assert_eq!(r.reasoning_effort, None);
        assert_eq!(r.dropped, vec!["output_config.effort".to_string()]);
        // format keeps its meaning next to effort; other fields are named
        let r = eff(
            r#"{"effort":"low","format":{"type":"json_schema","schema":{"type":"object"}},"zzz":1}"#,
            "null",
        );
        assert!(r.response_format.is_some());
        assert_eq!(r.dropped, vec!["output_config.zzz".to_string()]);
    }

    #[test]
    fn an_unknown_tool_choice_falls_back_to_auto() {
        let r = req(
            r#"[{"role":"user","content":"x"}]"#,
            r#""tools":[{"name":"f","input_schema":{"type":"object"}}],"tool_choice":{"type":"x"},"#,
        )
        .unwrap();
        assert_eq!(r.tool_choice, Some(ToolChoice::Auto));
        assert_eq!(r.dropped, vec!["tool_choice:x".to_string()]);
    }

    #[test]
    fn claude_code_history_blocks_translate() {
        let r = req(
            r#"[{"role":"user","content":"go"},
              {"role":"assistant","content":[{"type":"text","text":"a"},{"type":"tool_use","id":"t1","name":"w","input":{}},
                 {"type":"server_tool_use","id":"s1","name":"web_search","input":{}},
                 {"type":"web_search_tool_result","tool_use_id":"s1","content":[]}]},
              {"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":[
                  {"type":"text","text":"r"},
                  {"type":"image","source":{"type":"base64","media_type":"image/png","data":"QUJD"}},
                  {"type":"tool_reference","tool_name":"x"}]},
                 {"type":"text","text":"next","cache_control":{"type":"ephemeral"}}]}]"#,
            "",
        )
        .unwrap();
        // user, assistant (text + call), tool result, user (image + text)
        assert_eq!(r.messages.len(), 4);
        assert_eq!(r.messages[2].role, Role::Tool);
        assert_eq!(r.messages[2].joined_text(), "r");
        assert_eq!(r.messages[3].role, Role::User);
        assert!(r.messages[3].has_images());
        assert_eq!(r.messages[3].joined_text(), "next");
        assert_eq!(
            r.dropped,
            vec![
                "messages[1].content[2]:server_tool_use".to_string(),
                "messages[1].content[3]:web_search_tool_result".to_string(),
                "messages[2].content[0].content[2]:tool_reference".to_string(),
                "cache_control".to_string(),
            ]
        );
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
                ..Default::default()
            }),
        };
        let v = render_response(&r);
        assert_eq!(v["type"], "message");
        assert_eq!(v["role"], "assistant");
        assert_eq!(v["content"][0], json!({"type":"text","text":"hello"}));
        assert_eq!(v["stop_reason"], "max_tokens");
        assert_eq!(
            v["usage"],
            json!({"input_tokens":3,"output_tokens":2,"cache_read_input_tokens":0,"cache_creation_input_tokens":0})
        );
        let mut r = r;
        r.finish_reason = Some(FinishReason::Stop);
        assert_eq!(render_response(&r)["stop_reason"], "end_turn");
    }

    #[test]
    fn usage_renders_cache_reads_and_writes() {
        let u = Usage {
            input_tokens: 7,
            output_tokens: 3,
            cache_read_input_tokens: 100,
            cache_write_input_tokens: 20,
        };
        assert_eq!(
            usage_json(Some(u)),
            json!({"input_tokens":7,"output_tokens":3,"cache_read_input_tokens":100,"cache_creation_input_tokens":20})
        );
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
                ..Default::default()
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
    fn a_tool_result_image_follows_as_a_user_image() {
        let r = req(
            r#"[{"role":"user","content":[{"type":"tool_result","tool_use_id":"t","content":[{"type":"image","source":{"type":"url","url":"https://x/a.png"}}]}]}]"#,
            "",
        )
        .unwrap();
        assert_eq!(r.messages[0].role, Role::Tool);
        assert_eq!(r.messages[0].joined_text(), "");
        assert_eq!(r.messages[1].role, Role::User);
        assert!(r.messages[1].has_images());
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

    // Anthropic always enforces the schema, so the caller keeps that on an
    // OpenAI-shaped target too.
    #[test]
    fn an_output_schema_stays_strict_on_an_openai_target() {
        use crate::provider::{build_request, ProviderKind, Target};
        let r = parse_request(
            br#"{"model":"m","max_tokens":5,"messages":[{"role":"user","content":"x"}],
                "output_config":{"format":{"type":"json_schema","schema":{"type":"object"}}}}"#,
        )
        .unwrap();
        for kind in [ProviderKind::OpenAi, ProviderKind::Azure] {
            let target = Target {
                kind,
                base_url: "https://api.example.com/v1".into(),
                api_key: Some("k".into()),
                model: "gpt-4o".into(),
                api_version: None,
            };
            let sent: Value =
                serde_json::from_slice(&build_request(&target, &r).unwrap().body).unwrap();
            assert_eq!(
                sent["response_format"]["json_schema"]["strict"],
                json!(true),
                "{kind:?}"
            );
        }
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
                strict: Some(true),
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
