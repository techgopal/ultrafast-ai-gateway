//! OpenAI Chat Completions wire format, as received from and returned to callers.

use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::error::TranslateError;
use crate::types::{
    image_source, ChatRequest, ChatResponse, Message, Part, Role, StreamEvent, Tool, ToolCall,
    ToolChoice, Usage,
};

#[derive(Deserialize)]
struct WireRequest {
    model: String,
    messages: Vec<WireMessage>,
    #[serde(default)]
    max_tokens: Option<u32>,
    #[serde(default)]
    max_completion_tokens: Option<u32>,
    #[serde(default)]
    temperature: Option<f32>,
    #[serde(default)]
    top_p: Option<f32>,
    #[serde(default)]
    stop: Option<StopField>,
    #[serde(default)]
    stream: bool,
    #[serde(default)]
    tools: Option<Vec<Value>>,
    #[serde(default)]
    tool_choice: Option<Value>,
    #[serde(default)]
    parallel_tool_calls: Option<bool>,
    /// Every field that is not named above.
    #[serde(flatten)]
    extra: Map<String, Value>,
}

/// Request fields that are accepted and not used: they cannot change the
/// generated text.
const IGNORED_REQUEST_FIELDS: &[&str] = &[
    "user",
    "metadata",
    "store",
    "stream_options",
    "service_tier",
];

/// Rejects the first field that is present, not null and not in `allowed`.
fn reject_unknown(extra: &Map<String, Value>, allowed: &[&str]) -> Result<(), TranslateError> {
    match extra
        .iter()
        .find(|(k, v)| !v.is_null() && !allowed.contains(&k.as_str()))
    {
        Some((field, _)) => Err(unsupported_field(field)),
        None => Ok(()),
    }
}

fn unsupported_field(field: &str) -> TranslateError {
    TranslateError::Unsupported(format!("field '{field}' is not supported yet"))
}

#[derive(Deserialize)]
#[serde(untagged)]
enum StopField {
    One(String),
    Many(Vec<String>),
}

