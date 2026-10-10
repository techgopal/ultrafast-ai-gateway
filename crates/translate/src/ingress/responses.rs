//! OpenAI Responses API wire format (stateless), as received from and
//! returned to callers.
//!
//! The request becomes a [`ChatRequest`], so every provider, route, cache and
//! guardrail applies unchanged. Field names follow OpenAI's API reference
//! (checked against the official `openai` Python SDK types, which are
//! generated from OpenAPI): items and content parts, the flat
//! `text.format`, the flat function `tools`, the response object and the
//! streaming events with their `sequence_number`.

use std::cell::Cell;

use serde_json::{json, Map, Value};

use super::openai::{
    parse_reasoning_effort, parse_response_format, parse_tool_choice, parse_tools, reject_unknown,
};
use crate::error::TranslateError;
use crate::types::{
    image_source, ChatRequest, ChatResponse, FinishReason, Message, Part, ResponseFormat, Role,
    StreamEvent, ToolCall, ToolChoice, Usage,
};

use super::prompt::parse_prompt;
pub use super::prompt::PromptRef;

#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub request: ChatRequest,
    pub prompt: Option<PromptRef>,
}

/// The request settings a response object repeats.
#[derive(Debug, Clone, PartialEq)]
pub struct Echo {
    pub tools: Vec<Value>,
    pub tool_choice: Value,
    pub parallel_tool_calls: bool,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub max_output_tokens: Option<u32>,
}

impl Echo {
    pub fn of(r: &ChatRequest) -> Echo {
        Echo {
            tools: r
                .tools
                .iter()
                .map(|t| {
                    json!({
                        "type": "function", "name": t.name, "description": t.description,
                        "parameters": t.parameters, "strict": t.strict,
                    })
                })
                .collect(),
            tool_choice: match &r.tool_choice {
                None | Some(ToolChoice::Auto) => json!("auto"),
                Some(ToolChoice::None) => json!("none"),
                Some(ToolChoice::Required) => json!("required"),
                Some(ToolChoice::Tool(n)) => json!({"type": "function", "name": n}),
            },
            parallel_tool_calls: r.parallel_tool_calls.unwrap_or(true),
            temperature: r.temperature,
            top_p: r.top_p,
            max_output_tokens: r.max_tokens,
        }
    }
}

fn invalid(m: impl Into<String>) -> TranslateError {
    TranslateError::InvalidRequest(m.into())
}

fn unsupported(m: impl Into<String>) -> TranslateError {
    TranslateError::Unsupported(m.into())
}

fn unsupported_field(field: &str) -> TranslateError {
    unsupported(format!("field '{field}' is not supported yet"))
}

/// Accepted and not used: they cannot change the generated text.
const IGNORED_FIELDS: &[&str] = &[
    "metadata",
    "user",
    "service_tier",
    "stream_options",
    "safety_identifier",
    "prompt_cache_key",
    "prompt_cache_retention",
];

fn number(v: &Value, field: &str) -> Result<Option<f32>, TranslateError> {
    match v {
        Value::Null => Ok(None),
        Value::Number(n) => Ok(n.as_f64().map(|f| f as f32)),
        _ => Err(invalid(format!("{field} must be a number"))),
    }
}

fn string_field(item: &Value, field: &str, what: &str) -> Result<String, TranslateError> {
    item[field]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| invalid(format!("{what} '{field}' must be a string")))
}

/// One content part of a message with this role.
fn parse_part(part: &Value, role: Role) -> Result<Part, TranslateError> {
    let fields = part
        .as_object()
        .ok_or_else(|| invalid("a content part must be an object"))?;
    let kind = part["type"].as_str().unwrap_or("unknown");
    let allowed = match kind {
        "input_text" => role != Role::Assistant,
        "input_image" => role == Role::User,
        "output_text" => role == Role::Assistant,
        other => {
            return Err(unsupported(format!(
                "content part '{other}' is not supported yet"
            )))
        }
    };
    if !allowed {
        let who = match role {
            Role::System => "system or developer",
            Role::User => "user",
            _ => "assistant",
        };
        return Err(unsupported(format!(
            "content part '{kind}' is not allowed in a {who} message"
        )));
    }
    match kind {
        "input_image" => {
            if !part["file_id"].is_null() {
                return Err(unsupported(
                    "input_image 'file_id' is not supported; use image_url",
                ));
            }
            reject_unknown(fields, &["type", "image_url", "detail"])?;
            let url = part["image_url"]
                .as_str()
                .ok_or_else(|| invalid("input_image 'image_url' must be a string"))?;
            Ok(Part::Image(image_source(url)?))
        }
        "output_text" => {
            reject_unknown(fields, &["type", "text", "annotations", "logprobs"])?;
            Ok(Part::Text(string_field(part, "text", "output_text")?))
        }
        _ => {
            reject_unknown(fields, &["type", "text"])?;
            Ok(Part::Text(string_field(part, "text", "input_text")?))
        }
    }
}

fn parse_message(item: &Value, fields: &Map<String, Value>) -> Result<Message, TranslateError> {
    reject_unknown(fields, &["type", "role", "content", "status", "id"])?;
    let role = match item["role"].as_str() {
        Some("system" | "developer") => Role::System,
        Some("user") => Role::User,
        Some("assistant") => Role::Assistant,
        Some(other) => {
            return Err(unsupported(format!(
                "message role '{other}' is not supported yet"
            )))
        }
        None => return Err(invalid("a message needs a 'role'")),
    };
    let content = match &item["content"] {
        Value::String(t) => vec![Part::Text(t.clone())],
        Value::Array(parts) => parts
            .iter()
            .map(|p| parse_part(p, role))
            .collect::<Result<_, _>>()?,
        _ => return Err(invalid("message content is required")),
    };
    Ok(Message {
        role,
        content,
        name: None,
        tool_calls: Vec::new(),
        tool_call_id: None,
    })
}

fn parse_input(items: &[Value]) -> Result<Vec<Message>, TranslateError> {
    let mut out: Vec<Message> = Vec::with_capacity(items.len());
    for item in items {
        let fields = item
            .as_object()
            .ok_or_else(|| invalid("an input item must be an object"))?;
        let kind = match item["type"].as_str() {
            Some(k) => k,
            None if fields.contains_key("role") => "message",
            None => return Err(invalid("an input item needs a 'type'")),
        };
        match kind {
            "message" => out.push(parse_message(item, fields)?),
            "function_call" => {
                reject_unknown(
                    fields,
                    &["type", "id", "call_id", "name", "arguments", "status"],
                )?;
                let call = ToolCall {
                    id: string_field(item, "call_id", "function_call")?,
                    name: string_field(item, "name", "function_call")?,
                    arguments: string_field(item, "arguments", "function_call")?,
                };
                // Calls in a row, and a call after the assistant's text, are
                // one assistant turn.
                match out.last_mut() {
                    Some(m) if m.role == Role::Assistant => m.tool_calls.push(call),
                    _ => {
                        let mut m = Message::text(Role::Assistant, "");
                        m.content.clear();
                        m.tool_calls.push(call);
                        out.push(m);
                    }
                }
            }
            "function_call_output" => {
                reject_unknown(fields, &["type", "id", "call_id", "output", "status"])?;
                let call_id = string_field(item, "call_id", "function_call_output")?;
                let output = item["output"]
                    .as_str()
                    .ok_or_else(|| invalid("function_call_output 'output' must be a string"))?;
                let mut m = Message::text(Role::Tool, output);
                m.tool_call_id = Some(call_id);
                out.push(m);
            }
            // Opaque state of the provider's own reasoning (summary, encrypted
            // content): it means nothing once translated, so it is dropped.
            // The one place the gateway accepts input and ignores it, so that
            // clients which send back what they were given still work.
            "reasoning" => {}
            other => {
                return Err(unsupported(format!(
                    "input item type '{other}' is not supported yet"
                )))
            }
        }
    }
    Ok(out)
}

