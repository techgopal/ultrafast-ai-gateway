//! Requests with builder helpers and gateway tags.

use std::collections::BTreeMap;

use ultrafast_translate::embeddings::EmbeddingsRequest as Wire;
use ultrafast_translate::types::{
    self, image_source, Message, Part, Role, Tool, ToolCall, ToolChoice,
};

use crate::error::{Error, ErrorKind};

pub use ultrafast_translate::tags::MAX_TAGS_BYTES;

#[derive(Debug, Clone, PartialEq)]
pub struct ChatRequest {
    pub inner: types::ChatRequest,
    /// Sent to a gateway target only.
    pub tags: BTreeMap<String, String>,
}

impl ChatRequest {
    pub fn new(model: impl Into<String>) -> Self {
        ChatRequest {
            inner: types::ChatRequest {
                model: model.into(),
                messages: Vec::new(),
                max_tokens: None,
                temperature: None,
                top_p: None,
                stop: None,
                stream: false,
                tools: Vec::new(),
                tool_choice: None,
                parallel_tool_calls: None,
            },
            tags: BTreeMap::new(),
        }
    }

    pub fn message(mut self, role: Role, content: impl Into<String>) -> Self {
        self.inner.messages.push(Message::text(role, content));
        self
    }

    pub fn system(self, content: impl Into<String>) -> Self {
        self.message(Role::System, content)
    }

    pub fn user(self, content: impl Into<String>) -> Self {
        self.message(Role::User, content)
    }

    pub fn assistant(self, content: impl Into<String>) -> Self {
        self.message(Role::Assistant, content)
    }

    /// A user message of the given parts (text and images, in order).
    pub fn user_parts(mut self, parts: Vec<Part>) -> Self {
        self.inner.messages.push(Message {
            role: Role::User,
            content: parts,
            name: None,
            tool_calls: Vec::new(),
            tool_call_id: None,
        });
        self
    }

    /// Appends an image (an http(s) URL or a base64 `data:` URL) to the last
    /// message when it is a user message, or starts one. Fails when the URL
    /// is neither form or the image type is not supported.
    pub fn image(mut self, url_or_data_url: &str) -> Result<Self, Error> {
        let part = Part::Image(
            image_source(url_or_data_url)
                .map_err(|e| Error::new(ErrorKind::InvalidRequest, e.to_string()))?,
        );
        match self.inner.messages.last_mut() {
            Some(m) if m.role == Role::User => m.content.push(part),
            _ => return Ok(self.user_parts(vec![part])),
        }
        Ok(self)
    }

    pub fn tool(mut self, tool: Tool) -> Self {
        self.inner.tools.push(tool);
        self
    }

    pub fn tool_choice(mut self, choice: ToolChoice) -> Self {
        self.inner.tool_choice = Some(choice);
        self
    }

    pub fn parallel_tool_calls(mut self, v: bool) -> Self {
        self.inner.parallel_tool_calls = Some(v);
        self
    }

    /// An assistant message that asked for tools; `text` may be empty.
    pub fn assistant_tool_calls(mut self, text: impl Into<String>, calls: Vec<ToolCall>) -> Self {
        let text = text.into();
        self.inner.messages.push(Message {
            role: Role::Assistant,
            content: if text.is_empty() {
                Vec::new()
            } else {
                vec![Part::Text(text)]
            },
            name: None,
            tool_calls: calls,
            tool_call_id: None,
        });
        self
    }

    /// The result of the tool call `id`.
    pub fn tool_result(mut self, id: impl Into<String>, content: impl Into<String>) -> Self {
        self.inner.messages.push(Message {
            tool_call_id: Some(id.into()),
            ..Message::text(Role::Tool, content)
        });
        self
    }

    pub fn max_tokens(mut self, v: u32) -> Self {
        self.inner.max_tokens = Some(v);
        self
    }

    pub fn temperature(mut self, v: f32) -> Self {
        self.inner.temperature = Some(v);
        self
    }

    pub fn top_p(mut self, v: f32) -> Self {
        self.inner.top_p = Some(v);
        self
    }

