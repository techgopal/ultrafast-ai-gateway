//! Google Gemini (`generativelanguage`), the `generateContent` API.

use serde_json::{json, Value};

use super::{path_segment, saturate, with_calls, HttpRequest, StreamState, Target};
use crate::error::TranslateError;
use crate::sse::SseEvent;
use crate::types::{
    ChatRequest, ChatResponse, FinishReason, ImageSource, Message, Part, ResponseFormat, Role,
    StreamEvent, Tool, ToolCall, ToolChoice, Usage,
};

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

const SKIP_THOUGHT_SIGNATURE: &str = "skip_thought_signature_validator";

/// `call_<first 8 hex of sha256(responseId)>`: Gemini has no call ids, and
/// the same answer (stream chunks, a cached copy) must always give the same
/// ones, while two answers must not share any (a later turn would look like
/// a repeat of an earlier one). Without a responseId the 8 hex are random.
fn call_prefix(response_id: Option<&str>) -> String {
    use sha2::{Digest, Sha256};
    let hash = match response_id.filter(|s| !s.is_empty()) {
        Some(id) => {
            let d = Sha256::digest(id.as_bytes());
            u32::from_be_bytes([d[0], d[1], d[2], d[3]])
        }
        None => {
            // RandomState is seeded per process; the counter makes every call differ.
            use std::collections::hash_map::RandomState;
            use std::hash::{BuildHasher, Hasher};
            use std::sync::atomic::{AtomicU64, Ordering};
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let mut h = RandomState::new().build_hasher();
            h.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
            (h.finish() >> 32) as u32
        }
    };
    format!("call_{hash:08x}")
}

fn text_part(text: &str) -> Value {
    json!({ "parts": [{ "text": text }] })
}

fn invalid(m: &str) -> TranslateError {
    TranslateError::InvalidRequest(m.to_string())
}

/// Text parts and inline images of a user message. Empty text is left out.
fn user_parts(m: &Message) -> Result<Vec<Value>, TranslateError> {
    if !m.has_images() {
        return Ok(vec![json!({ "text": m.joined_text() })]);
    }
    let mut parts = Vec::new();
    for p in &m.content {
        match p {
            Part::Text(t) if t.is_empty() => {}
            Part::Text(t) => parts.push(json!({ "text": t })),
            Part::Image(ImageSource::Base64 { media_type, data }) => {
                parts.push(json!({ "inlineData": { "mimeType": media_type, "data": data } }));
            }
            Part::Image(ImageSource::Url(_)) => {
                return Err(TranslateError::Unsupported(
                    "Gemini takes images as data: URLs only".into(),
                ));
            }
        }
    }
    Ok(parts)
}

fn function_call_part(c: &ToolCall) -> Result<Value, TranslateError> {
    let args: Value = if c.arguments.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(&c.arguments)
            .map_err(|_| invalid("tool call arguments are not valid JSON"))?
    };
    if !args.is_object() {
        return Err(invalid("tool call arguments must be a JSON object"));
    }
    // Gemini 3 answers 400 to a functionCall in the history that has no
    // thought signature, and the gateway does not carry the real ones (they
    // are not part of any other format). Google documents this placeholder
    // for history that did not come from the model.
    Ok(json!({
        "functionCall": { "name": c.name, "args": args },
        "thoughtSignature": SKIP_THOUGHT_SIGNATURE,
    }))
}

/// The `contents` array. Consecutive tool messages become one user content of
/// `functionResponse` parts; each is named after the call it answers.
fn contents_value(req: &ChatRequest) -> Result<Vec<Value>, TranslateError> {
    let mut out: Vec<Value> = Vec::new();
    let mut results: Vec<Value> = Vec::new();
    let mut names: Vec<(&str, &str)> = Vec::new();
    for m in req.messages.iter().filter(|m| m.role != Role::System) {
        if m.role == Role::Tool {
            if m.has_images() {
                return Err(invalid("images are not allowed in a tool message"));
            }
            let id = m
                .tool_call_id
                .as_deref()
                .ok_or_else(|| invalid("a tool message needs a tool_call_id"))?;
            // The call the id points to, else the name the message carries.
            let name = names
                .iter()
                .rev()
                .find(|(i, _)| *i == id)
                .map(|(_, n)| *n)
                .or(m.name.as_deref())
                .ok_or_else(|| {
                    TranslateError::InvalidRequest(format!(
                        "tool result for unknown tool call '{id}'"
                    ))
                })?;
            results.push(
                json!({ "functionResponse": { "name": name, "response": { "content": m.joined_text() } } }),
            );
            continue;
        }
        if !results.is_empty() {
            out.push(json!({ "role": "user", "parts": std::mem::take(&mut results) }));
        }
        if m.role == Role::Assistant {
            if m.has_images() {
                return Err(invalid("images are only allowed in user messages"));
            }
            let mut parts = Vec::new();
            let text = m.joined_text();
            if !text.is_empty() || m.tool_calls.is_empty() {
                parts.push(json!({ "text": text }));
            }
            for c in &m.tool_calls {
                parts.push(function_call_part(c)?);
                names.push((&c.id, &c.name));
            }
            out.push(json!({ "role": "model", "parts": parts }));
        } else {
            out.push(json!({ "role": "user", "parts": user_parts(m)? }));
        }
    }
    if !results.is_empty() {
        out.push(json!({ "role": "user", "parts": results }));
    }
    Ok(out)
}

