//! The console playground: a chat or an image call made for the signed-in user.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::response::Response;
use serde::Serialize;

use super::{require, ApiError, AuthVia, Authed};
use crate::app::AppState;
use crate::identity::policy::Action;
use crate::proxy;

/// A chat request, as `/v1/chat/completions` takes it. The body is read by
/// the same parser as that call's, so any field it accepts is accepted here.
#[derive(Serialize, utoipa::ToSchema)]
pub struct PlaygroundChatRequest {
    /// A model as `provider/name`, or a route name. Required unless `prompt`
    /// is set and its template names a model.
    #[schema(required = false, nullable = false)]
    pub model: Option<String>,
    /// Required unless `prompt` is set; with a template these come after its
    /// messages.
    #[schema(required = false, nullable = false)]
    pub messages: Option<Vec<PlaygroundMessage>>,
    #[schema(nullable = false)]
    pub max_tokens: Option<u32>,
    #[schema(nullable = false)]
    pub temperature: Option<f64>,
    #[schema(nullable = false)]
    pub top_p: Option<f64>,
    #[schema(nullable = false)]
    pub stop: Option<Vec<String>>,
    /// Answer as server-sent events.
    #[schema(nullable = false)]
    pub stream: Option<bool>,
    /// Functions the model may call, as in `/v1/chat/completions`.
    #[schema(schema_with = tools_schema, nullable = false, required = false)]
    pub tools: Option<Vec<serde_json::Value>>,
    /// `auto`, `none`, `required` or a named function, as in
    /// `/v1/chat/completions`. Without `tools`, `required` and a named
    /// function are refused with 400.
    #[schema(schema_with = tool_choice_schema, nullable = false, required = false)]
    pub tool_choice: Option<serde_json::Value>,
    /// As in `/v1/chat/completions`; ignored without `tools`.
    #[schema(nullable = false)]
    pub parallel_tool_calls: Option<bool>,
    /// As in `/v1/chat/completions`: `{"type":"text"}`, `{"type":"json_object"}`
    /// or `{"type":"json_schema","json_schema":{"name","schema","strict"?,"description"?}}`.
    #[schema(schema_with = response_format_schema, nullable = false, required = false)]
    pub response_format: Option<serde_json::Value>,
    /// A stored prompt template to render in front of `messages`:
    /// `{"id": "<name>", "version": <n>?, "variables": {...}}`, as in
    /// `/v1/chat/completions`. With it `model` and `messages` may be left out
    /// (the template's model is used), and `temperature`, `top_p`,
    /// `max_tokens` and `response_format` left out take the template's.
    #[schema(schema_with = prompt_schema, nullable = false, required = false)]
    pub prompt: Option<serde_json::Value>,
}

/// The `prompt` object: a template name, an optional version (a number or a
/// string of digits) and the values of its variables.
fn prompt_schema() -> utoipa::openapi::schema::ObjectBuilder {
    use utoipa::openapi::schema::{AdditionalProperties, ObjectBuilder, Type};
    ObjectBuilder::new()
        .schema_type(Type::Object)
        .description(Some(
            "`id` is the template's name; `version` a positive integer, as a number or a string of digits (left out: the latest); `variables` maps each variable name to its text.",
        ))
        .property("id", ObjectBuilder::new().schema_type(Type::String))
        .property(
            "version",
            ObjectBuilder::new()
                .schema_type(Type::Integer)
                .description(Some("A positive integer. A string of digits is read as the same number.")),
        )
        .property(
            "variables",
            ObjectBuilder::new()
                .schema_type(Type::Object)
                .additional_properties(Some(AdditionalProperties::RefOr(
                    ObjectBuilder::new().schema_type(Type::String).into(),
                ))),
        )
        .required("id")
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct PlaygroundMessage {
    /// `system`, `user`, `assistant` or `tool`.
    pub role: String,
    #[schema(schema_with = content_schema, required = false)]
    pub content: serde_json::Value,
    /// The calls of an assistant message, as in `/v1/chat/completions`.
    #[schema(schema_with = tool_calls_schema, nullable = false, required = false)]
    pub tool_calls: Option<Vec<serde_json::Value>>,
    /// On a `tool` message: the id of the call it answers.
    #[schema(nullable = false)]
    pub tool_call_id: Option<String>,
}

/// An object with any members: the shape of the call is the provider API's.
fn free_object() -> utoipa::openapi::schema::ObjectBuilder {
    use utoipa::openapi::schema::{AdditionalProperties, ObjectBuilder, Type};
    ObjectBuilder::new()
        .schema_type(Type::Object)
        .additional_properties(Some(AdditionalProperties::FreeForm(true)))
}

/// A list of free objects. `schema_with` replaces the generated schema, so
/// the description of the field is given here.
fn free_objects(description: &str) -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    utoipa::openapi::schema::ArrayBuilder::new()
        .description(Some(description))
        .items(free_object())
        .into()
}

