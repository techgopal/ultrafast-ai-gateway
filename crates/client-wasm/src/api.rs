//! The boundary in plain Rust: strings and bytes in, a JSON string out. An
//! `Err` is always the JSON of a classified error, `{kind, retryable, status,
//! message, retry_after_secs}`, with the same kinds and retry rules as the
//! Rust client (both use `ultrafast_translate::classify`).

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{json, Map, Value};
use ultrafast_translate::classify::{classify_answer, parse_retry_after, Classified, ErrorKind};
use ultrafast_translate::embeddings::{self, EmbeddingsRequest};
use ultrafast_translate::ingress::openai::{
    parse_messages, parse_response_format, parse_tool_choice, parse_tools,
};
use ultrafast_translate::provider::{
    self, HttpRequest, ProviderKind, StreamDecoder, Target as WireTarget,
};
use ultrafast_translate::tags;
use ultrafast_translate::types::{ChatRequest, ChatResponse, FinishReason, StreamEvent, Usage};

pub type Failure = String;

fn error_json(c: &Classified) -> Failure {
    json!({
        "kind": c.kind.as_str(),
        "retryable": c.retryable,
        "status": c.status,
        "message": c.message,
        "retry_after_secs": c.retry_after_secs,
    })
    .to_string()
}

fn invalid(message: impl Into<String>) -> Failure {
    error_json(&Classified::new(ErrorKind::InvalidRequest, message))
}

fn classified(e: ultrafast_translate::error::TranslateError, retry_after: Option<&str>) -> Failure {
    error_json(&Classified::from_translate(
        e,
        retry_after.and_then(parse_retry_after),
    ))
}

/// A provider kind, or `gateway` (OpenAI's format under `{base_url}/v1`).
fn kind_of(s: &str) -> Result<ProviderKind, Failure> {
    if s == "gateway" {
        return Ok(ProviderKind::OpenAi);
    }
    ProviderKind::parse(s).ok_or_else(|| invalid(format!("unknown provider kind \"{s}\"")))
}

#[derive(Deserialize)]
struct TargetIn {
    kind: String,
    base_url: String,
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    api_version: Option<String>,
}

fn target_of(target_json: &str, model: &str) -> Result<(WireTarget, bool), Failure> {
    let t: TargetIn =
        serde_json::from_str(target_json).map_err(|e| invalid(format!("target: {e}")))?;
    let gateway = t.kind == "gateway";
    let base_url = if gateway {
        provider::gateway_base(&t.base_url)
    } else {
        t.base_url
    };
    Ok((
        WireTarget {
            kind: kind_of(&t.kind)?,
            base_url,
            api_key: t.api_key.filter(|k| !k.is_empty()),
            model: model.to_string(),
            api_version: t.api_version,
        },
        gateway,
    ))
}

