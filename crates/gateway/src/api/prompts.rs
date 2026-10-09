//! Prompt templates: named messages with `{{variables}}`, kept as versions
//! that never change, and rendered into a call by name (`prompt` on
//! `/v1/chat/completions` and `/v1/responses`).
//!
//! Admins manage every template; a team lead the ones they made. Everyone
//! signed in reads and renders any template, and anyone who can call may use
//! any template by name: a template is a convenience, not a secret.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use super::guardrails::{check_description, check_name};
use super::{path_id, refresh_snapshot, require, ApiError, ApiJson, Authed};
use crate::app::AppState;
use crate::identity::policy::Action;
use crate::prompts::{
    self, Params, TemplateMessage, Version, MAX_CONTENT_BYTES, MAX_MESSAGES, MAX_MODEL_CHARS,
    MAX_TEMPLATES, MAX_TOTAL_BYTES, MAX_VARIABLES, MAX_VERSIONS, ROLES,
};
use crate::store::{AuditEntry, NewVersion, StoreError, TemplateRow, VersionRow};
use ultrafast_translate::ingress::openai::parse_response_format;

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreatePromptRequest {
    /// Unique, 1 to 100 characters. This is the `id` a call names. Case
    /// sensitive.
    name: String,
    /// Up to 500 characters. Left out: none.
    description: Option<String>,
    /// Version 1: 1 to 64 messages. A variable is `{{name}}` with a name of
    /// letters, digits and `_` that does not start with a digit, up to 64
    /// characters; other braces are text. At most 64 variables.
    #[schema(value_type = Vec<TemplateMessage>)]
    messages: Vec<serde_json::Value>,
    /// Used when a call names no model: `provider/model` or a route.
    model: Option<String>,
    /// Settings used where a call sets none.
    #[schema(value_type = Option<Params>)]
    params: Option<serde_json::Value>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateVersionRequest {
    /// As for a new template. A version stands alone: it keeps nothing of
    /// the one before.
    #[schema(value_type = Vec<TemplateMessage>)]
    messages: Vec<serde_json::Value>,
    model: Option<String>,
    #[schema(value_type = Option<Params>)]
    params: Option<serde_json::Value>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderRequest {
    /// Left out: the latest version.
    version: Option<u32>,
    /// A value for every variable of the version, and for no other: a text
    /// of at most 32 KiB each. Put in as it is, once.
    #[serde(default)]
    variables: BTreeMap<String, String>,
}

/// One version of a template.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct VersionView {
    /// From 1.
    pub version: i64,
    pub messages: Vec<TemplateMessage>,
    /// The names the messages use, sorted.
    pub variables: Vec<String>,
    #[schema(required)]
    pub model: Option<String>,
    pub params: Params,
    /// `null` when the user is gone.
    #[schema(required)]
    pub created_by: Option<i64>,
    pub created_at: String,
}

/// A template in a list.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct PromptSummary {
    pub id: i64,
    pub name: String,
    pub description: String,
    /// The user who made it; `null` when they are gone. Admins manage
    /// every template, a lead the ones they made.
    #[schema(required)]
    pub created_by: Option<i64>,
    pub created_at: String,
    /// The version a call without `version` gets.
    pub latest_version: i64,
    pub version_count: i64,
    /// When the latest version was written.
    pub updated_at: String,
    /// The model and variables of the latest version.
    #[schema(required)]
    pub model: Option<String>,
    pub variables: Vec<String>,
    /// The latest version cannot be read, so `model` and `variables` are
    /// empty and a call by this name is refused. An admin can add a version.
    pub unreadable: bool,
}

/// A version in a list: its number and when and by whom it was written. Its
/// text is `GET /api/prompts/{id}/versions/{version}`.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct VersionStubView {
    pub version: i64,
    #[schema(required)]
    pub created_by: Option<i64>,
    pub created_at: String,
}

/// A template with the numbers of its versions, oldest first. The model and
/// variables are those of the latest version; the text of any version is
/// read from the version endpoint.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct PromptView {
    pub id: i64,
    pub name: String,
    pub description: String,
    #[schema(required)]
    pub created_by: Option<i64>,
    pub created_at: String,
    pub latest_version: i64,
    pub version_count: i64,
    pub updated_at: String,
    #[schema(required)]
    pub model: Option<String>,
    pub variables: Vec<String>,
    pub unreadable: bool,
    pub versions: Vec<VersionStubView>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct PromptList {
    /// By name.
    pub prompts: Vec<PromptSummary>,
}

