use serde_json::{json, Value};

use super::{saturate, HttpRequest, StreamState, Target};
use crate::error::TranslateError;
use crate::sse::SseEvent;
use crate::types::{
    ChatRequest, ChatResponse, FinishReason, ImageSource, Message, Part, Role, StreamEvent, Tool,
    ToolCall, ToolChoice, Usage,
};

const API_VERSION: &str = "2023-06-01";
/// Anthropic requires max_tokens. Used when the caller sets none.
const DEFAULT_MAX_TOKENS: u32 = 4096;

fn finish(s: &str) -> Option<FinishReason> {
    match s {
        "end_turn" | "stop_sequence" => Some(FinishReason::Stop),
        "max_tokens" => Some(FinishReason::Length),
        "tool_use" => Some(FinishReason::ToolCalls),
        "refusal" => Some(FinishReason::ContentFilter),
        _ => None,
    }
}

/// Blocks that carry text or can be left out without changing the answer.
fn is_supported_block(kind: Option<&str>) -> bool {
    matches!(
        kind,
        Some("text" | "thinking" | "redacted_thinking" | "tool_use")
    )
}

fn unsupported_block(kind: Option<&str>) -> TranslateError {
    TranslateError::Unsupported(format!(
        "response content block '{}' is not supported yet",
        kind.unwrap_or("unknown")
    ))
}

fn invalid(m: &str) -> TranslateError {
    TranslateError::InvalidRequest(m.to_string())
}

fn image_block(src: &ImageSource) -> Value {
    match src {
        ImageSource::Url(u) => json!({"type": "image", "source": {"type": "url", "url": u}}),
        ImageSource::Base64 { media_type, data } => json!({
            "type": "image",
            "source": {"type": "base64", "media_type": media_type, "data": data}
        }),
    }
}

/// Text parts and images of a user message as blocks. Empty text is left out.
fn part_blocks(m: &Message) -> Vec<Value> {
    m.content
        .iter()
        .filter_map(|p| match p {
            Part::Text(t) if t.is_empty() => None,
            Part::Text(t) => Some(json!({"type": "text", "text": t})),
            Part::Image(i) => Some(image_block(i)),
        })
        .collect()
}

fn tool_use_block(c: &ToolCall) -> Result<Value, TranslateError> {
    let input: Value = if c.arguments.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(&c.arguments)
            .map_err(|_| invalid("tool call arguments are not valid JSON"))?
    };
    if !input.is_object() {
        return Err(invalid("tool call arguments must be a JSON object"));
    }
    Ok(json!({"type": "tool_use", "id": c.id, "name": c.name, "input": input}))
}

fn tool_value(t: &Tool) -> Value {
    let mut o = json!({"name": t.name, "input_schema": t.parameters});
    if let Some(d) = &t.description {
        o["description"] = json!(d);
    }
    o
}

fn tool_choice_value(req: &ChatRequest) -> Option<Value> {
    let disable = req.parallel_tool_calls == Some(false);
    let mut v = match &req.tool_choice {
        Some(ToolChoice::Auto) => json!({"type": "auto"}),
        Some(ToolChoice::Required) => json!({"type": "any"}),
        Some(ToolChoice::None) => return Some(json!({"type": "none"})),
        Some(ToolChoice::Tool(n)) => json!({"type": "tool", "name": n}),
        None if disable => json!({"type": "auto"}),
        None => return None,
    };
    if disable {
        v["disable_parallel_tool_use"] = json!(true);
    }
    Some(v)
}

