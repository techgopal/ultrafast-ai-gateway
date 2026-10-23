//! Pass-through: a coding agent's request, forwarded as it came to a
//! provider that speaks the same API (Claude Code to Anthropic, Codex to
//! OpenAI and Azure), with the agent's own JSON as the source of truth.

use axum::http::HeaderMap;
use serde_json::{Map, Value};
use ufaigw_translate::error::TranslateError;
use ufaigw_translate::provider::{
    parse_responses_usage, HttpRequest, ProviderKind, StreamDecoder, Target,
};
use ufaigw_translate::types::{StreamEvent, Usage};

/// The API a request was made in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeApi {
    /// Anthropic's `/v1/messages`.
    Messages,
    /// OpenAI's `/v1/responses`.
    Responses,
}

/// What the agent sent: the body, the headers that may go on, the query.
#[derive(Debug, Clone)]
pub struct Native {
    pub api: NativeApi,
    pub body: Map<String, Value>,
    /// Already filtered to the forwardable set; a name may repeat.
    pub headers: Vec<(String, String)>,
    pub query: Option<String>,
    /// Why the body could not be turned into a common request. Only a
    /// translated attempt is refused with it; a pass-through attempt leaves
    /// the judgement to the provider.
    pub parse_error: Option<TranslateError>,
}

/// Header names that carry a credential of some kind, whatever their prefix.
fn carries_credential(name: &str) -> bool {
    [
        "authorization",
        "api-key",
        "apikey",
        "token",
        "secret",
        "cookie",
    ]
    .iter()
    .any(|word| name.contains(word))
}

/// The request headers that go to the provider: for Anthropic every
/// `anthropic-*`, for OpenAI and Azure every `openai-*`, every line of each.
/// Never the caller's credentials, the account of the caller
/// (`openai-organization`, `openai-project`), Anthropic's admin headers, or
/// anything the agent adds about itself.
pub fn forwardable(api: NativeApi, headers: &HeaderMap) -> Vec<(String, String)> {
    let (prefix, denied): (&str, &[&str]) = match api {
        NativeApi::Messages => ("anthropic-", &["anthropic-admin-"]),
        NativeApi::Responses => ("openai-", &["openai-organization", "openai-project"]),
    };
    headers
        .iter()
        .filter_map(|(name, value)| {
            let name = name.as_str();
            if !name.starts_with(prefix)
                || denied.iter().any(|d| name.starts_with(d))
                || carries_credential(name)
            {
                return None;
            }
            let value = value.to_str().ok()?;
            Some((name.to_string(), value.to_string()))
        })
        .collect()
}

/// Whether a provider of `kind` speaks the API.
pub fn matches(api: NativeApi, kind: ProviderKind) -> bool {
    matches!(
        (api, kind),
        (NativeApi::Messages, ProviderKind::Anthropic)
            | (
                NativeApi::Responses,
                ProviderKind::OpenAi | ProviderKind::Azure
            )
    )
}

/// The Responses API version used on Azure when a provider has none (the
/// default of the chat adapter predates the API).
const AZURE_RESPONSES_API_VERSION: &str = "2025-03-01-preview";

/// The `api-version` for an Azure Responses call: the provider's own when it
/// is at least the first one that has the API, else that one (a provider set
/// up for chat completions often carries an older version). A version that
/// is not a date (`v1`, `preview`) is the provider's choice and is kept.
fn responses_api_version(configured: Option<&str>) -> &str {
    let Some(version) = configured.filter(|v| !v.is_empty()) else {
        return AZURE_RESPONSES_API_VERSION;
    };
    let is_date = version.len() >= 10
        && version.as_bytes()[..4].iter().all(u8::is_ascii_digit)
        && version.as_bytes()[4] == b'-';
    if is_date && version[..10] < AZURE_RESPONSES_API_VERSION[..10] {
        AZURE_RESPONSES_API_VERSION
    } else {
        version
    }
}

/// The request to send to `target`: its URL for the API, its credential,
/// the forwarded headers, and the agent's body with the model replaced.
pub fn build(native: &Native, target: &Target) -> HttpRequest {
    let base = target.base_url.trim_end_matches('/');
    let mut headers = vec![("content-type".to_string(), "application/json".to_string())];
    let url = match target.kind {
        ProviderKind::Anthropic => {
            if let Some(k) = &target.api_key {
                headers.push(("x-api-key".to_string(), k.clone()));
            }
            let mut url = format!("{base}/v1/messages");
            if let Some(q) = native.query.as_deref().filter(|q| !q.is_empty()) {
                url.push('?');
                url.push_str(q);
            }
            url
        }
        ProviderKind::Azure => {
            if let Some(k) = &target.api_key {
                headers.push(("api-key".to_string(), k.clone()));
            }
            let version = responses_api_version(target.api_version.as_deref());
            format!("{base}/openai/responses?api-version={version}")
        }
        ProviderKind::OpenAi | ProviderKind::Gemini => {
            if let Some(k) = &target.api_key {
                headers.push(("authorization".to_string(), format!("Bearer {k}")));
            }
            format!("{base}/responses")
        }
    };
    // The API's default version, unless the agent named one.
    if native.api == NativeApi::Messages
        && !native
            .headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("anthropic-version"))
    {
        headers.push(("anthropic-version".to_string(), "2023-06-01".to_string()));
    }
    headers.extend(native.headers.iter().cloned());
    HttpRequest {
        method: "POST",
        url,
        headers,
        body: serde_json::to_vec(&WithModel {
            body: &native.body,
            model: &target.model,
        })
        .unwrap_or_default(),
    }
}