fn tools_schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    free_objects("Functions the model may call, as in `/v1/chat/completions`.")
}

fn response_format_schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    free_object()
        .description(Some(
            "As in `/v1/chat/completions`: `{\"type\":\"text\"}`, `{\"type\":\"json_object\"}` \
             or `{\"type\":\"json_schema\",\"json_schema\":{\"name\",\"schema\",\"strict\"?,\"description\"?}}`.",
        ))
        .into()
}

fn tool_calls_schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    free_objects("The calls of an assistant message, as in `/v1/chat/completions`.")
}

/// `auto`, `none` or `required`, or an object naming a function.
fn tool_choice_schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    use utoipa::openapi::schema::{ObjectBuilder, OneOfBuilder, Type};
    OneOfBuilder::new()
        .description(Some(
            "`auto`, `none`, `required` or a named function, as in \
             `/v1/chat/completions`. Without `tools`, `required` and a named \
             function are refused with 400.",
        ))
        .item(
            ObjectBuilder::new()
                .schema_type(Type::String)
                .enum_values(Some(["auto", "none", "required"])),
        )
        .item(free_object())
        .into()
}

/// A string, a list of content parts, or null.
fn content_schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    use utoipa::openapi::schema::{ArrayBuilder, ObjectBuilder, OneOfBuilder, Type};
    OneOfBuilder::new()
        .description(Some(
            "Text, or a list of parts (`text` and `image_url`) as in \
             `/v1/chat/completions`. Null is allowed on an assistant message that has \
             `tool_calls`. Images are `data:` URLs or, except for Gemini, `http(s)` \
             URLs, and count toward the request body limit (10 MiB).",
        ))
        .item(ObjectBuilder::new().schema_type(Type::String))
        .item(ArrayBuilder::new().items(free_object()))
        .item(ObjectBuilder::new().schema_type(Type::Null))
        .into()
}

/// The answer of `/v1/chat/completions`, in the OpenAI shape.
#[derive(Serialize, utoipa::ToSchema)]
pub struct PlaygroundChatAnswer {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    #[schema(value_type = Vec<Object>)]
    pub choices: Vec<serde_json::Value>,
    #[schema(value_type = Object, nullable = false)]
    pub usage: Option<serde_json::Value>,
}

/// An error of this call: the CSRF and sign-in errors have the `/api` shape
/// (`code`, `message`); the pipeline's own have the OpenAI shape (`message`,
/// `type`, and sometimes `code`).
#[derive(Serialize, utoipa::ToSchema)]
pub struct PlaygroundErrorBody {
    pub error: PlaygroundErrorDetail,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct PlaygroundErrorDetail {
    pub message: String,
    /// `null` in the OpenAI shape, except for a spent budget.
    pub code: Option<String>,
    /// Only in the OpenAI shape, such as `permission_error`.
    #[serde(rename = "type")]
    #[schema(nullable = false)]
    pub kind: Option<String>,
}

#[utoipa::path(
    post,
    path = "/playground/chat",
    tag = "playground",
    operation_id = "playground_chat",
    request_body = PlaygroundChatRequest,
    responses(
        (status = 200, description = "The answer, in the OpenAI shape; server-sent events when `stream` is true.",
            content(
                (PlaygroundChatAnswer = "application/json"),
                (String = "text/event-stream"),
            )),
        (status = 400, description = "The request is not a chat request. The body is in the OpenAI error shape, as on `/v1`.", body = PlaygroundErrorBody),
        (status = 401, description = "No valid session.", body = PlaygroundErrorBody),
        (status = 403, description = "The user may not call this model or route, the call was made with an access token (the playground is for a signed-in browser session only), or the CSRF token is missing or does not match. The body is in the OpenAI error shape when it is the model, as on `/v1`.", body = PlaygroundErrorBody),
        (status = 404, description = "No such model or route, in the OpenAI error shape.", body = PlaygroundErrorBody),
        (status = 429, description = "A limit or a budget refuses the call; `Retry-After` says when to come back. OpenAI error shape.", body = PlaygroundErrorBody),
        (status = 502, description = "The provider failed; OpenAI error shape.", body = PlaygroundErrorBody),
        (status = 503, description = "No provider could serve the call; OpenAI error shape.", body = PlaygroundErrorBody),
    ),
    security(("session" = [])),
)]
pub async fn chat(
    State(state): State<Arc<AppState>>,
    authed: Authed,
    body: Body,
) -> Result<Response, ApiError> {
    // A token is the admin SDK's credential, not a way to call models: it
    // would skip the expiry, revocation, allowlist and key limits of a key.
    if !matches!(authed.via, AuthVia::Session { .. }) {
        return Err(ApiError::forbidden());
    }
    require(&authed.principal, &Action::UsePlayground)?;
    Ok(proxy::playground(state, authed.principal.user_id, body).await)
}

