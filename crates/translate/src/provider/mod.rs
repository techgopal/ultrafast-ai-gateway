//! Outbound side: turning a common request into a provider call and back.

mod anthropic;
mod openai;

use std::fmt;

use serde_json::Value;

use crate::error::TranslateError;
use crate::sse::SseParser;
use crate::types::{ChatRequest, ChatResponse, FinishReason, StreamEvent, Usage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    /// OpenAI and every OpenAI-compatible API (Groq, Mistral, OpenRouter, Ollama).
    OpenAi,
    Anthropic,
}

impl ProviderKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "openai" => Some(ProviderKind::OpenAi),
            "anthropic" => Some(ProviderKind::Anthropic),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ProviderKind::OpenAi => "openai",
            ProviderKind::Anthropic => "anthropic",
        }
    }
}

const REDACTED: &str = "[redacted]";

/// Provider token counts are u64 on the wire; clamp rather than wrap.
pub(crate) fn saturate(n: u64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[derive(Clone, PartialEq)]
pub struct Target {
    pub kind: ProviderKind,
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
}

/// Never prints the API key.
impl fmt::Debug for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Target")
            .field("kind", &self.kind)
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key.as_ref().map(|_| REDACTED))
            .field("model", &self.model)
            .finish()
    }
}

#[derive(Clone, PartialEq)]
pub struct HttpRequest {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// Never prints credential header values or the body, which may hold user content.
impl fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let headers: Vec<(&str, &str)> = self
            .headers
            .iter()
            .map(|(k, v)| {
                let secret =
                    k.eq_ignore_ascii_case("authorization") || k.eq_ignore_ascii_case("x-api-key");
                (k.as_str(), if secret { REDACTED } else { v.as_str() })
            })
            .collect();
        f.debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("url", &self.url)
            .field("headers", &headers)
            .field("body", &format_args!("{} bytes", self.body.len()))
            .finish()
    }
}

pub fn build_request(target: &Target, req: &ChatRequest) -> Result<HttpRequest, TranslateError> {
    match target.kind {
        ProviderKind::OpenAi => openai::build(target, req),
        ProviderKind::Anthropic => anthropic::build(target, req),
    }
}

pub fn parse_response(
    kind: ProviderKind,
    status: u16,
    body: &[u8],
) -> Result<ChatResponse, TranslateError> {
    if status >= 400 {
        return Err(provider_error(status, body));
    }
    match kind {
        ProviderKind::OpenAi => openai::parse(body),
        ProviderKind::Anthropic => anthropic::parse(body),
    }
}

/// Builds a provider error from any body, JSON or not.
pub(crate) fn provider_error(status: u16, body: &[u8]) -> TranslateError {
    let message = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| {
            v["error"]["message"]
                .as_str()
                .or_else(|| v["message"].as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| {
            let text = String::from_utf8_lossy(body);
            text.chars().take(500).collect()
        });
    TranslateError::Provider {
        status,
        retryable: status == 408 || status == 429 || status >= 500,
        message,
    }
}

#[derive(Debug, Default)]
pub(crate) struct StreamState {
    pub finish: Option<FinishReason>,
    pub input_tokens: Option<u32>,
    pub output_tokens: Option<u32>,
}

impl StreamState {
    pub fn usage(&self) -> Option<Usage> {
        match (self.input_tokens, self.output_tokens) {
            (None, None) => None,
            (i, o) => Some(Usage {
                input_tokens: i.unwrap_or(0),
                output_tokens: o.unwrap_or(0),
            }),
        }
    }
}

pub struct StreamDecoder {
    kind: ProviderKind,
    sse: SseParser,
    state: StreamState,
}

impl StreamDecoder {
    pub fn new(kind: ProviderKind) -> Self {
        Self {
            kind,
            sse: SseParser::new(),
            state: StreamState::default(),
        }
    }

    pub fn feed(&mut self, chunk: &[u8]) -> Result<Vec<StreamEvent>, TranslateError> {
        let mut out = Vec::new();
        for ev in self.sse.feed(chunk) {
            // Keepalive events carry no data and are not provider JSON.
            if ev.data.trim().is_empty() {
                continue;
            }
            match self.kind {
                ProviderKind::OpenAi => openai::decode(&mut self.state, &ev, &mut out)?,
                ProviderKind::Anthropic => anthropic::decode(&mut self.state, &ev, &mut out)?,
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_debug_never_prints_the_api_key() {
        let mut t = Target {
            kind: ProviderKind::Anthropic,
            base_url: "https://api.anthropic.com".into(),
            api_key: Some("sk-secret-value".into()),
            model: "m".into(),
        };
        let s = format!("{t:?}");
        assert!(!s.contains("sk-secret-value"), "{s}");
        assert!(s.contains("Some(\"[redacted]\")"), "{s}");
        assert!(s.contains("https://api.anthropic.com"), "{s}");
        t.api_key = None;
        assert!(format!("{t:?}").contains("api_key: None"));
    }

    #[test]
    fn http_request_debug_never_prints_secrets() {
        let r = HttpRequest {
            method: "POST",
            url: "https://x/v1".into(),
            headers: vec![
                ("content-type".into(), "application/json".into()),
                ("Authorization".into(), "Bearer sk-secret-value".into()),
                ("X-Api-Key".into(), "sk-secret-value".into()),
            ],
            body: b"sk-secret-value in body".to_vec(),
        };
        let s = format!("{r:?}");
        assert!(!s.contains("sk-secret-value"), "{s}");
        assert!(s.contains("[redacted]"), "{s}");
        assert!(s.contains("application/json"), "{s}");
        assert!(s.contains("23"), "{s}");
    }

    #[test]
    fn events_with_empty_data_are_skipped_for_every_provider() {
        for kind in [ProviderKind::OpenAi, ProviderKind::Anthropic] {
            let mut d = StreamDecoder::new(kind);
            let got = d
                .feed(b"event: keepalive\n\ndata:\n\ndata:   \n\nevent: ping\ndata: \n\n")
                .unwrap();
            assert_eq!(got, vec![], "{kind:?}");
        }
    }
}
