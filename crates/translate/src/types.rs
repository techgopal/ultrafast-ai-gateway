use serde::{Deserialize, Serialize};

use crate::error::TranslateError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ImageSource {
    /// An http(s) URL, passed on as is.
    Url(String),
    /// `data:<media_type>;base64,<data>`, split.
    Base64 { media_type: String, data: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Part {
    Text(String),
    Image(ImageSource),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// The arguments as the JSON text the model produced.
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub description: Option<String>,
    /// JSON Schema of the arguments; `{"type":"object"}` when not given.
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ToolChoice {
    Auto,
    None,
    Required,
    Tool(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    /// May be empty for an assistant message with tool calls.
    pub content: Vec<Part>,
    pub name: Option<String>,
    /// Assistant only.
    pub tool_calls: Vec<ToolCall>,
    /// `Role::Tool` only, required there.
    pub tool_call_id: Option<String>,
}

impl Message {
    /// One text part, no tools.
    pub fn text(role: Role, text: impl Into<String>) -> Self {
        Message {
            role,
            content: vec![Part::Text(text.into())],
            name: None,
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }

    /// Concatenation of the text parts.
    pub fn joined_text(&self) -> String {
        let mut out = String::new();
        for p in &self.content {
            if let Part::Text(t) = p {
                out.push_str(t);
            }
        }
        out
    }

    pub fn has_images(&self) -> bool {
        self.content.iter().any(|p| matches!(p, Part::Image(_)))
    }
}

pub const IMAGE_TYPES: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];

/// The alphabet only: standard or URL-safe (not both), with `=` padding (at
/// most two) at the very end.
fn valid_base64(data: &str) -> bool {
    let body = data.trim_end_matches('=');
    if body.is_empty() || data.len() - body.len() > 2 {
        return false;
    }
    let std = body.bytes().any(|b| matches!(b, b'+' | b'/'));
    let url = body.bytes().any(|b| matches!(b, b'-' | b'_'));
    !(std && url)
        && body
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'-' | b'_'))
}

/// Checks the media type and the alphabet of base64 image data.
pub(crate) fn base64_source(media_type: &str, data: &str) -> Result<ImageSource, TranslateError> {
    if !IMAGE_TYPES.contains(&media_type) {
        return Err(TranslateError::InvalidRequest(format!(
            "image type '{media_type}' is not supported; use one of {}",
            IMAGE_TYPES.join(", ")
        )));
    }
    let valid = valid_base64(data);
    if !valid {
        return Err(TranslateError::InvalidRequest(
            "image data is not valid base64".to_string(),
        ));
    }
    Ok(ImageSource::Base64 {
        media_type: media_type.to_string(),
        data: data.to_string(),
    })
}

/// Parses `data:<type>;base64,<data>` or an http(s) URL. Shared by both ingresses.
pub fn image_source(url: &str) -> Result<ImageSource, TranslateError> {
    let bad = |m: &str| TranslateError::InvalidRequest(m.to_string());
    if let Some(rest) = url.strip_prefix("data:") {
        let (meta, data) = rest
            .split_once(',')
            .ok_or_else(|| bad("image data URL must be base64 encoded"))?;
        let media_type = meta
            .strip_suffix(";base64")
            .ok_or_else(|| bad("image data URL must be base64 encoded"))?;
        return base64_source(media_type, data);
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        return Ok(ImageSource::Url(url.to_string()));
    }
    Err(bad("image url must be an http(s) URL or a base64 data URL"))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub stop: Option<Vec<String>>,
    pub stream: bool,
    pub tools: Vec<Tool>,
    pub tool_choice: Option<ToolChoice>,
    pub parallel_tool_calls: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FinishReason {
    Stop,
    Length,
    ToolCalls,
    ContentFilter,
}

impl FinishReason {
    pub fn as_openai(self) -> &'static str {
        match self {
            FinishReason::Stop => "stop",
            FinishReason::Length => "length",
            FinishReason::ToolCalls => "tool_calls",
            FinishReason::ContentFilter => "content_filter",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatResponse {
    pub id: String,
    pub model: String,
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub finish_reason: Option<FinishReason>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    Delta {
        text: String,
    },
    /// A tool call begins. `index` counts tool calls of this answer from 0.
    ToolCallStart {
        index: u32,
        id: String,
        name: String,
    },
    /// More argument text for the call at `index`.
    ToolCallDelta {
        index: u32,
        arguments: String,
    },
    Done {
        finish_reason: Option<FinishReason>,
        usage: Option<Usage>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_source_parses_both_forms() {
        assert_eq!(
            image_source("https://x.test/a.png").unwrap(),
            ImageSource::Url("https://x.test/a.png".into())
        );
        assert_eq!(
            image_source("http://x.test/a.png").unwrap(),
            ImageSource::Url("http://x.test/a.png".into())
        );
        assert_eq!(
            image_source("data:image/png;base64,QUJD").unwrap(),
            ImageSource::Base64 {
                media_type: "image/png".into(),
                data: "QUJD".into()
            }
        );
        assert!(image_source("ftp://x/a.png").is_err());
        assert!(image_source("data:image/svg+xml;base64,QQ==").is_err());
    }

    #[test]
    fn base64_padding_and_alphabet_are_checked() {
        for ok in ["QUJD", "QQ==", "QUI=", "a-b_", "a+b/"] {
            assert!(
                image_source(&format!("data:image/png;base64,{ok}")).is_ok(),
                "{ok}"
            );
        }
        for bad in ["=QUJD", "QU=JD", "QQ===", "=", "a+b_", "a-b/"] {
            assert!(
                image_source(&format!("data:image/png;base64,{bad}")).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn joined_text_skips_images() {
        let m = Message {
            role: Role::User,
            content: vec![
                Part::Text("a".into()),
                Part::Image(ImageSource::Url("https://x/y.png".into())),
                Part::Text("b".into()),
            ],
            name: None,
            tool_calls: Vec::new(),
            tool_call_id: None,
        };
        assert_eq!(m.joined_text(), "ab");
        assert!(m.has_images());
        assert!(!Message::text(Role::User, "x").has_images());
    }
}
