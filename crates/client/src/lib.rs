//! Rust client for the Ultrafast gateway, or for a provider directly.
//!
//! One surface for both: `chat`, `chat_stream`, `embed`. The client does not
//! retry, route, cache or break circuits; every error says whether trying
//! again could help (`retryable`) and for how long to wait (`retry_after`).

mod client;
mod error;
mod request;
mod stream;
mod target;

pub use client::{Client, DEFAULT_MAX_RESPONSE_BYTES};
pub use error::{Error, ErrorKind};
pub use request::{ChatRequest, EmbeddingsRequest, MAX_TAGS_BYTES};
pub use target::Target;
pub use ultrafast_translate::embeddings::EmbeddingsResponse;
pub use ultrafast_translate::provider::ProviderKind;
pub use ultrafast_translate::types;
pub use ultrafast_translate::types::{
    ImageSource, Part, ResponseFormat, Tool, ToolCall, ToolChoice,
};
