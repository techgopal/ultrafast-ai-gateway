//! Google Gemini (`generativelanguage`), the `generateContent` API.

use serde_json::{json, Value};

use super::{path_segment, saturate, HttpRequest, StreamState, Target};
use crate::error::TranslateError;
use crate::sse::SseEvent;
use crate::types::{ChatRequest, ChatResponse, FinishReason, Role, StreamEvent, Usage};

fn finish(s: &str) -> Option<FinishReason> {
    match s {
        "STOP" => Some(FinishReason::Stop),
        "MAX_TOKENS" => Some(FinishReason::Length),
        "SAFETY" | "RECITATION" | "BLOCKLIST" | "PROHIBITED_CONTENT" | "SPII" | "IMAGE_SAFETY" => {
            Some(FinishReason::ContentFilter)
        }
        _ => None,
    }
}

fn text_part(text: &str) -> Value {
    json!({ "parts": [{ "text": text }] })
}

pub(crate) fn build(target: &Target, req: &ChatRequest) -> Result<HttpRequest, TranslateError> {
    if req.messages.iter().any(|m| m.name.is_some()) {
        return Err(TranslateError::Unsupported(
            "message field 'name' is not supported by this provider".into(),
        ));
    }
    let system: Vec<&str> = req
        .messages
        .iter()
        .filter(|m| m.role == Role::System)
        .map(|m| m.content.as_str())
        .collect();
    let contents: Vec<Value> = req
        .messages
        .iter()
        .filter(|m| m.role != Role::System)
        .map(|m| {
            let role = if m.role == Role::Assistant {
                "model"
            } else {
                "user"
            };
            let mut c = text_part(&m.content);
            c["role"] = json!(role);
            c
        })
        .collect();
    if contents.is_empty() {
        return Err(TranslateError::InvalidRequest(
            "at least one user or assistant message is required".into(),
        ));
    }
    let mut body = json!({ "contents": contents });
    if !system.is_empty() {
        body["systemInstruction"] = text_part(&system.join("\n\n"));
    }
    let mut config = serde_json::Map::new();
    if let Some(v) = req.max_tokens {
        config.insert("maxOutputTokens".into(), json!(v));
    }
    if let Some(v) = req.temperature {
        config.insert("temperature".into(), json!(v));
    }
    if let Some(v) = req.top_p {
        config.insert("topP".into(), json!(v));
    }
    if let Some(v) = &req.stop {
        config.insert("stopSequences".into(), json!(v));
    }
    if !config.is_empty() {
        body["generationConfig"] = Value::Object(config);
    }
    let method = if req.stream {
        "streamGenerateContent?alt=sse"
    } else {
        "generateContent"
    };
    let mut headers = vec![("content-type".to_string(), "application/json".to_string())];
    if let Some(k) = &target.api_key {
        headers.push(("x-goog-api-key".to_string(), k.clone()));
    }
    Ok(HttpRequest {
        method: "POST",
        url: format!(
            "{}/v1beta/models/{}:{method}",
            target.base_url.trim_end_matches('/'),
            path_segment(&target.model)
        ),
        headers,
        body: serde_json::to_vec(&body)
            .map_err(|e| TranslateError::InvalidRequest(e.to_string()))?,
    })
}

/// Reads `usageMetadata` into the state, when the chunk has it.
fn read_usage(state: &mut StreamState, v: &Value) {
    if let Some(u) = v.get("usageMetadata").filter(|u| u.is_object()) {
        state.input_tokens = u["promptTokenCount"].as_u64().map(saturate);
        state.output_tokens = Some(saturate(u["candidatesTokenCount"].as_u64().unwrap_or(0)));
    }
}

/// The text of one answer or chunk, and why it ended, if it did.
/// A prompt that was refused ends as a content filter.
fn read_candidate(v: &Value) -> Result<(String, Option<FinishReason>), TranslateError> {
    let blocked = v["promptFeedback"]["blockReason"].is_string();
    let Some(candidate) = v["candidates"].get(0) else {
        if blocked {
            return Ok((String::new(), Some(FinishReason::ContentFilter)));
        }
        return Err(TranslateError::Malformed(
            "response has no candidates".into(),
        ));
    };
    let mut text = String::new();
    for part in candidate["content"]["parts"]
        .as_array()
        .into_iter()
        .flatten()
    {
        if let Some(t) = part["text"].as_str() {
            // Thoughts are the model's working, not its answer.
            if part["thought"] != true {
                text.push_str(t);
            }
        } else {
            return Err(TranslateError::Unsupported(
                "response part is not text".into(),
            ));
        }
    }
    let reason = candidate["finishReason"].as_str();
    Ok((text, reason.and_then(finish)))
}