/// A message as the host sends it: `content` is a string, a list of
/// `{type:"text",text}` / `{type:"image",url}` parts, or null (an assistant
/// message that only calls tools).
#[derive(Deserialize)]
struct MessageIn {
    role: String,
    #[serde(default)]
    content: Option<ContentIn>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    tool_calls: Vec<ToolCallIn>,
    #[serde(default)]
    tool_call_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ContentIn {
    Text(String),
    Parts(Vec<PartIn>),
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum PartIn {
    Text { text: String },
    Image { url: String },
}

#[derive(Deserialize)]
struct ToolCallIn {
    id: String,
    name: String,
    arguments: String,
}

#[derive(Deserialize)]
struct ToolIn {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    parameters: Option<Value>,
    #[serde(default)]
    strict: Option<bool>,
}

/// The message in OpenAI's shape, so the one parser in `translate` checks it.
fn message_value(m: MessageIn) -> Value {
    let mut out = Map::new();
    out.insert("role".into(), m.role.into());
    match m.content {
        None => {}
        Some(ContentIn::Text(t)) => {
            out.insert("content".into(), t.into());
        }
        Some(ContentIn::Parts(parts)) => {
            let parts: Vec<Value> = parts
                .into_iter()
                .map(|p| match p {
                    PartIn::Text { text } => json!({"type": "text", "text": text}),
                    PartIn::Image { url } => {
                        json!({"type": "image_url", "image_url": {"url": url}})
                    }
                })
                .collect();
            out.insert("content".into(), parts.into());
        }
    }
    if let Some(n) = m.name {
        out.insert("name".into(), n.into());
    }
    if !m.tool_calls.is_empty() {
        let calls: Vec<Value> = m
            .tool_calls
            .into_iter()
            .map(|c| {
                json!({"id": c.id, "type": "function",
                       "function": {"name": c.name, "arguments": c.arguments}})
            })
            .collect();
        out.insert("tool_calls".into(), calls.into());
    }
    if let Some(id) = m.tool_call_id {
        out.insert("tool_call_id".into(), id.into());
    }
    Value::Object(out)
}

fn tool_value(t: ToolIn) -> Value {
    let mut f = Map::new();
    f.insert("name".into(), t.name.into());
    if let Some(d) = t.description {
        f.insert("description".into(), d.into());
    }
    if let Some(p) = t.parameters {
        f.insert("parameters".into(), p);
    }
    if let Some(s) = t.strict {
        f.insert("strict".into(), s.into());
    }
    json!({"type": "function", "function": f})
}

/// `"auto"`, `"none"`, `"required"` or `{"name": "<tool>"}`.
fn tool_choice_value(v: Value) -> Value {
    match v {
        Value::Object(o) if o.len() == 1 && o.get("name").is_some_and(Value::is_string) => {
            json!({"type": "function", "function": {"name": o["name"]}})
        }
        other => other,
    }
}

#[derive(Deserialize)]
struct ChatIn {
    model: String,
    messages: Vec<MessageIn>,
    #[serde(default)]
    tools: Vec<ToolIn>,
    #[serde(default)]
    tool_choice: Option<Value>,
    #[serde(default)]
    parallel_tool_calls: Option<bool>,
    /// OpenAI's `response_format` object, checked by the gateway's parser.
    #[serde(default)]
    response_format: Option<Value>,
    #[serde(default)]
    max_tokens: Option<u32>,
    #[serde(default)]
    temperature: Option<f32>,
    #[serde(default)]
    top_p: Option<f32>,
    #[serde(default)]
    stop: Option<Vec<String>>,
    #[serde(default)]
    stream: bool,
    /// Sent to a gateway target only.
    #[serde(default)]
    tags: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct EmbedIn {
    model: String,
    input: Vec<String>,
    #[serde(default)]
    dimensions: Option<u32>,
    #[serde(default)]
    tags: BTreeMap<String, String>,
}

fn http_json(
    req: HttpRequest,
    tag_header: Option<String>,
    event_stream: bool,
) -> Result<String, Failure> {
    let mut headers = Map::new();
    for (k, v) in req.headers {
        headers.insert(k.to_ascii_lowercase(), Value::String(v));
    }
    if event_stream {
        headers.insert("accept".into(), "text/event-stream".into());
    }
    if let Some(t) = tag_header {
        headers.insert(tags::HEADER.into(), Value::String(t));
    }
    let body = String::from_utf8(req.body).map_err(|_| invalid("the body is not UTF-8"))?;
    Ok(json!({"method": req.method, "url": req.url, "headers": headers, "body": body}).to_string())
}

fn tag_header_for(gateway: bool, t: &BTreeMap<String, String>) -> Result<Option<String>, Failure> {
    if !gateway {
        return Ok(None);
    }
    tags::tags_header(t).map_err(|c| error_json(&c))
}

pub fn build_request(target_json: &str, request_json: &str) -> Result<String, Failure> {
    let r: ChatIn =
        serde_json::from_str(request_json).map_err(|e| invalid(format!("request: {e}")))?;
    let (target, gateway) = target_of(target_json, &r.model)?;
    let tag_header = tag_header_for(gateway, &r.tags)?;
    let req = ChatRequest {
        model: r.model,
        messages: parse_messages(r.messages.into_iter().map(message_value).collect())
            .map_err(|e| classified(e, None))?,
        max_tokens: r.max_tokens,
        temperature: r.temperature,
        top_p: r.top_p,
        stop: r.stop,
        stream: r.stream,
        tools: parse_tools(r.tools.into_iter().map(tool_value).collect())
            .map_err(|e| classified(e, None))?,
        tool_choice: match r.tool_choice {
            None | Some(Value::Null) => None,
            Some(v) => {
                Some(parse_tool_choice(tool_choice_value(v)).map_err(|e| classified(e, None))?)
            }
        },
        parallel_tool_calls: r.parallel_tool_calls,
        response_format: match r.response_format {
            None | Some(Value::Null) => None,
            Some(v) => Some(parse_response_format(&v).map_err(|e| classified(e, None))?),
        },
        reasoning_effort: None,
    };
    let http = provider::build_request(&target, &req).map_err(|e| classified(e, None))?;
    http_json(http, tag_header, r.stream)
}

pub fn build_embeddings_request(target_json: &str, request_json: &str) -> Result<String, Failure> {
    let r: EmbedIn =
        serde_json::from_str(request_json).map_err(|e| invalid(format!("request: {e}")))?;
    let (target, gateway) = target_of(target_json, &r.model)?;
    let tag_header = tag_header_for(gateway, &r.tags)?;
    let req = EmbeddingsRequest {
        model: r.model,
        input: r.input,
        dimensions: r.dimensions,
    };
    let http = embeddings::build_request(&target, &req).map_err(|e| classified(e, None))?;
    http_json(http, tag_header, false)
}

fn finish_json(f: Option<FinishReason>) -> Value {
    f.map_or(Value::Null, |f| f.as_openai().into())
}

fn usage_json(u: Option<Usage>) -> Value {
    u.map_or(
        Value::Null,
        |u| json!({"input_tokens": u.input_tokens, "output_tokens": u.output_tokens}),
    )
}

fn response_json(r: &ChatResponse) -> Value {
    json!({
        "id": r.id,
        "model": r.model,
        "content": r.content,
        "tool_calls": r.tool_calls.iter().map(|c| json!({
            "id": c.id, "name": c.name, "arguments": c.arguments,
        })).collect::<Vec<_>>(),
        "finish_reason": finish_json(r.finish_reason),
        "usage": usage_json(r.usage),
    })
}

pub fn parse_response(
    kind: &str,
    status: u16,
    body: &[u8],
    retry_after: Option<&str>,
) -> Result<String, Failure> {
    let kind = kind_of(kind)?;
    if status >= 300 {
        return Err(error_json(&classify_answer(status, body, retry_after)));
    }
    provider::parse_response(kind, status, body)
        .map(|r| response_json(&r).to_string())
        .map_err(|e| classified(e, retry_after))
}

pub fn parse_embeddings(
    kind: &str,
    status: u16,
    body: &[u8],
    model: &str,
    retry_after: Option<&str>,
) -> Result<String, Failure> {
    let kind = kind_of(kind)?;
    if status >= 300 {
        return Err(error_json(&classify_answer(status, body, retry_after)));
    }
    embeddings::parse_response(kind, status, body, model)
        .map(|r| {
            json!({"model": r.model, "vectors": r.vectors, "prompt_tokens": r.prompt_tokens})
                .to_string()
        })
        .map_err(|e| classified(e, retry_after))
}

/// An error answer from any server, classified: what the TypeScript client
/// throws for an HTTP status of 300 or more.
pub fn classify_error(status: u16, body: &[u8], retry_after: Option<&str>) -> String {
    error_json(&classify_answer(status, body, retry_after))
}

/// A `Classified` of the given kind, for errors the host raises itself
/// (network, timeout, truncated stream), so every kind is spelled once.
pub fn host_error(kind: &str, message: &str) -> Result<String, Failure> {
    let kind = [
        ErrorKind::Auth,
        ErrorKind::Permission,
        ErrorKind::NotFound,
        ErrorKind::InvalidRequest,
        ErrorKind::RateLimited,
        ErrorKind::Upstream,
        ErrorKind::Network,
        ErrorKind::Timeout,
        ErrorKind::Malformed,
    ]
    .into_iter()
    .find(|k| k.as_str() == kind)
    .ok_or_else(|| invalid(format!("unknown error kind \"{kind}\"")))?;
    Ok(error_json(&Classified::new(kind, message)))
}

/// `message` with the key replaced by `[redacted]`.
pub fn scrub(message: &str, key: &str) -> String {
    ultrafast_translate::classify::scrub(message, key)
}

/// An error JSON (as thrown by any function here) with the key removed from
/// its message. The match is on the decoded text, so a key that JSON escapes
/// is still found. Anything that is not an error JSON is scrubbed as text.
pub fn scrub_error(error_json: &str, key: &str) -> String {
    match serde_json::from_str::<Value>(error_json) {
        Ok(mut v) if v.get("message").is_some_and(Value::is_string) => {
            let clean = scrub(v["message"].as_str().unwrap_or_default(), key);
            v["message"] = Value::String(clean);
            v.to_string()
        }
        _ => scrub(error_json, key),
    }
}

/// The `x-uf-tags` value for a JSON object of tags; None when empty.
pub fn tags_header(tags_json: &str) -> Result<Option<String>, Failure> {
    let t: BTreeMap<String, String> =
        serde_json::from_str(tags_json).map_err(|e| invalid(format!("tags: {e}")))?;
    tags::tags_header(&t).map_err(|c| error_json(&c))
}

fn event_json(e: &StreamEvent) -> Value {
    match e {
        StreamEvent::Delta { text } => json!({"type": "delta", "text": text}),
        StreamEvent::ToolCallStart { index, id, name } => {
            json!({"type": "tool_call_start", "index": index, "id": id, "name": name})
        }
        StreamEvent::ToolCallDelta { index, arguments } => {
            json!({"type": "tool_call_delta", "index": index, "arguments": arguments})
        }
        StreamEvent::Done {
            finish_reason,
            usage,
        } => json!({
            "type": "done",
            "finish_reason": finish_json(*finish_reason),
            "usage": usage_json(*usage),
        }),
    }
}

fn events_json(events: &[StreamEvent]) -> String {
    Value::Array(events.iter().map(event_json).collect()).to_string()
}

/// Decodes a provider's event stream, chunk by chunk.
pub struct Decoder {
    inner: StreamDecoder,
    done: bool,
}

impl Decoder {
    pub fn new(kind: &str) -> Result<Decoder, Failure> {
        Ok(Decoder {
            inner: StreamDecoder::new(kind_of(kind)?),
            done: false,
        })
    }

    /// Events (a JSON array) completed by `chunk`. Errs when the stream
    /// failed before any event came out of this chunk; an error after events
    /// is held for `take_error`.
    pub fn feed(&mut self, chunk: &[u8]) -> Result<String, Failure> {
        let events = self.inner.feed(chunk).map_err(|e| classified(e, None))?;
        self.note(&events);
        Ok(events_json(&events))
    }

    /// The events held back until the provider closed the stream.
    pub fn finish(&mut self) -> String {
        let events = self.inner.finish();
        self.note(&events);
        events_json(&events)
    }

    /// Whether a `done` event has come out. A stream that closes without
    /// one was cut short and the host must raise a `malformed` error.
    pub fn is_done(&self) -> bool {
        self.done
    }

    fn note(&mut self, events: &[StreamEvent]) {
        self.done |= events.iter().any(|e| matches!(e, StreamEvent::Done { .. }));
    }

    pub fn take_error(&mut self) -> Option<String> {
        self.inner.take_error().map(|e| classified(e, None))
    }
}