    pub fn stop<I, S>(mut self, v: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.inner.stop = Some(v.into_iter().map(Into::into).collect());
        self
    }

    pub fn tag(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.tags.insert(key.into(), value.into());
        self
    }
}

impl From<types::ChatRequest> for ChatRequest {
    fn from(inner: types::ChatRequest) -> Self {
        ChatRequest {
            inner,
            tags: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddingsRequest {
    pub inner: Wire,
    pub tags: BTreeMap<String, String>,
}

impl EmbeddingsRequest {
    pub fn new<I, S>(model: impl Into<String>, input: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        EmbeddingsRequest {
            inner: Wire {
                model: model.into(),
                input: input.into_iter().map(Into::into).collect(),
                dimensions: None,
            },
            tags: BTreeMap::new(),
        }
    }

    pub fn dimensions(mut self, v: u32) -> Self {
        self.inner.dimensions = Some(v);
        self
    }

    pub fn tag(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.tags.insert(key.into(), value.into());
        self
    }
}

impl From<Wire> for EmbeddingsRequest {
    fn from(inner: Wire) -> Self {
        EmbeddingsRequest {
            inner,
            tags: BTreeMap::new(),
        }
    }
}

/// The `x-uf-tags` value; see [`ultrafast_translate::tags::tags_header`].
pub(crate) fn tags_header(tags: &BTreeMap<String, String>) -> Result<Option<String>, Error> {
    ultrafast_translate::tags::tags_header(tags).map_err(|c| Error::new(c.kind, c.message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use ultrafast_translate::types::{ImageSource, Part, Tool, ToolCall, ToolChoice};

    fn weather() -> Tool {
        Tool {
            name: "weather".into(),
            description: Some("Current weather".into()),
            parameters: json!({"type": "object"}),
            strict: None,
        }
    }

    #[test]
    fn user_parts_and_image_build_one_user_message() {
        let r = ChatRequest::new("m")
            .user_parts(vec![Part::Text("what is this".into())])
            .image("https://x.test/a.png")
            .unwrap()
            .image("data:image/png;base64,AAAA")
            .unwrap();
        assert_eq!(r.inner.messages.len(), 1);
        assert_eq!(
            r.inner.messages[0].content,
            vec![
                Part::Text("what is this".into()),
                Part::Image(ImageSource::Url("https://x.test/a.png".into())),
                Part::Image(ImageSource::Base64 {
                    media_type: "image/png".into(),
                    data: "AAAA".into()
                }),
            ]
        );
    }

    #[test]
    fn image_starts_a_user_message_after_an_assistant_one_and_refuses_bad_input() {
        let r = ChatRequest::new("m")
            .user("hi")
            .assistant("hello")
            .image("https://x.test/a.png")
            .unwrap();
        assert_eq!(r.inner.messages.len(), 3);
        assert_eq!(r.inner.messages[2].role, Role::User);
        assert!(ChatRequest::new("m").image("ftp://x").is_err());
        assert!(ChatRequest::new("m")
            .image("data:text/plain;base64,AAAA")
            .is_err());
    }

    #[test]
    fn tools_and_a_tool_round_trip() {
        let call = ToolCall {
            id: "call_1".into(),
            name: "weather".into(),
            arguments: r#"{"city":"Paris"}"#.into(),
        };
        let r = ChatRequest::new("m")
            .user("weather?")
            .tool(weather())
            .tool_choice(ToolChoice::Required)
            .parallel_tool_calls(false)
            .assistant_tool_calls("", vec![call.clone()])
            .tool_result("call_1", "sunny");
        assert_eq!(r.inner.tools, vec![weather()]);
        assert_eq!(r.inner.tool_choice, Some(ToolChoice::Required));
        assert_eq!(r.inner.parallel_tool_calls, Some(false));
        let a = &r.inner.messages[1];
        assert_eq!(a.role, Role::Assistant);
        assert!(a.content.is_empty());
        assert_eq!(a.tool_calls, vec![call]);
        let t = &r.inner.messages[2];
        assert_eq!(t.role, Role::Tool);
        assert_eq!(t.tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(t.joined_text(), "sunny");
    }
}