#[derive(Deserialize)]
struct WireMessage {
    role: String,
    #[serde(default)]
    content: Option<WireContent>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    tool_call_id: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<Value>>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum WireContent {
    Text(String),
    Parts(Vec<Value>),
}

/// One content part. Images are only allowed where `allow_images` is set.
fn parse_part(part: &Value, allow_images: bool) -> Result<Part, TranslateError> {
    let kind = part["type"].as_str();
    match kind {
        Some("text") => {
            if let Some(fields) = part.as_object() {
                reject_unknown(fields, &["type", "text"])?;
            }
            part["text"]
                .as_str()
                .map(|t| Part::Text(t.to_string()))
                .ok_or_else(|| {
                    TranslateError::InvalidRequest("content part 'text' must be a string".into())
                })
        }
        Some("image_url") => {
            if !allow_images {
                return Err(TranslateError::InvalidRequest(
                    "images are only allowed in user messages".into(),
                ));
            }
            if let Some(fields) = part.as_object() {
                reject_unknown(fields, &["type", "image_url"])?;
            }
            let image = &part["image_url"];
            if let Some(fields) = image.as_object() {
                reject_unknown(fields, &["url", "detail"])?;
            }
            let url = image["url"].as_str().ok_or_else(|| {
                TranslateError::InvalidRequest("image_url 'url' must be a string".into())
            })?;
            Ok(Part::Image(image_source(url)?))
        }
        other => Err(TranslateError::Unsupported(format!(
            "content part '{}' is not supported yet",
            other.unwrap_or("unknown")
        ))),
    }
}

fn parse_tools(tools: Vec<Value>) -> Result<Vec<Tool>, TranslateError> {
    let mut out = Vec::with_capacity(tools.len());
    for t in &tools {
        let kind = t["type"].as_str().unwrap_or("unknown");
        if kind != "function" {
            return Err(TranslateError::Unsupported(format!(
                "tool type '{kind}' is not supported yet"
            )));
        }
        if let Some(fields) = t.as_object() {
            reject_unknown(fields, &["type", "function"])?;
        }
        let f = &t["function"];
        if let Some(fields) = f.as_object() {
            reject_unknown(fields, &["name", "description", "parameters", "strict"])?;
        }
        let name = f["name"]
            .as_str()
            .filter(|n| !n.is_empty())
            .ok_or_else(|| {
                TranslateError::InvalidRequest("tool function 'name' must be a string".into())
            })?;
        let parameters = match &f["parameters"] {
            Value::Null => json!({"type": "object"}),
            v @ Value::Object(_) => v.clone(),
            _ => {
                return Err(TranslateError::InvalidRequest(
                    "tool function 'parameters' must be an object".into(),
                ))
            }
        };
        let description = match &f["description"] {
            Value::Null => None,
            Value::String(d) => Some(d.clone()),
            _ => {
                return Err(TranslateError::InvalidRequest(
                    "tool function 'description' must be a string".into(),
                ))
            }
        };
        out.push(Tool {
            name: name.to_string(),
            description,
            parameters,
        });
    }
    Ok(out)
}

fn parse_tool_choice(v: Value) -> Result<ToolChoice, TranslateError> {
    match &v {
        Value::String(s) => match s.as_str() {
            "auto" => Ok(ToolChoice::Auto),
            "none" => Ok(ToolChoice::None),
            "required" => Ok(ToolChoice::Required),
            other => Err(TranslateError::InvalidRequest(format!(
                "tool_choice '{other}' is not valid"
            ))),
        },
        Value::Object(o) => {
            reject_unknown(o, &["type", "function"])?;
            if v["type"].as_str() != Some("function") {
                return Err(TranslateError::Unsupported(
                    "tool_choice type is not supported yet".into(),
                ));
            }
            v["function"]["name"]
                .as_str()
                .map(|n| ToolChoice::Tool(n.to_string()))
                .ok_or_else(|| {
                    TranslateError::InvalidRequest("tool_choice function 'name' is required".into())
                })
        }
        _ => Err(TranslateError::InvalidRequest(
            "tool_choice must be a string or an object".into(),
        )),
    }
}

fn parse_tool_calls(calls: Vec<Value>) -> Result<Vec<ToolCall>, TranslateError> {
    let mut out = Vec::with_capacity(calls.len());
    for c in &calls {
        if let Some(fields) = c.as_object() {
            reject_unknown(fields, &["id", "type", "function"])?;
        }
        if c["type"].as_str().is_some_and(|t| t != "function") {
            return Err(TranslateError::Unsupported(
                "tool call type is not supported yet".into(),
            ));
        }
        if let Some(fields) = c["function"].as_object() {
            reject_unknown(fields, &["name", "arguments"])?;
        }
        let field = |v: &Value, what: &str| -> Result<String, TranslateError> {
            v.as_str().map(str::to_string).ok_or_else(|| {
                TranslateError::InvalidRequest(format!("tool call '{what}' must be a string"))
            })
        };
        out.push(ToolCall {
            id: field(&c["id"], "id")?,
            name: field(&c["function"]["name"], "name")?,
            arguments: field(&c["function"]["arguments"], "arguments")?,
        });
    }
    Ok(out)
}

pub fn parse_request(body: &[u8]) -> Result<ChatRequest, TranslateError> {
    let mut wire: WireRequest =
        serde_json::from_slice(body).map_err(|e| TranslateError::InvalidRequest(e.to_string()))?;
    // `n` is only accepted as the integer 1, which is also the default.
    if let Some(n) = wire.extra.remove("n") {
        if !n.is_null() && n.as_u64() != Some(1) {
            return Err(unsupported_field("n"));
        }
    }
    reject_unknown(&wire.extra, IGNORED_REQUEST_FIELDS)?;
    if wire.messages.is_empty() {
        return Err(TranslateError::InvalidRequest(
            "messages must not be empty".into(),
        ));
    }
    let mut messages = Vec::with_capacity(wire.messages.len());
    for m in wire.messages {
        let role = match m.role.as_str() {
            "system" | "developer" => Role::System,
            "user" => Role::User,
            "assistant" => Role::Assistant,
            "tool" => Role::Tool,
            other => {
                return Err(TranslateError::Unsupported(format!(
                    "message role '{other}' is not supported yet"
                )))
            }
        };
        reject_unknown(&m.extra, &[])?;
        let tool_calls = parse_tool_calls(m.tool_calls.unwrap_or_default())?;
        if !tool_calls.is_empty() && role != Role::Assistant {
            return Err(TranslateError::InvalidRequest(
                "tool_calls are only allowed in assistant messages".into(),
            ));
        }
        if role != Role::Tool && m.tool_call_id.is_some() {
            return Err(TranslateError::InvalidRequest(
                "tool_call_id is only allowed in tool messages".into(),
            ));
        }
        if role == Role::Tool && m.tool_call_id.is_none() {
            return Err(TranslateError::InvalidRequest(
                "tool messages need a tool_call_id".into(),
            ));
        }
        let content = match m.content {
            None if !tool_calls.is_empty() => Vec::new(),
            None => {
                return Err(TranslateError::InvalidRequest(
                    "message content is required".into(),
                ))
            }
            Some(WireContent::Text(t)) if t.is_empty() && !tool_calls.is_empty() => Vec::new(),
            Some(WireContent::Text(t)) => vec![Part::Text(t)],
            Some(WireContent::Parts(parts)) => {
                let mut out = Vec::with_capacity(parts.len());
                for p in &parts {
                    out.push(parse_part(p, role == Role::User)?);
                }
                out
            }
        };
        messages.push(Message {
            role,
            content,
            name: m.name,
            tool_calls,
            tool_call_id: if role == Role::Tool {
                m.tool_call_id
            } else {
                None
            },
        });
    }
    Ok(ChatRequest {
        model: wire.model,
        messages,
        max_tokens: wire.max_completion_tokens.or(wire.max_tokens),
        temperature: wire.temperature,
        top_p: wire.top_p,
        stop: wire.stop.map(|s| match s {
            StopField::One(s) => vec![s],
            StopField::Many(v) => v,
        }),
        stream: wire.stream,
        tools: parse_tools(wire.tools.unwrap_or_default())?,
        tool_choice: match wire.tool_choice {
            None | Some(Value::Null) => None,
            Some(v) => Some(parse_tool_choice(v)?),
        },
        parallel_tool_calls: wire.parallel_tool_calls,
    })
}

/// The total is 64-bit: both counts may be saturated at `u32::MAX`.
fn usage_json(u: Usage) -> Value {
    json!({
        "prompt_tokens": u.input_tokens,
        "completion_tokens": u.output_tokens,
        "total_tokens": u64::from(u.input_tokens) + u64::from(u.output_tokens),
    })
}

pub fn render_response(r: &ChatResponse, created: u64) -> Value {
    let content = if r.content.is_empty() && !r.tool_calls.is_empty() {
        Value::Null
    } else {
        Value::String(r.content.clone())
    };
    let mut message = json!({ "role": "assistant", "content": content });
    if !r.tool_calls.is_empty() {
        message["tool_calls"] = r
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
    let mut v = json!({
        "id": r.id,
        "object": "chat.completion",
        "created": created,
        "model": r.model,
        "choices": [{
            "index": 0,
            "message": message,
            "finish_reason": r.finish_reason.map(|f| f.as_openai()),
        }],
    });
    if let Some(u) = r.usage {
        v["usage"] = usage_json(u);
    }
    v
}

pub fn render_stream_event(ev: &StreamEvent, id: &str, model: &str, created: u64) -> String {
    match ev {
        StreamEvent::Delta { text } => {
            let v = json!({
                "id": id, "object": "chat.completion.chunk", "created": created, "model": model,
                "choices": [{ "index": 0, "delta": { "content": text }, "finish_reason": null }],
            });
            format!("data: {v}\n\n")
        }
        StreamEvent::ToolCallStart {
            index,
            id: call_id,
            name,
        } => {
            let v = json!({
                "id": id, "object": "chat.completion.chunk", "created": created, "model": model,
                "choices": [{ "index": 0, "delta": { "tool_calls": [{
                    "index": index, "id": call_id, "type": "function",
                    "function": { "name": name, "arguments": "" },
                }] }, "finish_reason": null }],
            });
            format!("data: {v}\n\n")
        }
        StreamEvent::ToolCallDelta { index, arguments } => {
            let v = json!({
                "id": id, "object": "chat.completion.chunk", "created": created, "model": model,
                "choices": [{ "index": 0, "delta": { "tool_calls": [{
                    "index": index, "function": { "arguments": arguments },
                }] }, "finish_reason": null }],
            });
            format!("data: {v}\n\n")
        }
        StreamEvent::Done {
            finish_reason,
            usage,
        } => {
            let mut v = json!({
                "id": id, "object": "chat.completion.chunk", "created": created, "model": model,
                "choices": [{ "index": 0, "delta": {}, "finish_reason": finish_reason.map(|f| f.as_openai()) }],
            });
            if let Some(u) = usage {
                v["usage"] = usage_json(*u);
            }
            format!("data: {v}\n\ndata: [DONE]\n\n")
        }
    }
}

pub fn render_error(kind: &str, message: &str) -> Value {
    json!({ "error": { "message": message, "type": kind, "param": null, "code": null } })
}

pub fn render_stream_error(message: &str) -> String {
    format!("data: {}\n\n", render_error("upstream_error", message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::TranslateError;
    use crate::types::*;

    #[test]
    fn parses_minimal_request() {
        let body = br#"{"model":"openai/gpt-4o","messages":[{"role":"user","content":"hi"}]}"#;
        let req = parse_request(body).unwrap();
        assert_eq!(req.model, "openai/gpt-4o");
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.messages[0].role, Role::User);
        assert_eq!(req.messages[0].joined_text(), "hi");
        assert!(!req.stream);
    }

    #[test]
    fn parses_text_parts_and_single_stop() {
        let body = br#"{"model":"m","stream":true,"stop":"END","max_completion_tokens":9,
            "messages":[{"role":"developer","content":[{"type":"text","text":"a"},{"type":"text","text":"b"}]}]}"#;
        let req = parse_request(body).unwrap();
        assert_eq!(req.messages[0].role, Role::System);
        assert_eq!(
            req.messages[0].content,
            vec![Part::Text("a".into()), Part::Text("b".into())]
        );
        assert_eq!(req.stop, Some(vec!["END".to_string()]));
        assert_eq!(req.max_tokens, Some(9));
        assert!(req.stream);
    }

    fn req_with_tools(extra: &str) -> String {
        format!(r#"{{"model":"m","messages":[{{"role":"user","content":"x"}}],{extra}}}"#)
    }

    #[test]
    fn parses_tools_and_tool_choice() {
        let body = req_with_tools(
            r#""tools":[
              {"type":"function","function":{"name":"a","description":"does a","parameters":{"type":"object","properties":{"x":{"type":"string"}}},"strict":true}},
              {"type":"function","function":{"name":"b"}}],
              "tool_choice":"required","parallel_tool_calls":false"#,
        );
        let req = parse_request(body.as_bytes()).unwrap();
        assert_eq!(req.tools.len(), 2);
        assert_eq!(req.tools[0].name, "a");
        assert_eq!(req.tools[0].description.as_deref(), Some("does a"));
        assert_eq!(req.tools[0].parameters["properties"]["x"]["type"], "string");
        assert_eq!(req.tools[1].name, "b");
        assert_eq!(req.tools[1].description, None);
        assert_eq!(req.tools[1].parameters, json!({"type":"object"}));
        assert_eq!(req.tool_choice, Some(ToolChoice::Required));
        assert_eq!(req.parallel_tool_calls, Some(false));

        for (wire, want) in [
            (r#""auto""#, ToolChoice::Auto),
            (r#""none""#, ToolChoice::None),
            (r#""required""#, ToolChoice::Required),
            (
                r#"{"type":"function","function":{"name":"b"}}"#,
                ToolChoice::Tool("b".into()),
            ),
        ] {
            let req = parse_request(req_with_tools(&format!(r#""tool_choice":{wire}"#)).as_bytes())
                .unwrap();
            assert_eq!(req.tool_choice, Some(want), "{wire}");
        }
        let plain = parse_request(req_with_tools(r#""tool_choice":null"#).as_bytes()).unwrap();
        assert!(plain.tools.is_empty());
        assert_eq!(plain.tool_choice, None);
        assert_eq!(plain.parallel_tool_calls, None);
    }

    #[test]
    fn parses_a_tool_conversation() {
        let args1 = "{\\\"a\\\": 1,  \\\"b\\\":[ ]}";
        let body = format!(
            r#"{{"model":"m","messages":[
            {{"role":"user","content":"go"}},
            {{"role":"assistant","content":null,"tool_calls":[
              {{"id":"c1","type":"function","function":{{"name":"f","arguments":"{args1}"}}}},
              {{"id":"c2","type":"function","function":{{"name":"g","arguments":"{{}}"}}}}]}},
            {{"role":"tool","tool_call_id":"c1","content":"r1"}},
            {{"role":"tool","tool_call_id":"c2","content":[{{"type":"text","text":"r"}},{{"type":"text","text":"2"}}]}},
            {{"role":"user","content":"thanks"}}]}}"#
        );
        let req = parse_request(body.as_bytes()).unwrap();
        assert_eq!(req.messages.len(), 5);
        let a = &req.messages[1];
        assert_eq!(a.role, Role::Assistant);
        assert!(a.content.is_empty());
        assert_eq!(a.tool_calls.len(), 2);
        assert_eq!(a.tool_calls[0].id, "c1");
        assert_eq!(a.tool_calls[0].name, "f");
        assert_eq!(a.tool_calls[0].arguments, "{\"a\": 1,  \"b\":[ ]}");
        assert_eq!(a.tool_calls[1].id, "c2");
        assert_eq!(a.tool_calls[1].arguments, "{}");
        assert_eq!(req.messages[2].role, Role::Tool);
        assert_eq!(req.messages[2].tool_call_id.as_deref(), Some("c1"));
        assert_eq!(req.messages[2].joined_text(), "r1");
        assert_eq!(req.messages[3].tool_call_id.as_deref(), Some("c2"));
        assert_eq!(req.messages[3].joined_text(), "r2");
        assert_eq!(req.messages[4].joined_text(), "thanks");

        for content in [r#""content":"","#, ""] {
            let body = format!(
                r#"{{"model":"m","messages":[{{"role":"assistant",{content}"tool_calls":[{{"id":"c","type":"function","function":{{"name":"f","arguments":"{{}}"}}}}]}}]}}"#
            );
            let req = parse_request(body.as_bytes()).unwrap();
            assert_eq!(req.messages[0].tool_calls.len(), 1, "{content}");
            assert_eq!(req.messages[0].joined_text(), "");
        }
    }

    #[test]
    fn tool_message_without_tool_call_id_is_invalid() {
        let body = br#"{"model":"m","messages":[{"role":"tool","content":"x"}]}"#;
        assert!(matches!(
            parse_request(body),
            Err(TranslateError::InvalidRequest(_))
        ));
    }

    #[test]
    fn tool_call_id_on_a_non_tool_message_is_invalid() {
        let body =
            br#"{"model":"m","messages":[{"role":"user","content":"x","tool_call_id":"c"}]}"#;
        match parse_request(body) {
            Err(TranslateError::InvalidRequest(m)) => {
                assert_eq!(m, "tool_call_id is only allowed in tool messages")
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn tool_parameters_and_description_must_have_their_types() {
        for (f, want) in [
            (
                r#"{"name":"f","parameters":"x"}"#,
                "tool function 'parameters' must be an object",
            ),
            (
                r#"{"name":"f","parameters":[1]}"#,
                "tool function 'parameters' must be an object",
            ),
            (
                r#"{"name":"f","description":3}"#,
                "tool function 'description' must be a string",
            ),
        ] {
            let body = format!(
                r#"{{"model":"m","messages":[{{"role":"user","content":"x"}}],"tools":[{{"type":"function","function":{f}}}]}}"#
            );
            match parse_request(body.as_bytes()) {
                Err(TranslateError::InvalidRequest(m)) => assert_eq!(m, want),
                other => panic!("{other:?}"),
            }
        }
    }

    #[test]
    fn image_in_assistant_message_is_invalid() {
        let body = br#"{"model":"m","messages":[{"role":"assistant","content":[{"type":"image_url","image_url":{"url":"https://x/a.png"}}]}]}"#;
        match parse_request(body) {
            Err(TranslateError::InvalidRequest(m)) => {
                assert_eq!(m, "images are only allowed in user messages")
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn non_function_tool_is_unsupported() {
        let body = req_with_tools(r#""tools":[{"type":"code_interpreter"}]"#);
        assert!(matches!(
            parse_request(body.as_bytes()),
            Err(TranslateError::Unsupported(_))
        ));
    }

    #[test]
    fn legacy_function_call_is_unsupported() {
        let body = br#"{"model":"m","messages":[{"role":"assistant","content":"x","function_call":{"name":"f","arguments":"{}"}}]}"#;
        assert!(matches!(
            parse_request(body),
            Err(TranslateError::Unsupported(_))
        ));
    }

    #[test]
    fn parses_image_parts_in_order() {
        let body = with_part(
            r#"{"type":"text","text":"a"},
            {"type":"image_url","image_url":{"url":"https://x.test/p.png","detail":"high"}},
            {"type":"image_url","image_url":{"url":"data:image/png;base64,QUJD"}},
            {"type":"text","text":"b"}"#,
        );
        let req = parse_request(body.as_bytes()).unwrap();
        assert_eq!(
            req.messages[0].content,
            vec![
                Part::Text("a".into()),
                Part::Image(ImageSource::Url("https://x.test/p.png".into())),
                Part::Image(ImageSource::Base64 {
                    media_type: "image/png".into(),
                    data: "QUJD".into()
                }),
                Part::Text("b".into()),
            ]
        );
    }

    #[test]
    fn rejects_bad_data_urls() {
        for url in [
            "data:text/plain;base64,QQ==",
            "data:image/png,raw",
            "data:image/png;base64,***",
        ] {
            let body = with_part(&format!(
                r#"{{"type":"image_url","image_url":{{"url":"{url}"}}}}"#
            ));
            assert!(
                matches!(
                    parse_request(body.as_bytes()),
                    Err(TranslateError::InvalidRequest(_))
                ),
                "{url}"
            );
        }
    }

    #[test]
    fn renders_tool_calls_in_response() {
        let r = ChatResponse {
            id: "id1".into(),
            model: "m".into(),
            content: String::new(),
            tool_calls: vec![ToolCall {
                id: "c1".into(),
                name: "f".into(),
                arguments: "{\"a\": 1}".into(),
            }],
            finish_reason: Some(FinishReason::ToolCalls),
            usage: None,
        };
        let v = render_response(&r, 1);
        let m = &v["choices"][0]["message"];
        assert!(m["content"].is_null());
        assert_eq!(m["tool_calls"][0]["id"], "c1");
        assert_eq!(m["tool_calls"][0]["type"], "function");
        assert_eq!(m["tool_calls"][0]["function"]["name"], "f");
        assert_eq!(m["tool_calls"][0]["function"]["arguments"], "{\"a\": 1}");
        assert_eq!(v["choices"][0]["finish_reason"], "tool_calls");

        let with_text = ChatResponse {
            content: "hi".into(),
            ..r
        };
        let v = render_response(&with_text, 1);
        assert_eq!(v["choices"][0]["message"]["content"], "hi");
        assert!(v["choices"][0]["message"]["tool_calls"].is_array());
    }

    #[test]
    fn renders_tool_call_stream_events() {
        let chunk = |ev: &StreamEvent| -> serde_json::Value {
            let s = render_stream_event(ev, "id1", "m", 1);
            serde_json::from_str(s["data: ".len()..].trim()).unwrap()
        };
        let start = chunk(&StreamEvent::ToolCallStart {
            index: 0,
            id: "c1".into(),
            name: "f".into(),
        });
        let t = &start["choices"][0]["delta"]["tool_calls"][0];
        assert_eq!(t["index"], 0);
        assert_eq!(t["id"], "c1");
        assert_eq!(t["type"], "function");
        assert_eq!(t["function"]["name"], "f");
        assert_eq!(t["function"]["arguments"], "");

        for (part, want) in [("{\"a\"", "{\"a\""), (":1}", ":1}")] {
            let d = chunk(&StreamEvent::ToolCallDelta {
                index: 0,
                arguments: part.into(),
            });
            let t = &d["choices"][0]["delta"]["tool_calls"][0];
            assert_eq!(t["index"], 0);
            assert_eq!(t["function"]["arguments"], want);
            assert!(t.get("id").is_none());
            assert!(t.get("type").is_none());
            assert!(t["function"].get("name").is_none());
        }
    }

    #[test]
    fn rejects_bad_json_and_empty_messages() {
        assert!(matches!(
            parse_request(b"{"),
            Err(TranslateError::InvalidRequest(_))
        ));
        let empty = br#"{"model":"m","messages":[]}"#;
        assert!(matches!(
            parse_request(empty),
            Err(TranslateError::InvalidRequest(_))
        ));
    }

    #[test]
    fn renders_response() {
        let r = ChatResponse {
            id: "id1".into(),
            model: "gpt-4o".into(),
            content: "hello".into(),
            tool_calls: Vec::new(),
            finish_reason: Some(FinishReason::Stop),
            usage: Some(Usage {
                input_tokens: 3,
                output_tokens: 2,
            }),
        };
        let v = render_response(&r, 100);
        assert_eq!(v["object"], "chat.completion");
        assert_eq!(v["created"], 100);
        assert_eq!(v["choices"][0]["message"]["content"], "hello");
        assert_eq!(v["choices"][0]["finish_reason"], "stop");
        assert_eq!(v["usage"]["prompt_tokens"], 3);
        assert_eq!(v["usage"]["completion_tokens"], 2);
        assert_eq!(v["usage"]["total_tokens"], 5);
    }

    #[test]
    fn renders_stream_events() {
        let d = render_stream_event(
            &StreamEvent::Delta {
                text: "a\"b".into(),
            },
            "id1",
            "m",
            1,
        );
        assert!(d.starts_with("data: "));
        assert!(d.ends_with("\n\n"));
        let v: serde_json::Value = serde_json::from_str(d["data: ".len()..].trim()).unwrap();
        assert_eq!(v["object"], "chat.completion.chunk");
        assert_eq!(v["choices"][0]["delta"]["content"], "a\"b");

        let done = render_stream_event(
            &StreamEvent::Done {
                finish_reason: Some(FinishReason::Length),
                usage: None,
            },
            "id1",
            "m",
            1,
        );
        assert!(done.contains("\"finish_reason\":\"length\""));
        assert!(done.ends_with("data: [DONE]\n\n"));
    }

    #[test]
    fn error_text_is_escaped() {
        let v = render_error("invalid_request_error", "bad \"quote\"");
        assert_eq!(v["error"]["message"], "bad \"quote\"");
        assert_eq!(v["error"]["type"], "invalid_request_error");
        let s = render_stream_error("x\ny");
        let v: serde_json::Value = serde_json::from_str(s["data: ".len()..].trim()).unwrap();
        assert_eq!(v["error"]["message"], "x\ny");
    }

    fn unsupported_message(body: &str) -> String {
        match parse_request(body.as_bytes()) {
            Err(TranslateError::Unsupported(m)) => m,
            other => panic!("expected Unsupported for {body}, got {other:?}"),
        }
    }

    #[test]
    fn rejects_every_unsupported_top_level_field() {
        let cases = [
            ("functions", r#"[{"name":"f"}]"#),
            ("function_call", r#""auto""#),
            ("response_format", r#"{"type":"json_object"}"#),
            ("logit_bias", r#"{"50256":-100}"#),
            ("logprobs", "true"),
            ("top_logprobs", "2"),
            ("presence_penalty", "0.5"),
            ("frequency_penalty", "0.5"),
            ("seed", "7"),
            ("audio", r#"{"voice":"alloy","format":"wav"}"#),
            ("modalities", r#"["text","audio"]"#),
            ("prediction", r#"{"type":"content","content":"x"}"#),
            ("reasoning_effort", r#""low""#),
        ];
        for (field, value) in cases {
            let body = format!(
                r#"{{"model":"m","messages":[{{"role":"user","content":"x"}}],"{field}":{value}}}"#
            );
            let msg = unsupported_message(&body);
            assert!(msg.contains(&format!("'{field}'")), "{field}: {msg}");

            let null_body = format!(
                r#"{{"model":"m","messages":[{{"role":"user","content":"x"}}],"{field}":null}}"#
            );
            assert!(parse_request(null_body.as_bytes()).is_ok(), "{field} null");
        }
    }

    #[test]
    fn rejects_n_other_than_one() {
        let body = |n: &str| {
            format!(r#"{{"model":"m","messages":[{{"role":"user","content":"x"}}],"n":{n}}}"#)
        };
        assert!(parse_request(body("1").as_bytes()).is_ok());
        assert!(parse_request(body("null").as_bytes()).is_ok());
        for n in ["2", "0", "\"1\"", "1.5"] {
            let msg = unsupported_message(&body(n));
            assert!(msg.contains("'n'"), "{n}: {msg}");
        }
    }

    #[test]
    fn rejects_unsupported_message_fields() {
        let cases = [
            ("function_call", r#"{"name":"f","arguments":"{}"}"#),
            ("audio", r#"{"id":"a1"}"#),
        ];
        for (field, value) in cases {
            let body = format!(
                r#"{{"model":"m","messages":[{{"role":"assistant","content":"x","{field}":{value}}}]}}"#
            );
            let msg = unsupported_message(&body);
            assert!(msg.contains(&format!("'{field}'")), "{field}: {msg}");

            let null_body = format!(
                r#"{{"model":"m","messages":[{{"role":"assistant","content":"x","{field}":null}}]}}"#
            );
            assert!(parse_request(null_body.as_bytes()).is_ok(), "{field} null");
        }
    }

    #[test]
    fn rejects_missing_or_null_content() {
        for body in [
            r#"{"model":"m","messages":[{"role":"assistant"}]}"#,
            r#"{"model":"m","messages":[{"role":"assistant","content":null}]}"#,
            r#"{"model":"m","messages":[{"role":"user","content":null}]}"#,
        ] {
            match parse_request(body.as_bytes()) {
                Err(TranslateError::InvalidRequest(m)) => {
                    assert_eq!(m, "message content is required")
                }
                other => panic!("expected InvalidRequest for {body}, got {other:?}"),
            }
        }
    }

    #[test]
    fn accepts_and_ignores_fields_that_do_not_change_output() {
        let body = br#"{"model":"m","messages":[{"role":"user","content":"x"}],
            "user":"u1","metadata":{"k":"v"},"store":true,"stream_options":{"include_usage":true},
            "service_tier":"auto"}"#;
        let req = parse_request(body).unwrap();
        assert_eq!(req.messages[0].joined_text(), "x");
    }

    #[test]
    fn saturated_token_counts_do_not_overflow_the_total() {
        let usage = Some(Usage {
            input_tokens: u32::MAX,
            output_tokens: u32::MAX,
        });
        let r = ChatResponse {
            id: "id1".into(),
            model: "m".into(),
            content: "x".into(),
            tool_calls: Vec::new(),
            finish_reason: Some(FinishReason::Stop),
            usage,
        };
        let v = render_response(&r, 1);
        assert_eq!(v["usage"]["prompt_tokens"], u32::MAX);
        assert_eq!(v["usage"]["completion_tokens"], u32::MAX);
        assert_eq!(v["usage"]["total_tokens"], 8589934590u64);

        let done = render_stream_event(
            &StreamEvent::Done {
                finish_reason: Some(FinishReason::Stop),
                usage,
            },
            "id1",
            "m",
            1,
        );
        let first = done.lines().next().unwrap();
        let v: serde_json::Value = serde_json::from_str(&first["data: ".len()..]).unwrap();
        assert_eq!(v["usage"]["total_tokens"], 8589934590u64);
    }

    fn with_top_level(field: &str, value: &str) -> String {
        format!(r#"{{"model":"m","messages":[{{"role":"user","content":"x"}}],"{field}":{value}}}"#)
    }

    #[test]
    fn rejects_top_level_fields_that_are_not_on_the_allowlist() {
        let cases = [
            ("web_search_options", r#"{"search_context_size":"low"}"#),
            ("top_k", "40"),
            ("some_future_field", "123"),
        ];
        for (field, value) in cases {
            let msg = unsupported_message(&with_top_level(field, value));
            assert_eq!(msg, format!("field '{field}' is not supported yet"));
            assert!(
                parse_request(with_top_level(field, "null").as_bytes()).is_ok(),
                "{field} null"
            );
        }
    }

    #[test]
    fn accepts_each_ignored_field_on_its_own() {
        let cases = [
            ("user", r#""u1""#),
            ("metadata", r#"{"k":"v"}"#),
            ("store", "true"),
            ("stream_options", r#"{"include_usage":true}"#),
            ("service_tier", r#""auto""#),
        ];
        for (field, value) in cases {
            for v in [value, "null"] {
                let req = parse_request(with_top_level(field, v).as_bytes())
                    .unwrap_or_else(|e| panic!("{field}={v}: {e:?}"));
                assert_eq!(req.messages[0].joined_text(), "x");
            }
        }
    }

    #[test]
    fn carried_fields_are_still_carried() {
        let body = br#"{"model":"m","messages":[{"role":"user","content":"x","name":"bob"}],
            "max_tokens":5,"temperature":0.5,"top_p":0.25,"stop":["a","b"],"stream":true,"n":1}"#;
        let req = parse_request(body).unwrap();
        assert_eq!(req.max_tokens, Some(5));
        assert_eq!(req.temperature, Some(0.5));
        assert_eq!(req.top_p, Some(0.25));
        assert_eq!(req.stop, Some(vec!["a".to_string(), "b".to_string()]));
        assert!(req.stream);
        assert_eq!(req.messages[0].name.as_deref(), Some("bob"));
    }

    #[test]
    fn rejects_message_fields_that_are_not_on_the_allowlist() {
        for (field, value) in [("refusal", r#""no""#), ("some_future_field", "1")] {
            let body = format!(
                r#"{{"model":"m","messages":[{{"role":"assistant","content":"x","{field}":{value}}}]}}"#
            );
            assert_eq!(
                unsupported_message(&body),
                format!("field '{field}' is not supported yet")
            );
            let null_body = format!(
                r#"{{"model":"m","messages":[{{"role":"assistant","content":"x","{field}":null}}]}}"#
            );
            assert!(parse_request(null_body.as_bytes()).is_ok(), "{field} null");
        }
    }

    fn with_part(part: &str) -> String {
        format!(r#"{{"model":"m","messages":[{{"role":"user","content":[{part}]}}]}}"#)
    }

    #[test]
    fn text_part_without_a_string_text_is_invalid() {
        for part in [
            r#"{"type":"text"}"#,
            r#"{"type":"text","text":null}"#,
            r#"{"type":"text","text":5}"#,
        ] {
            let got = parse_request(with_part(part).as_bytes());
            assert!(
                matches!(got, Err(TranslateError::InvalidRequest(_))),
                "{part}: {got:?}"
            );
        }
    }

    #[test]
    fn rejects_content_part_fields_that_are_not_on_the_allowlist() {
        let part = r#"{"type":"text","text":"x","cache_control":{"type":"ephemeral"}}"#;
        assert_eq!(
            unsupported_message(&with_part(part)),
            "field 'cache_control' is not supported yet"
        );
        let part = r#"{"type":"text","text":"x","cache_control":null}"#;
        let req = parse_request(with_part(part).as_bytes()).unwrap();
        assert_eq!(req.messages[0].joined_text(), "x");
    }
}