/// An image generation request, as `/v1/images/generations` takes it. The body
/// is read by the same parser as that call's, so any field it accepts is
/// accepted here.
#[derive(Serialize, utoipa::ToSchema)]
pub struct PlaygroundImageRequest {
    /// A model as `provider/name`, or a route name.
    pub model: String,
    pub prompt: String,
    /// The number of images, 1 to 10.
    #[schema(nullable = false)]
    pub n: Option<u32>,
    /// For example `1024x1024`.
    #[schema(nullable = false)]
    pub size: Option<String>,
    #[schema(nullable = false)]
    pub quality: Option<String>,
    /// `transparent`, `opaque` or `auto`.
    #[schema(nullable = false)]
    pub background: Option<String>,
    /// `png`, `jpeg` or `webp`.
    #[schema(nullable = false)]
    pub output_format: Option<String>,
}

/// The answer of `/v1/images/generations`, in the OpenAI shape.
#[derive(Serialize, utoipa::ToSchema)]
pub struct PlaygroundImageAnswer {
    pub created: u64,
    /// Each image as `b64_json` or `url`, with `revised_prompt` when the
    /// model gave one.
    #[schema(value_type = Vec<Object>)]
    pub data: Vec<serde_json::Value>,
    /// Only when the provider reports token usage.
    #[schema(value_type = Object, nullable = false, required = false)]
    pub usage: Option<serde_json::Value>,
}

#[utoipa::path(
    post,
    path = "/playground/images",
    tag = "playground",
    operation_id = "playground_images",
    request_body = PlaygroundImageRequest,
    responses(
        (status = 200, description = "The answer, in the OpenAI shape.", body = PlaygroundImageAnswer),
        (status = 400, description = "The request is not an image request, or the model cannot generate images. The body is in the OpenAI error shape, as on `/v1`.", body = PlaygroundErrorBody),
        (status = 401, description = "No valid session.", body = PlaygroundErrorBody),
        (status = 403, description = "The user may not call this model or route, the call was made with an access token (the playground is for a signed-in browser session only), or the CSRF token is missing or does not match. The body is in the OpenAI error shape when it is the model, as on `/v1`.", body = PlaygroundErrorBody),
        (status = 404, description = "No such model or route, in the OpenAI error shape.", body = PlaygroundErrorBody),
        (status = 429, description = "A limit or a budget refuses the call; `Retry-After` says when to come back. OpenAI error shape.", body = PlaygroundErrorBody),
        (status = 502, description = "The provider failed; OpenAI error shape.", body = PlaygroundErrorBody),
        (status = 503, description = "No provider could serve the call; OpenAI error shape.", body = PlaygroundErrorBody),
    ),
    security(("session" = [])),
)]
pub async fn images(
    State(state): State<Arc<AppState>>,
    authed: Authed,
    body: Body,
) -> Result<Response, ApiError> {
    if !matches!(authed.via, AuthVia::Session { .. }) {
        return Err(ApiError::forbidden());
    }
    require(&authed.principal, &Action::UsePlayground)?;
    Ok(proxy::playground_images(state, authed.principal.user_id, body).await)
}

/// The form of a transcription, as `/v1/audio/transcriptions` takes it
/// (`multipart/form-data`). The form is read by the same reader as that
/// call's: the file may not be larger than the gateway's audio cap
/// (`UF_MAX_AUDIO_BYTES`, 25 MiB by default).
#[derive(Serialize, utoipa::ToSchema)]
pub struct PlaygroundTranscriptionForm {
    /// The audio file.
    #[schema(value_type = String, format = Binary)]
    pub file: Vec<u8>,
    /// A model as `provider/name`, or a route name.
    pub model: String,
    /// The language spoken, as an ISO-639-1 code.
    #[schema(nullable = false)]
    pub language: Option<String>,
    /// Text to guide the style of the transcript.
    #[schema(nullable = false)]
    pub prompt: Option<String>,
    /// `json`, `text`, `verbose_json`, `srt` or `vtt`.
    #[schema(nullable = false)]
    pub response_format: Option<String>,
    /// From 0 to 1.
    #[schema(nullable = false)]
    pub temperature: Option<f64>,
}