/// The `messages` array: tool messages become `tool_result` blocks of one user
/// message, joined with an immediately following user message.
fn messages_value(req: &ChatRequest) -> Result<Vec<Value>, TranslateError> {
    let mut out: Vec<Value> = Vec::new();
    let mut results: Vec<Value> = Vec::new();
    for m in req.messages.iter().filter(|m| m.role != Role::System) {
        match m.role {
            Role::Tool => {
                if m.has_images() {
                    return Err(invalid("images are not allowed in a tool message"));
                }
                let id = m
                    .tool_call_id
                    .as_ref()
                    .ok_or_else(|| invalid("a tool message needs a tool_call_id"))?;
                results.push(
                    json!({"type": "tool_result", "tool_use_id": id, "content": m.joined_text()}),
                );
            }
            Role::Assistant => {
                if !results.is_empty() {
                    out.push(json!({"role": "user", "content": std::mem::take(&mut results)}));
                }
                if m.has_images() {
                    return Err(invalid("images are only allowed in user messages"));
                }
                if m.tool_calls.is_empty() {
                    out.push(json!({"role": "assistant", "content": m.joined_text()}));
                } else {
                    let mut blocks = Vec::new();
                    let text = m.joined_text();
                    if !text.is_empty() {
                        blocks.push(json!({"type": "text", "text": text}));
                    }
                    for c in &m.tool_calls {
                        blocks.push(tool_use_block(c)?);
                    }
                    out.push(json!({"role": "assistant", "content": blocks}));
                }
            }
            _ => {
                if results.is_empty() && !m.has_images() {
                    out.push(json!({"role": "user", "content": m.joined_text()}));
                } else {
                    let mut blocks = std::mem::take(&mut results);
                    blocks.extend(part_blocks(m));
                    out.push(json!({"role": "user", "content": blocks}));
                }
            }
        }
    }
    if !results.is_empty() {
        out.push(json!({"role": "user", "content": results}));
    }
    Ok(out)
}

pub(crate) fn build(target: &Target, req: &ChatRequest) -> Result<HttpRequest, TranslateError> {
    super::check_tool_choice(req)?;
    if req.messages.iter().any(|m| m.name.is_some()) {
        return Err(TranslateError::Unsupported(
            "message field 'name' is not supported by this provider".into(),
        ));
    }
    if req
        .messages
        .iter()
        .any(|m| m.role == Role::System && m.has_images())
    {
        return Err(invalid("images are only allowed in user messages"));
    }
    let system: Vec<String> = req
        .messages
        .iter()
        .filter(|m| m.role == Role::System)
        .map(|m| m.joined_text())
        .collect();
    let messages = messages_value(req)?;
    if messages.is_empty() {
        return Err(TranslateError::InvalidRequest(
            "at least one user or assistant message is required".into(),
        ));
    }
    let mut body = json!({
        "model": target.model,
        "messages": messages,
        "max_tokens": req.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
    });
    if !system.is_empty() {
        body["system"] = json!(system.join("\n\n"));
    }
    if let Some(v) = req.temperature {
        body["temperature"] = json!(v);
    }
    if let Some(v) = req.top_p {
        body["top_p"] = json!(v);
    }
    if let Some(v) = &req.stop {
        body["stop_sequences"] = json!(v);
    }
    if !req.tools.is_empty() {
        body["tools"] = req.tools.iter().map(tool_value).collect();
        if let Some(c) = tool_choice_value(req) {
            body["tool_choice"] = c;
        }
    }
    if req.stream {
        body["stream"] = json!(true);
    }
    let mut headers = vec![
        ("content-type".to_string(), "application/json".to_string()),
        ("anthropic-version".to_string(), API_VERSION.to_string()),
    ];
    if let Some(k) = &target.api_key {
        headers.push(("x-api-key".to_string(), k.clone()));
    }
    Ok(HttpRequest {
        method: "POST",
        url: format!("{}/v1/messages", target.base_url.trim_end_matches('/')),
        headers,
        body: serde_json::to_vec(&body)
            .map_err(|e| TranslateError::InvalidRequest(e.to_string()))?,
    })
}

