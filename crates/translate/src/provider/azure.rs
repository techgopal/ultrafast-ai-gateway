//! Azure OpenAI: OpenAI's format on a deployment URL. Answers and streams are
//! read by the OpenAI module.

use super::{openai, path_segment, HttpRequest, Target, DEFAULT_AZURE_API_VERSION};
use crate::error::TranslateError;
use crate::types::ChatRequest;

pub(crate) fn build(target: &Target, req: &ChatRequest) -> Result<HttpRequest, TranslateError> {
    let api_version = target
        .api_version
        .as_deref()
        .unwrap_or(DEFAULT_AZURE_API_VERSION);
    let mut headers = vec![("content-type".to_string(), "application/json".to_string())];
    if let Some(k) = &target.api_key {
        headers.push(("api-key".to_string(), k.clone()));
    }
    Ok(HttpRequest {
        method: "POST",
        url: format!(
            "{}/openai/deployments/{}/chat/completions?api-version={}",
            target.base_url.trim_end_matches('/'),
            path_segment(&target.model),
            api_version
        ),
        headers,
        body: openai::body(req, None)?,
    })
}

#[cfg(test)]
mod tests {
    use crate::error::TranslateError;
    use crate::provider::*;
    use crate::types::*;

    fn target(api_version: Option<&str>) -> Target {
        Target {
            kind: ProviderKind::Azure,
            base_url: "https://res.openai.azure.com/".into(),
            api_key: Some("az-key".into()),
            model: "my-gpt4o".into(),
            api_version: api_version.map(str::to_string),
        }
    }

    fn request(stream: bool) -> ChatRequest {
        ChatRequest {
            model: "az/my-gpt4o".into(),
            messages: vec![
                Message::text(Role::System, "be brief"),
                Message {
                    name: Some("ann".into()),
                    ..Message::text(Role::User, "hi")
                },
            ],
            max_tokens: Some(5),
            temperature: Some(0.5),
            top_p: None,
            stop: Some(vec!["x".into()]),
            stream,
            tools: Vec::new(),
            tool_choice: None,
            parallel_tool_calls: None,
        }
    }

    #[test]
    fn builds_a_deployment_request() {
        let r = build_request(&target(Some("2025-01-01-preview")), &request(false)).unwrap();
        assert_eq!(r.method, "POST");
        assert_eq!(
            r.url,
            "https://res.openai.azure.com/openai/deployments/my-gpt4o/chat/completions?api-version=2025-01-01-preview"
        );
        assert!(r.headers.contains(&("api-key".into(), "az-key".into())));
        assert!(!r.headers.iter().any(|(k, _)| k == "authorization"));
        let v: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
        assert!(v.get("model").is_none(), "{v}");
        assert_eq!(v["messages"][0]["role"], "system");
        assert_eq!(v["messages"][1]["name"], "ann");
        assert_eq!(v["max_tokens"], 5);
        assert_eq!(v["stop"][0], "x");
        assert!(v.get("stream").is_none());
    }

    #[test]
    fn the_default_api_version_applies_when_none_is_set() {
        let r = build_request(&target(None), &request(false)).unwrap();
        assert!(r.url.ends_with("?api-version=2024-10-21"), "{}", r.url);
    }

    #[test]
    fn a_stream_asks_for_usage() {
        let r = build_request(&target(None), &request(true)).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(v["stream"], true);
        assert_eq!(v["stream_options"]["include_usage"], true);
    }

    #[test]
    fn the_deployment_name_is_escaped() {
        let mut t = target(None);
        t.model = "a/b?c".into();
        let r = build_request(&t, &request(false)).unwrap();
        assert!(r.url.contains("/deployments/a%2Fb%3Fc/chat"), "{}", r.url);
    }

    #[test]
    fn parses_a_response_with_filter_annotations() {
        let body = include_bytes!("../../tests/fixtures/azure/response.json");
        let r = parse_response(ProviderKind::Azure, 200, body).unwrap();
        assert_eq!(r.content, "Hello from Azure");
        assert_eq!(r.model, "gpt-4o-2024-08-06");
        assert_eq!(r.finish_reason, Some(FinishReason::Stop));
        assert_eq!(
            r.usage,
            Some(Usage {
                input_tokens: 9,
                output_tokens: 4
            })
        );
    }

    #[test]
    fn errors_keep_their_status_and_message() {
        let body = include_bytes!("../../tests/fixtures/azure/error_429.json");
        let e = parse_response(ProviderKind::Azure, 429, body).unwrap_err();
        match e {
            TranslateError::Provider {
                status,
                retryable,
                message,
            } => {
                assert_eq!(status, 429);
                assert!(retryable);
                assert!(message.contains("token rate limit"), "{message}");
            }
            other => panic!("{other:?}"),
        }
        let body = include_bytes!("../../tests/fixtures/azure/error_content_filter.json");
        let e = parse_response(ProviderKind::Azure, 400, body).unwrap_err();
        assert!(
            matches!(&e, TranslateError::Provider { status: 400, retryable: false, message } if message.contains("content management policy")),
            "{e:?}"
        );
    }

    #[test]
    fn decodes_a_stream_with_a_leading_filter_chunk_at_every_split() {
        let input = include_bytes!("../../tests/fixtures/azure/stream.sse");
        for split in 0..=input.len() {
            let mut d = StreamDecoder::new(ProviderKind::Azure);
            let mut events = d.feed(&input[..split]).unwrap();
            events.extend(d.feed(&input[split..]).unwrap());
            assert_eq!(
                events,
                vec![
                    StreamEvent::Delta { text: "Hel".into() },
                    StreamEvent::Delta { text: "lo".into() },
                    StreamEvent::Done {
                        finish_reason: Some(FinishReason::Stop),
                        usage: Some(Usage {
                            input_tokens: 9,
                            output_tokens: 2
                        }),
                    },
                ],
                "split {split}"
            );
        }
    }

    #[test]
    fn azure_body_carries_tools() {
        let mut req = request(false);
        req.tools = vec![Tool {
            name: "f".into(),
            description: None,
            parameters: serde_json::json!({"type": "object"}),
        }];
        req.tool_choice = Some(ToolChoice::Required);
        let r = build_request(&target(None), &req).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(v["tools"][0]["function"]["name"], "f");
        assert!(v["tools"][0]["function"].get("description").is_none());
        assert_eq!(v["tool_choice"], "required");
        assert!(v.get("model").is_none());
    }
}
