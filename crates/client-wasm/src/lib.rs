//! `ultrafast-translate` for WebAssembly. Every call takes strings and bytes
//! and returns a JSON string; an error is thrown as a JSON string (see
//! [`api`]). The TypeScript client owns the HTTP, this crate owns the wire.

pub mod api;

use wasm_bindgen::prelude::*;

/// `{method, url, headers, body}` for a chat call. `target_json`:
/// `{kind, base_url, api_key?, api_version?}` with `kind` one of `openai`,
/// `anthropic`, `gemini`, `azure`, `gateway`. `request_json`: `{model,
/// messages, max_tokens?, temperature?, top_p?, stop?, stream?, tags?}`.
#[wasm_bindgen(js_name = buildRequest)]
pub fn build_request(target_json: &str, request_json: &str) -> Result<String, String> {
    api::build_request(target_json, request_json)
}

#[wasm_bindgen(js_name = buildEmbeddingsRequest)]
pub fn build_embeddings_request(target_json: &str, request_json: &str) -> Result<String, String> {
    api::build_embeddings_request(target_json, request_json)
}

/// The response as JSON, or throws the classified error (any status of 300
/// or more included). `retry_after` is the `Retry-After` header, if any.
#[wasm_bindgen(js_name = parseResponse)]
pub fn parse_response(
    kind: &str,
    status: u16,
    body: &[u8],
    retry_after: Option<String>,
) -> Result<String, String> {
    api::parse_response(kind, status, body, retry_after.as_deref())
}

#[wasm_bindgen(js_name = parseEmbeddings)]
pub fn parse_embeddings(
    kind: &str,
    status: u16,
    body: &[u8],
    model: &str,
    retry_after: Option<String>,
) -> Result<String, String> {
    api::parse_embeddings(kind, status, body, model, retry_after.as_deref())
}

/// `{kind, retryable, status, message, retry_after_secs}` for an HTTP error
/// answer: the same mapping the Rust client uses.
#[wasm_bindgen(js_name = classifyError)]
pub fn classify_error(status: u16, body: &[u8], retry_after: Option<String>) -> String {
    api::classify_error(status, body, retry_after.as_deref())
}

/// The error JSON for a failure the host raised itself (`network`,
/// `timeout`, `malformed`, ...), so kinds and retryability are defined once.
#[wasm_bindgen(js_name = hostError)]
pub fn host_error(kind: &str, message: &str) -> Result<String, String> {
    api::host_error(kind, message)
}

/// `message` with every occurrence of the key replaced by `[redacted]`; run
/// every error message through it before showing it.
#[wasm_bindgen]
pub fn scrub(message: &str, key: &str) -> String {
    api::scrub(message, key)
}

/// The `x-uf-tags` header value for a JSON object of tags; `undefined` when
/// there are none.
#[wasm_bindgen(js_name = tagsHeader)]
pub fn tags_header(tags_json: &str) -> Result<Option<String>, String> {
    api::tags_header(tags_json)
}

#[wasm_bindgen(js_name = StreamDecoder)]
pub struct StreamDecoder(api::Decoder);

#[wasm_bindgen(js_class = StreamDecoder)]
impl StreamDecoder {
    #[wasm_bindgen(constructor)]
    pub fn new(kind: &str) -> Result<StreamDecoder, String> {
        api::Decoder::new(kind).map(StreamDecoder)
    }

    /// A JSON array of events, or throws the error JSON when the stream
    /// failed with nothing decoded in this chunk.
    pub fn feed(&mut self, chunk: &[u8]) -> Result<String, String> {
        self.0.feed(chunk)
    }

    pub fn finish(&mut self) -> String {
        self.0.finish()
    }

    /// The error that ended the stream after events were returned, once.
    #[wasm_bindgen(js_name = takeError)]
    pub fn take_error(&mut self) -> Option<String> {
        self.0.take_error()
    }
}