pub(crate) fn parse(body: &[u8]) -> Result<ChatResponse, TranslateError> {
    let v: Value =
        serde_json::from_slice(body).map_err(|e| TranslateError::Malformed(e.to_string()))?;
    let blocks = v["content"]
        .as_array()
        .ok_or_else(|| TranslateError::Malformed("response has no content".into()))?;
    let mut content = String::new();
    let mut tool_calls = Vec::new();
    for b in blocks {
        match b["type"].as_str() {
            Some("text") => content.push_str(b["text"].as_str().unwrap_or("")),
            Some("tool_use") => {
                let (Some(id), Some(name)) = (b["id"].as_str(), b["name"].as_str()) else {
                    return Err(TranslateError::Malformed(
                        "tool_use block has no id or name".into(),
                    ));
                };
                tool_calls.push(ToolCall {
                    id: id.to_string(),
                    name: name.to_string(),
                    arguments: serde_json::to_string(&b["input"])
                        .map_err(|e| TranslateError::Malformed(e.to_string()))?,
                });
            }
            kind if is_supported_block(kind) => {}
            kind => return Err(unsupported_block(kind)),
        }
    }
    let usage = v.get("usage").filter(|u| u.is_object()).map(|u| Usage {
        input_tokens: saturate(u["input_tokens"].as_u64().unwrap_or(0)),
        output_tokens: saturate(u["output_tokens"].as_u64().unwrap_or(0)),
    });
    Ok(ChatResponse {
        id: v["id"].as_str().unwrap_or_default().to_string(),
        model: v["model"].as_str().unwrap_or_default().to_string(),
        content,
        tool_calls,
        finish_reason: v["stop_reason"].as_str().and_then(finish),
        usage,
    })
}