fn parse_tools_field(tools: &[Value]) -> Result<Vec<crate::types::Tool>, TranslateError> {
    let mut wrapped = Vec::with_capacity(tools.len());
    for t in tools {
        let kind = t["type"].as_str().unwrap_or("unknown");
        if kind != "function" {
            return Err(unsupported(format!(
                "tool type '{kind}' is not supported yet"
            )));
        }
        let fields = t
            .as_object()
            .ok_or_else(|| invalid("a tool must be an object"))?;
        reject_unknown(
            fields,
            &["type", "name", "description", "parameters", "strict"],
        )?;
        let mut f = fields.clone();
        f.remove("type");
        wrapped.push(json!({"type": "function", "function": Value::Object(f)}));
    }
    parse_tools(wrapped)
}

fn parse_choice(v: &Value) -> Result<ToolChoice, TranslateError> {
    match v {
        Value::Object(o) => {
            let kind = v["type"].as_str().unwrap_or("unknown");
            if kind != "function" {
                return Err(unsupported(format!(
                    "tool_choice type '{kind}' is not supported yet"
                )));
            }
            reject_unknown(o, &["type", "name"])?;
            parse_tool_choice(json!({"type": "function", "function": {"name": v["name"]}}))
        }
        other => parse_tool_choice(other.clone()),
    }
}

fn parse_text(v: &Value) -> Result<Option<ResponseFormat>, TranslateError> {
    let fields = v
        .as_object()
        .ok_or_else(|| invalid("text must be an object"))?;
    reject_unknown(fields, &["format", "verbosity"])
        .map_err(|_| unsupported_field("text (only 'format' is supported)"))?;
    if !fields.get("verbosity").is_none_or(Value::is_null) {
        return Err(unsupported_field("text.verbosity"));
    }
    let format = match fields.get("format") {
        None | Some(Value::Null) => return Ok(None),
        Some(f) => f,
    };
    let f = format
        .as_object()
        .ok_or_else(|| invalid("text.format must be an object"))?;
    let wrapped = if format["type"] == "json_schema" {
        let mut spec = f.clone();
        spec.remove("type");
        json!({"type": "json_schema", "json_schema": Value::Object(spec)})
    } else {
        format.clone()
    };
    parse_response_format(&wrapped).map(Some)
}

/// `reasoning`: `effort` goes on to the provider (OpenAI and Azure; the
/// others refuse it); `summary` and `generate_summary` are checked and
/// ignored, since no summaries are produced.
fn parse_reasoning(v: &Value) -> Result<Option<String>, TranslateError> {
    let o = v
        .as_object()
        .ok_or_else(|| invalid("reasoning must be an object"))?;
    for (k, val) in o {
        match k.as_str() {
            "effort" => {}
            "summary" | "generate_summary" => match val {
                Value::Null => {}
                Value::String(s) if ["auto", "concise", "detailed"].contains(&s.as_str()) => {}
                _ => {
                    return Err(invalid(format!(
                        "reasoning {k} must be auto, concise or detailed"
                    )))
                }
            },
            other if val.is_null() => {
                let _ = other;
            }
            other => return Err(unsupported_field(&format!("reasoning.{other}"))),
        }
    }
    parse_reasoning_effort(o.get("effort").unwrap_or(&Value::Null))
}

/// `include`: the encrypted reasoning content is accepted and ignored (none
/// is produced); every other value asks for output the gateway cannot give.
fn check_include(v: Option<&Value>) -> Result<(), TranslateError> {
    match v {
        None | Some(Value::Null) => Ok(()),
        Some(Value::Array(items)) => {
            for i in items {
                match i.as_str() {
                    Some("reasoning.encrypted_content") => {}
                    Some(other) => {
                        return Err(unsupported(format!(
                            "include value '{other}' is not supported yet"
                        )))
                    }
                    None => return Err(invalid("include must be an array of strings")),
                }
            }
            Ok(())
        }
        Some(_) => Err(invalid("include must be an array of strings")),
    }
}

pub fn parse_request(body: &[u8]) -> Result<Parsed, TranslateError> {
    let v: Value = serde_json::from_slice(body).map_err(|e| invalid(e.to_string()))?;
    let o = v
        .as_object()
        .ok_or_else(|| invalid("the request body must be a JSON object"))?;
    // What the gateway does not do is named, not dropped.
    let flag = |name: &str| -> Result<bool, TranslateError> {
        match o.get(name) {
            None | Some(Value::Null) => Ok(false),
            Some(Value::Bool(b)) => Ok(*b),
            _ => Err(invalid(format!("{name} must be a boolean"))),
        }
    };
    if flag("store")? {
        return Err(invalid(
            "store is not supported; the gateway keeps no responses",
        ));
    }
    for field in ["previous_response_id", "conversation"] {
        if !o.get(field).is_none_or(Value::is_null) {
            return Err(invalid(format!(
                "{field} is not supported; the gateway keeps no responses"
            )));
        }
    }
    if flag("background")? {
        return Err(invalid(
            "background is not supported; the gateway keeps no responses",
        ));
    }
    const KNOWN: &[&str] = &[
        "model",
        "input",
        "instructions",
        "tools",
        "tool_choice",
        "parallel_tool_calls",
        "max_output_tokens",
        "temperature",
        "top_p",
        "text",
        "prompt",
        "stream",
        "store",
        "background",
        "previous_response_id",
        "conversation",
        "reasoning",
        "include",
        "truncation",
    ];
    for (k, val) in o {
        if !val.is_null() && !KNOWN.contains(&k.as_str()) && !IGNORED_FIELDS.contains(&k.as_str()) {
            return Err(unsupported_field(k));
        }
    }
    let prompt = match o.get("prompt") {
        None | Some(Value::Null) => None,
        Some(p) => Some(parse_prompt(p)?),
    };
    // With a prompt the model may come from the template.
    let model = match (&o.get("model"), &prompt) {
        (Some(Value::String(m)), _) if !m.is_empty() => m.clone(),
        (None | Some(Value::Null), Some(_)) => String::new(),
        _ => return Err(invalid("model is required")),
    };
    let mut messages = Vec::new();
    match o.get("instructions") {
        None | Some(Value::Null) => {}
        Some(Value::String(s)) => messages.push(Message::text(Role::System, s.as_str())),
        Some(_) => return Err(invalid("instructions must be a string")),
    }
    match o.get("input") {
        None | Some(Value::Null) if prompt.is_some() => {}
        None | Some(Value::Null) => return Err(invalid("input is required")),
        Some(Value::String(s)) => messages.push(Message::text(Role::User, s.as_str())),
        Some(Value::Array(items)) => {
            if items.is_empty() && prompt.is_none() {
                return Err(invalid("input must not be empty"));
            }
            let parsed = parse_input(items)?;
            if parsed.is_empty() && prompt.is_none() {
                return Err(invalid("input has nothing but reasoning items"));
            }
            messages.extend(parsed);
        }
        Some(_) => return Err(invalid("input must be a string or an array of items")),
    }
    let stream = flag("stream")?;
    let parallel_tool_calls = match o.get("parallel_tool_calls") {
        None | Some(Value::Null) => None,
        Some(Value::Bool(b)) => Some(*b),
        _ => return Err(invalid("parallel_tool_calls must be a boolean")),
    };
    let max_tokens = match o.get("max_output_tokens") {
        None | Some(Value::Null) => None,
        Some(n) => Some(
            n.as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or_else(|| invalid("max_output_tokens must be a positive integer"))?,
        ),
    };
    let reasoning_effort = match o.get("reasoning") {
        None | Some(Value::Null) => None,
        Some(r) => parse_reasoning(r)?,
    };
    check_include(o.get("include"))?;
    match o.get("truncation") {
        None | Some(Value::Null) => {}
        Some(Value::String(t)) if t == "disabled" => {}
        Some(Value::String(t)) if t == "auto" => return Err(unsupported(
            "truncation 'auto' is not supported; the gateway never drops input (use 'disabled')",
        )),
        Some(_) => return Err(invalid("truncation must be 'auto' or 'disabled'")),
    }
    let tools = match o.get("tools") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(t)) => parse_tools_field(t)?,
        Some(_) => return Err(invalid("tools must be an array")),
    };
    let tool_choice = match o.get("tool_choice") {
        None | Some(Value::Null) => None,
        Some(c) => Some(parse_choice(c)?),
    };
    let response_format = match o.get("text") {
        None | Some(Value::Null) => None,
        Some(t) => parse_text(t)?,
    };
    Ok(Parsed {
        request: ChatRequest {
            model,
            messages,
            max_tokens,
            temperature: number(o.get("temperature").unwrap_or(&Value::Null), "temperature")?,
            top_p: number(o.get("top_p").unwrap_or(&Value::Null), "top_p")?,
            stop: None,
            stream,
            tools,
            tool_choice,
            parallel_tool_calls,
            response_format,
            reasoning_effort,
        },
        prompt,
    })
}