pub(crate) fn parse(body: &[u8]) -> Result<ChatResponse, TranslateError> {
    let v: Value =
        serde_json::from_slice(body).map_err(|e| TranslateError::Malformed(e.to_string()))?;
    let (content, finish_reason) = read_candidate(&v)?;
    let usage = v
        .get("usageMetadata")
        .filter(|u| u.is_object())
        .map(|u| Usage {
            input_tokens: saturate(u["promptTokenCount"].as_u64().unwrap_or(0)),
            output_tokens: saturate(u["candidatesTokenCount"].as_u64().unwrap_or(0)),
        });
    Ok(ChatResponse {
        id: v["responseId"].as_str().unwrap_or_default().to_string(),
        model: v["modelVersion"].as_str().unwrap_or_default().to_string(),
        content,
        finish_reason,
        usage,
    })
}

/// Gemini ends a stream by closing it: the chunk that carries a finish reason
/// is the last, so it also ends the decoded stream.
pub(crate) fn decode(
    state: &mut StreamState,
    ev: &SseEvent,
    out: &mut Vec<StreamEvent>,
) -> Result<(), TranslateError> {
    let v: Value =
        serde_json::from_str(&ev.data).map_err(|e| TranslateError::Malformed(e.to_string()))?;
    if let Some(msg) = v["error"]["message"].as_str() {
        // Reported as 401 so it is handled like a rejected credential.
        let credential = matches!(v["error"]["code"].as_u64(), Some(401 | 403));
        return Err(TranslateError::Provider {
            status: if credential { 401 } else { 502 },
            retryable: false,
            message: msg.to_string(),
        });
    }
    read_usage(state, &v);
    // A chunk of usage alone carries nothing else.
    if v["candidates"].get(0).is_none() && !v["promptFeedback"]["blockReason"].is_string() {
        return Ok(());
    }
    let (text, finish_reason) = read_candidate(&v)?;
    if !text.is_empty() {
        out.push(StreamEvent::Delta { text });
    }
    // A candidate that ends for a reason GEMINI reports as OTHER still ends the stream.
    let ended = v["candidates"][0]["finishReason"].is_string()
        || v["promptFeedback"]["blockReason"].is_string();
    if ended {
        state.finish = finish_reason;
        out.push(StreamEvent::Done {
            finish_reason: state.finish,
            usage: state.usage(),
        });
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
            kind: ProviderKind::Gemini,
            base_url: "https://generativelanguage.googleapis.com/".into(),
            api_key: Some("g-key".into()),
            model: "gemini-2.0-flash".into(),
            api_version: None,
        }
    }

    fn msg(role: Role, content: &str) -> Message {
        Message {
            role,
            content: content.into(),
            name: None,
        }
    }

    fn request(stream: bool) -> ChatRequest {
        ChatRequest {
            model: "g/gemini-2.0-flash".into(),
            messages: vec![
                msg(Role::System, "be brief"),
                msg(Role::User, "hi"),
                msg(Role::Assistant, "hello"),
                msg(Role::System, "and kind"),
                msg(Role::User, "bye"),
            ],
            max_tokens: Some(5),
            temperature: Some(0.5),
            top_p: Some(0.9),
            stop: Some(vec!["x".into()]),
            stream,
        }
    }

    fn body_of(r: &HttpRequest) -> serde_json::Value {
        serde_json::from_slice(&r.body).unwrap()
    }

    #[test]
    fn builds_a_generate_content_request() {
        let r = build_request(&target(), &request(false)).unwrap();
        assert_eq!(r.method, "POST");
        assert_eq!(
            r.url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:generateContent"
        );
        assert!(r
            .headers
            .contains(&("x-goog-api-key".into(), "g-key".into())));
        assert!(!r.url.contains("g-key"));
        let v = body_of(&r);
        assert_eq!(
            v["systemInstruction"]["parts"][0]["text"],
            "be brief\n\nand kind"
        );
        let contents = v["contents"].as_array().unwrap();
        let roles: Vec<_> = contents
            .iter()
            .map(|c| c["role"].as_str().unwrap())
            .collect();
        assert_eq!(roles, ["user", "model", "user"]);
        assert_eq!(contents[1]["parts"][0]["text"], "hello");
        let g = &v["generationConfig"];
        assert_eq!(g["maxOutputTokens"], 5);
        assert_eq!(g["temperature"], 0.5);
        assert!((g["topP"].as_f64().unwrap() - 0.9).abs() < 1e-6);
        assert_eq!(g["stopSequences"][0], "x");
        assert!(v.get("model").is_none() && v.get("stream").is_none());
    }

    #[test]
    fn a_stream_uses_the_sse_endpoint() {
        let r = build_request(&target(), &request(true)).unwrap();
        assert_eq!(
            r.url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:streamGenerateContent?alt=sse"
        );
    }

    #[test]
    fn unset_options_leave_out_the_generation_config() {
        let mut req = request(false);
        req.max_tokens = None;
        req.temperature = None;
        req.top_p = None;
        req.stop = None;
        let v = body_of(&build_request(&target(), &req).unwrap());
        assert!(v.get("generationConfig").is_none(), "{v}");
        req.messages.remove(0);
        req.messages.remove(2);
        let v = body_of(&build_request(&target(), &req).unwrap());
        assert!(v.get("systemInstruction").is_none(), "{v}");
    }

    #[test]
    fn refuses_what_it_cannot_say() {
        let mut req = request(false);
        req.messages[1].name = Some("ann".into());
        assert!(matches!(
            build_request(&target(), &req),
            Err(TranslateError::Unsupported(_))
        ));
        req.messages = vec![msg(Role::System, "only")];
        assert!(matches!(
            build_request(&target(), &req),
            Err(TranslateError::InvalidRequest(_))
        ));
    }

    #[test]
    fn the_model_name_is_escaped() {
        let mut t = target();
        t.model = "a/b:c".into();
        let r = build_request(&t, &request(false)).unwrap();
        assert!(
            r.url.contains("/models/a%2Fb%3Ac:generateContent"),
            "{}",
            r.url
        );
    }

    #[test]
    fn parses_a_response() {
        let body = include_bytes!("../../tests/fixtures/gemini/response.json");
        let r = parse_response(ProviderKind::Gemini, 200, body).unwrap();
        assert_eq!(r.content, "Hello there");
        assert_eq!(r.id, "resp-1");
        assert_eq!(r.model, "gemini-2.0-flash");
        assert_eq!(r.finish_reason, Some(FinishReason::Stop));
        assert_eq!(
            r.usage,
            Some(Usage {
                input_tokens: 7,
                output_tokens: 3
            })
        );
    }

    #[test]
    fn maps_finish_reasons() {
        let body = include_bytes!("../../tests/fixtures/gemini/response_max_tokens.json");
        let r = parse_response(ProviderKind::Gemini, 200, body).unwrap();
        assert_eq!(r.finish_reason, Some(FinishReason::Length));
        let body = include_bytes!("../../tests/fixtures/gemini/response_safety.json");
        let r = parse_response(ProviderKind::Gemini, 200, body).unwrap();
        assert_eq!(r.finish_reason, Some(FinishReason::ContentFilter));
        assert_eq!(r.content, "");
        for (reason, want) in [
            ("RECITATION", Some(FinishReason::ContentFilter)),
            ("BLOCKLIST", Some(FinishReason::ContentFilter)),
            ("OTHER", None),
        ] {
            let body = format!(r#"{{"candidates":[{{"finishReason":"{reason}"}}]}}"#);
            let r = parse_response(ProviderKind::Gemini, 200, body.as_bytes()).unwrap();
            assert_eq!(r.finish_reason, want, "{reason}");
        }
    }

    #[test]
    fn a_blocked_prompt_is_a_content_filter_answer() {
        let body = include_bytes!("../../tests/fixtures/gemini/response_blocked_prompt.json");
        let r = parse_response(ProviderKind::Gemini, 200, body).unwrap();
        assert_eq!(r.content, "");
        assert_eq!(r.finish_reason, Some(FinishReason::ContentFilter));
    }

    #[test]
    fn function_calls_are_unsupported_and_garbage_is_malformed() {
        let body = include_bytes!("../../tests/fixtures/gemini/response_function_call.json");
        assert!(matches!(
            parse_response(ProviderKind::Gemini, 200, body),
            Err(TranslateError::Unsupported(_))
        ));
        for bad in [&b"not json"[..], b"{}", b"[]"] {
            assert!(
                matches!(
                    parse_response(ProviderKind::Gemini, 200, bad),
                    Err(TranslateError::Malformed(_))
                ),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn errors_use_the_common_rules() {
        let body = include_bytes!("../../tests/fixtures/gemini/error_429.json");
        let e = parse_response(ProviderKind::Gemini, 429, body).unwrap_err();
        assert!(
            matches!(&e, TranslateError::Provider { status: 429, retryable: true, message } if message.contains("exhausted")),
            "{e:?}"
        );
        let e = parse_response(ProviderKind::Gemini, 400, body).unwrap_err();
        assert!(matches!(
            e,
            TranslateError::Provider {
                retryable: false,
                ..
            }
        ));
    }

    fn expected_stream() -> Vec<StreamEvent> {
        vec![
            StreamEvent::Delta { text: "Hel".into() },
            StreamEvent::Delta { text: "lo".into() },
            StreamEvent::Done {
                finish_reason: Some(FinishReason::Stop),
                usage: Some(Usage {
                    input_tokens: 7,
                    output_tokens: 2,
                }),
            },
        ]
    }

    #[test]
    fn decodes_a_stream_at_every_split() {
        let input = include_bytes!("../../tests/fixtures/gemini/stream.sse");
        for split in 0..=input.len() {
            let mut d = StreamDecoder::new(ProviderKind::Gemini);
            let mut events = d.feed(&input[..split]).unwrap();
            events.extend(d.feed(&input[split..]).unwrap());
            assert_eq!(events, expected_stream(), "split {split}");
        }
    }

    #[test]
    fn a_stream_error_ends_the_stream() {
        let mut d = StreamDecoder::new(ProviderKind::Gemini);
        // The event is not complete until its blank line.
        let early = d
            .feed(
                br#"data: {"error":{"code":403,"message":"bad key","status":"PERMISSION_DENIED"}}"#,
            )
            .unwrap();
        assert_eq!(early, vec![]);
        let e = d.feed(b"\n\n").unwrap_err();
        assert!(
            matches!(
                e,
                TranslateError::Provider {
                    status: 401,
                    retryable: false,
                    ..
                }
            ),
            "{e:?}"
        );
    }

    #[test]
    fn a_blocked_prompt_in_a_stream_ends_it() {
        let mut d = StreamDecoder::new(ProviderKind::Gemini);
        let got = d
            .feed(b"data: {\"promptFeedback\":{\"blockReason\":\"SAFETY\"},\"usageMetadata\":{\"promptTokenCount\":3}}\n\n")
            .unwrap();
        assert_eq!(
            got,
            vec![StreamEvent::Done {
                finish_reason: Some(FinishReason::ContentFilter),
                usage: Some(Usage {
                    input_tokens: 3,
                    output_tokens: 0
                }),
            }]
        );
    }

    #[test]
    fn a_usage_only_chunk_is_skipped() {
        let mut d = StreamDecoder::new(ProviderKind::Gemini);
        let got = d
            .feed(b"data: {\"usageMetadata\":{\"promptTokenCount\":3}}\n\n")
            .unwrap();
        assert_eq!(got, vec![]);
    }

    #[test]
    fn function_calls_in_a_stream_are_unsupported() {
        let mut d = StreamDecoder::new(ProviderKind::Gemini);
        let e = d
            .feed(b"data: {\"candidates\":[{\"content\":{\"parts\":[{\"functionCall\":{\"name\":\"f\"}}]}}]}\n\n")
            .unwrap_err();
        assert!(matches!(e, TranslateError::Unsupported(_)), "{e:?}");
    }
}