/// What a version renders to.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct RenderedPrompt {
    pub version: i64,
    /// The messages with the values put in, as a call would get them.
    pub messages: Vec<TemplateMessage>,
    /// The model the template names, used when a call names none.
    #[schema(required)]
    pub model: Option<String>,
    pub params: Params,
}

fn version_view(row: &VersionRow) -> Result<VersionView, ApiError> {
    let unreadable = || {
        tracing::error!(
            template_id = row.template_id,
            version = row.version,
            "a stored prompt version cannot be read"
        );
        ApiError::internal()
    };
    Ok(VersionView {
        version: row.version,
        messages: serde_json::from_str(&row.messages).map_err(|_| unreadable())?,
        variables: serde_json::from_str(&row.variables).map_err(|_| unreadable())?,
        model: row.model.clone(),
        params: serde_json::from_str(&row.params).map_err(|_| unreadable())?,
        created_by: row.created_by,
        created_at: row.created_at.clone(),
    })
}

fn summary_of(t: &TemplateRow, latest: Option<&VersionRow>, count: i64) -> PromptSummary {
    let read = latest.and_then(|row| version_view(row).ok());
    PromptSummary {
        id: t.id,
        name: t.name.clone(),
        description: t.description.clone(),
        created_by: t.created_by,
        created_at: t.created_at.clone(),
        latest_version: latest.map_or(0, |v| v.version),
        version_count: count,
        updated_at: latest.map_or_else(|| t.created_at.clone(), |v| v.created_at.clone()),
        unreadable: read.is_none(),
        model: read.as_ref().and_then(|v| v.model.clone()),
        variables: read.map(|v| v.variables).unwrap_or_default(),
    }
}

async fn view_of(state: &AppState, t: &TemplateRow) -> Result<PromptView, ApiError> {
    let latest = state.store.prompt_latest_version(t.id).await?;
    let stubs = state.store.prompt_version_stubs(t.id).await?;
    let s = summary_of(t, latest.as_ref(), stubs.len() as i64);
    Ok(PromptView {
        id: s.id,
        name: s.name,
        description: s.description,
        created_by: s.created_by,
        created_at: s.created_at,
        latest_version: s.latest_version,
        version_count: s.version_count,
        updated_at: s.updated_at,
        model: s.model,
        variables: s.variables,
        unreadable: s.unreadable,
        versions: stubs
            .into_iter()
            .map(|v| VersionStubView {
                version: v.version,
                created_by: v.created_by,
                created_at: v.created_at,
            })
            .collect(),
    })
}

/// A version as it is stored, after it passed [`check_version`].
pub(crate) struct Checked {
    messages: String,
    variables: String,
    model: Option<String>,
    params: String,
}

impl Checked {
    fn new_version(&self) -> NewVersion<'_> {
        NewVersion {
            messages: &self.messages,
            variables: &self.variables,
            model: self.model.as_deref(),
            params: &self.params,
        }
    }
}