/// A request float as the JSON number the caller wrote (not its f64 widening).
fn float(f: Option<f32>) -> Value {
    f.and_then(|f| f.to_string().parse::<f64>().ok())
        .map_or(Value::Null, |f| json!(f))
}

fn suffix(id: &str) -> &str {
    id.strip_prefix("resp_").unwrap_or(id)
}

fn message_item(id: &str, text: &str, status: &str) -> Value {
    let content = if status == "in_progress" {
        json!([])
    } else {
        json!([{"type": "output_text", "text": text, "annotations": []}])
    };
    json!({
        "id": format!("msg_{}", suffix(id)), "type": "message", "role": "assistant",
        "status": status, "content": content,
    })
}

fn call_item(item_id: &str, call_id: &str, name: &str, arguments: &str, status: &str) -> Value {
    json!({
        "id": item_id, "type": "function_call", "call_id": call_id, "name": name,
        "arguments": arguments, "status": status,
    })
}

fn call_item_id(id: &str, n: usize) -> String {
    format!("fc_{}{n}", suffix(id))
}

fn finished_arguments(arguments: &str) -> &str {
    if arguments.is_empty() {
        "{}"
    } else {
        arguments
    }
}

fn usage_json(u: Option<Usage>) -> Value {
    match u {
        None => Value::Null,
        Some(u) => json!({
            "input_tokens": u.input_tokens,
            "input_tokens_details": {"cached_tokens": 0, "cache_write_tokens": 0},
            "output_tokens": u.output_tokens,
            "output_tokens_details": {"reasoning_tokens": 0},
            "total_tokens": u64::from(u.input_tokens) + u64::from(u.output_tokens),
        }),
    }
}

/// `completed`, or `incomplete` with the reason.
fn status_of(finish: Option<FinishReason>) -> (&'static str, Value) {
    match finish {
        Some(FinishReason::Length) => ("incomplete", json!({"reason": "max_output_tokens"})),
        Some(FinishReason::ContentFilter) => ("incomplete", json!({"reason": "content_filter"})),
        _ => ("completed", Value::Null),
    }
}

struct Object<'a> {
    id: &'a str,
    created: u64,
    model: &'a str,
    echo: &'a Echo,
}

impl Object<'_> {
    fn render(&self, status: &str, incomplete: Value, output: Vec<Value>, usage: Value) -> Value {
        json!({
            "id": self.id, "object": "response", "created_at": self.created,
            "status": status, "incomplete_details": incomplete, "error": null,
            "model": self.model, "output": output, "usage": usage,
            "instructions": null, "metadata": {},
            "parallel_tool_calls": self.echo.parallel_tool_calls,
            "temperature": float(self.echo.temperature),
            "top_p": float(self.echo.top_p),
            "max_output_tokens": self.echo.max_output_tokens,
            "tool_choice": self.echo.tool_choice, "tools": self.echo.tools,
        })
    }
}

pub fn render_response(r: &ChatResponse, id: &str, created: u64, echo: &Echo) -> Value {
    let (status, incomplete) = status_of(r.finish_reason);
    let mut output = Vec::new();
    if !r.content.is_empty() {
        output.push(message_item(id, &r.content, "completed"));
    }
    for (n, c) in r.tool_calls.iter().enumerate() {
        output.push(call_item(
            &call_item_id(id, n),
            &c.id,
            &c.name,
            finished_arguments(&c.arguments),
            "completed",
        ));
    }
    Object {
        id,
        created,
        model: &r.model,
        echo,
    }
    .render(status, incomplete, output, usage_json(r.usage))
}

struct CallState {
    call_index: u32,
    output_index: u32,
    item_id: String,
    call_id: String,
    name: String,
    arguments: String,
}

/// Renders the events of one answer as Responses API server-sent events.
///
/// Items are opened when their first piece arrives and stay open until the
/// answer ends, because providers interleave tool-call arguments; then each
/// is closed in the order of its `output_index`. All the text of an answer is
/// one message item, wherever it comes.
pub struct StreamRenderer {
    id: String,
    model: String,
    created: u64,
    echo: Echo,
    seq: Cell<u32>,
    started: bool,
    finished: bool,
    next_output: u32,
    /// The message item: its output index and the text so far.
    text: Option<(u32, String)>,
    calls: Vec<CallState>,
}

impl StreamRenderer {
    pub fn new(id: &str, model: &str, created: u64, echo: Echo) -> Self {
        Self {
            id: id.to_string(),
            model: model.to_string(),
            created,
            echo,
            seq: Cell::new(0),
            started: false,
            finished: false,
            next_output: 0,
            text: None,
            calls: Vec::new(),
        }
    }

