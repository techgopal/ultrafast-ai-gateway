//! Requests with builder helpers and gateway tags.

use std::collections::BTreeMap;

use ultrafast_translate::embeddings::EmbeddingsRequest as Wire;
use ultrafast_translate::types::{self, Message, Role};

use crate::error::{Error, ErrorKind};

/// The most the `x-uf-tags` header may hold.
pub const MAX_TAGS_BYTES: usize = 1024;

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
            },
            tags: BTreeMap::new(),
        }
    }

    pub fn message(mut self, role: Role, content: impl Into<String>) -> Self {
        self.inner.messages.push(Message {
            role,
            content: content.into(),
            name: None,
        });
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

/// The `x-uf-tags` value: compact JSON in ASCII, at most 1 KiB. None when
/// there are no tags.
pub(crate) fn tags_header(tags: &BTreeMap<String, String>) -> Result<Option<String>, Error> {
    if tags.is_empty() {
        return Ok(None);
    }
    let json = serde_json::to_string(tags)
        .map_err(|_| Error::new(ErrorKind::InvalidRequest, "tags could not be encoded"))?;
    let mut out = String::with_capacity(json.len());
    for c in json.chars() {
        if (' '..'\u{7f}').contains(&c) {
            out.push(c);
        } else {
            let mut units = [0u16; 2];
            for u in c.encode_utf16(&mut units) {
                out.push_str(&format!("\\u{u:04x}"));
            }
        }
    }
    if out.len() > MAX_TAGS_BYTES {
        return Err(Error::new(
            ErrorKind::InvalidRequest,
            format!("tags exceed {MAX_TAGS_BYTES} bytes"),
        ));
    }
    Ok(Some(out))
}