/// Checks the messages, model and settings of a version, naming every field
/// at fault (`messages[0].role`, `params.temperature`).
pub(crate) fn check_version(
    messages: Vec<serde_json::Value>,
    model: Option<String>,
    params: Option<serde_json::Value>,
    fields: &mut BTreeMap<String, String>,
) -> Option<Checked> {
    let before = fields.len();
    let mut parsed: Vec<TemplateMessage> = Vec::new();
    if messages.is_empty() {
        fields.insert("messages".into(), "add at least one message".into());
    } else if messages.len() > MAX_MESSAGES {
        fields.insert(
            "messages".into(),
            format!("at most {MAX_MESSAGES} messages"),
        );
    } else {
        for (i, value) in messages.into_iter().enumerate() {
            match serde_json::from_value::<TemplateMessage>(value) {
                Err(_) => {
                    fields.insert(
                        format!("messages[{i}]"),
                        "a message is {\"role\": ..., \"content\": ...} and nothing else".into(),
                    );
                }
                Ok(m) if !ROLES.contains(&m.role.as_str()) => {
                    fields.insert(
                        format!("messages[{i}].role"),
                        "role must be system, developer, user or assistant".into(),
                    );
                }
                Ok(m) if m.content.is_empty() => {
                    fields.insert(format!("messages[{i}].content"), "must not be empty".into());
                }
                Ok(m) if m.content.len() > MAX_CONTENT_BYTES => {
                    fields.insert(
                        format!("messages[{i}].content"),
                        format!("must be at most {MAX_CONTENT_BYTES} bytes"),
                    );
                }
                Ok(m) => parsed.push(m),
            }
        }
    }
    let total: usize = parsed.iter().map(|m| m.content.len()).sum();
    if total > MAX_TOTAL_BYTES {
        fields.insert(
            "messages".into(),
            format!("the messages together must be at most {MAX_TOTAL_BYTES} bytes"),
        );
    }
    let variables = prompts::variables_in(parsed.iter().map(|m| m.content.as_str()));
    if variables.len() > MAX_VARIABLES {
        fields.insert(
            "messages".into(),
            format!("at most {MAX_VARIABLES} variables"),
        );
    }
    if let Some(model) = &model {
        let chars = model.chars().count();
        if chars == 0 || chars > MAX_MODEL_CHARS || model.chars().any(char::is_control) {
            fields.insert(
                "model".into(),
                format!("model must be 1 to {MAX_MODEL_CHARS} characters"),
            );
        }
    }
    let params = match params {
        None | Some(serde_json::Value::Null) => Params::default(),
        Some(value) => match serde_json::from_value::<Params>(value) {
            Ok(p) => {
                check_params(&p, fields);
                p
            }
            Err(_) => {
                fields.insert(
                    "params".into(),
                    "params holds temperature, max_tokens, top_p and response_format, and nothing else"
                        .into(),
                );
                Params::default()
            }
        },
    };
    if fields.len() > before {
        return None;
    }
    Some(Checked {
        messages: serde_json::to_string(&parsed).ok()?,
        variables: serde_json::to_string(&variables).ok()?,
        model,
        params: serde_json::to_string(&params).ok()?,
    })
}

fn check_params(p: &Params, fields: &mut BTreeMap<String, String>) {
    if let Some(t) = p.temperature {
        if !(0.0..=2.0).contains(&t) {
            fields.insert("params.temperature".into(), "must be 0 to 2".into());
        }
    }
    if let Some(t) = p.top_p {
        if !(0.0..=1.0).contains(&t) {
            fields.insert("params.top_p".into(), "must be 0 to 1".into());
        }
    }
    if p.max_tokens == Some(0) {
        fields.insert("params.max_tokens".into(), "must be at least 1".into());
    }
    if let Some(format) = &p.response_format {
        if parse_response_format(format).is_err() {
            fields.insert(
                "params.response_format".into(),
                "must be {\"type\":\"text\"}, {\"type\":\"json_object\"} or a json_schema with a name and a schema"
                    .into(),
            );
        }
    }
}

async fn template_of(state: &AppState, raw_id: &str) -> Result<TemplateRow, ApiError> {
    let id = path_id(raw_id)?;
    state
        .store
        .prompt_template(id)
        .await?
        .ok_or_else(ApiError::not_found)
}

/// The name of a template: as any name, and without `@`, which separates
/// the name from the version in the request log (`name@3`).
pub(crate) fn check_template_name(name: &str, fields: &mut BTreeMap<String, String>) {
    check_name(name, fields);
    if name.contains('@') && !fields.contains_key("name") {
        fields.insert(
            "name".into(),
            "name must not contain @ (the log writes name@version)".into(),
        );
    }
}

fn taken() -> ApiError {
    ApiError::conflict(
        "prompt_exists",
        "A prompt template with this name already exists.",
    )
}

#[utoipa::path(
    get,
    path = "/prompts",
    tag = "prompts",
    operation_id = "prompts_list",
    responses(
        (status = 200, description = "Every prompt template, by name. Any signed-in user may read them: anyone who can call may use any template.", body = PromptList),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    authed: Authed,
) -> Result<Response, ApiError> {
    require(&authed.principal, &Action::ListPrompts)?;
    let templates = state.store.list_prompt_templates().await?;
    let latest = state.store.list_latest_prompt_versions().await?;
    let counts = state.store.prompt_version_counts().await?;
    let prompts: Vec<PromptSummary> = templates
        .iter()
        .map(|t| {
            let newest = latest.iter().find(|v| v.template_id == t.id);
            let count = counts
                .iter()
                .find(|(id, _)| *id == t.id)
                .map_or(0, |(_, n)| *n);
            summary_of(t, newest, count)
        })
        .collect();
    Ok(Json(PromptList { prompts }).into_response())
}