pub(crate) fn decode(
    state: &mut StreamState,
    ev: &SseEvent,
    out: &mut Vec<StreamEvent>,
) -> Result<(), TranslateError> {
    let v: Value =
        serde_json::from_str(&ev.data).map_err(|e| TranslateError::Malformed(e.to_string()))?;
    match v["type"].as_str() {
        Some("message_start") => {
            let u = &v["message"]["usage"];
            state.input_tokens = u["input_tokens"].as_u64().map(saturate);
            state.output_tokens = u["output_tokens"].as_u64().map(saturate);
        }
        Some("content_block_start") => {
            let kind = v["content_block"]["type"].as_str();
            if !is_supported_block(kind) {
                return Err(unsupported_block(kind));
            }
            if kind == Some("tool_use") {
                let block = &v["content_block"];
                let (Some(at), Some(id), Some(name)) = (
                    v["index"].as_u64(),
                    block["id"].as_str(),
                    block["name"].as_str(),
                ) else {
                    return Err(TranslateError::Malformed(
                        "tool_use block has no index, id or name".into(),
                    ));
                };
                let index = u32::try_from(state.tool_blocks.len())
                    .map_err(|_| TranslateError::Malformed("too many tool calls".into()))?;
                state.tool_blocks.push((at, index));
                // The start block's `input` is always `{}`; the arguments follow as deltas.
                out.push(StreamEvent::ToolCallStart {
                    index,
                    id: id.to_string(),
                    name: name.to_string(),
                });
            }
        }
        Some("content_block_delta") => {
            if v["delta"]["type"] == "input_json_delta" {
                let part = v["delta"]["partial_json"].as_str().unwrap_or("");
                let index = v["index"].as_u64().and_then(|at| {
                    state
                        .tool_blocks
                        .iter()
                        .find(|(b, _)| *b == at)
                        .map(|(_, i)| *i)
                });
                let Some(index) = index else {
                    return Err(TranslateError::Malformed(
                        "tool call delta before its start".into(),
                    ));
                };
                if !part.is_empty() {
                    out.push(StreamEvent::ToolCallDelta {
                        index,
                        arguments: part.to_string(),
                    });
                }
            }
            if v["delta"]["type"] == "text_delta" {
                if let Some(t) = v["delta"]["text"].as_str() {
                    if !t.is_empty() {
                        out.push(StreamEvent::Delta {
                            text: t.to_string(),
                        });
                    }
                }
            }
        }
        Some("message_delta") => {
            if let Some(s) = v["delta"]["stop_reason"].as_str() {
                state.finish = finish(s);
            }
            if let Some(n) = v["usage"]["output_tokens"].as_u64() {
                state.output_tokens = Some(saturate(n));
            }
        }
        Some("message_stop") => {
            out.push(StreamEvent::Done {
                finish_reason: state.finish,
                usage: state.usage(),
            });
        }
        Some("error") => {
            let kind = v["error"]["type"].as_str().unwrap_or("");
            // Reported as 401 so it is handled like a rejected credential.
            let credential = kind == "authentication_error" || kind == "permission_error";
            return Err(TranslateError::Provider {
                status: if credential { 401 } else { 502 },
                retryable: kind == "overloaded_error" || kind == "api_error",
                message: v["error"]["message"]
                    .as_str()
                    .unwrap_or("provider error")
                    .to_string(),
            });
        }
        _ => {}
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
            kind: ProviderKind::Anthropic,
            base_url: "https://api.anthropic.com".into(),
            api_key: Some("sk-ant".into()),
            model: "claude-sonnet-5".into(),
            api_version: None,
        }
    }

    fn msg(role: Role, content: &str) -> Message {
        Message::text(role, content)
    }

    fn request(messages: Vec<Message>) -> ChatRequest {
        ChatRequest {
            model: "anthropic/claude-sonnet-5".into(),
            messages,
            max_tokens: None,
            temperature: Some(0.5),
            top_p: None,
            stop: Some(vec!["END".into()]),
            stream: false,
            tools: Vec::new(),
            tool_choice: None,
            parallel_tool_calls: None,
        }
    }

    #[test]
    fn builds_request_with_system_field_and_default_max_tokens() {
        let req = request(vec![
            msg(Role::System, "be brief"),
            msg(Role::Assistant, "earlier"),
            msg(Role::System, "be kind"),
            msg(Role::User, "hi"),
        ]);
        let r = build_request(&target(), &req).unwrap();
        assert_eq!(r.url, "https://api.anthropic.com/v1/messages");
        assert!(r.headers.contains(&("x-api-key".into(), "sk-ant".into())));
        assert!(r
            .headers
            .contains(&("anthropic-version".into(), "2023-06-01".into())));
        let v: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(v["model"], "claude-sonnet-5");
        assert_eq!(v["system"], "be brief\n\nbe kind");
        assert_eq!(v["max_tokens"], 4096);
        assert_eq!(v["stop_sequences"][0], "END");
        assert_eq!(v["messages"].as_array().unwrap().len(), 2);
        assert_eq!(v["messages"][0]["role"], "assistant");
        assert_eq!(v["messages"][1]["role"], "user");
    }

    #[test]
    fn request_with_only_system_messages_is_invalid() {
        let e = build_request(&target(), &request(vec![msg(Role::System, "x")])).unwrap_err();
        assert!(matches!(e, TranslateError::InvalidRequest(_)));
    }

    #[test]
    fn parses_response_joining_text_blocks() {
        let body = br#"{"id":"m1","model":"claude-sonnet-5","content":[{"type":"text","text":"a"},{"type":"text","text":"b"}],"stop_reason":"max_tokens","usage":{"input_tokens":7,"output_tokens":3}}"#;
        let r = parse_response(ProviderKind::Anthropic, 200, body).unwrap();
        assert_eq!(r.content, "ab");
        assert_eq!(r.finish_reason, Some(FinishReason::Length));
        assert_eq!(
            r.usage,
            Some(Usage {
                input_tokens: 7,
                output_tokens: 3
            })
        );
    }

    #[test]
    fn error_body_keeps_message() {
        let body =
            br#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#;
        let e = parse_response(ProviderKind::Anthropic, 529, body).unwrap_err();
        assert_eq!(
            e,
            TranslateError::Provider {
                status: 529,
                retryable: true,
                message: "Overloaded".into()
            }
        );
    }

    const STREAM: &str = concat!(
        "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"m1\",\"model\":\"claude-sonnet-5\",\"usage\":{\"input_tokens\":9,\"output_tokens\":1}}}\n\n",
        "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: ping\ndata: {\"type\":\"ping\"}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"h\u{e9}\"}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"llo\"}}\n\n",
        "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":12}}\n\n",
        "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
    );

    fn expected() -> Vec<StreamEvent> {
        vec![
            StreamEvent::Delta {
                text: "h\u{e9}".into(),
            },
            StreamEvent::Delta { text: "llo".into() },
            StreamEvent::Done {
                finish_reason: Some(FinishReason::Stop),
                usage: Some(Usage {
                    input_tokens: 9,
                    output_tokens: 12,
                }),
            },
        ]
    }

    #[test]
    fn decodes_stream_identically_at_every_split_point() {
        let bytes = STREAM.as_bytes();
        for i in 0..=bytes.len() {
            let mut d = StreamDecoder::new(ProviderKind::Anthropic);
            let mut got = d.feed(&bytes[..i]).unwrap();
            got.extend(d.feed(&bytes[i..]).unwrap());
            assert_eq!(got, expected(), "split at byte {i}");
        }
    }

    #[test]
    fn error_event_inside_stream_is_reported() {
        let mut d = StreamDecoder::new(ProviderKind::Anthropic);
        let e = d
            .feed(b"event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n")
            .unwrap_err();
        assert!(
            matches!(e, TranslateError::Provider { message, retryable: true, .. } if message == "Overloaded")
        );
    }

    #[test]
    fn response_token_counts_saturate_instead_of_truncating() {
        let body = br#"{"id":"m1","model":"m","content":[{"type":"text","text":"a"}],"stop_reason":"end_turn","usage":{"input_tokens":4294967301,"output_tokens":3}}"#;
        let r = parse_response(ProviderKind::Anthropic, 200, body).unwrap();
        assert_eq!(
            r.usage,
            Some(Usage {
                input_tokens: u32::MAX,
                output_tokens: 3
            })
        );
    }

    #[test]
    fn stream_token_counts_saturate_instead_of_truncating() {
        let mut d = StreamDecoder::new(ProviderKind::Anthropic);
        let got = d
            .feed(
                concat!(
                    "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":4294967301,\"output_tokens\":1}}}\n\n",
                    "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":4294967302}}\n\n",
                    "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
                )
                .as_bytes(),
            )
            .unwrap();
        assert_eq!(
            got,
            vec![StreamEvent::Done {
                finish_reason: Some(FinishReason::Stop),
                usage: Some(Usage {
                    input_tokens: u32::MAX,
                    output_tokens: u32::MAX
                }),
            }]
        );
    }

    fn tool(name: &str) -> Tool {
        Tool {
            name: name.into(),
            description: Some("d".into()),
            parameters: serde_json::json!({"type": "object"}),
        }
    }

    fn call(id: &str, args: &str) -> ToolCall {
        ToolCall {
            id: id.into(),
            name: "get_weather".into(),
            arguments: args.into(),
        }
    }

    fn body_of(req: &ChatRequest) -> serde_json::Value {
        serde_json::from_slice(&build_request(&target(), req).unwrap().body).unwrap()
    }

    fn tool_message(id: &str, text: &str) -> Message {
        Message {
            tool_call_id: Some(id.into()),
            ..msg(Role::Tool, text)
        }
    }

    #[test]
    fn builds_tool_use_and_tool_result_blocks() {
        let mut req = request(vec![
            msg(Role::User, "weather in Paris and Rome?"),
            Message {
                tool_calls: vec![
                    call("t1", "{\"city\":\"Paris\"}"),
                    call("t2", "{\"city\":\"Rome\"}"),
                ],
                ..msg(Role::Assistant, "Checking.")
            },
            tool_message("t1", "sunny"),
            tool_message("t2", "rain"),
            msg(Role::User, "thanks"),
        ]);
        req.tools = vec![tool("get_weather")];
        let v = body_of(&req);
        assert_eq!(
            v["messages"],
            serde_json::json!([
                {"role": "user", "content": "weather in Paris and Rome?"},
                {"role": "assistant", "content": [
                    {"type": "text", "text": "Checking."},
                    {"type": "tool_use", "id": "t1", "name": "get_weather", "input": {"city": "Paris"}},
                    {"type": "tool_use", "id": "t2", "name": "get_weather", "input": {"city": "Rome"}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "t1", "content": "sunny"},
                    {"type": "tool_result", "tool_use_id": "t2", "content": "rain"},
                    {"type": "text", "text": "thanks"}
                ]}
            ])
        );
        assert_eq!(
            v["tools"],
            serde_json::json!([{"name": "get_weather", "description": "d", "input_schema": {"type": "object"}}])
        );
    }

    #[test]
    fn tool_results_alone_and_assistant_without_text() {
        let mut req = request(vec![
            msg(Role::User, "q"),
            Message {
                tool_calls: vec![call("t1", "")],
                ..msg(Role::Assistant, "")
            },
            tool_message("t1", "ok"),
        ]);
        req.tools = vec![tool("get_weather")];
        let v = body_of(&req);
        assert_eq!(
            v["messages"][1]["content"],
            serde_json::json!([{"type": "tool_use", "id": "t1", "name": "get_weather", "input": {}}])
        );
        assert_eq!(
            v["messages"][2],
            serde_json::json!({"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "t1", "content": "ok"}]})
        );
        assert_eq!(v["messages"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn builds_image_blocks_both_sources() {
        let m = Message {
            content: vec![
                Part::Text("what is this?".into()),
                Part::Image(ImageSource::Url("https://x.test/a.png".into())),
                Part::Image(ImageSource::Base64 {
                    media_type: "image/jpeg".into(),
                    data: "QUJD".into(),
                }),
            ],
            ..msg(Role::User, "")
        };
        let v = body_of(&request(vec![m]));
        assert_eq!(
            v["messages"][0]["content"],
            serde_json::json!([
                {"type": "text", "text": "what is this?"},
                {"type": "image", "source": {"type": "url", "url": "https://x.test/a.png"}},
                {"type": "image", "source": {"type": "base64", "media_type": "image/jpeg", "data": "QUJD"}}
            ])
        );
    }

    #[test]
    fn invalid_tool_arguments_are_invalid_request() {
        for bad in ["{not json", "[1]", "null", "\"s\""] {
            let req = request(vec![
                msg(Role::User, "q"),
                Message {
                    tool_calls: vec![call("t1", bad)],
                    ..msg(Role::Assistant, "")
                },
            ]);
            let e = build_request(&target(), &req).unwrap_err();
            assert!(
                matches!(e, TranslateError::InvalidRequest(_)),
                "{bad}: {e:?}"
            );
        }
    }

    #[test]
    fn tool_choice_and_disable_parallel() {
        let cases = [
            (
                Some(ToolChoice::Auto),
                None,
                Some(serde_json::json!({"type": "auto"})),
            ),
            (
                Some(ToolChoice::Required),
                None,
                Some(serde_json::json!({"type": "any"})),
            ),
            (
                Some(ToolChoice::None),
                None,
                Some(serde_json::json!({"type": "none"})),
            ),
            (
                Some(ToolChoice::Tool("f".into())),
                None,
                Some(serde_json::json!({"type": "tool", "name": "f"})),
            ),
            (None, None, None),
            (None, Some(true), None),
            (
                None,
                Some(false),
                Some(serde_json::json!({"type": "auto", "disable_parallel_tool_use": true})),
            ),
            (
                Some(ToolChoice::Required),
                Some(false),
                Some(serde_json::json!({"type": "any", "disable_parallel_tool_use": true})),
            ),
            (
                Some(ToolChoice::Tool("f".into())),
                Some(false),
                Some(
                    serde_json::json!({"type": "tool", "name": "f", "disable_parallel_tool_use": true}),
                ),
            ),
        ];
        for (choice, parallel, want) in cases {
            let mut req = request(vec![msg(Role::User, "hi")]);
            req.tools = vec![tool("f")];
            req.tool_choice = choice.clone();
            req.parallel_tool_calls = parallel;
            let v = body_of(&req);
            match want {
                Some(w) => assert_eq!(v["tool_choice"], w, "{choice:?} {parallel:?}"),
                None => assert!(v.get("tool_choice").is_none(), "{choice:?} {parallel:?}"),
            }
        }
    }

    #[test]
    fn tool_options_without_tools_are_ignored_or_refused() {
        let mut req = request(vec![msg(Role::User, "hi")]);
        req.tool_choice = Some(ToolChoice::Auto);
        req.parallel_tool_calls = Some(false);
        let v = body_of(&req);
        assert!(v.get("tool_choice").is_none() && v.get("tools").is_none());
        req.tool_choice = Some(ToolChoice::Required);
        assert_eq!(
            build_request(&target(), &req).unwrap_err(),
            TranslateError::InvalidRequest("tool_choice 'required' needs tools".into())
        );
    }

    #[test]
    fn text_only_body_is_unchanged() {
        let mut req = request(vec![msg(Role::System, "s"), msg(Role::User, "hi")]);
        req.stream = true;
        req.max_tokens = Some(10);
        let r = build_request(&target(), &req).unwrap();
        assert_eq!(
            String::from_utf8(r.body).unwrap(),
            r#"{"max_tokens":10,"messages":[{"content":"hi","role":"user"}],"model":"claude-sonnet-5","stop_sequences":["END"],"stream":true,"system":"s","temperature":0.5}"#
        );
    }

    #[test]
    fn parses_tool_use_response() {
        let body = br#"{"id":"m1","model":"m","content":[{"type":"text","text":"a"},{"type":"tool_use","id":"t","name":"f","input":{"x":1}}],"stop_reason":"tool_use","usage":{"input_tokens":1,"output_tokens":1}}"#;
        let r = parse_response(ProviderKind::Anthropic, 200, body).unwrap();
        assert_eq!(r.content, "a");
        assert_eq!(
            r.tool_calls,
            vec![ToolCall {
                id: "t".into(),
                name: "f".into(),
                arguments: "{\"x\":1}".into()
            }]
        );
        assert_eq!(r.finish_reason, Some(FinishReason::ToolCalls));
    }

    #[test]
    fn server_tool_blocks_stay_unsupported() {
        for kind in ["server_tool_use", "web_search_tool_result"] {
            let body = format!(
                r#"{{"id":"m1","model":"m","content":[{{"type":"{kind}"}}],"stop_reason":"end_turn"}}"#
            );
            let e = parse_response(ProviderKind::Anthropic, 200, body.as_bytes()).unwrap_err();
            assert!(matches!(e, TranslateError::Unsupported(_)), "{kind}");
        }
    }

    #[test]
    fn decodes_streamed_tool_use() {
        let input = include_bytes!("../../tests/fixtures/anthropic/stream_tool_use.txt");
        let start = |index, id: &str, name: &str| StreamEvent::ToolCallStart {
            index,
            id: id.into(),
            name: name.into(),
        };
        let delta = |index, a: &str| StreamEvent::ToolCallDelta {
            index,
            arguments: a.into(),
        };
        let want = vec![
            StreamEvent::Delta {
                text: "Let me check.".into(),
            },
            start(0, "toolu_a", "get_weather"),
            delta(0, "{\"city\":"),
            delta(0, "\"Paris\"}"),
            start(1, "toolu_b", "get_time"),
            delta(1, "{\"tz\":\"UTC\"}"),
            StreamEvent::Done {
                finish_reason: Some(FinishReason::ToolCalls),
                usage: Some(Usage {
                    input_tokens: 20,
                    output_tokens: 30,
                }),
            },
        ];
        for split in 0..=input.len() {
            let mut d = StreamDecoder::new(ProviderKind::Anthropic);
            let mut got = d.feed(&input[..split]).unwrap();
            got.extend(d.feed(&input[split..]).unwrap());
            assert_eq!(got, want, "split {split}");
        }
    }

    #[test]
    fn tool_delta_for_unknown_block_is_malformed() {
        let mut d = StreamDecoder::new(ProviderKind::Anthropic);
        let e = d
            .feed(b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":3,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{}\"}}\n\n")
            .unwrap_err();
        assert!(matches!(e, TranslateError::Malformed(_)));
    }

    #[test]
    fn openai_shaped_tool_conversation_round_trips_to_anthropic() {
        let body = br#"{"model":"m","messages":[
            {"role":"system","content":"be brief"},
            {"role":"user","content":[{"type":"text","text":"weather?"},
                {"type":"image_url","image_url":{"url":"data:image/png;base64,QUJD"}}]},
            {"role":"assistant","content":null,"tool_calls":[
                {"id":"call_1","type":"function","function":{"name":"get_weather","arguments":"{\"city\":\"Paris\"}"}}]},
            {"role":"tool","tool_call_id":"call_1","content":"sunny"},
            {"role":"user","content":"and tomorrow?"}
        ],"tools":[{"type":"function","function":{"name":"get_weather","description":"d","parameters":{"type":"object","properties":{"city":{"type":"string"}}}}}],
        "tool_choice":"required","parallel_tool_calls":false}"#;
        let req = crate::ingress::openai::parse_request(body).unwrap();
        let v = body_of(&req);
        assert_eq!(v["system"], "be brief");
        assert_eq!(
            v["messages"][0]["content"][1]["source"]["media_type"],
            "image/png"
        );
        assert_eq!(
            v["messages"][1]["content"],
            serde_json::json!([{"type": "tool_use", "id": "call_1", "name": "get_weather", "input": {"city": "Paris"}}])
        );
        assert_eq!(
            v["messages"][2]["content"],
            serde_json::json!([
                {"type": "tool_result", "tool_use_id": "call_1", "content": "sunny"},
                {"type": "text", "text": "and tomorrow?"}
            ])
        );
        assert_eq!(v["messages"].as_array().unwrap().len(), 3);
        assert_eq!(
            v["tool_choice"],
            serde_json::json!({"type": "any", "disable_parallel_tool_use": true})
        );
        assert_eq!(
            v["tools"][0]["input_schema"]["properties"]["city"]["type"],
            "string"
        );
    }

    #[test]
    fn message_name_is_unsupported() {
        let mut m = msg(Role::User, "hi");
        m.name = Some("bob".into());
        let e = build_request(&target(), &request(vec![m])).unwrap_err();
        assert_eq!(
            e,
            TranslateError::Unsupported(
                "message field 'name' is not supported by this provider".into()
            )
        );
    }

    fn stream_error_status(kind: &str) -> u16 {
        let mut d = StreamDecoder::new(ProviderKind::Anthropic);
        let input = format!(
            "event: error\ndata: {{\"type\":\"error\",\"error\":{{\"type\":\"{kind}\",\"message\":\"invalid x-api-key sk-ant-abc\"}}}}\n\n"
        );
        match d.feed(input.as_bytes()).unwrap_err() {
            TranslateError::Provider { status, .. } => status,
            other => panic!("expected a provider error, got {other:?}"),
        }
    }

    #[test]
    fn credential_errors_inside_stream_have_status_401() {
        assert_eq!(stream_error_status("authentication_error"), 401);
        assert_eq!(stream_error_status("permission_error"), 401);
    }

    #[test]
    fn other_errors_inside_stream_keep_status_502() {
        assert_eq!(stream_error_status("overloaded_error"), 502);
        assert_eq!(stream_error_status("api_error"), 502);
        assert_eq!(stream_error_status("invalid_request_error"), 502);
    }
}
