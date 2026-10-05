//! Outbound side: turning a common request into a provider call and back.

mod anthropic;
mod azure;
mod gemini;
mod openai;

use std::fmt;

use serde_json::Value;

use crate::error::TranslateError;
use crate::sse::SseParser;
use crate::types::{ChatRequest, ChatResponse, FinishReason, StreamEvent, ToolChoice, Usage};

/// Without tools, `tool_choice` auto/none and `parallel_tool_calls` mean
/// nothing and are left out; a choice that demands a tool cannot be met.
pub(crate) fn check_tool_choice(req: &ChatRequest) -> Result<(), TranslateError> {
    if !req.tools.is_empty() {
        return Ok(());
    }
    let what = match &req.tool_choice {
        Some(ToolChoice::Required) => "required",
        Some(ToolChoice::Tool(name)) => name.as_str(),
        _ => return Ok(()),
    };
    Err(TranslateError::InvalidRequest(format!(
        "tool_choice '{what}' needs tools"
    )))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    /// OpenAI and every OpenAI-compatible API (Groq, Mistral, OpenRouter, Ollama).
    OpenAi,
    Anthropic,
    /// Google's Gemini API (`generativelanguage`).
    Gemini,
    /// Azure OpenAI: OpenAI's format on a deployment URL.
    Azure,
}

impl ProviderKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "openai" => Some(ProviderKind::OpenAi),
            "anthropic" => Some(ProviderKind::Anthropic),
            "gemini" => Some(ProviderKind::Gemini),
            "azure" => Some(ProviderKind::Azure),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ProviderKind::OpenAi => "openai",
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::Gemini => "gemini",
            ProviderKind::Azure => "azure",
        }
    }
}

/// Where a gateway's OpenAI API lives: the address a caller gave, without a
/// trailing `/` or `/v1`, plus `/v1`.
pub fn gateway_base(base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    let root = base.strip_suffix("/v1").unwrap_or(base);
    format!("{}/v1", root.trim_end_matches('/'))
}

/// Escapes everything but the characters that are safe in a URL path segment.
pub(crate) fn path_segment(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for b in raw.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(char::from(b));
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The Azure OpenAI API version used when a provider has none.
pub const DEFAULT_AZURE_API_VERSION: &str = "2024-10-21";

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
    /// Azure OpenAI only; the other kinds ignore it.
    pub api_version: Option<String>,
}

/// Never prints the API key.
impl fmt::Debug for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Target")
            .field("kind", &self.kind)
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key.as_ref().map(|_| REDACTED))
            .field("model", &self.model)
            .field("api_version", &self.api_version)
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
                let secret = ["authorization", "x-api-key", "api-key", "x-goog-api-key"]
                    .iter()
                    .any(|name| k.eq_ignore_ascii_case(name));
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
        ProviderKind::Gemini => gemini::build(target, req),
        ProviderKind::Azure => azure::build(target, req),
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
        ProviderKind::OpenAi | ProviderKind::Azure => openai::parse(body),
        ProviderKind::Anthropic => anthropic::parse(body),
        ProviderKind::Gemini => gemini::parse(body),
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
    /// The stream has ended by its own account but its end is not yet given
    /// (Gemini: a usage-only chunk may still follow).
    pub ended: bool,
    /// Tool calls started so far (Gemini).
    pub tool_calls_started: u32,
    /// OpenAI tool calls: the id of each started call, by our tool index.
    pub tool_call_ids: Vec<String>,
    /// OpenAI tool calls: provider `index` -> our tool index of its newest call.
    pub tool_call_slots: std::collections::HashMap<u32, u32>,
    /// Anthropic: (content block index, tool call index) of each tool block.
    pub tool_blocks: Vec<(u64, u32)>,
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
    /// Set once an error has been hit; nothing is decoded afterwards.
    failed: bool,
    /// An error hit after events were already produced in the same `feed`.
    pending_error: Option<TranslateError>,
}

impl StreamDecoder {
    pub fn new(kind: ProviderKind) -> Self {
        Self {
            kind,
            sse: SseParser::new(),
            state: StreamState::default(),
            failed: false,
            pending_error: None,
        }
    }