#[utoipa::path(
    get,
    path = "/prompts/{id}",
    tag = "prompts",
    operation_id = "prompts_view",
    params(("id" = i64, Path, description = "The id of the template.")),
    responses(
        (status = 200, description = "The template with all its versions.", body = PromptView),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "It does not exist.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn view(
    State(state): State<Arc<AppState>>,
    Path(raw_id): Path<String>,
    authed: Authed,
) -> Result<Response, ApiError> {
    require(&authed.principal, &Action::ListPrompts)?;
    let template = template_of(&state, &raw_id).await?;
    Ok(Json(view_of(&state, &template).await?).into_response())
}

#[utoipa::path(
    get,
    path = "/prompts/{id}/versions/{version}",
    tag = "prompts",
    operation_id = "prompts_version",
    params(
        ("id" = i64, Path, description = "The id of the template."),
        ("version" = i64, Path, description = "The version, from 1."),
    ),
    responses(
        (status = 200, description = "The version, as it was written. A version never changes.", body = VersionView),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "The template or the version does not exist.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn version(
    State(state): State<Arc<AppState>>,
    Path((raw_id, raw_version)): Path<(String, String)>,
    authed: Authed,
) -> Result<Response, ApiError> {
    require(&authed.principal, &Action::ListPrompts)?;
    let id = path_id(&raw_id)?;
    let number = path_id(&raw_version)?;
    let row = state
        .store
        .prompt_version(id, number)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(version_view(&row)?).into_response())
}

#[utoipa::path(
    post,
    path = "/prompts",
    tag = "prompts",
    operation_id = "prompts_create",
    request_body = CreatePromptRequest,
    responses(
        (status = 201, description = "The new template, with its version 1.", body = PromptView),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "Only admins and team leads make templates, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 409, description = "`prompt_exists`: the name is taken.", body = super::openapi::ApiErrorBody),
        (status = 413, description = "The request body is larger than 1 MiB.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "Some fields are not valid; `fields` names each of them (`messages[0].role`, `params.temperature`, `name` for an `@` in the name or when there are already 1000 templates, ...).", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn create(
    State(state): State<Arc<AppState>>,
    authed: Authed,
    ApiJson(req): ApiJson<CreatePromptRequest>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    require(me, &Action::CreatePrompt)?;
    let mut fields = BTreeMap::new();
    check_template_name(&req.name, &mut fields);
    if let Some(description) = &req.description {
        check_description(description, &mut fields);
    }
    let checked = check_version(req.messages, req.model, req.params, &mut fields);
    let (true, Some(checked)) = (fields.is_empty(), checked) else {
        return Err(ApiError::validation(fields));
    };
    let name = req.name.trim().to_string();
    let description = req.description.unwrap_or_default();
    let mut tx = state.store.begin_immediate().await?;
    if tx.count_prompt_templates().await? >= MAX_TEMPLATES as i64 {
        return Err(ApiError::invalid_field(
            "name",
            &format!("there are already {MAX_TEMPLATES} templates, the most there can be; delete one first"),
        ));
    }
    let id = match tx
        .insert_prompt_template(&name, &description, Some(me.user_id))
        .await
    {
        Ok(id) => id,
        Err(e) => {
            return Err(match e.downcast_ref::<StoreError>() {
                Some(StoreError::Duplicate) => taken(),
                None => e.into(),
            })
        }
    };
    tx.insert_prompt_version(id, checked.new_version(), Some(me.user_id))
        .await?;
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "prompt.create",
        target_type: "prompt",
        target_id: Some(id),
        summary: &format!("Created prompt template {name}"),
    })
    .await?;
    tx.commit().await?;
    refresh_snapshot(&state).await?;
    let template = template_of(&state, &id.to_string()).await?;
    Ok((StatusCode::CREATED, Json(view_of(&state, &template).await?)).into_response())
}

