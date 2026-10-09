use serde::Deserialize;
use serde_json::{json, Value};

use super::{saturate, with_calls, HttpRequest, StreamState, Target};
use crate::error::TranslateError;
use crate::sse::SseEvent;
use crate::types::{
    ChatRequest, ChatResponse, FinishReason, ImageSource, Message, Part, ResponseFormat, Role,
    StreamEvent, Tool, ToolCall, ToolChoice, Usage,
};

fn role_str(r: Role) -> &'static str {
    match r {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
}

fn finish(s: &str) -> Option<FinishReason> {
    match s {
        "stop" => Some(FinishReason::Stop),
        "length" => Some(FinishReason::Length),
        "tool_calls" => Some(FinishReason::ToolCalls),
        "content_filter" => Some(FinishReason::ContentFilter),
        _ => None,
    }
}

/// The arguments of a tool call as text, one rule for streams and whole
/// answers: a string as it is, an object serialized (some compatibles send
/// one), anything else nothing.
fn arguments_text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Object(_) => Some(v.to_string()),
        _ => None,
    }
}

/// True when a `tool_calls` / `function_call` value carries something.
fn has_tool_call(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Array(a) => !a.is_empty(),
        _ => true,
    }
}

fn function_call_unsupported() -> TranslateError {
    TranslateError::Unsupported("provider response contains a legacy function call".into())
}

fn image_url(src: &ImageSource) -> String {
    match src {
        ImageSource::Url(u) => u.clone(),
        ImageSource::Base64 { media_type, data } => format!("data:{media_type};base64,{data}"),
    }
}

/// Text-only messages keep a string `content` (bodies stay as they were
/// before parts existed); an array is used only when an image is present.
fn content_value(m: &Message) -> Value {
    if !m.has_images() {
        let text = m.joined_text();
        if text.is_empty() && !m.tool_calls.is_empty() {
            return Value::Null;
        }
        return json!(text);
    }
    let parts: Vec<Value> = m
        .content
        .iter()
        .map(|p| match p {
            Part::Text(t) => json!({ "type": "text", "text": t }),
            Part::Image(i) => json!({ "type": "image_url", "image_url": { "url": image_url(i) } }),
        })
        .collect();
    Value::Array(parts)
}

fn message_value(m: &Message) -> Value {
    let mut o = json!({ "role": role_str(m.role), "content": content_value(m) });
    if let Some(n) = &m.name {
        o["name"] = json!(n);
    }
    if m.role == Role::Tool {
        if let Some(id) = &m.tool_call_id {
            o["tool_call_id"] = json!(id);
        }
    }
    if !m.tool_calls.is_empty() {
        o["tool_calls"] = m
            .tool_calls
            .iter()
            .map(|c| {
                json!({
                    "id": c.id,
                    "type": "function",
                    "function": { "name": c.name, "arguments": c.arguments },
                })
            })
            .collect();
    }
    o
}

fn tool_value(t: &Tool) -> Value {
    let mut f = json!({ "name": t.name, "parameters": t.parameters });
    if let Some(d) = &t.description {
        f["description"] = json!(d);
    }
    if let Some(s) = t.strict {
        f["strict"] = json!(s);
    }
    json!({ "type": "function", "function": f })
}

fn tool_choice_value(c: &ToolChoice) -> Value {
    match c {
        ToolChoice::Auto => json!("auto"),
        ToolChoice::None => json!("none"),
        ToolChoice::Required => json!("required"),
        ToolChoice::Tool(n) => json!({ "type": "function", "function": { "name": n } }),
    }
}

fn response_format_value(f: &ResponseFormat) -> Value {
    match f {
        ResponseFormat::Text => json!({ "type": "text" }),
        ResponseFormat::JsonObject => json!({ "type": "json_object" }),
        ResponseFormat::JsonSchema {
            name,
            schema,
            strict,
            description,
        } => {
            let mut spec = json!({ "name": name, "schema": schema });
            if let Some(d) = description {
                spec["description"] = json!(d);
            }
            if let Some(s) = strict {
                spec["strict"] = json!(s);
            }
            json!({ "type": "json_schema", "json_schema": spec })
        }
    }
}

pub(crate) fn build(target: &Target, req: &ChatRequest) -> Result<HttpRequest, TranslateError> {
    let mut headers = vec![("content-type".to_string(), "application/json".to_string())];
    if let Some(k) = &target.api_key {
        headers.push(("authorization".to_string(), format!("Bearer {k}")));
    }
    Ok(HttpRequest {
        method: "POST",
        url: format!("{}/chat/completions", target.base_url.trim_end_matches('/')),
        headers,
        body: body(req, Some(&target.model))?,
    })
}