fn declaration(t: &Tool) -> Value {
    // `parametersJsonSchema` takes a full JSON Schema; `parameters` is an
    // OpenAPI subset that rejects what OpenAI-style schemas carry (additionalProperties, ...).
    let mut o = json!({ "name": t.name, "parametersJsonSchema": t.parameters });
    if let Some(d) = &t.description {
        o["description"] = json!(d);
    }
    o
}

fn tool_config(choice: &ToolChoice) -> Value {
    let config = match choice {
        ToolChoice::Auto => json!({ "mode": "AUTO" }),
        ToolChoice::None => json!({ "mode": "NONE" }),
        ToolChoice::Required => json!({ "mode": "ANY" }),
        ToolChoice::Tool(n) => json!({ "mode": "ANY", "allowedFunctionNames": [n] }),
    };
    json!({ "functionCallingConfig": config })
}

pub(crate) fn build(target: &Target, req: &ChatRequest) -> Result<HttpRequest, TranslateError> {
    super::check_tool_choice(req)?;
    if !req.tools.is_empty() && req.parallel_tool_calls == Some(false) {
        return Err(TranslateError::Unsupported(
            "parallel_tool_calls=false is not supported by this provider".into(),
        ));
    }
    if req
        .messages
        .iter()
        .any(|m| m.name.is_some() && m.role != Role::Tool)
    {
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
    let contents = contents_value(req)?;
    if contents.is_empty() {
        return Err(TranslateError::InvalidRequest(
            "at least one user or assistant message is required".into(),
        ));
    }
    let mut body = json!({ "contents": contents });
    if !system.is_empty() {
        body["systemInstruction"] = text_part(&system.join("\n\n"));
    }
    if !req.tools.is_empty() {
        let declarations: Vec<Value> = req.tools.iter().map(declaration).collect();
        body["tools"] = json!([{ "functionDeclarations": declarations }]);
        if let Some(c) = &req.tool_choice {
            body["toolConfig"] = tool_config(c);
        }
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
    match &req.response_format {
        None | Some(ResponseFormat::Text) => {}
        Some(ResponseFormat::JsonObject) => {
            config.insert("responseMimeType".into(), json!("application/json"));
        }
        Some(ResponseFormat::JsonSchema { schema, .. }) => {
            config.insert("responseMimeType".into(), json!("application/json"));
            // `responseJsonSchema` takes a full JSON Schema (`responseSchema`
            // is the OpenAPI subset).
            config.insert("responseJsonSchema".into(), schema.clone());
        }
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
            path_segment(
                target
                    .model
                    .strip_prefix("models/")
                    .unwrap_or(&target.model)
            )
        ),
        headers,
        body: serde_json::to_vec(&body)
            .map_err(|e| TranslateError::InvalidRequest(e.to_string()))?,
    })
}

/// Thinking is billed as output.
fn output_tokens(u: &Value) -> u32 {
    saturate(
        u["candidatesTokenCount"]
            .as_u64()
            .unwrap_or(0)
            .saturating_add(u["thoughtsTokenCount"].as_u64().unwrap_or(0)),
    )
}

/// Reads `usageMetadata` into the state, when the chunk has it.
fn read_usage(state: &mut StreamState, v: &Value) {
    if let Some(u) = v.get("usageMetadata").filter(|u| u.is_object()) {
        state.input_tokens = u["promptTokenCount"].as_u64().map(saturate);
        state.output_tokens = Some(output_tokens(u));
    }
}

/// What one answer or chunk holds: text, function calls as (name, arguments)
/// and why it ended, if it did. A prompt that was refused ends as a content filter.
struct Candidate {
    text: String,
    calls: Vec<(String, String)>,
    finish: Option<FinishReason>,
}

