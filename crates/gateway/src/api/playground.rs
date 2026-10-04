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
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct PlaygroundMessage {
    /// `system`, `user` or `assistant`.
    pub role: String,
    pub content: String,
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