/// The JSON body of a chat completion. Azure names the model in the URL and
/// leaves it out here.
pub(crate) fn body(req: &ChatRequest, model: Option<&str>) -> Result<Vec<u8>, TranslateError> {
    super::check_tool_choice(req)?;
    let messages: Vec<Value> = req.messages.iter().map(message_value).collect();
    let mut body = json!({ "messages": messages });
    if let Some(model) = model {
        body["model"] = json!(model);
    }
    if let Some(v) = req.max_tokens {
        body["max_tokens"] = json!(v);
    }
    if let Some(v) = req.temperature {
        body["temperature"] = json!(v);
    }
    if let Some(v) = req.top_p {
        body["top_p"] = json!(v);
    }
    if let Some(v) = &req.stop {
        body["stop"] = json!(v);
    }
    if !req.tools.is_empty() {
        body["tools"] = req.tools.iter().map(tool_value).collect();
        if let Some(c) = &req.tool_choice {
            body["tool_choice"] = tool_choice_value(c);
        }
        if let Some(p) = req.parallel_tool_calls {
            body["parallel_tool_calls"] = json!(p);
        }
    }
    if let Some(f) = &req.response_format {
        body["response_format"] = response_format_value(f);
    }
    if let Some(e) = &req.reasoning_effort {
        body["reasoning_effort"] = json!(e);
    }
    if req.stream {
        body["stream"] = json!(true);
        body["stream_options"] = json!({ "include_usage": true });
    }
    serde_json::to_vec(&body).map_err(|e| TranslateError::InvalidRequest(e.to_string()))
}

#[derive(Deserialize)]
struct WireResponse {
    id: String,
    model: String,
    choices: Vec<WireChoice>,
    #[serde(default)]
    usage: Option<WireUsage>,
}

#[derive(Deserialize)]
struct WireChoice {
    message: WireMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct WireMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    refusal: Option<String>,
    #[serde(default)]
    tool_calls: Value,
    #[serde(default)]
    function_call: Value,
}

#[derive(Deserialize)]
struct WireUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
}

pub(crate) fn parse(body: &[u8]) -> Result<ChatResponse, TranslateError> {
    let w: WireResponse =
        serde_json::from_slice(body).map_err(|e| TranslateError::Malformed(e.to_string()))?;
    let choice = w
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| TranslateError::Malformed("response has no choices".into()))?;
    let message = choice.message;
    if has_tool_call(&message.function_call) {
        return Err(function_call_unsupported());
    }
    let mut tool_calls = Vec::new();
    if let Value::Array(items) = &message.tool_calls {
        for item in items {
            // A missing type is a function (the stream and the ingress agree).
            if !item["type"].is_null() && item["type"] != "function" {
                return Err(TranslateError::Malformed(
                    "tool call type is not 'function'".into(),
                ));
            }
            let f = &item["function"];
            let (Some(id), Some(name)) = (item["id"].as_str(), f["name"].as_str()) else {
                return Err(TranslateError::Malformed(
                    "tool call has no id or name".into(),
                ));
            };
            tool_calls.push(ToolCall {
                id: id.to_string(),
                name: name.to_string(),
                arguments: arguments_text(&f["arguments"]).unwrap_or_else(|| "{}".to_string()),
            });
        }
    } else if has_tool_call(&message.tool_calls) {
        return Err(TranslateError::Malformed("tool_calls is not a list".into()));
    }
    let mut content = message.content.unwrap_or_default();
    let mut finish_reason = choice.finish_reason.as_deref().and_then(finish);
    if let Some(refusal) = message.refusal {
        content.push_str(&refusal);
        finish_reason = Some(FinishReason::ContentFilter);
    }
    let finish_reason = with_calls(finish_reason, !tool_calls.is_empty());
    Ok(ChatResponse {
        id: w.id,
        model: w.model,
        content,
        tool_calls,
        finish_reason,
        usage: w.usage.map(|u| Usage {
            input_tokens: saturate(u.prompt_tokens),
            output_tokens: saturate(u.completion_tokens),
        }),
    })
}

/// One element of `delta.tool_calls`. A call starts with the element that
/// carries its `id`; a different id on a provider index that already has a
/// call starts a new call (some compatibles send every parallel call with
/// index 0). Later chunks go to the newest call of their provider index. A
/// name arriving later for a call that started without one is ignored: the
/// start event has already been sent. A missing `index` (Ollama, older Groq:
/// a whole call in one chunk) starts a call at the next unused tool index.
fn decode_tool_call(
    state: &mut StreamState,
    item: &Value,
    out: &mut Vec<StreamEvent>,
) -> Result<(), TranslateError> {
    let provider_index = match item["index"].as_u64() {
        Some(i) => Some(
            u32::try_from(i)
                .map_err(|_| TranslateError::Malformed("tool call index out of range".into()))?,
        ),
        None => None,
    };
    let f = &item["function"];
    let id = item["id"].as_str().filter(|s| !s.is_empty());
    let current = provider_index.and_then(|p| state.tool_call_slots.get(&p).copied());
    let index = match (id, current) {
        (Some(id), Some(t)) if state.tool_call_ids[t as usize] == id => t,
        (Some(id), _) => {
            let t = u32::try_from(state.tool_call_ids.len())
                .map_err(|_| TranslateError::Malformed("too many tool calls".into()))?;
            state.tool_call_ids.push(id.to_string());
            if let Some(p) = provider_index {
                state.tool_call_slots.insert(p, t);
            }
            out.push(StreamEvent::ToolCallStart {
                index: t,
                id: id.to_string(),
                name: f["name"].as_str().unwrap_or_default().to_string(),
            });
            t
        }
        (None, Some(t)) => t,
        (None, None) => {
            return Err(TranslateError::Malformed(
                "tool call delta before its start".into(),
            ));
        }
    };
    let args = arguments_text(&f["arguments"]).unwrap_or_default();
    if !args.is_empty() {
        out.push(StreamEvent::ToolCallDelta {
            index,
            arguments: args,
        });
    }
    Ok(())
}

