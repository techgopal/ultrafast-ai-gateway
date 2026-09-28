//! Outbound side: turning a common request into a provider call and back.

mod openai;

use serde_json::Value;

use crate::error::TranslateError;
use crate::sse::SseParser;
use crate::types::{ChatRequest, ChatResponse, FinishReason, StreamEvent, Usage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    /// OpenAI and every OpenAI-compatible API (Groq, Mistral, OpenRouter, Ollama).
    OpenAi,
}

impl ProviderKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "openai" => Some(ProviderKind::OpenAi),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ProviderKind::OpenAi => "openai",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub kind: ProviderKind,
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HttpRequest {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

pub fn build_request(target: &Target, req: &ChatRequest) -> Result<HttpRequest, TranslateError> {
    match target.kind {
        ProviderKind::OpenAi => openai::build(target, req),
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
            match self.kind {
                ProviderKind::OpenAi => openai::decode(&mut self.state, &ev, &mut out)?,
            }
        }
        Ok(out)
    }
}