#[utoipa::path(
    post,
    path = "/prompts/{id}/versions",
    tag = "prompts",
    operation_id = "prompts_add_version",
    params(("id" = i64, Path, description = "The id of the template.")),
    request_body = CreateVersionRequest,
    responses(
        (status = 201, description = "The new version, numbered after the latest. The versions before it are as they were.", body = VersionView),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "Admins change any template, a team lead the ones they made; or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "It does not exist.", body = super::openapi::ApiErrorBody),
                (status = 413, description = "The request body is larger than 1 MiB.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "Some fields are not valid, or the template has 200 versions; `fields` names each of them.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn add_version(
    State(state): State<Arc<AppState>>,
    Path(raw_id): Path<String>,
    authed: Authed,
    ApiJson(req): ApiJson<CreateVersionRequest>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    let id = path_id(&raw_id)?;
    // Who may is decided on what the transaction reads, and the next number
    // is read and written under the same lock.
    let mut tx = state.store.begin_immediate().await?;
    let template = tx
        .prompt_template(id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    require(
        me,
        &Action::ManagePrompt {
            created_by: template.created_by,
        },
    )?;
    let mut fields = BTreeMap::new();
    let checked = check_version(req.messages, req.model, req.params, &mut fields);
    let (true, Some(checked)) = (fields.is_empty(), checked) else {
        return Err(ApiError::validation(fields));
    };
    if tx.latest_prompt_version(id).await? >= MAX_VERSIONS as i64 {
        return Err(ApiError::invalid_field(
            "messages",
            &format!("a template has at most {MAX_VERSIONS} versions; delete the template or start another"),
        ));
    }
    let number = tx
        .insert_prompt_version(id, checked.new_version(), Some(me.user_id))
        .await?;
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "prompt.version",
        target_type: "prompt",
        target_id: Some(id),
        summary: &format!(
            "Added version {number} to prompt template {}",
            template.name
        ),
    })
    .await?;
    tx.commit().await?;
    refresh_snapshot(&state).await?;
    let row = state
        .store
        .prompt_version(id, number)
        .await?
        .ok_or_else(ApiError::internal)?;
    Ok((StatusCode::CREATED, Json(version_view(&row)?)).into_response())
}

#[utoipa::path(
    delete,
    path = "/prompts/{id}",
    tag = "prompts",
    operation_id = "prompts_delete",
    params(("id" = i64, Path, description = "The id of the template.")),
    responses(
        (status = 204, description = "The template and its versions are deleted. The request logs keep the `name@version` they recorded."),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "Admins delete any template, a team lead the ones they made; or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "It does not exist.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn delete(
    State(state): State<Arc<AppState>>,
    Path(raw_id): Path<String>,
    authed: Authed,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    let id = path_id(&raw_id)?;
    let mut tx = state.store.begin_immediate().await?;
    let template = tx
        .prompt_template(id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    require(
        me,
        &Action::ManagePrompt {
            created_by: template.created_by,
        },
    )?;
    tx.delete_prompt_template(id).await?;
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "prompt.delete",
        target_type: "prompt",
        target_id: Some(id),
        summary: &format!("Deleted prompt template {}", template.name),
    })
    .await?;
    tx.commit().await?;
    refresh_snapshot(&state).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[utoipa::path(
    post,
    path = "/prompts/{id}/render",
    tag = "prompts",
    operation_id = "prompts_render",
    params(("id" = i64, Path, description = "The id of the template.")),
    request_body = RenderRequest,
    responses(
        (status = 200, description = "The messages with the values put in, exactly as a call that names the template gets them.", body = RenderedPrompt),
        (status = 400, description = "A variable is missing or unknown, a value is longer than 32 KiB, or the rendered text is larger than 1 MiB; the message names the variable.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "The template or the version does not exist.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn render(
    State(state): State<Arc<AppState>>,
    Path(raw_id): Path<String>,
    authed: Authed,
    ApiJson(req): ApiJson<RenderRequest>,
) -> Result<Response, ApiError> {
    require(&authed.principal, &Action::ListPrompts)?;
    let template = template_of(&state, &raw_id).await?;
    let row = match req.version {
        None => state.store.prompt_latest_version(template.id).await?,
        Some(n) => {
            state
                .store
                .prompt_version(template.id, i64::from(n))
                .await?
        }
    }
    .ok_or_else(ApiError::not_found)?;
    let version = Version::of_row(&row).ok_or_else(|| {
        tracing::error!(template = %template.name, "a stored prompt version cannot be read");
        ApiError::internal()
    })?;
    let messages = version
        .render_messages(&req.variables)
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok(Json(RenderedPrompt {
        version: version.number,
        messages,
        model: version.model.clone(),
        params: version.params.clone(),
    })
    .into_response())
}