pub(crate) fn decode(
    state: &mut StreamState,
    ev: &SseEvent,
    out: &mut Vec<StreamEvent>,
) -> Result<(), TranslateError> {
    if ev.data == "[DONE]" {
        out.push(StreamEvent::Done {
            finish_reason: with_calls(state.finish, !state.tool_call_ids.is_empty()),
            usage: state.usage(),
        });
        return Ok(());
    }
    let v: Value =
        serde_json::from_str(&ev.data).map_err(|e| TranslateError::Malformed(e.to_string()))?;
    if let Some(msg) = v["error"]["message"].as_str() {
        // Reported as 401 so it is handled like a rejected credential.
        let credential = v["error"]["code"] == "invalid_api_key";
        return Err(TranslateError::Provider {
            status: if credential { 401 } else { 502 },
            retryable: false,
            message: msg.to_string(),
        });
    }
    if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
        state.input_tokens = u["prompt_tokens"].as_u64().map(saturate);
        state.output_tokens = u["completion_tokens"].as_u64().map(saturate);
    }
    let choice = &v["choices"][0];
    let delta = &choice["delta"];
    if has_tool_call(&delta["function_call"]) {
        return Err(function_call_unsupported());
    }
    match &delta["tool_calls"] {
        Value::Null => {}
        Value::Array(items) => {
            for item in items {
                decode_tool_call(state, item, out)?;
            }
        }
        _ => return Err(TranslateError::Malformed("tool_calls is not a list".into())),
    }
    if let Some(f) = choice["finish_reason"].as_str() {
        // A refusal already decided the finish reason; a later "stop" must not hide it.
        if state.finish != Some(FinishReason::ContentFilter) {
            state.finish = finish(f);
        }
    }
    if let Some(t) = delta["refusal"].as_str() {
        state.finish = Some(FinishReason::ContentFilter);
        if !t.is_empty() {
            out.push(StreamEvent::Delta {
                text: t.to_string(),
            });
        }
    }
    if let Some(t) = delta["content"].as_str() {
        if !t.is_empty() {
            out.push(StreamEvent::Delta {
                text: t.to_string(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::error::TranslateError;
    use crate::provider::*;
    use crate::types::*;

    fn target() -> Target {
        Target {
            kind: ProviderKind::OpenAi,
            base_url: "https://api.example.com/v1/".into(),
            api_key: Some("sk-x".into()),
            model: "gpt-4o".into(),
            api_version: None,
        }
    }

    fn request(stream: bool) -> ChatRequest {
        ChatRequest {
            model: "openai/gpt-4o".into(),
            messages: vec![Message::text(Role::User, "hi")],
            max_tokens: Some(5),
            temperature: None,
            top_p: None,
            stop: None,
            stream,
            tools: Vec::new(),
            tool_choice: None,
            parallel_tool_calls: None,
            response_format: None,
            reasoning_effort: None,
        }
    }

    #[test]
    fn builds_request_with_target_model_and_auth() {
        let r = build_request(&target(), &request(false)).unwrap();
        assert_eq!(r.method, "POST");
        assert_eq!(r.url, "https://api.example.com/v1/chat/completions");
        assert!(r
            .headers
            .contains(&("authorization".into(), "Bearer sk-x".into())));
        let v: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(v["model"], "gpt-4o");
        assert_eq!(v["max_tokens"], 5);
        assert_eq!(v["messages"][0]["content"], "hi");
        assert!(v.get("stream").is_none());
        assert!(v.get("temperature").is_none());
    }

    #[test]
    fn stream_request_asks_for_usage() {
        let r = build_request(&target(), &request(true)).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(v["stream"], true);
        assert_eq!(v["stream_options"]["include_usage"], true);
    }

    #[test]
    fn omits_auth_header_without_key() {
        let mut t = target();
        t.api_key = None;
        let r = build_request(&t, &request(false)).unwrap();
        assert!(!r.headers.iter().any(|(k, _)| k == "authorization"));
    }

    #[test]
    fn parses_response() {
        let body = br#"{"id":"c1","model":"gpt-4o","choices":[{"message":{"role":"assistant","content":"yo"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":2}}"#;
        let r = parse_response(ProviderKind::OpenAi, 200, body).unwrap();
        assert_eq!(r.content, "yo");
        assert_eq!(r.finish_reason, Some(FinishReason::Stop));
        assert_eq!(
            r.usage,
            Some(Usage {
                input_tokens: 1,
                output_tokens: 2
            })
        );
    }

    #[test]
    fn json_error_body_keeps_message() {
        let body = br#"{"error":{"message":"bad key","type":"auth"}}"#;
        let e = parse_response(ProviderKind::OpenAi, 401, body).unwrap_err();
        assert_eq!(
            e,
            TranslateError::Provider {
                status: 401,
                retryable: false,
                message: "bad key".into()
            }
        );
    }

    #[test]
    fn html_error_body_is_a_retryable_provider_error() {
        let e = parse_response(ProviderKind::OpenAi, 502, b"<html>Bad Gateway</html>").unwrap_err();
        assert_eq!(
            e,
            TranslateError::Provider {
                status: 502,
                retryable: true,
                message: "<html>Bad Gateway</html>".into()
            }
        );
    }

    #[test]
    fn success_status_with_unreadable_body_is_malformed() {
        let e = parse_response(ProviderKind::OpenAi, 200, b"not json").unwrap_err();
        assert!(matches!(e, TranslateError::Malformed(_)));
    }

    const STREAM: &str = concat!(
        "data: {\"id\":\"c\",\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"c\",\"choices\":[{\"delta\":{\"content\":\"h\u{e9}\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"c\",\"choices\":[{\"delta\":{\"content\":\"y \u{1f600}\"},\"finish_reason\":null}]}\n\n",
        "data: {\"id\":\"c\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: {\"id\":\"c\",\"choices\":[],\"usage\":{\"prompt_tokens\":4,\"completion_tokens\":6}}\n\n",
        "data: [DONE]\n\n",
    );

    fn expected() -> Vec<StreamEvent> {
        vec![
            StreamEvent::Delta {
                text: "h\u{e9}".into(),
            },
            StreamEvent::Delta {
                text: "y \u{1f600}".into(),
            },
            StreamEvent::Done {
                finish_reason: Some(FinishReason::Stop),
                usage: Some(Usage {
                    input_tokens: 4,
                    output_tokens: 6,
                }),
            },
        ]
    }

    #[test]
    fn decodes_stream() {
        let mut d = StreamDecoder::new(ProviderKind::OpenAi);
        assert_eq!(d.feed(STREAM.as_bytes()).unwrap(), expected());
    }

    #[test]
    fn decodes_stream_identically_at_every_split_point() {
        let bytes = STREAM.as_bytes();
        for i in 0..=bytes.len() {
            let mut d = StreamDecoder::new(ProviderKind::OpenAi);
            let mut got = d.feed(&bytes[..i]).unwrap();
            got.extend(d.feed(&bytes[i..]).unwrap());
            assert_eq!(got, expected(), "split at byte {i}");
        }
    }

    #[test]
    fn error_inside_stream_is_reported() {
        let mut d = StreamDecoder::new(ProviderKind::OpenAi);
        let e = d
            .feed(b"data: {\"error\":{\"message\":\"overloaded\"}}\n\n")
            .unwrap_err();
        assert!(matches!(e, TranslateError::Provider { message, .. } if message == "overloaded"));
    }

    #[test]
    fn response_token_counts_saturate_instead_of_truncating() {
        let body = br#"{"id":"c1","model":"m","choices":[{"message":{"content":"yo"},"finish_reason":"stop"}],"usage":{"prompt_tokens":4294967301,"completion_tokens":2}}"#;
        let r = parse_response(ProviderKind::OpenAi, 200, body).unwrap();
        assert_eq!(
            r.usage,
            Some(Usage {
                input_tokens: u32::MAX,
                output_tokens: 2
            })
        );
    }

    #[test]
    fn stream_token_counts_saturate_instead_of_truncating() {
        let mut d = StreamDecoder::new(ProviderKind::OpenAi);
        let got = d
            .feed(b"data: {\"choices\":[],\"usage\":{\"prompt_tokens\":4294967301,\"completion_tokens\":6}}\n\ndata: [DONE]\n\n")
            .unwrap();
        assert_eq!(
            got,
            vec![StreamEvent::Done {
                finish_reason: None,
                usage: Some(Usage {
                    input_tokens: u32::MAX,
                    output_tokens: 6
                }),
            }]
        );
    }

    #[test]
    fn response_refusal_becomes_content_with_content_filter() {
        let body = br#"{"id":"c1","model":"m","choices":[{"message":{"content":null,"refusal":"I cannot help"},"finish_reason":"stop"}]}"#;
        let r = parse_response(ProviderKind::OpenAi, 200, body).unwrap();
        assert_eq!(r.content, "I cannot help");
        assert_eq!(r.finish_reason, Some(FinishReason::ContentFilter));
    }

    #[test]
    fn null_refusal_and_empty_tool_calls_are_ignored() {
        let body = br#"{"id":"c1","model":"m","choices":[{"message":{"content":"yo","refusal":null,"tool_calls":[]},"finish_reason":"stop"}]}"#;
        let r = parse_response(ProviderKind::OpenAi, 200, body).unwrap();
        assert_eq!(r.content, "yo");
        assert_eq!(r.finish_reason, Some(FinishReason::Stop));
    }

    #[test]
    fn stream_refusal_becomes_text_with_content_filter() {
        let mut d = StreamDecoder::new(ProviderKind::OpenAi);
        let got = d
            .feed(
                concat!(
                "data: {\"choices\":[{\"delta\":{\"refusal\":\"no\"},\"finish_reason\":null}]}\n\n",
                "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                "data: [DONE]\n\n",
            )
                .as_bytes(),
            )
            .unwrap();
        assert_eq!(
            got,
            vec![
                StreamEvent::Delta { text: "no".into() },
                StreamEvent::Done {
                    finish_reason: Some(FinishReason::ContentFilter),
                    usage: None
                },
            ]
        );
    }

    fn stream_error_status(data: &str) -> u16 {
        let mut d = StreamDecoder::new(ProviderKind::OpenAi);
        match d.feed(format!("data: {data}\n\n").as_bytes()).unwrap_err() {
            TranslateError::Provider { status, .. } => status,
            other => panic!("expected a provider error, got {other:?}"),
        }
    }

    #[test]
    fn credential_error_inside_stream_has_status_401() {
        assert_eq!(
            stream_error_status(
                r#"{"error":{"message":"Incorrect API key provided: sk-abc","type":"invalid_request_error","code":"invalid_api_key"}}"#
            ),
            401
        );
    }

    #[test]
    fn other_error_inside_stream_keeps_status_502() {
        assert_eq!(
            stream_error_status(r#"{"error":{"message":"overloaded","code":"server_error"}}"#),
            502
        );
        assert_eq!(
            stream_error_status(r#"{"error":{"message":"overloaded"}}"#),
            502
        );
    }

    fn tool(name: &str) -> Tool {
        Tool {
            name: name.into(),
            description: Some("d".into()),
            parameters: serde_json::json!({"type": "object", "properties": {"city": {"type": "string"}}}),
            strict: None,
        }
    }

    fn body_of(req: &ChatRequest) -> serde_json::Value {
        let r = build_request(&target(), req).unwrap();
        serde_json::from_slice(&r.body).unwrap()
    }

    fn tool_conversation() -> ChatRequest {
        let mut r = request(false);
        r.tools = vec![tool("get_weather")];
        r.tool_choice = Some(ToolChoice::Tool("get_weather".into()));
        r.parallel_tool_calls = Some(false);
        r.messages = vec![
            Message::text(Role::User, "weather?"),
            Message {
                tool_calls: vec![ToolCall {
                    id: "call_1".into(),
                    name: "get_weather".into(),
                    arguments: "{\"city\":\"Paris\"}".into(),
                }],
                ..Message::text(Role::Assistant, "")
            },
            Message {
                tool_call_id: Some("call_1".into()),
                ..Message::text(Role::Tool, "sunny")
            },
        ];
        r
    }

    #[test]
    fn strict_is_sent_only_when_set() {
        let mut r = request(false);
        r.tools = vec![tool("a"), tool("b"), tool("c")];
        r.tools[0].strict = Some(true);
        r.tools[1].strict = Some(false);
        let v = body_of(&r);
        assert_eq!(v["tools"][0]["function"]["strict"], true);
        assert_eq!(v["tools"][1]["function"]["strict"], false);
        assert!(v["tools"][2]["function"].get("strict").is_none());
    }

    #[test]
    fn a_named_tool_choice_must_name_a_tool() {
        let mut r = tool_conversation();
        r.tool_choice = Some(ToolChoice::Tool("nope".into()));
        assert_eq!(
            build_request(&target(), &r).unwrap_err(),
            TranslateError::InvalidRequest(
                "tool_choice names 'nope', which is not in tools".into()
            )
        );
    }

    #[test]
    fn non_stream_tool_calls_follow_the_stream_rules() {
        // Object arguments are serialized, a missing type is a function,
        // absent arguments are "{}".
        let body = br#"{"id":"i","model":"m","choices":[{"message":{"content":null,"tool_calls":[
            {"id":"a","function":{"name":"f","arguments":{"a":1}}},
            {"id":"b","type":"function","function":{"name":"g"}},
            {"id":"c","type":"function","function":{"name":"h","arguments":"{\"x\":2}"}}]},
            "finish_reason":"stop"}]}"#;
        let r = parse_response(ProviderKind::OpenAi, 200, body).unwrap();
        let got: Vec<_> = r
            .tool_calls
            .iter()
            .map(|c| (c.id.as_str(), c.arguments.as_str()))
            .collect();
        assert_eq!(got, [("a", r#"{"a":1}"#), ("b", "{}"), ("c", r#"{"x":2}"#)]);
        // A type other than function stays malformed.
        let body = br#"{"id":"i","model":"m","choices":[{"message":{"content":null,"tool_calls":[
            {"id":"a","type":"custom","function":{"name":"f"}}]}}]}"#;
        assert!(matches!(
            parse_response(ProviderKind::OpenAi, 200, body),
            Err(TranslateError::Malformed(_))
        ));
    }

    #[test]
    fn a_stop_with_tool_calls_finishes_as_tool_calls() {
        let body = br#"{"id":"i","model":"m","choices":[{"message":{"content":null,"tool_calls":[
            {"id":"a","type":"function","function":{"name":"f","arguments":"{}"}}]},
            "finish_reason":"stop"}]}"#;
        let r = parse_response(ProviderKind::OpenAi, 200, body).unwrap();
        assert_eq!(r.finish_reason, Some(FinishReason::ToolCalls));
        // Without calls a stop stays a stop, and a length stop stays.
        let body = br#"{"id":"i","model":"m","choices":[{"message":{"content":"x"},"finish_reason":"stop"}]}"#;
        let r = parse_response(ProviderKind::OpenAi, 200, body).unwrap();
        assert_eq!(r.finish_reason, Some(FinishReason::Stop));
        let body = br#"{"id":"i","model":"m","choices":[{"message":{"content":null,"tool_calls":[
            {"id":"a","type":"function","function":{"name":"f","arguments":"{}"}}]},
            "finish_reason":"length"}]}"#;
        let r = parse_response(ProviderKind::OpenAi, 200, body).unwrap();
        assert_eq!(r.finish_reason, Some(FinishReason::Length));
        // The stream, for both OpenAI-format kinds.
        for kind in [ProviderKind::OpenAi, ProviderKind::Azure] {
            let mut d = StreamDecoder::new(kind);
            let got = d
                .feed(
                    concat!(
                        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"a\",\"function\":{\"name\":\"f\",\"arguments\":\"{}\"}}]},\"finish_reason\":null}]}\n\n",
                        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                        "data: [DONE]\n\n",
                    )
                    .as_bytes(),
                )
                .unwrap();
            assert_eq!(
                got.last(),
                Some(&StreamEvent::Done {
                    finish_reason: Some(FinishReason::ToolCalls),
                    usage: None
                }),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn builds_tool_conversation_body() {
        let v = body_of(&tool_conversation());
        assert_eq!(
            v,
            serde_json::json!({
                "model": "gpt-4o",
                "max_tokens": 5,
                "messages": [
                    {"role": "user", "content": "weather?"},
                    {"role": "assistant", "content": null, "tool_calls": [
                        {"id": "call_1", "type": "function",
                         "function": {"name": "get_weather", "arguments": "{\"city\":\"Paris\"}"}}
                    ]},
                    {"role": "tool", "tool_call_id": "call_1", "content": "sunny"}
                ],
                "tools": [{"type": "function", "function": {
                    "name": "get_weather", "description": "d",
                    "parameters": {"type": "object", "properties": {"city": {"type": "string"}}}
                }}],
                "tool_choice": {"type": "function", "function": {"name": "get_weather"}},
                "parallel_tool_calls": false
            })
        );
    }

    #[test]
    fn tool_choice_strings() {
        for (c, s) in [
            (ToolChoice::Auto, "auto"),
            (ToolChoice::None, "none"),
            (ToolChoice::Required, "required"),
        ] {
            let mut r = tool_conversation();
            r.tool_choice = Some(c);
            r.parallel_tool_calls = None;
            let v = body_of(&r);
            assert_eq!(v["tool_choice"], s);
            assert!(v.get("parallel_tool_calls").is_none());
        }
    }

    #[test]
    fn text_only_messages_keep_string_content() {
        let mut r = request(false);
        r.messages = vec![Message {
            content: vec![Part::Text("a".into()), Part::Text("b".into())],
            ..Message::text(Role::User, "")
        }];
        let v = body_of(&r);
        assert_eq!(v["messages"][0]["content"], "ab");
        assert!(v.get("tools").is_none());
        assert!(v["messages"][0].get("tool_calls").is_none());
        // An empty assistant text without tool calls stays an empty string.
        r.messages = vec![Message::text(Role::Assistant, "")];
        assert_eq!(body_of(&r)["messages"][0]["content"], "");
    }

    #[test]
    fn builds_image_parts_with_data_url_rejoined() {
        let mut r = request(false);
        r.messages = vec![Message {
            content: vec![
                Part::Text("what?".into()),
                Part::Image(ImageSource::Url("https://x.test/a.png".into())),
                Part::Image(ImageSource::Base64 {
                    media_type: "image/png".into(),
                    data: "QUJD".into(),
                }),
            ],
            ..Message::text(Role::User, "")
        }];
        assert_eq!(
            body_of(&r)["messages"][0]["content"],
            serde_json::json!([
                {"type": "text", "text": "what?"},
                {"type": "image_url", "image_url": {"url": "https://x.test/a.png"}},
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,QUJD"}}
            ])
        );
    }

    #[test]
    fn tool_options_without_tools_are_ignored_or_refused() {
        let mut r = request(false);
        r.parallel_tool_calls = Some(true);
        r.tool_choice = Some(ToolChoice::None);
        let v = body_of(&r);
        assert!(v.get("parallel_tool_calls").is_none());
        assert!(v.get("tool_choice").is_none());
        r.tool_choice = Some(ToolChoice::Auto);
        assert!(body_of(&r).get("tool_choice").is_none());
        r.tool_choice = Some(ToolChoice::Required);
        assert_eq!(
            build_request(&target(), &r).unwrap_err(),
            TranslateError::InvalidRequest("tool_choice 'required' needs tools".into())
        );
        r.tool_choice = Some(ToolChoice::Tool("f".into()));
        assert_eq!(
            build_request(&target(), &r).unwrap_err(),
            TranslateError::InvalidRequest("tool_choice 'f' needs tools".into())
        );
        r.tool_choice = None;
        r.tools = vec![tool("f")];
        r.parallel_tool_calls = None;
        assert!(body_of(&r).get("parallel_tool_calls").is_none());
        r.parallel_tool_calls = Some(true);
        assert_eq!(body_of(&r)["parallel_tool_calls"], true);
    }

    #[test]
    fn image_message_and_tool_call_message_in_one_request() {
        let mut r = request(false);
        r.messages = vec![
            Message {
                content: vec![
                    Part::Text("see".into()),
                    Part::Image(ImageSource::Url("https://x.test/a.png".into())),
                ],
                ..Message::text(Role::User, "")
            },
            Message {
                tool_calls: vec![ToolCall {
                    id: "c".into(),
                    name: "f".into(),
                    arguments: "{}".into(),
                }],
                ..Message::text(Role::Assistant, "ok")
            },
            Message {
                tool_call_id: Some("c".into()),
                name: Some("f".into()),
                ..Message::text(Role::Tool, "r")
            },
        ];
        let v = body_of(&r);
        assert_eq!(v["messages"][0]["content"][1]["type"], "image_url");
        assert_eq!(v["messages"][1]["content"], "ok");
        assert_eq!(v["messages"][1]["tool_calls"][0]["id"], "c");
        assert_eq!(v["messages"][2]["name"], "f");
        assert_eq!(v["messages"][2]["tool_call_id"], "c");
    }

    #[test]
    fn whole_calls_all_with_index_zero_are_separate_calls() {
        let mut d = StreamDecoder::new(ProviderKind::OpenAi);
        let got = d
            .feed(
                concat!(
                    "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"a\",\"function\":{\"name\":\"f\",\"arguments\":\"{}\"}}]},\"finish_reason\":null}]}\n\n",
                    "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"b\",\"function\":{\"name\":\"g\",\"arguments\":\"{\\\"x\\\":1}\"}}]},\"finish_reason\":null}]}\n\n",
                )
                .as_bytes(),
            )
            .unwrap();
        assert_eq!(
            got,
            vec![
                tc_start(0, "a", "f"),
                tc_delta(0, "{}"),
                tc_start(1, "b", "g"),
                tc_delta(1, "{\"x\":1}"),
            ]
        );
    }

    #[test]
    fn out_of_order_starts_work_and_unstarted_index_stays_malformed() {
        let mut d = StreamDecoder::new(ProviderKind::OpenAi);
        let got = d
            .feed(
                concat!(
                    "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"id\":\"b\",\"function\":{\"name\":\"g\"}}]},\"finish_reason\":null}]}\n\n",
                    "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"a\",\"function\":{\"name\":\"f\"}}]},\"finish_reason\":null}]}\n\n",
                    "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"1\"}}]},\"finish_reason\":null}]}\n\n",
                )
                .as_bytes(),
            )
            .unwrap();
        assert_eq!(
            got,
            vec![
                tc_start(0, "b", "g"),
                tc_start(1, "a", "f"),
                tc_delta(1, "1")
            ]
        );
        let e = d
            .feed(b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":5,\"function\":{\"arguments\":\"1\"}}]},\"finish_reason\":null}]}\n\n")
            .unwrap_err();
        assert!(matches!(e, TranslateError::Malformed(_)));
    }

    #[test]
    fn streamed_tool_calls_that_is_not_a_list_is_malformed() {
        let mut d = StreamDecoder::new(ProviderKind::OpenAi);
        let e = d
            .feed(b"data: {\"choices\":[{\"delta\":{\"tool_calls\":{\"index\":0}},\"finish_reason\":null}]}\n\n")
            .unwrap_err();
        assert_eq!(
            e,
            TranslateError::Malformed("tool_calls is not a list".into())
        );
    }

    #[test]
    fn parses_response_tool_calls() {
        let body = br#"{"id":"c1","model":"m","choices":[{"message":{"content":null,"tool_calls":[{"id":"t1","type":"function","function":{"name":"f","arguments":"{\"a\":1}"}},{"id":"t2","type":"function","function":{"name":"g","arguments":"{}"}}]},"finish_reason":"tool_calls"}]}"#;
        let r = parse_response(ProviderKind::OpenAi, 200, body).unwrap();
        assert_eq!(r.content, "");
        assert_eq!(r.finish_reason, Some(FinishReason::ToolCalls));
        assert_eq!(
            r.tool_calls,
            vec![
                ToolCall {
                    id: "t1".into(),
                    name: "f".into(),
                    arguments: "{\"a\":1}".into()
                },
                ToolCall {
                    id: "t2".into(),
                    name: "g".into(),
                    arguments: "{}".into()
                },
            ]
        );
    }

    #[test]
    fn non_function_tool_call_is_malformed_and_legacy_function_call_unsupported() {
        let body = br#"{"id":"c1","model":"m","choices":[{"message":{"content":null,"tool_calls":[{"id":"t1","type":"custom","custom":{}}]},"finish_reason":"tool_calls"}]}"#;
        assert!(matches!(
            parse_response(ProviderKind::OpenAi, 200, body).unwrap_err(),
            TranslateError::Malformed(_)
        ));
        let body = br#"{"id":"c1","model":"m","choices":[{"message":{"content":null,"function_call":{"name":"f","arguments":"{}"}},"finish_reason":"function_call"}]}"#;
        assert!(matches!(
            parse_response(ProviderKind::OpenAi, 200, body).unwrap_err(),
            TranslateError::Unsupported(_)
        ));
    }

    fn tc_start(index: u32, id: &str, name: &str) -> StreamEvent {
        StreamEvent::ToolCallStart {
            index,
            id: id.into(),
            name: name.into(),
        }
    }

    fn tc_delta(index: u32, a: &str) -> StreamEvent {
        StreamEvent::ToolCallDelta {
            index,
            arguments: a.into(),
        }
    }

    #[test]
    fn decodes_streamed_parallel_tool_calls() {
        let input = include_bytes!("../../tests/fixtures/openai/stream_tool_calls.txt");
        let want = vec![
            tc_start(0, "call_a", "get_weather"),
            tc_start(1, "call_b", "get_time"),
            tc_delta(0, "{\"city\":"),
            tc_delta(1, "{\"tz\":\"UTC\"}"),
            tc_delta(0, "\"Paris\"}"),
            StreamEvent::Done {
                finish_reason: Some(FinishReason::ToolCalls),
                usage: Some(Usage {
                    input_tokens: 30,
                    output_tokens: 20,
                }),
            },
        ];
        for split in 0..=input.len() {
            let mut d = StreamDecoder::new(ProviderKind::OpenAi);
            let mut got = d.feed(&input[..split]).unwrap();
            got.extend(d.feed(&input[split..]).unwrap());
            assert_eq!(got, want, "split {split}");
        }
    }

    #[test]
    fn decodes_whole_call_without_index() {
        let mut d = StreamDecoder::new(ProviderKind::OpenAi);
        let got = d
            .feed(
                concat!(
                    "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"id\":\"a\",\"type\":\"function\",\"function\":{\"name\":\"f\",\"arguments\":\"{}\"}}]},\"finish_reason\":null}]}\n\n",
                    "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"id\":\"b\",\"type\":\"function\",\"function\":{\"name\":\"g\",\"arguments\":\"{\\\"x\\\":1}\"}}]},\"finish_reason\":null}]}\n\n",
                )
                .as_bytes(),
            )
            .unwrap();
        assert_eq!(
            got,
            vec![
                tc_start(0, "a", "f"),
                tc_delta(0, "{}"),
                tc_start(1, "b", "g"),
                tc_delta(1, "{\"x\":1}"),
            ]
        );
    }

    #[test]
    fn late_name_is_ignored_and_repeated_id_is_not_a_second_start() {
        let mut d = StreamDecoder::new(ProviderKind::OpenAi);
        let got = d
            .feed(
                concat!(
                    "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"a\",\"function\":{\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n",
                    "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"name\":\"late\",\"arguments\":\"{\"}}]},\"finish_reason\":null}]}\n\n",
                    "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"a\",\"function\":{\"arguments\":\"}\"}}]},\"finish_reason\":null}]}\n\n",
                )
                .as_bytes(),
            )
            .unwrap();
        assert_eq!(
            got,
            vec![tc_start(0, "a", ""), tc_delta(0, "{"), tc_delta(0, "}")]
        );
    }

    #[test]
    fn delta_for_unknown_index_is_malformed() {
        let mut d = StreamDecoder::new(ProviderKind::OpenAi);
        let e = d
            .feed(b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":2,\"function\":{\"arguments\":\"{}\"}}]},\"finish_reason\":null}]}\n\n")
            .unwrap_err();
        assert_eq!(
            e,
            TranslateError::Malformed("tool call delta before its start".into())
        );
    }

    #[test]
    fn anthropic_shaped_tool_conversation_round_trips_to_openai() {
        let body = br#"{"model":"m","max_tokens":50,"messages":[
            {"role":"user","content":"weather?"},
            {"role":"assistant","content":[
                {"type":"text","text":"Checking."},
                {"type":"tool_use","id":"toolu_1","name":"get_weather","input":{"city":"Paris"}}]},
            {"role":"user","content":[
                {"type":"tool_result","tool_use_id":"toolu_1","content":"sunny"}]}
        ],"tools":[{"name":"get_weather","description":"d","input_schema":{"type":"object"}}],
        "tool_choice":{"type":"any"}}"#;
        let req = crate::ingress::anthropic::parse_request(body).unwrap();
        let v = body_of(&req);
        assert_eq!(v["messages"][1]["role"], "assistant");
        assert_eq!(v["messages"][1]["content"], "Checking.");
        assert_eq!(v["messages"][1]["tool_calls"][0]["id"], "toolu_1");
        let args = v["messages"][1]["tool_calls"][0]["function"]["arguments"]
            .as_str()
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(args).unwrap(),
            serde_json::json!({"city": "Paris"})
        );
        assert_eq!(v["messages"][2]["role"], "tool");
        assert_eq!(v["messages"][2]["tool_call_id"], "toolu_1");
        assert_eq!(v["messages"][2]["content"], "sunny");
        assert_eq!(v["tool_choice"], "required");
        assert_eq!(v["tools"][0]["function"]["name"], "get_weather");
    }

    #[test]
    fn sends_reasoning_effort_as_is() {
        let mut req = request(false);
        let v: serde_json::Value =
            serde_json::from_slice(&build_request(&target(), &req).unwrap().body).unwrap();
        assert!(v.get("reasoning_effort").is_none());
        req.reasoning_effort = Some("low".into());
        let v: serde_json::Value =
            serde_json::from_slice(&build_request(&target(), &req).unwrap().body).unwrap();
        assert_eq!(v["reasoning_effort"], "low");
    }

    #[test]
    fn sends_response_format_as_is() {
        use crate::types::ResponseFormat;
        let mut req = request(false);
        let v: serde_json::Value =
            serde_json::from_slice(&build_request(&target(), &req).unwrap().body).unwrap();
        assert!(v.get("response_format").is_none());
        req.response_format = Some(ResponseFormat::Text);
        let v: serde_json::Value =
            serde_json::from_slice(&build_request(&target(), &req).unwrap().body).unwrap();
        assert_eq!(v["response_format"], serde_json::json!({"type":"text"}));
        req.response_format = Some(ResponseFormat::JsonObject);
        let v: serde_json::Value =
            serde_json::from_slice(&build_request(&target(), &req).unwrap().body).unwrap();
        assert_eq!(
            v["response_format"],
            serde_json::json!({"type":"json_object"})
        );
        req.response_format = Some(ResponseFormat::JsonSchema {
            name: "person".into(),
            schema: serde_json::json!({"type":"object","properties":{"a":{"type":"string"}},"required":["a"],"additionalProperties":false}),
            strict: Some(true),
            description: Some("a person".into()),
        });
        let v: serde_json::Value =
            serde_json::from_slice(&build_request(&target(), &req).unwrap().body).unwrap();
        assert_eq!(
            v["response_format"],
            serde_json::json!({"type":"json_schema","json_schema":{
                "name":"person","description":"a person","strict":true,
                "schema":{"type":"object","properties":{"a":{"type":"string"}},"required":["a"],"additionalProperties":false}}})
        );
    }
}