/// The agent's body with its `model` replaced, written without copying the
/// body: the other members are in their places, and `model` goes at the end
/// when the body had none.
struct WithModel<'a> {
    body: &'a Map<String, Value>,
    model: &'a str,
}

impl serde::Serialize for WithModel<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        for (k, v) in self.body {
            if k == "model" {
                map.serialize_entry(k, self.model)?;
            } else {
                map.serialize_entry(k, v)?;
            }
        }
        if !self.body.contains_key("model") {
            map.serialize_entry("model", self.model)?;
        }
        map.end()
    }
}

/// Pushes the strings of `v`: itself when a string, or the `text` of its
/// blocks whose type is one of `blocks` when an array.
fn strings_of<'a>(v: &'a mut Value, blocks: &[&str], slots: &mut Vec<&'a mut String>) {
    match v {
        Value::String(s) => slots.push(s),
        Value::Array(parts) => {
            for b in parts {
                if b["type"].as_str().is_some_and(|t| blocks.contains(&t)) {
                    if let Some(Value::String(t)) = b.get_mut("text") {
                        slots.push(t);
                    }
                }
            }
        }
        _ => {}
    }
}

/// Pushes every string a value holds, at any depth (not the keys).
fn leaves<'a>(v: &'a mut Value, slots: &mut Vec<&'a mut String>) {
    match v {
        Value::String(s) => slots.push(s),
        Value::Array(items) => {
            for x in items {
                leaves(x, slots);
            }
        }
        Value::Object(map) => {
            for (_, x) in map.iter_mut() {
                leaves(x, slots);
            }
        }
        _ => {}
    }
}

/// Pushes the string in field `key` of `v`, when it is one.
fn field<'a>(v: &'a mut Value, key: &str, slots: &mut Vec<&'a mut String>) {
    if let Some(Value::String(s)) = v.get_mut(key) {
        slots.push(s);
    }
}