    /// Decodes the events completed by `chunk`.
    ///
    /// An error ends the stream. If events were already decoded in this call
    /// they are returned and the error is kept for [`Self::take_error`];
    /// otherwise the error is returned directly. After an error every call
    /// returns no events.
    pub fn feed(&mut self, chunk: &[u8]) -> Result<Vec<StreamEvent>, TranslateError> {
        let mut out = Vec::new();
        if self.failed {
            return Ok(out);
        }
        for ev in self.sse.feed(chunk) {
            // Keepalive events carry no data and are not provider JSON.
            if ev.data.trim().is_empty() {
                continue;
            }
            // A failed decode must not leave partial output behind.
            let produced = out.len();
            let result = match self.kind {
                ProviderKind::OpenAi | ProviderKind::Azure => {
                    openai::decode(&mut self.state, &ev, &mut out)
                }
                ProviderKind::Anthropic => anthropic::decode(&mut self.state, &ev, &mut out),
                ProviderKind::Gemini => gemini::decode(&mut self.state, &ev, &mut out),
            };
            if let Err(e) = result {
                self.failed = true;
                out.truncate(produced);
                if out.is_empty() {
                    return Err(e);
                }
                self.pending_error = Some(e);
                return Ok(out);
            }
        }
        if self.sse.overflowed() {
            self.failed = true;
            let e = TranslateError::Malformed("stream event exceeds the size limit".into());
            if out.is_empty() {
                return Err(e);
            }
            self.pending_error = Some(e);
        }
        Ok(out)
    }

    /// Called when the provider closes the stream: the events that were held
    /// back until the end (Gemini's last event, which carries the final
    /// usage). Given once.
    pub fn finish(&mut self) -> Vec<StreamEvent> {
        if self.failed || !std::mem::take(&mut self.state.ended) {
            return Vec::new();
        }
        vec![StreamEvent::Done {
            finish_reason: self.state.finish,
            usage: self.state.usage(),
        }]
    }

