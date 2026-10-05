use serde_json::{json, Value};

use super::{saturate, HttpRequest, StreamState, Target};
use crate::error::TranslateError;
use crate::sse::SseEvent;
use crate::types::{ChatRequest, ChatResponse, FinishReason, Role, StreamEvent, Usage};

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
    matches!(kind, Some("text" | "thinking" | "redacted_thinking"))
}

fn unsupported_block(kind: Option<&str>) -> TranslateError {
    TranslateError::Unsupported(format!(
        "response content block '{}' is not supported yet",
        kind.unwrap_or("unknown")
    ))
}

pub(crate) fn build(target: &Target, req: &ChatRequest) -> Result<HttpRequest, TranslateError> {
    super::reject_tools_and_images(req)?;
    if req.messages.iter().any(|m| m.name.is_some()) {
        return Err(TranslateError::Unsupported(
            "message field 'name' is not supported by this provider".into(),
        ));
    }
    let system: Vec<String> = req
        .messages
        .iter()
        .filter(|m| m.role == Role::System)
        .map(|m| m.joined_text())
        .collect();
    let messages: Vec<Value> = req
        .messages
        .iter()
        .filter(|m| m.role != Role::System)
        .map(|m| {
            let role = if m.role == Role::Assistant {
                "assistant"
            } else {
                "user"
            };
            json!({ "role": role, "content": m.joined_text() })
        })
        .collect();
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
    for b in blocks {
        match b["type"].as_str() {
            Some("text") => content.push_str(b["text"].as_str().unwrap_or("")),
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
        tool_calls: Vec::new(),
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
        }
        Some("content_block_delta") => {
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

    #[test]
    fn response_with_tool_use_block_is_unsupported() {
        let body = br#"{"id":"m1","model":"m","content":[{"type":"text","text":"a"},{"type":"tool_use","id":"t","name":"f","input":{}}],"stop_reason":"tool_use","usage":{"input_tokens":1,"output_tokens":1}}"#;
        let e = parse_response(ProviderKind::Anthropic, 200, body).unwrap_err();
        assert!(matches!(e, TranslateError::Unsupported(_)));
    }

    #[test]
    fn stream_tool_use_block_is_unsupported() {
        let mut d = StreamDecoder::new(ProviderKind::Anthropic);
        let e = d
            .feed(b"event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"t\",\"name\":\"f\",\"input\":{}}}\n\n")
            .unwrap_err();
        assert!(matches!(e, TranslateError::Unsupported(_)));
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
