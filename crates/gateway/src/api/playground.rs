//! The console playground: a chat call made for the signed-in user.

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
    /// A model as `provider/name`, or a route name.
    pub model: String,
    pub messages: Vec<PlaygroundMessage>,
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
    #[schema(schema_with = free_objects, nullable = false, required = false)]
    pub tools: Option<Vec<serde_json::Value>>,
    /// `auto`, `none`, `required` or a named function, as in
    /// `/v1/chat/completions`. Without `tools`, `required` and a named
    /// function are refused with 400.
    #[schema(schema_with = tool_choice_schema, nullable = false, required = false)]
    pub tool_choice: Option<serde_json::Value>,
    /// As in `/v1/chat/completions`; ignored without `tools`.
    #[schema(nullable = false)]
    pub parallel_tool_calls: Option<bool>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct PlaygroundMessage {
    /// `system`, `user`, `assistant` or `tool`.
    pub role: String,
    #[schema(schema_with = content_schema, required = false)]
    pub content: serde_json::Value,
    /// The calls of an assistant message, as in `/v1/chat/completions`.
    #[schema(schema_with = free_objects, nullable = false, required = false)]
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

/// A list of free objects.
fn free_objects() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
    utoipa::openapi::schema::ArrayBuilder::new()
        .items(free_object())
        .into()
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