    fn event(&self, name: &str, mut data: Value) -> String {
        let n = self.seq.get();
        self.seq.set(n + 1);
        data["type"] = json!(name);
        data["sequence_number"] = json!(n);
        format!("event: {name}\ndata: {data}\n\n")
    }

    fn object(&self) -> Object<'_> {
        Object {
            id: &self.id,
            created: self.created,
            model: &self.model,
            echo: &self.echo,
        }
    }

    fn open(&mut self) -> String {
        if self.started {
            return String::new();
        }
        self.started = true;
        let response = self
            .object()
            .render("in_progress", Value::Null, Vec::new(), Value::Null);
        let mut out = self.event("response.created", json!({"response": response}));
        out.push_str(&self.event("response.in_progress", json!({"response": response})));
        out
    }

    fn msg_id(&self) -> String {
        format!("msg_{}", suffix(&self.id))
    }

    pub fn render(&mut self, ev: &StreamEvent) -> String {
        if self.finished {
            return String::new();
        }
        let mut out = self.open();
        match ev {
            StreamEvent::Delta { text } => {
                if self.text.is_none() {
                    let index = self.next_output;
                    self.next_output += 1;
                    self.text = Some((index, String::new()));
                    out.push_str(&self.event(
                        "response.output_item.added",
                        json!({"output_index": index,
                            "item": message_item(&self.id, "", "in_progress")}),
                    ));
                    out.push_str(&self.event(
                        "response.content_part.added",
                        json!({"output_index": index, "item_id": self.msg_id(),
                            "content_index": 0,
                            "part": {"type": "output_text", "text": "", "annotations": []}}),
                    ));
                }
                let (index, buf) = self.text.as_mut().expect("opened above");
                buf.push_str(text);
                let index = *index;
                out.push_str(&self.event(
                    "response.output_text.delta",
                    json!({"output_index": index, "item_id": self.msg_id(),
                        "content_index": 0, "delta": text, "logprobs": []}),
                ));
            }
            StreamEvent::ToolCallStart { index, id, name } => {
                if self.calls.iter().any(|c| c.call_index == *index) {
                    return out;
                }
                let output_index = self.next_output;
                self.next_output += 1;
                let item_id = call_item_id(&self.id, self.calls.len());
                out.push_str(&self.event(
                    "response.output_item.added",
                    json!({"output_index": output_index,
                        "item": call_item(&item_id, id, name, "", "in_progress")}),
                ));
                self.calls.push(CallState {
                    call_index: *index,
                    output_index,
                    item_id,
                    call_id: id.clone(),
                    name: name.clone(),
                    arguments: String::new(),
                });
            }
            StreamEvent::ToolCallDelta { index, arguments } => {
                // Every decoder refuses a delta for an unknown call before it
                // gets here.
                let Some(c) = self.calls.iter_mut().find(|c| c.call_index == *index) else {
                    return out;
                };
                c.arguments.push_str(arguments);
                let (output_index, item_id) = (c.output_index, c.item_id.clone());
                out.push_str(&self.event(
                    "response.function_call_arguments.delta",
                    json!({"output_index": output_index, "item_id": item_id,
                        "delta": arguments}),
                ));
            }
            StreamEvent::Done {
                finish_reason,
                usage,
            } => {
                self.finished = true;
                out.push_str(&self.close(*finish_reason, *usage));
            }
        }
        out
    }

    fn close(&mut self, finish: Option<FinishReason>, usage: Option<Usage>) -> String {
        enum Open {
            Text(String),
            Call(usize),
        }
        let mut order: Vec<(u32, Open)> = Vec::new();
        if let Some((index, text)) = &self.text {
            order.push((*index, Open::Text(text.clone())));
        }
        for (i, c) in self.calls.iter().enumerate() {
            order.push((c.output_index, Open::Call(i)));
        }
        order.sort_by_key(|(index, _)| *index);
        let mut out = String::new();
        let mut output = Vec::new();
        for (index, open) in order {
            match open {
                Open::Text(text) => {
                    let part = json!({"type": "output_text", "text": text, "annotations": []});
                    out.push_str(&self.event(
                        "response.output_text.done",
                        json!({"output_index": index, "item_id": self.msg_id(),
                            "content_index": 0, "text": text, "logprobs": []}),
                    ));
                    out.push_str(&self.event(
                        "response.content_part.done",
                        json!({"output_index": index, "item_id": self.msg_id(),
                            "content_index": 0, "part": part}),
                    ));
                    let item = message_item(&self.id, &text, "completed");
                    out.push_str(&self.event(
                        "response.output_item.done",
                        json!({"output_index": index, "item": item}),
                    ));
                    output.push(item);
                }
                Open::Call(i) => {
                    let (item_id, call_id, name, mut arguments) = {
                        let c = &self.calls[i];
                        (
                            c.item_id.clone(),
                            c.call_id.clone(),
                            c.name.clone(),
                            c.arguments.clone(),
                        )
                    };
                    if arguments.is_empty() {
                        // A call with no arguments is an empty object, and
                        // the deltas must add up to what `done` says.
                        arguments = finished_arguments(&arguments).to_string();
                        out.push_str(&self.event(
                            "response.function_call_arguments.delta",
                            json!({"output_index": index, "item_id": item_id,
                                "delta": arguments}),
                        ));
                    }
                    out.push_str(&self.event(
                        "response.function_call_arguments.done",
                        json!({"output_index": index, "item_id": item_id,
                            "arguments": arguments}),
                    ));
                    let item = call_item(&item_id, &call_id, &name, &arguments, "completed");
                    out.push_str(&self.event(
                        "response.output_item.done",
                        json!({"output_index": index, "item": item}),
                    ));
                    output.push(item);
                }
            }
        }
        let (status, incomplete) = status_of(finish);
        let response = self
            .object()
            .render(status, incomplete, output, usage_json(usage));
        let name = if status == "completed" {
            "response.completed"
        } else {
            "response.incomplete"
        };
        out.push_str(&self.event(name, json!({"response": response})));
        out
    }

    /// An error ends the stream; it takes the next sequence number.
    pub fn error(&self, message: &str) -> String {
        self.event(
            "error",
            json!({"code": null, "message": message, "param": null}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;
    use serde_json::json;

    fn parse(v: Value) -> Result<Parsed, TranslateError> {
        parse_request(v.to_string().as_bytes())
    }

    fn ok(v: Value) -> ChatRequest {
        parse(v).unwrap().request
    }

    fn refused(v: Value) -> String {
        match parse(v).unwrap_err() {
            TranslateError::InvalidRequest(m) | TranslateError::Unsupported(m) => m,
            e => panic!("{e}"),
        }
    }

    #[test]
    fn a_string_input_is_one_user_message_and_instructions_come_first() {
        let r = ok(json!({"model":"p/m","input":"hi","instructions":"be brief",
            "max_output_tokens":9,"temperature":0.5,"top_p":0.9,"stream":true,
            "parallel_tool_calls":false,"metadata":{"a":"b"},"user":"u","store":false}));
        assert_eq!(r.model, "p/m");
        assert_eq!(
            r.messages,
            vec![
                Message::text(Role::System, "be brief"),
                Message::text(Role::User, "hi")
            ]
        );
        assert_eq!(r.max_tokens, Some(9));
        assert_eq!(r.temperature, Some(0.5));
        assert_eq!(r.top_p, Some(0.9));
        assert!(r.stream);
        assert_eq!(r.parallel_tool_calls, Some(false));
        assert_eq!(r.stop, None);
    }

    #[test]
    fn message_items_of_every_role_and_content_kind() {
        let r = ok(json!({"model":"m","input":[
            {"type":"message","role":"developer","content":"rules"},
            {"role":"system","content":[{"type":"input_text","text":"more"}]},
            {"type":"message","role":"user","content":[
                {"type":"input_text","text":"look"},
                {"type":"input_image","image_url":"https://x.test/a.png","detail":"auto"},
                {"type":"input_image","image_url":"data:image/png;base64,QUJD"}]},
            {"type":"message","role":"assistant","content":[{"type":"output_text","text":"seen","annotations":[]}]},
            {"role":"user","content":"thanks"}
        ]}));
        assert_eq!(r.messages.len(), 5);
        assert_eq!(r.messages[0], Message::text(Role::System, "rules"));
        assert_eq!(r.messages[1], Message::text(Role::System, "more"));
        assert_eq!(
            r.messages[2].content,
            vec![
                Part::Text("look".into()),
                Part::Image(ImageSource::Url("https://x.test/a.png".into())),
                Part::Image(ImageSource::Base64 {
                    media_type: "image/png".into(),
                    data: "QUJD".into()
                }),
            ]
        );
        assert_eq!(r.messages[3], Message::text(Role::Assistant, "seen"));
        assert_eq!(r.messages[4], Message::text(Role::User, "thanks"));
    }

    #[test]
    fn function_calls_and_their_outputs_round_trip() {
        let r = ok(json!({"model":"m","input":[
            {"role":"user","content":"weather?"},
            {"type":"function_call","id":"fc_1","call_id":"call_1","name":"w","arguments":"{\"a\":1}","status":"completed"},
            {"type":"function_call","call_id":"call_2","name":"x","arguments":"{}"},
            {"type":"function_call_output","call_id":"call_1","output":"sunny"},
            {"type":"function_call_output","call_id":"call_2","output":"rain"}
        ]}));
        assert_eq!(r.messages.len(), 4);
        // Two calls in a row are one assistant turn.
        assert_eq!(r.messages[1].role, Role::Assistant);
        assert!(r.messages[1].content.is_empty());
        assert_eq!(
            r.messages[1].tool_calls,
            vec![
                ToolCall {
                    id: "call_1".into(),
                    name: "w".into(),
                    arguments: "{\"a\":1}".into()
                },
                ToolCall {
                    id: "call_2".into(),
                    name: "x".into(),
                    arguments: "{}".into()
                },
            ]
        );
        assert_eq!(r.messages[2].role, Role::Tool);
        assert_eq!(r.messages[2].tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(r.messages[2].joined_text(), "sunny");
        assert_eq!(r.messages[3].tool_call_id.as_deref(), Some("call_2"));
    }

    #[test]
    fn a_call_after_assistant_text_joins_that_turn() {
        let r = ok(json!({"model":"m","input":[
            {"role":"user","content":"x"},
            {"role":"assistant","content":[{"type":"output_text","text":"checking"}]},
            {"type":"function_call","call_id":"c","name":"w","arguments":"{}"}
        ]}));
        assert_eq!(r.messages.len(), 2);
        assert_eq!(r.messages[1].joined_text(), "checking");
        assert_eq!(r.messages[1].tool_calls.len(), 1);
    }

    #[test]
    fn tools_tool_choice_and_text_format() {
        let r = ok(json!({"model":"m","input":"x",
            "tools":[{"type":"function","name":"w","description":"d","parameters":{"type":"object","properties":{"a":{"type":"string"}}},"strict":true}],
            "tool_choice":{"type":"function","name":"w"},
            "text":{"format":{"type":"json_schema","name":"out","schema":{"type":"object"},"strict":true,"description":"dd"}}}));
        assert_eq!(r.tools.len(), 1);
        assert_eq!(r.tools[0].name, "w");
        assert_eq!(r.tools[0].description.as_deref(), Some("d"));
        assert_eq!(r.tools[0].strict, Some(true));
        assert_eq!(r.tool_choice, Some(ToolChoice::Tool("w".into())));
        assert_eq!(
            r.response_format,
            Some(ResponseFormat::JsonSchema {
                name: "out".into(),
                schema: json!({"type":"object"}),
                strict: Some(true),
                description: Some("dd".into())
            })
        );
        for (s, want) in [
            ("auto", ToolChoice::Auto),
            ("none", ToolChoice::None),
            ("required", ToolChoice::Required),
        ] {
            assert_eq!(
                ok(json!({"model":"m","input":"x","tool_choice":s})).tool_choice,
                Some(want)
            );
        }
        assert_eq!(
            ok(json!({"model":"m","input":"x","text":{"format":{"type":"json_object"}}}))
                .response_format,
            Some(ResponseFormat::JsonObject)
        );
        assert_eq!(
            ok(json!({"model":"m","input":"x","text":{"format":{"type":"text"}}})).response_format,
            Some(ResponseFormat::Text)
        );
    }

    #[test]
    fn the_prompt_reference_is_parsed_for_later() {
        let p = parse(json!({"model":"m","input":"x","prompt":{"id":"greet","version":"2","variables":{"name":"Ada"}}})).unwrap();
        let prompt = p.prompt.unwrap();
        assert_eq!(prompt.id, "greet");
        assert_eq!(prompt.version.as_deref(), Some("2"));
        assert_eq!(prompt.variables["name"], "Ada");
        assert!(parse(json!({"model":"m","input":"x"}))
            .unwrap()
            .prompt
            .is_none());
        assert!(refused(json!({"model":"m","input":"x","prompt":{"version":"1"}})).contains("id"));
        assert!(
            refused(json!({"model":"m","input":"x","prompt":{"id":"a","variables":{"n":3}}}))
                .contains("variables")
        );
    }

    #[test]
    fn a_prompt_lets_the_model_and_input_be_left_out_and_takes_a_version_as_text_or_number() {
        let p = parse(json!({"prompt":{"id":"greet","version":3}})).unwrap();
        assert_eq!(p.request.model, "");
        assert!(p.request.messages.is_empty());
        assert_eq!(p.prompt.unwrap().version.as_deref(), Some("3"));
        assert_eq!(
            refused(json!({"input":"x"})),
            "model is required",
            "no prompt, no model"
        );
        for bad in [
            json!("x"),
            json!("1.5"),
            json!(-2),
            json!(1.5),
            json!(true),
            json!(""),
        ] {
            assert!(
                refused(json!({"model":"m","input":"x","prompt":{"id":"a","version":bad}}))
                    .contains("version"),
                "{bad}"
            );
        }
    }

    #[test]
    fn everything_the_gateway_does_not_do_is_refused_with_a_reason() {
        let table: Vec<(Value, &str)> = vec![
            (
                json!({"store":true}),
                "store is not supported; the gateway keeps no responses",
            ),
            (
                json!({"previous_response_id":"resp_1"}),
                "previous_response_id",
            ),
            (json!({"conversation":"conv_1"}), "conversation"),
            (json!({"background":true}), "background"),
            (json!({"reasoning":{"effort":"loud"}}), "reasoning"),
            (json!({"reasoning":{"effort":3}}), "reasoning"),
            (json!({"reasoning":{"summary":"verbose"}}), "summary"),
            (json!({"reasoning":{"mode":"pro"}}), "mode"),
            (json!({"reasoning":"low"}), "reasoning"),
            (json!({"include":["file_search_call.results"]}), "include"),
            (
                json!({"include":["reasoning.encrypted_content","message.output_text.logprobs"]}),
                "include",
            ),
            (json!({"include":"reasoning.encrypted_content"}), "include"),
            (json!({"truncation":"auto"}), "truncation"),
            (json!({"truncation":"sometimes"}), "truncation"),
            (json!({"store":"true"}), "store"),
            (json!({"store":1}), "store"),
            (json!({"background":"true"}), "background"),
            (json!({"background":0}), "background"),
            (json!({"max_tool_calls":1}), "max_tool_calls"),
            (json!({"text":{"verbosity":"low"}}), "verbosity"),
            (json!({"tools":[{"type":"web_search"}]}), "web_search"),
            (
                json!({"tools":[{"type":"file_search","vector_store_ids":[]}]}),
                "file_search",
            ),
            (
                json!({"tools":[{"type":"computer_use_preview"}]}),
                "computer_use_preview",
            ),
            (
                json!({"tools":[{"type":"function","name":"w","extra":1}]}),
                "extra",
            ),
            (json!({"tool_choice":{"type":"web_search"}}), "tool_choice"),
            (json!({"tool_choice":"sometimes"}), "sometimes"),
            (
                json!({"input":[{"type":"reasoning","summary":[]}]}),
                "reasoning",
            ),
            (
                json!({"input":[{"type":"item_reference","id":"x"}]}),
                "item_reference",
            ),
            (
                json!({"input":[{"role":"user","content":[{"type":"input_file","file_id":"f"}]}]}),
                "input_file",
            ),
            (
                json!({"input":[{"role":"user","content":[{"type":"input_image","file_id":"f"}]}]}),
                "image_url",
            ),
            (
                json!({"input":[{"role":"user","content":[{"type":"output_text","text":"x"}]}]}),
                "output_text",
            ),
            (
                json!({"input":[{"role":"assistant","content":[{"type":"input_text","text":"x"}]}]}),
                "input_text",
            ),
            (
                json!({"input":[{"role":"assistant","content":[{"type":"refusal","refusal":"no"}]}]}),
                "refusal",
            ),
            (
                json!({"input":[{"role":"user","content":[{"type":"input_text","text":"x","bogus":1}]}]}),
                "bogus",
            ),
            (json!({"input":[{"role":"tool","content":"x"}]}), "tool"),
            (json!({"input":[{"role":"user"}]}), "content"),
            (
                json!({"input":[{"type":"function_call","name":"w","arguments":"{}"}]}),
                "call_id",
            ),
            (
                json!({"input":[{"type":"function_call_output","call_id":"c","output":[{"type":"input_text","text":"x"}]}]}),
                "output",
            ),
            (
                json!({"input":[{"type":"function_call_output","output":"x"}]}),
                "call_id",
            ),
            (json!({"input":[]}), "input"),
            (json!({"input":5}), "input"),
            (json!({"input":"x","stream":"yes"}), "stream"),
            (json!({"input":"x","bogus":1}), "bogus"),
        ];
        for (extra, want) in table {
            let mut body = json!({"model":"m","input":"x"});
            for (k, v) in extra.as_object().unwrap() {
                body[k] = v.clone();
            }
            let m = refused(body.clone());
            assert!(m.contains(want), "{body}: {m}");
        }
        // Falsy forms of the refused fields are accepted.
        ok(
            json!({"model":"m","input":"x","store":false,"background":false,"previous_response_id":null,"conversation":null,"reasoning":null}),
        );
        // No model, or no input.
        assert!(refused(json!({"input":"x"})).contains("model"));
        assert!(refused(json!({"model":"m"})).contains("input"));
    }

    #[test]
    fn reasoning_effort_summary_include_and_truncation_are_accepted() {
        for e in ["none", "minimal", "low", "medium", "high", "xhigh", "max"] {
            let r = ok(json!({"model":"m","input":"x","reasoning":{"effort":e}}));
            assert_eq!(r.reasoning_effort.as_deref(), Some(e));
        }
        let r = ok(
            json!({"model":"m","input":"x","reasoning":{"effort":null,"summary":"auto","generate_summary":"concise"}}),
        );
        assert_eq!(r.reasoning_effort, None);
        assert_eq!(
            ok(json!({"model":"m","input":"x","reasoning":null})).reasoning_effort,
            None
        );
        // The summary is accepted and ignored: nothing of it reaches the request.
        let r = ok(json!({"model":"m","input":"x","reasoning":{"summary":"detailed"}}));
        assert_eq!(r.reasoning_effort, None);
        ok(json!({"model":"m","input":"x","include":["reasoning.encrypted_content"]}));
        ok(json!({"model":"m","input":"x","include":[]}));
        ok(json!({"model":"m","input":"x","include":null,"truncation":null}));
        ok(json!({"model":"m","input":"x","truncation":"disabled"}));
        ok(json!({"model":"m","input":"x","store":false,"background":false}));
    }

    #[test]
    fn reasoning_items_are_accepted_and_dropped() {
        let r = ok(json!({"model":"m","input":[
            {"role":"user","content":"weather?"},
            {"type":"reasoning","id":"rs_1","summary":[{"type":"summary_text","text":"thinking"}],"encrypted_content":"abc","status":"completed"},
            {"type":"function_call","call_id":"c1","name":"w","arguments":"{}"},
            {"type":"reasoning","id":"rs_2","summary":[]},
            {"type":"function_call","call_id":"c2","name":"x","arguments":"{}"},
            {"type":"function_call_output","call_id":"c1","output":"a"},
            {"type":"function_call_output","call_id":"c2","output":"b"}
        ]}));
        // The two calls stay one assistant turn; no message stands for the reasoning.
        assert_eq!(r.messages.len(), 4);
        assert_eq!(r.messages[1].tool_calls.len(), 2);
        // Only reasoning in the input is still an empty conversation.
        assert!(
            refused(json!({"model":"m","input":[{"type":"reasoning","summary":[]}]}))
                .contains("input")
        );
    }

    fn answer(content: &str, calls: Vec<ToolCall>, finish: Option<FinishReason>) -> ChatResponse {
        ChatResponse {
            id: "c".into(),
            model: "m".into(),
            content: content.into(),
            tool_calls: calls,
            finish_reason: finish,
            usage: Some(Usage {
                input_tokens: 4,
                output_tokens: 2,
            }),
        }
    }

    fn echo() -> Echo {
        Echo::of(&ok(json!({"model":"m","input":"x"})))
    }

    #[test]
    fn renders_a_response_with_text_and_calls() {
        let r = answer(
            "hello",
            vec![ToolCall {
                id: "call_1".into(),
                name: "w".into(),
                arguments: "{\"a\":1}".into(),
            }],
            Some(FinishReason::ToolCalls),
        );
        let v = render_response(&r, "resp_abc", 77, &echo());
        assert_eq!(v["id"], "resp_abc");
        assert_eq!(v["object"], "response");
        assert_eq!(v["created_at"], 77);
        assert_eq!(v["status"], "completed");
        assert_eq!(v["incomplete_details"], Value::Null);
        assert_eq!(v["error"], Value::Null);
        assert_eq!(v["model"], "m");
        assert_eq!(v["parallel_tool_calls"], true);
        assert_eq!(v["tool_choice"], "auto");
        assert_eq!(v["tools"], json!([]));
        let out = v["output"].as_array().unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0]["type"], "message");
        assert_eq!(out[0]["role"], "assistant");
        assert_eq!(out[0]["status"], "completed");
        assert_eq!(
            out[0]["content"],
            json!([{"type":"output_text","text":"hello","annotations":[]}])
        );
        assert!(out[0]["id"].as_str().unwrap().starts_with("msg_"));
        assert_eq!(out[1]["type"], "function_call");
        assert_eq!(out[1]["call_id"], "call_1");
        assert_eq!(out[1]["name"], "w");
        assert_eq!(out[1]["arguments"], "{\"a\":1}");
        assert!(out[1]["id"].as_str().unwrap().starts_with("fc_"));
        assert_ne!(out[0]["id"], out[1]["id"]);
        assert_eq!(v["usage"]["input_tokens"], 4);
        assert_eq!(v["usage"]["output_tokens"], 2);
        assert_eq!(v["usage"]["total_tokens"], 6);
        assert_eq!(v["usage"]["input_tokens_details"]["cached_tokens"], 0);
        assert_eq!(v["usage"]["output_tokens_details"]["reasoning_tokens"], 0);
    }

    #[test]
    fn calls_only_have_no_message_and_empty_arguments_are_an_object() {
        let r = answer(
            "",
            vec![ToolCall {
                id: "c".into(),
                name: "w".into(),
                arguments: String::new(),
            }],
            Some(FinishReason::ToolCalls),
        );
        let v = render_response(&r, "resp_a", 1, &echo());
        assert_eq!(v["output"].as_array().unwrap().len(), 1);
        assert_eq!(v["output"][0]["arguments"], "{}");
    }

    #[test]
    fn finish_reasons_map_to_status() {
        let v = render_response(
            &answer("a", vec![], Some(FinishReason::Length)),
            "resp_a",
            1,
            &echo(),
        );
        assert_eq!(v["status"], "incomplete");
        assert_eq!(
            v["incomplete_details"],
            json!({"reason":"max_output_tokens"})
        );
        let v = render_response(
            &answer("a", vec![], Some(FinishReason::ContentFilter)),
            "resp_a",
            1,
            &echo(),
        );
        assert_eq!(v["status"], "incomplete");
        assert_eq!(v["incomplete_details"], json!({"reason":"content_filter"}));
        for f in [
            None,
            Some(FinishReason::Stop),
            Some(FinishReason::ToolCalls),
        ] {
            let v = render_response(&answer("a", vec![], f), "resp_a", 1, &echo());
            assert_eq!(v["status"], "completed");
        }
        let mut r = answer("a", vec![], None);
        r.usage = None;
        assert_eq!(
            render_response(&r, "resp_a", 1, &echo())["usage"],
            Value::Null
        );
    }

    #[test]
    fn the_response_repeats_the_request_settings() {
        let req = ok(
            json!({"model":"m","input":"x","temperature":0.2,"top_p":0.5,"max_output_tokens":7,
            "parallel_tool_calls":false,"tool_choice":"required",
            "tools":[{"type":"function","name":"w","description":"d","parameters":{"type":"object"},"strict":false}]}),
        );
        let v = render_response(&answer("a", vec![], None), "resp_a", 1, &Echo::of(&req));
        assert_eq!(v["parallel_tool_calls"], false);
        assert_eq!(v["tool_choice"], "required");
        assert_eq!(v["max_output_tokens"], 7);
        assert_eq!(v["top_p"], 0.5);
        assert_eq!(
            v["tools"],
            json!([{"type":"function","name":"w","description":"d","parameters":{"type":"object"},"strict":false}])
        );
        let req = ok(json!({"model":"m","input":"x","tool_choice":{"type":"function","name":"w"}}));
        assert_eq!(
            Echo::of(&req).tool_choice,
            json!({"type":"function","name":"w"})
        );
    }

    /// `(event name, data)` of every event.
    pub(crate) fn events(body: &str) -> Vec<(String, Value)> {
        body.split("\n\n")
            .filter(|e| !e.trim().is_empty())
            .map(|e| {
                let name = e.lines().find_map(|l| l.strip_prefix("event: ")).unwrap();
                let data = e.lines().find_map(|l| l.strip_prefix("data: ")).unwrap();
                let v: Value = serde_json::from_str(data).unwrap();
                assert_eq!(v["type"], name, "{e}");
                (name.to_string(), v)
            })
            .collect()
    }

    fn stream(evs: &[StreamEvent]) -> String {
        let mut r = StreamRenderer::new("resp_abc", "m", 5, echo());
        evs.iter().map(|e| r.render(e)).collect()
    }

    fn text_two_calls() -> Vec<StreamEvent> {
        vec![
            StreamEvent::Delta { text: "Hel".into() },
            StreamEvent::Delta { text: "lo".into() },
            StreamEvent::ToolCallStart {
                index: 0,
                id: "call_a".into(),
                name: "w".into(),
            },
            StreamEvent::ToolCallDelta {
                index: 0,
                arguments: "{\"a\":".into(),
            },
            StreamEvent::ToolCallStart {
                index: 1,
                id: "call_b".into(),
                name: "x".into(),
            },
            StreamEvent::ToolCallDelta {
                index: 0,
                arguments: "1}".into(),
            },
            StreamEvent::ToolCallDelta {
                index: 1,
                arguments: "{}".into(),
            },
            StreamEvent::Done {
                finish_reason: Some(FinishReason::ToolCalls),
                usage: Some(Usage {
                    input_tokens: 3,
                    output_tokens: 8,
                }),
            },
        ]
    }

    #[test]
    fn a_stream_of_text_then_two_calls_is_ordered_and_consistent() {
        let all = events(&stream(&text_two_calls()));
        let names: Vec<&str> = all.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                "response.created",
                "response.in_progress",
                "response.output_item.added",
                "response.content_part.added",
                "response.output_text.delta",
                "response.output_text.delta",
                "response.output_item.added",
                "response.function_call_arguments.delta",
                "response.output_item.added",
                "response.function_call_arguments.delta",
                "response.function_call_arguments.delta",
                "response.output_text.done",
                "response.content_part.done",
                "response.output_item.done",
                "response.function_call_arguments.done",
                "response.output_item.done",
                "response.function_call_arguments.done",
                "response.output_item.done",
                "response.completed",
            ]
        );
        // sequence_number counts 0.. without a gap.
        for (i, (_, v)) in all.iter().enumerate() {
            assert_eq!(v["sequence_number"], i, "{v}");
        }
        // Created and in_progress carry an empty in-progress response.
        assert_eq!(all[0].1["response"]["status"], "in_progress");
        assert_eq!(all[0].1["response"]["output"], json!([]));
        assert_eq!(all[0].1["response"]["id"], "resp_abc");
        assert_eq!(all[1].1["response"]["status"], "in_progress");
        // The text is output 0, the calls 1 and 2.
        let msg_id = all[2].1["item"]["id"].as_str().unwrap().to_string();
        assert_eq!(all[2].1["output_index"], 0);
        assert_eq!(all[2].1["item"]["type"], "message");
        assert_eq!(all[2].1["item"]["status"], "in_progress");
        assert_eq!(all[2].1["item"]["content"], json!([]));
        assert_eq!(all[3].1["item_id"], msg_id.as_str());
        assert_eq!(all[3].1["content_index"], 0);
        assert_eq!(
            all[3].1["part"],
            json!({"type":"output_text","text":"","annotations":[]})
        );
        assert_eq!(all[4].1["delta"], "Hel");
        assert_eq!(all[4].1["item_id"], msg_id.as_str());
        assert_eq!(all[4].1["output_index"], 0);
        assert_eq!(all[4].1["logprobs"], json!([]));
        let fc_a = all[6].1["item"]["id"].as_str().unwrap().to_string();
        let fc_b = all[8].1["item"]["id"].as_str().unwrap().to_string();
        assert_ne!(fc_a, fc_b);
        assert_eq!(all[6].1["output_index"], 1);
        assert_eq!(all[6].1["item"]["type"], "function_call");
        assert_eq!(all[6].1["item"]["call_id"], "call_a");
        assert_eq!(all[6].1["item"]["name"], "w");
        assert_eq!(all[6].1["item"]["arguments"], "");
        assert_eq!(all[8].1["output_index"], 2);
        // Argument deltas name their item and output index.
        assert_eq!(all[7].1["item_id"], fc_a.as_str());
        assert_eq!(all[7].1["output_index"], 1);
        assert_eq!(all[9].1["item_id"], fc_a.as_str());
        assert_eq!(all[9].1["delta"], "1}");
        assert_eq!(all[10].1["item_id"], fc_b.as_str());
        assert_eq!(all[10].1["output_index"], 2);
        // The closing events repeat the whole text and arguments.
        assert_eq!(all[11].1["text"], "Hello");
        assert_eq!(all[11].1["item_id"], msg_id.as_str());
        assert_eq!(all[11].1["content_index"], 0);
        assert_eq!(all[11].1["output_index"], 0);
        assert_eq!(all[11].1["logprobs"], json!([]));
        assert_eq!(all[12].1["content_index"], 0);
        assert_eq!(all[12].1["item_id"], msg_id.as_str());
        assert_eq!(all[12].1["output_index"], 0);
        // The same index on the delta events and on content_part.added.
        assert_eq!(all[3].1["output_index"], 0);
        assert_eq!(all[5].1["content_index"], 0);
        assert_eq!(all[5].1["item_id"], msg_id.as_str());
        assert_eq!(all[5].1["output_index"], 0);
        assert_eq!(
            all[12].1["part"],
            json!({"type":"output_text","text":"Hello","annotations":[]})
        );
        assert_eq!(all[13].1["item"]["status"], "completed");
        assert_eq!(all[13].1["item"]["content"][0]["text"], "Hello");
        assert_eq!(all[14].1["arguments"], "{\"a\":1}");
        assert_eq!(all[14].1["output_index"], 1);
        assert_eq!(all[15].1["item"]["arguments"], "{\"a\":1}");
        assert_eq!(all[15].1["item"]["status"], "completed");
        assert_eq!(all[16].1["arguments"], "{}");
        assert_eq!(all[17].1["output_index"], 2);
        // The final response has the same items.
        let fin = &all[18].1["response"];
        assert_eq!(fin["status"], "completed");
        assert_eq!(fin["output"].as_array().unwrap().len(), 3);
        assert_eq!(fin["output"][0]["id"], msg_id.as_str());
        assert_eq!(fin["output"][1]["id"], fc_a.as_str());
        assert_eq!(fin["output"][1]["arguments"], "{\"a\":1}");
        assert_eq!(fin["output"][2]["id"], fc_b.as_str());
        assert_eq!(fin["usage"]["total_tokens"], 11);
        // No [DONE] on this API.
        assert!(!stream(&text_two_calls()).contains("[DONE]"));
    }

    #[test]
    fn a_length_stop_ends_with_incomplete() {
        let all = events(&stream(&[
            StreamEvent::Delta { text: "a".into() },
            StreamEvent::Done {
                finish_reason: Some(FinishReason::Length),
                usage: None,
            },
        ]));
        let (name, last) = all.last().unwrap();
        assert_eq!(name, "response.incomplete");
        assert_eq!(last["response"]["status"], "incomplete");
        assert_eq!(
            last["response"]["incomplete_details"],
            json!({"reason":"max_output_tokens"})
        );
        assert_eq!(last["response"]["usage"], Value::Null);
    }

    #[test]
    fn text_after_a_call_joins_the_one_message_and_empty_arguments_get_an_object() {
        let all = events(&stream(&[
            StreamEvent::ToolCallStart {
                index: 0,
                id: "c".into(),
                name: "w".into(),
            },
            StreamEvent::Delta {
                text: "late".into(),
            },
            StreamEvent::Done {
                finish_reason: Some(FinishReason::ToolCalls),
                usage: None,
            },
        ]));
        let fin = &all.last().unwrap().1["response"];
        assert_eq!(fin["output"][0]["type"], "function_call");
        assert_eq!(fin["output"][0]["arguments"], "{}");
        assert_eq!(fin["output"][1]["type"], "message");
        assert_eq!(fin["output"][1]["content"][0]["text"], "late");
        // Every delta names an output index that was added before it.
        let mut added = Vec::new();
        for (n, v) in &all {
            if n == "response.output_item.added" {
                added.push(v["output_index"].as_u64().unwrap());
            }
            if n.ends_with(".delta") {
                assert!(added.contains(&v["output_index"].as_u64().unwrap()), "{n}");
            }
        }
        for (i, (_, v)) in all.iter().enumerate() {
            assert_eq!(v["sequence_number"], i);
        }
    }

    #[test]
    fn an_empty_answer_still_opens_and_completes() {
        let all = events(&stream(&[StreamEvent::Done {
            finish_reason: None,
            usage: None,
        }]));
        let names: Vec<&str> = all.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                "response.created",
                "response.in_progress",
                "response.completed"
            ]
        );
    }

    #[test]
    fn a_stream_error_is_an_error_event_with_the_next_sequence_number() {
        let mut r = StreamRenderer::new("resp_abc", "m", 5, echo());
        let a = r.render(&StreamEvent::Delta { text: "a".into() });
        let n = events(&a).len();
        let e = events(&r.error("lost"));
        assert_eq!(e[0].0, "error");
        assert_eq!(e[0].1["message"], "lost");
        assert_eq!(e[0].1["sequence_number"], n);
        assert!(e[0].1["code"].is_null() || e[0].1["code"].is_string());
        // Before anything was sent the error is still sequence 0.
        let r = StreamRenderer::new("resp_abc", "m", 5, echo());
        assert_eq!(events(&r.error("x"))[0].1["sequence_number"], 0);
    }
}
