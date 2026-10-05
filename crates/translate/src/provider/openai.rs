use serde::Deserialize;
use serde_json::{json, Value};

use super::{saturate, HttpRequest, StreamState, Target};
use crate::error::TranslateError;
use crate::sse::SseEvent;
use crate::types::{ChatRequest, ChatResponse, FinishReason, Role, StreamEvent, Usage};

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

/// True when a `tool_calls` / `function_call` value carries something.
fn has_tool_call(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Array(a) => !a.is_empty(),
        _ => true,
    }
}

fn tool_calls_unsupported() -> TranslateError {
    TranslateError::Unsupported("provider response contains tool calls".into())
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
    super::reject_tools_and_images(req)?;
    let messages: Vec<Value> = req
        .messages
        .iter()
        .map(|m| {
            let mut o = json!({ "role": role_str(m.role), "content": m.joined_text() });
            if let Some(n) = &m.name {
                o["name"] = json!(n);
            }
            o
        })
        .collect();
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
    if has_tool_call(&message.tool_calls) || has_tool_call(&message.function_call) {
        return Err(tool_calls_unsupported());
    }
    let mut content = message.content.unwrap_or_default();
    let mut finish_reason = choice.finish_reason.as_deref().and_then(finish);
    if let Some(refusal) = message.refusal {
        content.push_str(&refusal);
        finish_reason = Some(FinishReason::ContentFilter);
    }
    Ok(ChatResponse {
        id: w.id,
        model: w.model,
        content,
        tool_calls: Vec::new(),
        finish_reason,
        usage: w.usage.map(|u| Usage {
            input_tokens: saturate(u.prompt_tokens),
            output_tokens: saturate(u.completion_tokens),
        }),
    })
}

pub(crate) fn decode(
    state: &mut StreamState,
    ev: &SseEvent,
    out: &mut Vec<StreamEvent>,
) -> Result<(), TranslateError> {
    if ev.data == "[DONE]" {
        out.push(StreamEvent::Done {
            finish_reason: state.finish,
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
    if has_tool_call(&delta["tool_calls"]) || has_tool_call(&delta["function_call"]) {
        return Err(tool_calls_unsupported());
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
    fn response_with_tool_calls_is_unsupported() {
        let body = br#"{"id":"c1","model":"m","choices":[{"message":{"content":null,"tool_calls":[{"id":"t","type":"function","function":{"name":"f","arguments":"{}"}}]},"finish_reason":"tool_calls"}]}"#;
        let e = parse_response(ProviderKind::OpenAi, 200, body).unwrap_err();
        assert!(matches!(e, TranslateError::Unsupported(_)));
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
    fn stream_tool_calls_are_unsupported() {
        let mut d = StreamDecoder::new(ProviderKind::OpenAi);
        let e = d
            .feed(b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"name\":\"f\"}}]},\"finish_reason\":null}]}\n\n")
            .unwrap_err();
        assert!(matches!(e, TranslateError::Unsupported(_)));
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
}