    /// Returns the error that ended the stream, once, if `feed` did not
    /// return it directly.
    pub fn take_error(&mut self) -> Option<TranslateError> {
        self.pending_error.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gateway_base_is_its_root_plus_v1_whatever_the_caller_wrote() {
        for given in [
            "http://gw:3900",
            "http://gw:3900/",
            "http://gw:3900/v1",
            "http://gw:3900/v1/",
        ] {
            assert_eq!(gateway_base(given), "http://gw:3900/v1", "{given}");
        }
        assert_eq!(gateway_base("http://gw/api/v1"), "http://gw/api/v1");
        assert_eq!(gateway_base("http://gw/api"), "http://gw/api/v1");
    }

    #[test]
    fn target_debug_never_prints_the_api_key() {
        let mut t = Target {
            kind: ProviderKind::Anthropic,
            base_url: "https://api.anthropic.com".into(),
            api_key: Some("sk-secret-value".into()),
            model: "m".into(),
            api_version: None,
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
        for kind in ALL_KINDS {
            let mut d = StreamDecoder::new(kind);
            let got = d
                .feed(b"event: keepalive\n\ndata:\n\ndata:   \n\nevent: ping\ndata: \n\n")
                .unwrap();
            assert_eq!(got, vec![], "{kind:?}");
        }
    }

    const ALL_KINDS: [ProviderKind; 4] = [
        ProviderKind::OpenAi,
        ProviderKind::Anthropic,
        ProviderKind::Gemini,
        ProviderKind::Azure,
    ];

    #[test]
    fn kinds_round_trip_through_their_names() {
        for kind in ALL_KINDS {
            assert_eq!(ProviderKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(ProviderKind::parse("Gemini"), None);
        assert_eq!(ProviderKind::parse("palm"), None);
    }

    #[test]
    fn http_request_debug_hides_every_credential_header() {
        let r = HttpRequest {
            method: "POST",
            url: "https://x".into(),
            headers: vec![
                ("api-key".into(), "sk-secret-value".into()),
                ("x-goog-api-key".into(), "sk-secret-value".into()),
            ],
            body: vec![],
        };
        assert!(!format!("{r:?}").contains("sk-secret-value"));
    }

    /// Two deltas, then an error event, then one more delta.
    fn stream_with_error(kind: ProviderKind) -> Vec<u8> {
        match kind {
            ProviderKind::OpenAi | ProviderKind::Azure => concat!(
                "data: {\"choices\":[{\"delta\":{\"content\":\"a\"},\"finish_reason\":null}]}\n\n",
                "data: {\"choices\":[{\"delta\":{\"content\":\"b\"},\"finish_reason\":null}]}\n\n",
                "data: {\"error\":{\"message\":\"overloaded\"}}\n\n",
                "data: {\"choices\":[{\"delta\":{\"content\":\"never\"},\"finish_reason\":null}]}\n\n",
            ),
            ProviderKind::Gemini => concat!(
                "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"a\"}]}}]}\n\n",
                "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"b\"}]}}]}\n\n",
                "data: {\"error\":{\"code\":503,\"message\":\"overloaded\",\"status\":\"UNAVAILABLE\"}}\n\n",
                "data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"never\"}]}}]}\n\n",
            ),
            ProviderKind::Anthropic => concat!(
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"a\"}}\n\n",
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"b\"}}\n\n",
                "event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"overloaded\"}}\n\n",
                "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"never\"}}\n\n",
            ),
        }
        .as_bytes()
        .to_vec()
    }

    fn valid_delta(kind: ProviderKind) -> &'static [u8] {
        match kind {
            ProviderKind::Gemini => {
                b"data: {\"candidates\":[{\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"late\"}]}}]}\n\n"
            }
            ProviderKind::OpenAi | ProviderKind::Azure => {
                b"data: {\"choices\":[{\"delta\":{\"content\":\"late\"},\"finish_reason\":null}]}\n\n"
            }
            ProviderKind::Anthropic => {
                b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"late\"}}\n\n"
            }
        }
    }

    fn two_deltas() -> Vec<StreamEvent> {
        vec![
            StreamEvent::Delta { text: "a".into() },
            StreamEvent::Delta { text: "b".into() },
        ]
    }

    #[test]
    fn events_before_an_error_in_the_same_chunk_are_kept() {
        for kind in ALL_KINDS {
            let mut d = StreamDecoder::new(kind);
            let got = d.feed(&stream_with_error(kind)).unwrap();
            assert_eq!(got, two_deltas(), "{kind:?}");
            let e = d.take_error();
            assert!(
                matches!(&e, Some(TranslateError::Provider { message, .. }) if message == "overloaded"),
                "{kind:?}: {e:?}"
            );
            assert!(d.take_error().is_none(), "{kind:?}");
            assert_eq!(d.feed(valid_delta(kind)).unwrap(), vec![], "{kind:?}");
            assert!(d.take_error().is_none(), "{kind:?}");
        }
    }

    #[test]
    fn events_before_an_error_are_kept_at_every_split_point() {
        for kind in ALL_KINDS {
            let input = stream_with_error(kind);
            for split in 0..=input.len() {
                let mut d = StreamDecoder::new(kind);
                let mut events = Vec::new();
                let mut errors = 0;
                for part in [&input[..split], &input[split..]] {
                    match d.feed(part) {
                        Ok(evs) => events.extend(evs),
                        Err(_) => errors += 1,
                    }
                    if d.take_error().is_some() {
                        errors += 1;
                    }
                }
                assert_eq!(events, two_deltas(), "{kind:?} split {split}");
                assert_eq!(errors, 1, "{kind:?} split {split}");
                assert_eq!(d.feed(valid_delta(kind)).unwrap(), vec![], "{kind:?}");
            }
        }
    }

    fn oversized_event() -> Vec<u8> {
        let mut v = b"data: ".to_vec();
        v.resize(crate::sse::MAX_EVENT_BYTES + 1, b'x');
        v
    }

    fn is_size_limit_error(e: &TranslateError) -> bool {
        *e == TranslateError::Malformed("stream event exceeds the size limit".into())
    }

    #[test]
    fn oversized_event_is_a_malformed_error() {
        for kind in ALL_KINDS {
            let mut d = StreamDecoder::new(kind);
            let e = d.feed(&oversized_event()).unwrap_err();
            assert!(is_size_limit_error(&e), "{kind:?}: {e:?}");
            assert!(d.take_error().is_none(), "{kind:?}");
            assert_eq!(d.feed(valid_delta(kind)).unwrap(), vec![], "{kind:?}");
            assert!(d.take_error().is_none(), "{kind:?}");
        }
    }

    #[test]
    fn events_before_an_oversized_event_are_kept() {
        for kind in ALL_KINDS {
            let mut input = valid_delta(kind).to_vec();
            input.extend(oversized_event());
            let mut d = StreamDecoder::new(kind);
            let got = d.feed(&input).unwrap();
            assert_eq!(
                got,
                vec![StreamEvent::Delta {
                    text: "late".into()
                }],
                "{kind:?}"
            );
            let e = d.take_error().expect("the overflow must be reported");
            assert!(is_size_limit_error(&e), "{kind:?}: {e:?}");
            assert!(d.take_error().is_none(), "{kind:?}");
        }
    }
}