/// The text of the agent's request that input guardrails read and may
/// rewrite, in the body itself, whatever the translator makes of it.
/// Everything the common request of 3.0 was scanned for, and what only the
/// agent's own body has. Anthropic: `system`, and in every message, of any
/// role (a `system` one included), text blocks, the strings of `tool_use`
/// input, `tool_result` content and the text of `document` sources.
/// Responses: `instructions`, the input string, the text parts of messages of
/// any role (assistant ones included), the arguments of function calls, the
/// input of custom tool calls, and the output of both kinds of call, as a
/// string or in parts. Not touched: tool definitions, signed thinking and
/// encrypted reasoning.
pub fn text_slots(native: &mut Native) -> Vec<&mut String> {
    let mut slots = Vec::new();
    let api = native.api;
    for (key, value) in native.body.iter_mut() {
        match (api, key.as_str()) {
            (NativeApi::Messages, "system") => strings_of(value, &["text"], &mut slots),
            (NativeApi::Messages, "messages") => {
                for m in value.as_array_mut().into_iter().flatten() {
                    if let Some(content) = m.get_mut("content") {
                        anthropic_content(content, &mut slots);
                    }
                }
            }
            (NativeApi::Responses, "instructions") => {
                if let Value::String(s) = value {
                    slots.push(s);
                }
            }
            (NativeApi::Responses, "input") => match value {
                Value::String(s) => slots.push(s),
                Value::Array(items) => {
                    for item in items {
                        responses_item(item, &mut slots);
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
    slots
}

fn anthropic_content<'a>(content: &'a mut Value, slots: &mut Vec<&'a mut String>) {
    match content {
        Value::String(s) => slots.push(s),
        Value::Array(blocks) => {
            for b in blocks {
                match b["type"].as_str() {
                    Some("text") => field(b, "text", slots),
                    Some("tool_use") => {
                        if let Some(input) = b.get_mut("input") {
                            leaves(input, slots);
                        }
                    }
                    Some("tool_result") => {
                        if let Some(c) = b.get_mut("content") {
                            strings_of(c, &["text"], slots);
                        }
                    }
                    Some("document") => {
                        if let Some(source) = b.get_mut("source") {
                            match source["type"].as_str() {
                                Some("text") => field(source, "data", slots),
                                Some("content") => {
                                    if let Some(c) = source.get_mut("content") {
                                        strings_of(c, &["text"], slots);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

fn responses_item<'a>(item: &'a mut Value, slots: &mut Vec<&'a mut String>) {
    const TEXT: &[&str] = &["input_text", "output_text", "text"];
    match item["type"].as_str() {
        Some("function_call") => field(item, "arguments", slots),
        Some("custom_tool_call") => field(item, "input", slots),
        Some("function_call_output" | "custom_tool_call_output") => {
            if let Some(out) = item.get_mut("output") {
                strings_of(out, TEXT, slots);
            }
        }
        Some("message") | None => {
            if let Some(content) = item.get_mut("content") {
                strings_of(content, TEXT, slots);
            }
        }
        _ => {}
    }
}

/// The text of one item of a Responses answer that guardrails read and may
/// rewrite: the words of the model and the arguments or input of its calls.
fn response_item_slots<'a>(item: &'a mut Value, slots: &mut Vec<&'a mut String>) {
    match item["type"].as_str() {
        Some("message") => {
            if let Some(content) = item.get_mut("content") {
                strings_of(content, &["output_text"], slots);
            }
        }
        Some("function_call") => field(item, "arguments", slots),
        Some("custom_tool_call") => field(item, "input", slots),
        _ => {}
    }
}

/// The text of a whole answer that guardrails read and may rewrite: the text
/// of the model and the inputs of the calls it makes.
pub fn answer_slots(api: NativeApi, answer: &mut Value) -> Vec<&mut String> {
    let mut slots = Vec::new();
    match api {
        NativeApi::Messages => {
            for block in answer["content"].as_array_mut().into_iter().flatten() {
                match block["type"].as_str() {
                    Some("text") => field(block, "text", &mut slots),
                    Some("tool_use") => {
                        if let Some(input) = block.get_mut("input") {
                            leaves(input, &mut slots);
                        }
                    }
                    _ => {}
                }
            }
        }
        NativeApi::Responses => {
            for item in answer["output"].as_array_mut().into_iter().flatten() {
                response_item_slots(item, &mut slots);
            }
        }
    }
    slots
}

/// The full-text fields of a Responses stream event that repeat what the
/// deltas carried: the `.done` events and the final response. Empty for any
/// other event.
pub fn repeated_slots(v: &mut Value) -> Vec<&mut String> {
    let mut slots = Vec::new();
    let ty = v["type"].as_str().unwrap_or("").to_string();
    match ty.as_str() {
        "response.output_text.done" => field(v, "text", &mut slots),
        "response.function_call_arguments.done" => field(v, "arguments", &mut slots),
        "response.custom_tool_call_input.done" => field(v, "input", &mut slots),
        "response.content_part.done" => {
            if let Some(part) = v.get_mut("part") {
                field(part, "text", &mut slots);
            }
        }
        "response.output_item.done" => {
            if let Some(item) = v.get_mut("item") {
                response_item_slots(item, &mut slots);
            }
        }
        "response.completed" | "response.incomplete" => {
            if let Some(items) = v
                .get_mut("response")
                .and_then(|r| r.get_mut("output"))
                .and_then(Value::as_array_mut)
            {
                for item in items {
                    response_item_slots(item, &mut slots);
                }
            }
        }
        _ => {}
    }
    slots
}

/// What the agent's own body is worth as input, in characters of text and a
/// count of images and documents: everything in it, whatever translation would
/// have dropped. Encoded media are counted as one each, not by their size.
pub fn input_size(native: &Native) -> (usize, u64) {
    fn walk(v: &Value, chars: &mut usize, media: &mut u64) {
        match v {
            Value::String(s) => *chars += s.chars().count(),
            Value::Array(a) => a.iter().for_each(|x| walk(x, chars, media)),
            Value::Object(m) => {
                let ty = m.get("type").and_then(Value::as_str);
                let inline = m.get("source").is_some_and(|s| s["type"] == "base64")
                    || m.get("image_url")
                        .and_then(Value::as_str)
                        .is_some_and(|u| u.starts_with("data:"))
                    || m.get("file_data").is_some();
                if matches!(
                    ty,
                    Some("image" | "input_image" | "document" | "input_file")
                ) && inline
                {
                    *media += 1;
                    return;
                }
                if matches!(ty, Some("image" | "input_image")) {
                    *media += 1;
                }
                m.values().for_each(|x| walk(x, chars, media));
            }
            _ => {}
        }
    }
    let (mut chars, mut media) = (0, 0);
    native
        .body
        .values()
        .for_each(|v| walk(v, &mut chars, &mut media));
    (chars, media)
}

/// The usage of a whole answer, read from its bytes; `None` when it has none.
pub fn usage_of(api: NativeApi, body: &[u8]) -> Option<Usage> {
    match api {
        NativeApi::Responses => parse_responses_usage(body),
        NativeApi::Messages => {
            let v: Value = serde_json::from_slice(body).ok()?;
            let u = v.get("usage").filter(|u| u.is_object())?;
            let n = |key: &str| u32::try_from(u[key].as_u64().unwrap_or(0)).unwrap_or(u32::MAX);
            Some(Usage {
                input_tokens: n("input_tokens"),
                output_tokens: n("output_tokens"),
                cache_read_input_tokens: n("cache_read_input_tokens"),
                cache_write_input_tokens: n("cache_creation_input_tokens"),
            })
        }
    }
}

/// Whether an output item of a Responses answer is a web search that was made
/// and billed: a completed `web_search_call` of the search action (opening a
/// page or finding in one is not a search, and a failed one is not billed).
pub fn is_search_call(item: &Value) -> bool {
    item["type"] == "web_search_call"
        && item["status"].as_str().is_none_or(|s| s == "completed")
        && item["action"]["type"]
            .as_str()
            .is_none_or(|a| a == "search")
}

/// The calls of hosted tools (web searches) a whole answer reports.
pub fn hosted_calls_of(api: NativeApi, body: &[u8]) -> u32 {
    let Ok(v) = serde_json::from_slice::<Value>(body) else {
        return 0;
    };
    let n = match api {
        NativeApi::Messages => v["usage"]["server_tool_use"]["web_search_requests"]
            .as_u64()
            .unwrap_or(0),
        NativeApi::Responses => v["output"]
            .as_array()
            .map_or(0, |o| o.iter().filter(|i| is_search_call(i)).count() as u64),
    };
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// The message of a provider's error body, for the gateway's own record.
pub fn error_message(body: &[u8]) -> String {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| {
            v["error"]["message"]
                .as_str()
                .or_else(|| v["message"].as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| String::from_utf8_lossy(body).chars().take(500).collect())
}

/// The response headers a pass-through answer keeps.
pub fn keep_response_headers(headers: &reqwest::header::HeaderMap) -> Vec<(String, String)> {
    headers
        .iter()
        .filter_map(|(name, value)| {
            let name = name.as_str();
            let kept = matches!(
                name,
                "content-type" | "retry-after" | "x-should-retry" | "request-id" | "x-request-id"
            ) || name.starts_with("anthropic-ratelimit-");
            if !kept {
                return None;
            }
            Some((name.to_string(), value.to_str().ok()?.to_string()))
        })
        .collect()
}

// ------------------------------------------------------------------ streams

/// Splits the complete SSE events off the front of `buf`: each is returned
/// with its own delimiter, byte for byte. A partial event stays in `buf`.
pub fn split_events(buf: &mut Vec<u8>) -> Vec<Vec<u8>> {
    split_events_from(buf, 0)
}

/// [`split_events`] for a buffer whose first `from` bytes are known to hold
/// no delimiter (it may start in the last three of them): a partial event is
/// not searched again each time more of it arrives.
pub fn split_events_from(buf: &mut Vec<u8>, mut from: usize) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let found = (from..buf.len()).find_map(|i| {
            if buf[i..].starts_with(b"\n\n") {
                Some(i + 2)
            } else if buf[i..].starts_with(b"\r\n\r\n") {
                Some(i + 4)
            } else {
                None
            }
        });
        let Some(end) = found else { return out };
        out.push(buf.drain(..end).collect());
        from = 0;
    }
}

/// The largest event a pass-through stream may hold while it is read: that
/// of the translated path's parser.
pub const MAX_EVENT_BYTES: usize = ufaigw_translate::sse::MAX_EVENT_BYTES;

/// What an event of a pass-through stream is, for the guardrails.
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    /// A piece of the model's text.
    Text,
    /// A piece of the arguments of tool call `index`.
    Tool(u32),
    /// The end of a content block or of the answer: what a scanner holds
    /// back is released before it.
    End,
    Other,
}

/// The JSON of the `data:` lines of an event.
pub fn data_of(event: &[u8]) -> Option<Value> {
    let text = std::str::from_utf8(event).ok()?;
    let data: Vec<&str> = text
        .lines()
        .filter_map(|l| l.strip_prefix("data:"))
        .map(|d| d.strip_prefix(' ').unwrap_or(d))
        .collect();
    if data.is_empty() {
        return None;
    }
    serde_json::from_str(&data.join("\n")).ok()
}

pub fn kind_of(api: NativeApi, v: &Value) -> Kind {
    let ty = v["type"].as_str().unwrap_or("");
    match api {
        NativeApi::Messages => match (ty, v["delta"]["type"].as_str()) {
            ("content_block_delta", Some("text_delta")) => Kind::Text,
            ("content_block_delta", Some("input_json_delta")) => {
                Kind::Tool(u32::try_from(v["index"].as_u64().unwrap_or(0)).unwrap_or(0))
            }
            ("content_block_stop" | "message_delta" | "message_stop", _) => Kind::End,
            _ => Kind::Other,
        },
        NativeApi::Responses => match ty {
            "response.output_text.delta" => Kind::Text,
            "response.function_call_arguments.delta" | "response.custom_tool_call_input.delta" => {
                Kind::Tool(u32::try_from(v["output_index"].as_u64().unwrap_or(0)).unwrap_or(0))
            }
            t if t.ends_with(".done")
                || matches!(
                    t,
                    "response.completed" | "response.incomplete" | "response.failed"
                ) =>
            {
                Kind::End
            }
            _ => Kind::Other,
        },
    }
}

fn text_slot(api: NativeApi, v: &mut Value) -> Option<&mut Value> {
    match api {
        NativeApi::Messages => {
            let delta = v.get_mut("delta")?;
            if delta["type"] == "input_json_delta" {
                delta.get_mut("partial_json")
            } else {
                delta.get_mut("text")
            }
        }
        NativeApi::Responses => v.get_mut("delta"),
    }
}

/// The text of a delta event.
pub fn delta_of(api: NativeApi, v: &Value) -> Option<String> {
    let mut v = v.clone();
    text_slot(api, &mut v)?.as_str().map(str::to_string)
}

/// The event with `text` in place of the text of its delta; everything else
/// in the event is as it was. Only this event is re-serialised.
pub fn rewritten(api: NativeApi, event: &[u8], v: &Value, text: &str) -> Vec<u8> {
    let mut v = v.clone();
    if let Some(slot) = text_slot(api, &mut v) {
        *slot = Value::String(text.to_string());
    }
    reserialized(event, &v)
}

/// `v` as the event `event` was: the same `event:` line, one `data:` line.
pub fn reserialized(event: &[u8], v: &Value) -> Vec<u8> {
    let name = std::str::from_utf8(event)
        .ok()
        .and_then(|t| t.lines().find_map(|l| l.strip_prefix("event:")))
        .map(|n| n.trim().to_string());
    let mut out = String::new();
    if let Some(name) = name {
        out.push_str(&format!("event: {name}\n"));
    }
    out.push_str(&format!(
        "data: {}\n\n",
        serde_json::to_string(v).unwrap_or_default()
    ));
    out.into_bytes()
}

/// A Responses event whose `sequence_number` does not rise past the last one
/// let out (`last`) is given the next number: text held back and let out
/// again is a copy of an earlier event. Events that already rise, and events
/// with no number, are as they were.
pub fn resequenced(event: Vec<u8>, last: &mut Option<u64>) -> Vec<u8> {
    let Some(mut v) = data_of(&event) else {
        return event;
    };
    let Some(seq) = v["sequence_number"].as_u64() else {
        return event;
    };
    let next = match *last {
        Some(l) if seq <= l => l + 1,
        _ => seq,
    };
    *last = Some(next);
    if next == seq {
        return event;
    }
    v["sequence_number"] = Value::from(next);
    reserialized(&event, &v)
}

/// The API's own error event.
pub fn error_event(api: NativeApi, message: &str) -> String {
    let data = match api {
        NativeApi::Messages => serde_json::json!({
            "type": "error",
            "error": { "type": "api_error", "message": message }
        }),
        NativeApi::Responses => serde_json::json!({
            "type": "error", "code": "upstream_error", "message": message, "param": null,
            "sequence_number": 0
        }),
    };
    format!("event: error\ndata: {data}\n\n")
}

/// What reads a pass-through stream for what the gateway records: the usage,
/// the characters streamed, the end of the answer and the provider's own
/// error. It reads the events, never the bytes, and relays nothing: the
/// stream is the provider's own. Blocks and events it does not know (server
/// tools, search results, citations, whatever comes next) are skipped; they
/// are the provider's business and never end the reading.
pub struct StreamReader {
    decoder: StreamDecoder,
    api: NativeApi,
    buf: Vec<u8>,
    /// Content blocks (by index) of a type the common decoder cannot hold.
    skipped: std::collections::HashSet<u64>,
    /// The decoder met an error: nothing more is read.
    stopped: bool,
    /// The provider's last event of the answer was seen.
    ended: bool,
    /// An event past the limit is being skipped, up to its end.
    discarding: bool,
    hosted: u32,
}

/// What a feed of bytes to a [`StreamReader`] came to.
#[derive(Default)]
pub struct Read {
    pub events: Vec<StreamEvent>,
    /// The provider's own error event, or an event the decoder could not read.
    pub error: Option<TranslateError>,
}

impl StreamReader {
    pub fn new(api: NativeApi) -> Self {
        Self {
            decoder: match api {
                NativeApi::Responses => StreamDecoder::responses(),
                NativeApi::Messages => StreamDecoder::new(ProviderKind::Anthropic),
            },
            api,
            buf: Vec::new(),
            skipped: std::collections::HashSet::new(),
            stopped: false,
            ended: false,
            discarding: false,
            hosted: 0,
        }
    }

    /// Whether the provider's final event of the answer has been seen
    /// (`message_stop`, `response.completed`, a failure or an error).
    pub fn ended(&self) -> bool {
        self.ended
    }

    /// The calls of hosted tools (web searches) the provider reported so far.
    pub fn hosted_calls(&self) -> u32 {
        self.hosted
    }

    /// The usage reported so far.
    pub fn usage_so_far(&self) -> Option<Usage> {
        self.decoder.usage_so_far()
    }

    /// Reads `chunk`. An event over [`MAX_EVENT_BYTES`] is not held: it is
    /// skipped up to the delimiter that ends it, and the events after it are
    /// read as ever, so the end of the answer and its usage are not lost.
    pub fn feed(&mut self, chunk: &[u8]) -> Read {
        let mut read = Read::default();
        let mut from = self.buf.len().saturating_sub(3);
        self.buf.extend_from_slice(chunk);
        let mut complete = Vec::new();
        if self.discarding {
            // The first complete event is the end of the skipped one.
            let mut found = split_events_from(&mut self.buf, 0).into_iter();
            if found.next().is_none() {
                let keep = self.buf.len().saturating_sub(3);
                self.buf.drain(..keep);
                return read;
            }
            self.discarding = false;
            complete.extend(found);
            from = 0;
        }
        complete.extend(split_events_from(&mut self.buf, from));
        for event in complete {
            self.event(&event, &mut read);
        }
        if self.buf.len() > MAX_EVENT_BYTES {
            self.discarding = true;
            let keep = self.buf.len() - 3;
            self.buf.drain(..keep);
        }
        read
    }

    fn hosted_of(&mut self, usage: &Value) {
        if let Some(n) = usage["server_tool_use"]["web_search_requests"].as_u64() {
            self.hosted = u32::try_from(n).unwrap_or(u32::MAX);
        }
    }

    fn event(&mut self, event: &[u8], read: &mut Read) {
        // Too large to be read (the decoder would stop on it): skipped.
        if event.len() > MAX_EVENT_BYTES {
            return;
        }
        // What is not JSON is not an event of the answer.
        let Some(v) = data_of(event) else { return };
        let ty = v["type"].as_str().unwrap_or("");
        match (self.api, ty) {
            (NativeApi::Messages, "message_start") => {
                self.hosted_of(&v["message"]["usage"]);
            }
            (NativeApi::Messages, "message_delta") => {
                self.hosted_of(&v["usage"]);
            }
            (NativeApi::Responses, "response.output_item.done") => {
                if is_search_call(&v["item"]) {
                    self.hosted += 1;
                }
            }
            _ => {}
        }
        match (self.api, ty) {
            (NativeApi::Messages, "content_block_start") => {
                let known = matches!(
                    v["content_block"]["type"].as_str(),
                    Some("text" | "thinking" | "redacted_thinking" | "tool_use")
                );
                if !known {
                    self.skipped.extend(v["index"].as_u64());
                    return;
                }
            }
            (NativeApi::Messages, "content_block_delta" | "content_block_stop") => {
                if v["index"]
                    .as_u64()
                    .is_some_and(|i| self.skipped.contains(&i))
                {
                    return;
                }
            }
            (NativeApi::Messages, "message_stop" | "error") => self.ended = true,
            (
                NativeApi::Responses,
                "response.completed" | "response.incomplete" | "response.failed" | "error",
            ) => {
                self.ended = true;
                let searches = v["response"]["output"].as_array().map_or(0, |o| {
                    o.iter().filter(|i| i["type"] == "web_search_call").count()
                });
                self.hosted = self.hosted.max(u32::try_from(searches).unwrap_or(u32::MAX));
            }
            _ => {}
        }
        if self.stopped {
            return;
        }
        match self.decoder.feed(event) {
            Ok(events) => {
                read.events.extend(events);
                if let Some(e) = self.decoder.take_error() {
                    self.stopped = true;
                    read.error = Some(e);
                }
            }
            Err(e) => {
                self.stopped = true;
                read.error = Some(e);
            }
        }
    }
}

const STATE_REFUSAL: &str =
    "is not supported through the gateway: it reads or writes state kept in the shared provider account.";

/// Whether a content block or part names a stored file, where the API reads
/// that: the `source` of an Anthropic block, an `input_file` or `input_image`
/// of Responses. Nothing the model or a tool wrote (tool input, arguments,
/// results) is looked into for it.
fn names_file(block: &Value) -> bool {
    let source = &block["source"];
    source["file_id"].is_string()
        || source["type"] == "file"
        || (matches!(block["type"].as_str(), Some("input_file" | "input_image"))
            && block["file_id"].is_string())
}

/// `names_file` for a block or part, and for the blocks of a tool result.
fn reads_file(block: &Value) -> bool {
    names_file(block)
        || ["content", "output"].iter().any(|k| {
            block[k]
                .as_array()
                .is_some_and(|parts| parts.iter().any(reads_file))
        })
}

fn set(v: Option<&Value>) -> bool {
    !matches!(v, None | Some(Value::Null) | Some(Value::Bool(false)))
}

/// The hosted tools of the Responses API other than the web search: they run
/// in the provider's account, on its files, vector stores and connectors, and
/// are billed per call.
fn hosted_tool(ty: &str) -> bool {
    [
        "file_search",
        "code_interpreter",
        "computer",
        "mcp",
        "image_generation",
        "code_execution",
    ]
    .iter()
    .any(|p| ty == *p || ty.starts_with(&format!("{p}_")))
}

/// The message that refuses a pass-through request which reads or writes the
/// state of the provider account the gateway holds a credential for, naming
/// the field, or `None` when there is nothing of the kind in it. What
/// Claude Code and Codex send is not in it: Codex sends `store: false`, and
/// the web search tools of both are the provider's own business.
pub fn stored_state(native: &Native, target: ProviderKind) -> Option<String> {
    let body = &native.body;
    let refuse = |field: &str| Some(format!("'{field}' {STATE_REFUSAL}"));
    match native.api {
        NativeApi::Responses => {
            // Codex is built to send `store: true` when it is pointed at an
            // Azure host (its binary carries the Azure host names), so an
            // Azure target takes it: what it stores cannot be read back
            // without the fields refused below.
            if body.get("store") == Some(&Value::Bool(true)) && target != ProviderKind::Azure {
                return refuse("store");
            }
            for field in ["previous_response_id", "conversation", "background"] {
                if set(body.get(field)) {
                    return refuse(field);
                }
            }
            for item in body
                .get("input")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if item["type"] == "item_reference" {
                    return refuse("item_reference");
                }
                if reads_file(item) {
                    return refuse("file_id");
                }
            }
            for tool in body
                .get("tools")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if set(tool.get("vector_store_ids")) {
                    return refuse("vector_store_ids");
                }
                for field in ["container", "environment"] {
                    if set(tool.get(field)) {
                        return refuse(&format!("tools[].{field}"));
                    }
                }
                if let Some(ty) = tool["type"].as_str().filter(|t| hosted_tool(t)) {
                    return refuse(&format!("tools[].type {ty}"));
                }
            }
        }
        NativeApi::Messages => {
            for field in ["container", "mcp_servers"] {
                let given = match body.get(field) {
                    Some(Value::Array(a)) => !a.is_empty(),
                    other => set(other),
                };
                if given {
                    return refuse(field);
                }
            }
            for tool in body
                .get("tools")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(ty) = tool["type"]
                    .as_str()
                    .filter(|t| t.starts_with("code_execution"))
                {
                    return refuse(&format!("tools[].type {ty}"));
                }
            }
            for message in body
                .get("messages")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if message["content"]
                    .as_array()
                    .is_some_and(|blocks| blocks.iter().any(reads_file))
                {
                    return refuse("file_id");
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn target(kind: ProviderKind) -> Target {
        Target {
            kind,
            base_url: "https://x.example/".into(),
            api_key: Some("k".into()),
            model: "m".into(),
            api_version: None,
        }
    }

    fn native(api: NativeApi, query: Option<&str>) -> Native {
        let body: Map<String, Value> =
            serde_json::from_str(r#"{"zeta":1,"model":"gw/name","alpha":[true]}"#).unwrap();
        Native {
            api,
            body,
            headers: vec![("anthropic-beta".into(), "a".into())],
            query: query.map(str::to_string),
            parse_error: None,
        }
    }

    #[test]
    fn only_the_apis_own_headers_are_forwardable() {
        let mut h = HeaderMap::new();
        for (k, v) in [
            ("anthropic-beta", "a"),
            ("anthropic-beta", "b"),
            ("anthropic-version", "v"),
            ("anthropic-admin-key", "no"),
            ("openai-beta", "o"),
            ("openai-organization", "no"),
            ("x-claude-code-session-id", "no"),
            ("authorization", "no"),
        ] {
            h.append(
                axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                HeaderValue::from_str(v).unwrap(),
            );
        }
        let anthropic = forwardable(NativeApi::Messages, &h);
        assert_eq!(
            anthropic,
            [
                ("anthropic-beta".to_string(), "a".to_string()),
                ("anthropic-beta".to_string(), "b".to_string()),
                ("anthropic-version".to_string(), "v".to_string()),
            ]
        );
        assert_eq!(
            forwardable(NativeApi::Responses, &h),
            [("openai-beta".to_string(), "o".to_string())]
        );
    }

    #[test]
    fn the_model_is_replaced_in_place() {
        let r = build(
            &native(NativeApi::Messages, Some("beta=true")),
            &target(ProviderKind::Anthropic),
        );
        assert_eq!(r.url, "https://x.example/v1/messages?beta=true");
        assert_eq!(
            String::from_utf8(r.body).unwrap(),
            r#"{"zeta":1,"model":"m","alpha":[true]}"#
        );
        assert!(r.headers.contains(&("x-api-key".into(), "k".into())));
        assert!(r
            .headers
            .contains(&("anthropic-version".into(), "2023-06-01".into())));
    }

    #[test]
    fn urls_and_credentials_follow_the_provider() {
        let n = native(NativeApi::Responses, None);
        let r = build(&n, &target(ProviderKind::OpenAi));
        assert_eq!(r.url, "https://x.example/responses");
        assert!(r
            .headers
            .contains(&("authorization".into(), "Bearer k".into())));
        let r = build(&n, &target(ProviderKind::Azure));
        assert_eq!(
            r.url,
            "https://x.example/openai/responses?api-version=2025-03-01-preview"
        );
        assert!(r.headers.contains(&("api-key".into(), "k".into())));
    }

    #[test]
    fn events_split_with_their_delimiters_and_a_partial_one_waits() {
        let mut buf = b"event: a\ndata: {}\n\nevent: b\r\n\r\nevent: c\nda".to_vec();
        let got = split_events(&mut buf);
        assert_eq!(
            got,
            [
                b"event: a\ndata: {}\n\n".to_vec(),
                b"event: b\r\n\r\n".to_vec()
            ]
        );
        assert_eq!(buf, b"event: c\nda");
    }

    #[test]
    fn a_delta_is_rewritten_and_nothing_else_changes() {
        let ev = b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"a@b.c\"}}\n\n";
        let v = data_of(ev).unwrap();
        assert_eq!(kind_of(NativeApi::Messages, &v), Kind::Text);
        assert_eq!(delta_of(NativeApi::Messages, &v).as_deref(), Some("a@b.c"));
        let out = rewritten(NativeApi::Messages, ev, &v, "[x]");
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"[x]\"}}\n\n"
        );
    }

    #[test]
    fn usage_is_read_from_either_api() {
        let u = usage_of(
            NativeApi::Messages,
            br#"{"usage":{"input_tokens":7,"output_tokens":3,"cache_read_input_tokens":100,"cache_creation_input_tokens":20}}"#,
        )
        .unwrap();
        assert_eq!(
            (
                u.input_tokens,
                u.cache_read_input_tokens,
                u.cache_write_input_tokens
            ),
            (7, 100, 20)
        );
        assert_eq!(usage_of(NativeApi::Messages, b"{}"), None);
    }

    #[test]
    fn the_reader_skips_what_it_does_not_know_and_still_ends_on_the_last_event() {
        let mut r = StreamReader::new(NativeApi::Messages);
        let events = concat!(
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":7,\"output_tokens\":1}}}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"mcp_tool_use\",\"id\":\"x\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{}\"}}\n\n",
            ": a comment\n\n",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{},\"usage\":{\"output_tokens\":5}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        );
        // Fed in pieces that split events.
        let mut got = Read::default();
        for chunk in events.as_bytes().chunks(37) {
            let read = r.feed(chunk);
            got.events.extend(read.events);
            assert!(read.error.is_none());
        }
        assert!(r.ended());
        let done = got.events.iter().find_map(|e| match e {
            StreamEvent::Done { usage, .. } => *usage,
            _ => None,
        });
        let u = done.unwrap();
        assert_eq!((u.input_tokens, u.output_tokens), (7, 5));
    }

    #[test]
    fn a_reader_passes_the_providers_own_error_on() {
        let mut r = StreamReader::new(NativeApi::Messages);
        let read = r.feed(
            b"event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n",
        );
        assert!(matches!(read.error, Some(TranslateError::Provider { .. })));
        assert!(r.ended());
    }

    #[test]
    fn a_sequence_number_never_repeats_or_goes_back() {
        let ev = |n: u64| {
            format!("event: e\ndata: {{\"type\":\"e\",\"sequence_number\":{n}}}\n\n").into_bytes()
        };
        let mut last = None;
        let got: Vec<u64> = [1, 2, 2, 3, 4, 9]
            .into_iter()
            .map(|n| {
                data_of(&resequenced(ev(n), &mut last)).unwrap()["sequence_number"]
                    .as_u64()
                    .unwrap()
            })
            .collect();
        assert_eq!(got, [1, 2, 3, 4, 5, 9]);
    }

    #[test]
    fn state_of_the_provider_account_is_found_and_the_agents_own_are_not() {
        let check = |api, body: Value| {
            stored_state(
                &Native {
                    api,
                    body: body.as_object().unwrap().clone(),
                    headers: Vec::new(),
                    query: None,
                    parse_error: None,
                },
                ProviderKind::OpenAi,
            )
        };
        let r = NativeApi::Responses;
        assert!(check(r, serde_json::json!({"store": true})).is_some());
        assert!(check(
            r,
            serde_json::json!({"store": false, "include": ["reasoning.encrypted_content"],
            "tools": [{"type": "web_search"}, {"type": "tool_search"}, {"type": "custom"},
                      {"type": "function", "parameters": {"properties": {"file_id": {}}}}]})
        )
        .is_none());
        assert!(check(r, serde_json::json!({"tools": [{"type": "mcp"}]})).is_some());
        let m = NativeApi::Messages;
        assert!(check(m, serde_json::json!({"container": "c"})).is_some());
        assert!(check(m, serde_json::json!({"container": null, "mcp_servers": []})).is_none());
        assert!(check(
            m,
            serde_json::json!({"tools": [{"type": "web_search_20250305"}]})
        )
        .is_none());
    }

    #[test]
    fn azure_gets_a_version_that_has_the_responses_api() {
        assert_eq!(responses_api_version(None), "2025-03-01-preview");
        assert_eq!(
            responses_api_version(Some("2024-06-01")),
            "2025-03-01-preview"
        );
        assert_eq!(
            responses_api_version(Some("2025-03-01-preview")),
            "2025-03-01-preview"
        );
        assert_eq!(
            responses_api_version(Some("2025-04-01-preview")),
            "2025-04-01-preview"
        );
        assert_eq!(responses_api_version(Some("v1")), "v1");
        let mut t = target(ProviderKind::Azure);
        t.api_version = Some("2023-05-15".into());
        let r = build(&native(NativeApi::Responses, None), &t);
        assert!(
            r.url.ends_with("api-version=2025-03-01-preview"),
            "{}",
            r.url
        );
    }

    #[test]
    fn a_body_without_a_model_gets_one_and_the_rest_keeps_its_place() {
        let mut n = native(NativeApi::Messages, None);
        n.body.shift_remove("model");
        let r = build(&n, &target(ProviderKind::Anthropic));
        assert_eq!(
            String::from_utf8(r.body).unwrap(),
            r#"{"zeta":1,"alpha":[true],"model":"m"}"#
        );
    }
}