#[utoipa::path(
    post,
    path = "/playground/transcriptions",
    tag = "playground",
    operation_id = "playground_transcriptions",
    request_body(content = PlaygroundTranscriptionForm, content_type = "multipart/form-data"),
    responses(
        (status = 200, description = "The transcript, in the format asked for (JSON, or text).", body = String, content_type = "application/json"),
        (status = 400, description = "The form is not a transcription request, or the model cannot transcribe. The body is in the OpenAI error shape, as on `/v1`.", body = PlaygroundErrorBody),
        (status = 401, description = "No valid session.", body = PlaygroundErrorBody),
        (status = 403, description = "The user may not call this model or route, the call was made with an access token (the playground is for a signed-in browser session only), or the CSRF token is missing or does not match. The body is in the OpenAI error shape when it is the model, as on `/v1`.", body = PlaygroundErrorBody),
        (status = 404, description = "No such model or route, in the OpenAI error shape.", body = PlaygroundErrorBody),
        (status = 413, description = "The file is larger than the audio cap.", body = PlaygroundErrorBody),
        (status = 429, description = "A limit or a budget refuses the call; `Retry-After` says when to come back. OpenAI error shape.", body = PlaygroundErrorBody),
        (status = 502, description = "The provider failed; OpenAI error shape.", body = PlaygroundErrorBody),
        (status = 503, description = "No provider could serve the call; OpenAI error shape.", body = PlaygroundErrorBody),
        (status = 504, description = "The provider did not answer in time; the call was not repeated. OpenAI error shape.", body = PlaygroundErrorBody),
    ),
    security(("session" = [])),
)]
pub async fn transcriptions(
    State(state): State<Arc<AppState>>,
    authed: Authed,
    headers: axum::http::HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    if !matches!(authed.via, AuthVia::Session { .. }) {
        return Err(ApiError::forbidden());
    }
    require(&authed.principal, &Action::UsePlayground)?;
    let form_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok());
    Ok(proxy::playground_transcriptions(state, authed.principal.user_id, body, form_type).await)
}

/// A speech request, as `/v1/audio/speech` takes it.
#[derive(Serialize, utoipa::ToSchema)]
pub struct PlaygroundSpeechRequest {
    /// A model as `provider/name`, or a route name.
    pub model: String,
    /// The text to speak, at most 4096 characters.
    pub input: String,
    /// A voice name such as `alloy`.
    pub voice: String,
    /// `mp3`, `opus`, `aac`, `flac`, `wav` or `pcm`.
    #[schema(nullable = false)]
    pub response_format: Option<String>,
    /// From 0.25 to 4.0.
    #[schema(nullable = false)]
    pub speed: Option<f64>,
    /// How the text should be spoken (not for `tts-1` models).
    #[schema(nullable = false)]
    pub instructions: Option<String>,
}

#[utoipa::path(
    post,
    path = "/playground/speech",
    tag = "playground",
    operation_id = "playground_speech",
    request_body = PlaygroundSpeechRequest,
    responses(
        (status = 200, description = "The audio, streamed as the provider makes it, with the provider's content type.", content_type = "audio/mpeg", body = Vec<u8>),
        (status = 400, description = "The request is not a speech request, or the model cannot speak. The body is in the OpenAI error shape, as on `/v1`.", body = PlaygroundErrorBody),
        (status = 401, description = "No valid session.", body = PlaygroundErrorBody),
        (status = 403, description = "The user may not call this model or route, the call was made with an access token (the playground is for a signed-in browser session only), or the CSRF token is missing or does not match. The body is in the OpenAI error shape when it is the model, as on `/v1`.", body = PlaygroundErrorBody),
        (status = 404, description = "No such model or route, in the OpenAI error shape.", body = PlaygroundErrorBody),
        (status = 429, description = "A limit or a budget refuses the call; `Retry-After` says when to come back. OpenAI error shape.", body = PlaygroundErrorBody),
        (status = 502, description = "The provider failed; OpenAI error shape.", body = PlaygroundErrorBody),
        (status = 503, description = "No provider could serve the call; OpenAI error shape.", body = PlaygroundErrorBody),
        (status = 504, description = "The provider did not answer in time; the call was not repeated. OpenAI error shape.", body = PlaygroundErrorBody),
    ),
    security(("session" = [])),
)]
pub async fn speech(
    State(state): State<Arc<AppState>>,
    authed: Authed,
    body: Body,
) -> Result<Response, ApiError> {
    if !matches!(authed.via, AuthVia::Session { .. }) {
        return Err(ApiError::forbidden());
    }
    require(&authed.principal, &Action::UsePlayground)?;
    Ok(proxy::playground_speech(state, authed.principal.user_id, body).await)
}