fn read_candidate(v: &Value) -> Result<Candidate, TranslateError> {
    let blocked = v["promptFeedback"]["blockReason"].is_string();
    let Some(candidate) = v["candidates"].get(0) else {
        if blocked {
            return Ok(Candidate {
                text: String::new(),
                calls: Vec::new(),
                finish: Some(FinishReason::ContentFilter),
            });
        }
        return Err(TranslateError::Malformed(
            "response has no candidates".into(),
        ));
    };
    let mut text = String::new();
    let mut calls = Vec::new();
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
        } else if let Some(call) = part.get("functionCall") {
            // A thoughtSignature beside it is dropped.
            let name = call["name"]
                .as_str()
                .filter(|n| !n.is_empty())
                .ok_or_else(|| TranslateError::Malformed("function call has no name".into()))?;
            let args = call.get("args").filter(|a| !a.is_null());
            let arguments = match args {
                Some(a) => serde_json::to_string(a)
                    .map_err(|e| TranslateError::Malformed(e.to_string()))?,
                None => "{}".to_string(),
            };
            calls.push((name.to_string(), arguments));
        } else if part
            .as_object()
            .is_some_and(|o| o.keys().all(|k| k == "thoughtSignature" || k == "thought"))
        {
            // A signature alone carries no content.
        } else {
            return Err(TranslateError::Unsupported(
                "response part is not text".into(),
            ));
        }
    }
    let reason = candidate["finishReason"].as_str().and_then(finish);
    // Gemini ends an answer with calls as STOP.
    let finish = with_calls(reason, !calls.is_empty());
    Ok(Candidate {
        text,
        calls,
        finish,
    })
}

pub(crate) fn parse(body: &[u8]) -> Result<ChatResponse, TranslateError> {
    let v: Value =
        serde_json::from_slice(body).map_err(|e| TranslateError::Malformed(e.to_string()))?;
    let Candidate {
        text: content,
        calls,
        finish: finish_reason,
    } = read_candidate(&v)?;
    let prefix = call_prefix(v["responseId"].as_str());
    let tool_calls = calls
        .into_iter()
        .enumerate()
        .map(|(n, (name, arguments))| ToolCall {
            id: format!("{prefix}_{n}"),
            name,
            arguments,
        })
        .collect();
    let usage = v
        .get("usageMetadata")
        .filter(|u| u.is_object())
        .map(|u| Usage {
            input_tokens: saturate(u["promptTokenCount"].as_u64().unwrap_or(0)),
            output_tokens: output_tokens(u),
        });
    Ok(ChatResponse {
        id: v["responseId"].as_str().unwrap_or_default().to_string(),
        model: v["modelVersion"].as_str().unwrap_or_default().to_string(),
        content,
        tool_calls,
        finish_reason,
        usage,
    })
}

