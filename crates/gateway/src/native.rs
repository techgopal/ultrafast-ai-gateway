//! Pass-through: a coding agent's request, forwarded as it came to a
//! provider that speaks the same API (Claude Code to Anthropic, Codex to
//! OpenAI and Azure), with the agent's own JSON as the source of truth.

use axum::http::HeaderMap;
use serde_json::{Map, Value};
use ufaigw_translate::error::TranslateError;
use ufaigw_translate::provider::{parse_responses_usage, HttpRequest, ProviderKind, Target};
use ufaigw_translate::types::Usage;

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
            let version = target
                .api_version
                .as_deref()
                .unwrap_or(AZURE_RESPONSES_API_VERSION);
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
    let mut body = native.body.clone();
    body.insert("model".to_string(), Value::String(target.model.clone()));
    HttpRequest {
        method: "POST",
        url,
        headers,
        body: serde_json::to_vec(&body).unwrap_or_default(),
    }
}

/// The text of a whole answer that guardrails read and may rewrite: the text
/// of the model and the arguments of the calls it makes.
pub fn answer_slots(api: NativeApi, answer: &mut Value) -> Vec<&mut String> {
    let mut slots = Vec::new();
    match api {
        NativeApi::Messages => {
            for block in answer["content"].as_array_mut().into_iter().flatten() {
                if block["type"] == "text" {
                    if let Some(Value::String(text)) = block.get_mut("text") {
                        slots.push(text);
                    }
                }
            }
        }
        NativeApi::Responses => {
            for item in answer["output"].as_array_mut().into_iter().flatten() {
                match item["type"].as_str() {
                    Some("message") => {
                        for part in item["content"].as_array_mut().into_iter().flatten() {
                            if part["type"] == "output_text" {
                                if let Some(Value::String(text)) = part.get_mut("text") {
                                    slots.push(text);
                                }
                            }
                        }
                    }
                    Some("function_call") => {
                        if let Some(Value::String(args)) = item.get_mut("arguments") {
                            slots.push(args);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    slots
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
}