/// Gemini ends a stream by closing it. The chunk with a finish reason marks the
/// end, but `Done` is given by `StreamDecoder::finish` so that usage reported
/// after it is not lost.
pub(crate) fn decode(
    state: &mut StreamState,
    ev: &SseEvent,
    out: &mut Vec<StreamEvent>,
) -> Result<(), TranslateError> {
    let v: Value =
        serde_json::from_str(&ev.data).map_err(|e| TranslateError::Malformed(e.to_string()))?;
    if let Some(msg) = v["error"]["message"].as_str() {
        // The code is read as an HTTP status, so the common rules apply.
        let status = v["error"]["code"]
            .as_u64()
            .filter(|c| (400..600).contains(c))
            .map_or(502, |c| c as u16);
        return Err(TranslateError::Provider {
            status,
            retryable: status == 408 || status == 429 || status >= 500,
            message: msg.to_string(),
        });
    }
    read_usage(state, &v);
    // A chunk of usage alone carries nothing else.
    if v["candidates"].get(0).is_none() && !v["promptFeedback"]["blockReason"].is_string() {
        return Ok(());
    }
    let Candidate {
        text,
        calls,
        finish,
    } = read_candidate(&v)?;
    if !text.is_empty() {
        out.push(StreamEvent::Delta { text });
    }
    for (name, arguments) in calls {
        // One prefix for the whole answer, taken when its first call comes.
        let prefix = state
            .call_prefix
            .get_or_insert_with(|| call_prefix(v["responseId"].as_str()));
        let index = state.tool_calls_started;
        state.tool_calls_started = index.saturating_add(1);
        out.push(StreamEvent::ToolCallStart {
            index,
            id: format!("{prefix}_{index}"),
            name,
        });
        out.push(StreamEvent::ToolCallDelta { index, arguments });
    }
    // A candidate that ends for a reason GEMINI reports as OTHER still ends the stream.
    let ended = v["candidates"][0]["finishReason"].is_string()
        || v["promptFeedback"]["blockReason"].is_string();
    if ended {
        // Given when the stream closes: a chunk of usage may still follow.
        // The calls may have come in an earlier chunk.
        state.finish = with_calls(finish, state.tool_calls_started > 0);
        state.ended = true;
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
        Message::text(role, content)
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
            tools: Vec::new(),
            tool_choice: None,
            parallel_tool_calls: None,
            response_format: None,
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
    fn a_leading_models_prefix_is_stripped_from_the_model_name() {
        let mut t = target();
        t.model = "models/gemini-2.0-flash".into();
        let r = build_request(&t, &request(false)).unwrap();
        assert_eq!(
            r.url,
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash:generateContent"
        );
        // Only a leading prefix, and only once.
        t.model = "models/models/x".into();
        let r = build_request(&t, &request(false)).unwrap();
        assert!(r.url.contains("/models/models%2Fx:"), "{}", r.url);
    }

    #[test]
    fn thoughts_are_skipped_and_their_tokens_count_as_output() {
        let body = include_bytes!("../../tests/fixtures/gemini/response_thought.json");
        let r = parse_response(ProviderKind::Gemini, 200, body).unwrap();
        // The thought part is skipped; the part with only a signature is too.
        assert_eq!(r.content, "Answer");
        assert_eq!(
            r.usage,
            Some(Usage {
                input_tokens: 7,
                output_tokens: 14
            })
        );
    }

    #[test]
    fn a_part_without_text_is_skipped_in_a_stream() {
        let mut d = StreamDecoder::new(ProviderKind::Gemini);
        let got = d
            .feed(b"data: {\"candidates\":[{\"content\":{\"parts\":[{\"thoughtSignature\":\"c2ln\"}]}}]}\n\n")
            .unwrap();
        assert_eq!(got, vec![]);
    }

    #[test]
    fn a_usage_only_chunk_after_the_finish_chunk_updates_the_usage() {
        let input = concat!(
            "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hi\"}]},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":7,\"candidatesTokenCount\":1}}\n\n",
            "data: {\"usageMetadata\":{\"promptTokenCount\":7,\"candidatesTokenCount\":1,\"thoughtsTokenCount\":4}}\n\n",
        )
        .as_bytes();
        let want = vec![
            StreamEvent::Delta { text: "Hi".into() },
            StreamEvent::Done {
                finish_reason: Some(FinishReason::Stop),
                usage: Some(Usage {
                    input_tokens: 7,
                    output_tokens: 5,
                }),
            },
        ];
        for split in 0..=input.len() {
            let mut d = StreamDecoder::new(ProviderKind::Gemini);
            let mut events = d.feed(&input[..split]).unwrap();
            events.extend(d.feed(&input[split..]).unwrap());
            events.extend(d.finish());
            assert_eq!(events, want, "split {split}");
            assert_eq!(d.finish(), vec![], "Done is given once");
        }
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
    fn garbage_is_malformed() {
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
    fn parses_function_calls_with_stable_ids_and_tool_calls_finish() {
        let body = include_bytes!("../../tests/fixtures/gemini/response_function_call.json");
        let r = parse_response(ProviderKind::Gemini, 200, body).unwrap();
        assert_eq!(
            r.tool_calls,
            vec![ToolCall {
                id: "call_9a8fd095_0".into(),
                name: "f".into(),
                arguments: "{}".into()
            }]
        );
        assert_eq!(r.finish_reason, Some(FinishReason::ToolCalls));
        let body = br#"{"candidates":[{"content":{"parts":[
            {"text":"ok"},
            {"functionCall":{"name":"a","args":{"x":1}},"thoughtSignature":"c2ln"},
            {"functionCall":{"name":"b"}}]},"finishReason":"STOP"}]}"#;
        let r = parse_response(ProviderKind::Gemini, 200, body).unwrap();
        assert_eq!(r.content, "ok");
        let got: Vec<_> = r
            .tool_calls
            .iter()
            .map(|c| (c.id.as_str(), c.name.as_str(), c.arguments.as_str()))
            .collect();
        let prefix = r.tool_calls[0].id.strip_suffix("_0").unwrap();
        assert!(
            prefix.len() == "call_".len() + 8
                && prefix.starts_with("call_")
                && prefix[5..].bytes().all(|b| b.is_ascii_hexdigit()),
            "{prefix}"
        );
        assert_eq!(
            got,
            [
                (format!("{prefix}_0").as_str(), "a", r#"{"x":1}"#),
                (format!("{prefix}_1").as_str(), "b", "{}")
            ]
        );
        assert_eq!(r.finish_reason, Some(FinishReason::ToolCalls));
        // A length stop with calls keeps its reason.
        let body = br#"{"candidates":[{"content":{"parts":[{"functionCall":{"name":"a"}}]},"finishReason":"MAX_TOKENS"}]}"#;
        let r = parse_response(ProviderKind::Gemini, 200, body).unwrap();
        assert_eq!(r.finish_reason, Some(FinishReason::Length));
        // A function call without a name is malformed.
        let body = br#"{"candidates":[{"content":{"parts":[{"functionCall":{}}]}}]}"#;
        assert!(matches!(
            parse_response(ProviderKind::Gemini, 200, body),
            Err(TranslateError::Malformed(_))
        ));
    }

    #[test]
    fn gemini_errors_pass_through_the_common_rules() {
        let body = include_bytes!("../../tests/fixtures/gemini/error_429.json");
        let e = parse_response(ProviderKind::Gemini, 429, body).unwrap_err();
        assert!(
            matches!(&e, TranslateError::Provider { status: 429, retryable: true, message } if message.contains("exhausted")),
            "{e:?}"
        );
        let e = parse_response(ProviderKind::Gemini, 400, body).unwrap_err();
        assert!(
            matches!(
                &e,
                TranslateError::Provider {
                    status: 400,
                    retryable: false,
                    ..
                }
            ),
            "{e:?}"
        );
        for (status, retryable) in [
            (408, true),
            (500, true),
            (503, true),
            (401, false),
            (403, false),
        ] {
            let e = parse_response(ProviderKind::Gemini, status, body).unwrap_err();
            assert!(
                matches!(&e, TranslateError::Provider { status: s, retryable: r, .. } if *s == status && *r == retryable),
                "{status}: {e:?}"
            );
        }
    }

    #[test]
    fn in_stream_errors_map_their_code_like_http_errors() {
        for (code, retryable) in [
            (429, true),
            (408, true),
            (500, true),
            (503, true),
            (401, false),
            (403, false),
            (400, false),
        ] {
            let mut d = StreamDecoder::new(ProviderKind::Gemini);
            let line = format!(
                "data: {{\"error\":{{\"code\":{code},\"message\":\"m\",\"status\":\"X\"}}}}\n\n"
            );
            let e = d.feed(line.as_bytes()).unwrap_err();
            assert!(
                matches!(&e, TranslateError::Provider { status, retryable: r, .. } if *status == code && *r == retryable),
                "{code}: {e:?}"
            );
        }
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
            events.extend(d.finish());
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
                    status: 403,
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
        assert_eq!(got, vec![]);
        let got = d.finish();
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
    fn decodes_streamed_function_calls() {
        let input = include_bytes!("../../tests/fixtures/gemini/stream_function_call.txt");
        let want = vec![
            StreamEvent::Delta { text: "Ok".into() },
            StreamEvent::ToolCallStart {
                index: 0,
                id: "call_9a8fd095_0".into(),
                name: "get_weather".into(),
            },
            StreamEvent::ToolCallDelta {
                index: 0,
                arguments: r#"{"city":"Paris"}"#.into(),
            },
            StreamEvent::ToolCallStart {
                index: 1,
                id: "call_9a8fd095_1".into(),
                name: "ping".into(),
            },
            StreamEvent::ToolCallDelta {
                index: 1,
                arguments: "{}".into(),
            },
            StreamEvent::Done {
                finish_reason: Some(FinishReason::ToolCalls),
                usage: Some(Usage {
                    input_tokens: 7,
                    output_tokens: 5,
                }),
            },
        ];
        for split in 0..=input.len() {
            let mut d = StreamDecoder::new(ProviderKind::Gemini);
            let mut events = d.feed(&input[..split]).unwrap();
            events.extend(d.feed(&input[split..]).unwrap());
            events.extend(d.finish());
            assert_eq!(events, want, "split {split}");
        }
    }

    #[test]
    fn call_ids_differ_between_answers_and_hold_across_stream_chunks() {
        // Without a responseId the prefix is random per answer, but one
        // answer keeps one prefix across all its chunks.
        let chunk = |name: &str| {
            format!(
                "data: {{\"candidates\":[{{\"content\":{{\"parts\":[{{\"functionCall\":{{\"name\":\"{name}\"}}}}]}}}}]}}\n\n"
            )
        };
        let ids = |input: &str| {
            let mut d = StreamDecoder::new(ProviderKind::Gemini);
            d.feed(input.as_bytes())
                .unwrap()
                .into_iter()
                .filter_map(|e| match e {
                    StreamEvent::ToolCallStart { id, .. } => Some(id),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let a = ids(&format!("{}{}", chunk("x"), chunk("y")));
        assert_eq!(a.len(), 2);
        let prefix = |id: &str| id.rsplit_once('_').unwrap().0.to_string();
        assert_eq!(prefix(&a[0]), prefix(&a[1]), "{a:?}");
        assert!(a[0].ends_with("_0") && a[1].ends_with("_1"), "{a:?}");
        let b = ids(&chunk("x"));
        assert_ne!(prefix(&a[0]), prefix(&b[0]), "{a:?} {b:?}");
        // The same non-stream answer without a responseId also differs.
        let body = br#"{"candidates":[{"content":{"parts":[{"functionCall":{"name":"a"}}]}}]}"#;
        let one = parse_response(ProviderKind::Gemini, 200, body).unwrap();
        let two = parse_response(ProviderKind::Gemini, 200, body).unwrap();
        assert_ne!(one.tool_calls[0].id, two.tool_calls[0].id);
    }

    fn weather_tool() -> Tool {
        Tool {
            name: "get_weather".into(),
            description: Some("Weather".into()),
            parameters: serde_json::json!({"type": "object", "properties": {"city": {"type": "string"}}}),
            strict: None,
        }
    }

    fn with_tools() -> ChatRequest {
        let mut req = request(false);
        req.tools = vec![weather_tool()];
        req
    }

    #[test]
    fn builds_function_declarations_and_tool_config() {
        let v = body_of(&build_request(&target(), &with_tools()).unwrap());
        assert_eq!(
            v["tools"],
            serde_json::json!([{"functionDeclarations": [{
                "name": "get_weather",
                "description": "Weather",
                "parametersJsonSchema": {"type": "object", "properties": {"city": {"type": "string"}}}
            }]}])
        );
        assert!(v.get("toolConfig").is_none());
        let mut req = with_tools();
        req.tools[0].description = None;
        let v = body_of(&build_request(&target(), &req).unwrap());
        assert!(v["tools"][0]["functionDeclarations"][0]
            .get("description")
            .is_none());
        for (choice, want) in [
            (ToolChoice::Auto, serde_json::json!({"mode": "AUTO"})),
            (ToolChoice::None, serde_json::json!({"mode": "NONE"})),
            (ToolChoice::Required, serde_json::json!({"mode": "ANY"})),
            (
                ToolChoice::Tool("get_weather".into()),
                serde_json::json!({"mode": "ANY", "allowedFunctionNames": ["get_weather"]}),
            ),
        ] {
            let mut req = with_tools();
            req.tool_choice = Some(choice.clone());
            let v = body_of(&build_request(&target(), &req).unwrap());
            assert_eq!(v["toolConfig"]["functionCallingConfig"], want, "{choice:?}");
        }
        let mut req = with_tools();
        req.parallel_tool_calls = Some(true);
        let v = body_of(&build_request(&target(), &req).unwrap());
        assert!(v.get("toolConfig").is_none());
    }

    #[test]
    fn parallel_false_is_unsupported() {
        let mut req = with_tools();
        req.parallel_tool_calls = Some(false);
        assert_eq!(
            build_request(&target(), &req).unwrap_err(),
            TranslateError::Unsupported(
                "parallel_tool_calls=false is not supported by this provider".into()
            )
        );
    }

    #[test]
    fn tool_options_without_tools_are_ignored_or_refused() {
        let mut req = request(false);
        req.tool_choice = Some(ToolChoice::None);
        req.parallel_tool_calls = Some(false);
        let r = build_request(&target(), &req).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
        assert!(v.get("toolConfig").is_none() && v.get("tools").is_none());
        req.tool_choice = Some(ToolChoice::Tool("f".into()));
        assert_eq!(
            build_request(&target(), &req).unwrap_err(),
            TranslateError::InvalidRequest("tool_choice 'f' needs tools".into())
        );
    }

    fn call(id: &str, name: &str, args: &str) -> ToolCall {
        ToolCall {
            id: id.into(),
            name: name.into(),
            arguments: args.into(),
        }
    }

    fn tool_msg(id: &str, text: &str) -> Message {
        Message {
            tool_call_id: Some(id.into()),
            ..Message::text(Role::Tool, text)
        }
    }

    #[test]
    fn builds_function_call_and_response_history() {
        let mut req = with_tools();
        req.messages = vec![
            msg(Role::User, "weather?"),
            Message {
                tool_calls: vec![
                    call("c1", "get_weather", r#"{"city":"Paris"}"#),
                    call("c2", "ping", ""),
                ],
                ..msg(Role::Assistant, "")
            },
            tool_msg("c1", "sunny"),
            tool_msg("c2", "pong"),
            msg(Role::User, "thanks"),
            Message {
                tool_calls: vec![call("c3", "ping", "{}")],
                ..msg(Role::Assistant, "calling")
            },
        ];
        let v = body_of(&build_request(&target(), &req).unwrap());
        let want = serde_json::json!([
            {"role": "user", "parts": [{"text": "weather?"}]},
            {"role": "model", "parts": [
                {"functionCall": {"name": "get_weather", "args": {"city": "Paris"}}, "thoughtSignature": "skip_thought_signature_validator"},
                {"functionCall": {"name": "ping", "args": {}}, "thoughtSignature": "skip_thought_signature_validator"}
            ]},
            {"role": "user", "parts": [
                {"functionResponse": {"name": "get_weather", "response": {"content": "sunny"}}},
                {"functionResponse": {"name": "ping", "response": {"content": "pong"}}}
            ]},
            {"role": "user", "parts": [{"text": "thanks"}]},
            {"role": "model", "parts": [
                {"text": "calling"},
                {"functionCall": {"name": "ping", "args": {}}, "thoughtSignature": "skip_thought_signature_validator"}
            ]}
        ]);
        assert_eq!(v["contents"], want);
    }

    #[test]
    fn repeated_call_ids_name_the_latest_call() {
        let mut req = with_tools();
        req.messages = vec![
            msg(Role::User, "x"),
            Message {
                tool_calls: vec![call("call_0", "first", "{}")],
                ..msg(Role::Assistant, "")
            },
            tool_msg("call_0", "a"),
            Message {
                tool_calls: vec![call("call_0", "second", "{}")],
                ..msg(Role::Assistant, "")
            },
            tool_msg("call_0", "b"),
        ];
        let v = body_of(&build_request(&target(), &req).unwrap());
        let c = &v["contents"];
        assert_eq!(c[2]["parts"][0]["functionResponse"]["name"], "first");
        assert_eq!(c[4]["parts"][0]["functionResponse"]["name"], "second");
    }

    #[test]
    fn a_tool_message_may_carry_a_name() {
        let mut req = with_tools();
        req.messages = vec![
            msg(Role::User, "x"),
            Message {
                tool_calls: vec![call("c1", "real", "{}")],
                ..msg(Role::Assistant, "")
            },
            Message {
                name: Some("ignored".into()),
                ..tool_msg("c1", "a")
            },
            // The id matches no call: the message's own name is the fallback.
            Message {
                name: Some("fallback".into()),
                ..tool_msg("other", "b")
            },
        ];
        let v = body_of(&build_request(&target(), &req).unwrap());
        let parts = &v["contents"][2]["parts"];
        assert_eq!(parts[0]["functionResponse"]["name"], "real");
        assert_eq!(parts[1]["functionResponse"]["name"], "fallback");
        // A name on any other message is still refused.
        req.messages[1].name = Some("n".into());
        assert!(matches!(
            build_request(&target(), &req),
            Err(TranslateError::Unsupported(_))
        ));
    }

    #[test]
    fn strict_is_ignored() {
        let mut req = with_tools();
        req.tools[0].strict = Some(true);
        let with = body_of(&build_request(&target(), &req).unwrap());
        req.tools[0].strict = None;
        assert_eq!(with, body_of(&build_request(&target(), &req).unwrap()));
    }

    #[test]
    fn auto_without_tools_is_accepted() {
        let mut req = request(false);
        req.tool_choice = Some(ToolChoice::Auto);
        let v = body_of(&build_request(&target(), &req).unwrap());
        assert!(v.get("toolConfig").is_none() && v.get("tools").is_none());
    }

    #[test]
    fn invalid_tool_arguments_are_invalid_request() {
        for bad in ["{oops", "[1]", "3"] {
            let mut req = with_tools();
            req.messages.push(Message {
                tool_calls: vec![call("c", "f", bad)],
                ..msg(Role::Assistant, "")
            });
            assert!(
                matches!(
                    build_request(&target(), &req),
                    Err(TranslateError::InvalidRequest(_))
                ),
                "{bad}"
            );
        }
    }

    #[test]
    fn tool_result_for_unknown_call_is_invalid() {
        let mut req = with_tools();
        req.messages = vec![msg(Role::User, "x"), tool_msg("nope", "r")];
        assert_eq!(
            build_request(&target(), &req).unwrap_err(),
            TranslateError::InvalidRequest("tool result for unknown tool call 'nope'".into())
        );
        // A call made later does not count.
        req.messages = vec![
            msg(Role::User, "x"),
            tool_msg("c", "r"),
            Message {
                tool_calls: vec![call("c", "f", "{}")],
                ..msg(Role::Assistant, "")
            },
        ];
        assert!(matches!(
            build_request(&target(), &req),
            Err(TranslateError::InvalidRequest(_))
        ));
        // No id at all.
        req.messages = vec![msg(Role::User, "x"), msg(Role::Tool, "r")];
        assert!(matches!(
            build_request(&target(), &req),
            Err(TranslateError::InvalidRequest(_))
        ));
    }

    #[test]
    fn https_image_is_unsupported() {
        let mut req = request(false);
        req.messages[1]
            .content
            .push(Part::Image(ImageSource::Url("https://x.test/a.png".into())));
        assert_eq!(
            build_request(&target(), &req).unwrap_err(),
            TranslateError::Unsupported("Gemini takes images as data: URLs only".into())
        );
    }

    #[test]
    fn data_image_becomes_inline_data() {
        let mut req = request(false);
        req.messages[1].content = vec![
            Part::Text("look".into()),
            Part::Image(ImageSource::Base64 {
                media_type: "image/png".into(),
                data: "aGk=".into(),
            }),
        ];
        let v = body_of(&build_request(&target(), &req).unwrap());
        assert_eq!(
            v["contents"][0]["parts"],
            serde_json::json!([
                {"text": "look"},
                {"inlineData": {"mimeType": "image/png", "data": "aGk="}}
            ])
        );
    }

    #[test]
    fn images_outside_user_messages_are_invalid() {
        let image = Part::Image(ImageSource::Base64 {
            media_type: "image/png".into(),
            data: "aGk=".into(),
        });
        for role in [Role::System, Role::Assistant] {
            let mut req = request(false);
            let mut m = msg(role, "x");
            m.content.push(image.clone());
            req.messages.push(m);
            assert!(
                matches!(
                    build_request(&target(), &req),
                    Err(TranslateError::InvalidRequest(_))
                ),
                "{role:?}"
            );
        }
        let mut req = with_tools();
        req.messages = vec![
            msg(Role::User, "x"),
            Message {
                tool_calls: vec![call("c", "f", "{}")],
                ..msg(Role::Assistant, "")
            },
            Message {
                tool_call_id: Some("c".into()),
                content: vec![image],
                ..msg(Role::Tool, "")
            },
        ];
        assert!(matches!(
            build_request(&target(), &req),
            Err(TranslateError::InvalidRequest(_))
        ));
    }

    #[test]
    fn response_format_sets_mime_type_and_json_schema() {
        use crate::types::ResponseFormat;
        let body = |rf: Option<ResponseFormat>| {
            let mut req = request(false);
            req.response_format = rf;
            body_of(&build_request(&target(), &req).unwrap())
        };
        assert!(body(None)["generationConfig"]
            .get("responseMimeType")
            .is_none());
        assert!(body(Some(ResponseFormat::Text))["generationConfig"]
            .get("responseMimeType")
            .is_none());
        let g = &body(Some(ResponseFormat::JsonObject))["generationConfig"];
        assert_eq!(g["responseMimeType"], "application/json");
        assert!(g.get("responseJsonSchema").is_none());
        assert_eq!(g["maxOutputTokens"], 5);
        let v = body(Some(ResponseFormat::JsonSchema {
            name: "person".into(),
            schema: serde_json::json!({"type":"object","properties":{"a":{"type":"string"}},"required":["a"],"additionalProperties":false}),
            strict: Some(true),
            description: Some("a person".into()),
        }));
        let g = &v["generationConfig"];
        assert_eq!(g["responseMimeType"], "application/json");
        assert_eq!(
            g["responseJsonSchema"],
            serde_json::json!({"type":"object","properties":{"a":{"type":"string"}},"required":["a"],"additionalProperties":false})
        );
        assert!(g.get("responseSchema").is_none());
    }

    #[test]
    fn response_format_alone_still_makes_a_generation_config() {
        use crate::types::ResponseFormat;
        let mut req = request(false);
        req.max_tokens = None;
        req.temperature = None;
        req.top_p = None;
        req.stop = None;
        req.response_format = Some(ResponseFormat::JsonObject);
        let v = body_of(&build_request(&target(), &req).unwrap());
        assert_eq!(
            v["generationConfig"],
            serde_json::json!({"responseMimeType":"application/json"})
        );
    }
}
