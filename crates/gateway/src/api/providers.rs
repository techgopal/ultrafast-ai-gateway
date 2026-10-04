//! Upstream providers. Their credentials go in and never come out.

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::anyhow;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::json;
use ultrafast_translate::provider::{ProviderKind, DEFAULT_AZURE_API_VERSION};

use super::{path_id, refresh_snapshot, require, ApiError, ApiJson, Authed};
use crate::app::AppState;
use crate::config::{same_host, validate_api_version, validate_base_url, validate_provider_name};
use crate::identity::policy::Action;
use crate::secrets::Cipher;
use crate::store::{AuditEntry, ProviderRow, Store, StoreError};

// The request types hold an API key, so they have neither `Debug` nor
// `Serialize`: there is no way to print one.

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateProviderRequest {
    name: String,
    kind: String,
    base_url: String,
    #[schema(write_only)]
    api_key: Option<String>,
    /// Azure OpenAI only; `2024-10-21` when left out.
    api_version: Option<String>,
}

#[derive(Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateProviderRequest {
    base_url: Option<String>,
    /// Azure OpenAI only.
    api_version: Option<String>,
    /// Absent leaves the key, `null` removes it, a string replaces it.
    #[serde(default, deserialize_with = "present")]
    #[schema(value_type = Option<String>, write_only)]
    api_key: Option<Option<String>>,
}

/// Reads a field that is there, so `null` differs from a missing field.
fn present<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(Some)
}

/// A provider as `/api` shows it: whether it has a credential, never the
/// credential.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ProviderView {
    pub id: i64,
    pub name: String,
    pub kind: String,
    /// Where the provider is called. Admins only: `null` for anybody else.
    #[schema(required)]
    pub base_url: Option<String>,
    pub has_credential: bool,
    /// Set for Azure OpenAI providers.
    pub api_version: Option<String>,
}

impl ProviderView {
    /// Built field by field: the credential is looked at, not carried.
    fn of(p: &ProviderRow) -> Self {
        Self {
            id: p.id,
            name: p.name.clone(),
            kind: p.kind.clone(),
            base_url: Some(p.base_url.clone()),
            has_credential: p.credential.is_some(),
            api_version: p.api_version.clone(),
        }
    }

    /// As `of`, for who may see where the provider is: an admin.
    fn for_viewer(p: &ProviderRow, admin: bool) -> Self {
        let mut view = Self::of(p);
        if !admin {
            view.base_url = None;
        }
        view
    }
}

/// The messages of the checks in `config` do not repeat what was sent.
pub(crate) fn checked(
    field: &str,
    result: anyhow::Result<()>,
    fields: &mut BTreeMap<String, String>,
) {
    if let Err(e) = result {
        fields.insert(field.to_string(), e.to_string());
    }
}

/// Only Azure OpenAI has an API version. For Azure a missing one is the
/// default; the result is what is stored.
pub(crate) fn check_api_version(
    kind: &str,
    api_version: Option<&str>,
    fields: &mut BTreeMap<String, String>,
) -> Option<String> {
    if ProviderKind::parse(kind) != Some(ProviderKind::Azure) {
        if api_version.is_some() {
            let message = "only Azure OpenAI providers have an API version";
            fields.insert("api_version".to_string(), message.to_string());
        }
        return None;
    }
    let version = api_version.unwrap_or(DEFAULT_AZURE_API_VERSION);
    checked("api_version", validate_api_version(version), fields);
    Some(version.to_string())
}

/// Refuses an API key that is empty or only whitespace.
fn check_api_key(api_key: Option<&str>, fields: &mut BTreeMap<String, String>) {
    if api_key.is_some_and(|key| key.trim().is_empty()) {
        fields.insert("api_key".to_string(), "must not be empty".to_string());
    }
}

/// Whitespace around a pasted key is not part of it.
fn encrypted(cipher: &Cipher, api_key: &str) -> Vec<u8> {
    cipher.encrypt(api_key.trim().as_bytes())
}

/// The provider of a path, or the answer for one that does not exist.
async fn provider_of(store: &Store, raw_id: &str) -> Result<ProviderRow, ApiError> {
    let id = path_id(raw_id)?;
    store
        .provider_by_id(id)
        .await?
        .ok_or_else(ApiError::not_found)
}

#[utoipa::path(
    get,
    path = "/providers",
    tag = "providers",
    operation_id = "providers_list",
    responses(
        (status = 200, description = "Every provider. Only an admin gets `base_url`; anybody else gets `null`.", body = super::openapi::ProviderList),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn list(
    State(state): State<Arc<AppState>>,
    authed: Authed,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    require(me, &Action::ListProviders)?;
    let providers = state.store.list_providers().await?;
    let providers: Vec<ProviderView> = providers
        .iter()
        .map(|p| ProviderView::for_viewer(p, me.is_admin()))
        .collect();
    Ok(Json(json!({ "providers": providers })).into_response())
}

#[utoipa::path(
    post,
    path = "/providers",
    tag = "providers",
    operation_id = "providers_create",
    request_body = CreateProviderRequest,
    responses(
        (status = 201, description = "The new provider.", body = ProviderView),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 409, description = "`provider_exists`: the name is taken.", body = super::openapi::ApiErrorBody),
        (status = 413, description = "The request body is too large.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "Some fields are not valid; `fields` names each of them.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn create(
    State(state): State<Arc<AppState>>,
    authed: Authed,
    ApiJson(req): ApiJson<CreateProviderRequest>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    require(me, &Action::ManageProviders)?;

    let mut fields = BTreeMap::new();
    checked("name", validate_provider_name(&req.name), &mut fields);
    if ProviderKind::parse(&req.kind).is_none() {
        let message = "kind must be openai, anthropic, gemini or azure";
        fields.insert("kind".to_string(), message.to_string());
    }
    checked("base_url", validate_base_url(&req.base_url), &mut fields);
    check_api_key(req.api_key.as_deref(), &mut fields);
    let api_version = check_api_version(&req.kind, req.api_version.as_deref(), &mut fields);
    if !fields.is_empty() {
        return Err(ApiError::validation(fields));
    }
    let CreateProviderRequest {
        name,
        kind,
        base_url,
        api_key,
        ..
    } = req;
    // From here on only the encrypted form exists.
    let credential = api_key.map(|key| encrypted(&state.cipher, &key));

    let store = &state.store;
    let mut tx = store.begin().await?;
    let inserted = tx
        .insert_provider_versioned(
            &name,
            &kind,
            &base_url,
            credential.as_deref(),
            api_version.as_deref(),
        )
        .await;
    let id = match inserted {
        Ok(id) => id,
        Err(e) => {
            return Err(match e.downcast_ref::<StoreError>() {
                Some(StoreError::Duplicate) => ApiError::conflict(
                    "provider_exists",
                    "A provider with this name already exists.",
                ),
                None => e.into(),
            })
        }
    };
    let summary = if credential.is_some() {
        format!("Created provider {name} ({kind}), credential set")
    } else {
        format!("Created provider {name} ({kind})")
    };
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "provider.create",
        target_type: "provider",
        target_id: Some(id),
        summary: &summary,
    })
    .await?;
    tx.commit().await?;
    refresh_snapshot(&state).await?;

    let row = store
        .provider_by_id(id)
        .await?
        .ok_or_else(|| anyhow!("the provider is missing after it was created"))?;
    Ok((StatusCode::CREATED, Json(ProviderView::of(&row))).into_response())
}

#[utoipa::path(
    patch,
    path = "/providers/{id}",
    tag = "providers",
    operation_id = "providers_update",
    params(
        ("id" = i64, Path, description = "The id of the provider."),
    ),
    request_body = UpdateProviderRequest,
    responses(
        (status = 200, description = "The provider after the change.", body = ProviderView),
        (status = 400, description = "The request is not of the expected form.", body = super::openapi::ApiErrorBody),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "It does not exist, or it is hidden from the caller.", body = super::openapi::ApiErrorBody),
        (status = 413, description = "The request body is too large.", body = super::openapi::ApiErrorBody),
        (status = 422, description = "Some fields are not valid; `fields` names each of them.", body = super::openapi::ApiErrorBody),
        (status = 500, description = "Something went wrong.", body = super::openapi::ApiErrorBody),
    ),
    security(("session" = []), ("token" = [])),
)]
pub async fn update(
    State(state): State<Arc<AppState>>,
    Path(raw_id): Path<String>,
    authed: Authed,
    ApiJson(req): ApiJson<UpdateProviderRequest>,
) -> Result<Response, ApiError> {
    let me = &authed.principal;
    let store = &state.store;
    // The answer does not depend on the provider, so it comes before the
    // id is looked at: a refusal is the same for every id.
    require(me, &Action::ManageProviders)?;
    let target = provider_of(store, &raw_id).await?;
    if req.base_url.is_none() && req.api_key.is_none() && req.api_version.is_none() {
        return Err(ApiError::bad_request(
            "Send at least one of base_url, api_key and api_version.",
        ));
    }

    let mut fields = BTreeMap::new();
    if let Some(base_url) = &req.base_url {
        checked("base_url", validate_base_url(base_url), &mut fields);
    }
    check_api_key(req.api_key.as_ref().and_then(|k| k.as_deref()), &mut fields);
    if req.api_version.is_some() {
        check_api_version(&target.kind, req.api_version.as_deref(), &mut fields);
    }
    if !fields.is_empty() {
        return Err(ApiError::validation(fields));
    }
    let UpdateProviderRequest {
        base_url,
        api_key,
        api_version,
    } = req;
    // From here on only the encrypted form exists.
    let credential: Option<Option<Vec<u8>>> =
        api_key.map(|key| key.map(|key| encrypted(&state.cipher, &key)));

    let mut tx = store.begin().await?;
    // What is compared and recorded is what the transaction sees.
    let was = tx
        .provider_by_id(target.id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let base_url = base_url.filter(|url| *url != was.base_url);
    // The stored key goes only to the host it was given for: another host
    // needs it again, or its removal.
    if was.credential.is_some()
        && credential.is_none()
        && base_url
            .as_deref()
            .is_some_and(|url| !same_host(&was.base_url, url))
    {
        return Err(ApiError::invalid_field(
            "api_key",
            "Enter the API key again: the host changed.",
        ));
    }
    let api_version = api_version.filter(|v| was.api_version.as_deref() != Some(v.as_str()));
    let credential_change = match (&credential, was.credential.is_some()) {
        (None, _) | (Some(None), false) => None,
        (Some(None), true) => Some("credential removed"),
        (Some(Some(_)), true) => Some("credential replaced"),
        (Some(Some(_)), false) => Some("credential set"),
    };
    let mut changes = Vec::new();
    if base_url.is_some() {
        changes.push("base URL changed");
    }
    if api_version.is_some() {
        changes.push("API version changed");
    }
    changes.extend(credential_change);
    if changes.is_empty() {
        // Nothing to change, so nothing to record.
        drop(tx);
        // An earlier call may have made the change and failed to refresh.
        refresh_snapshot(&state).await?;
        return Ok(Json(ProviderView::of(&was)).into_response());
    }

    let credential = credential_change.and(credential);
    let updated = tx
        .update_provider(
            was.id,
            base_url.as_deref(),
            credential.as_ref().map(|c| c.as_deref()),
        )
        .await?;
    if !updated {
        return Err(ApiError::not_found());
    }
    if let Some(version) = &api_version {
        tx.set_provider_api_version(was.id, Some(version)).await?;
    }
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "provider.update",
        target_type: "provider",
        target_id: Some(was.id),
        summary: &format!("Updated provider {}: {}", was.name, changes.join(", ")),
    })
    .await?;
    tx.commit().await?;
    refresh_snapshot(&state).await?;

    let row = store
        .provider_by_id(was.id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(ProviderView::of(&row)).into_response())
}

#[utoipa::path(
    delete,
    path = "/providers/{id}",
    tag = "providers",
    operation_id = "providers_delete",
    params(
        ("id" = i64, Path, description = "The id of the provider."),
    ),
    responses(
        (status = 204, description = "The provider is deleted."),
        (status = 401, description = "No valid session or access token.", body = super::openapi::ApiErrorBody),
        (status = 403, description = "The caller is not allowed to do this, or the CSRF token is missing or does not match.", body = super::openapi::ApiErrorBody),
        (status = 404, description = "It does not exist, or it is hidden from the caller.", body = super::openapi::ApiErrorBody),
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
    let store = &state.store;
    // The answer does not depend on the provider, so it comes before the
    // id is looked at: a refusal is the same for every id.
    require(me, &Action::ManageProviders)?;
    let target = match provider_of(store, &raw_id).await {
        Ok(target) => target,
        Err(e) => {
            // An earlier call may have deleted it and failed to refresh.
            refresh_snapshot(&state).await?;
            return Err(e);
        }
    };

    let mut tx = store.begin().await?;
    // The foreign keys take the models and their grants along.
    let models = tx.count_models_of(target.id).await?;
    if !tx.delete_provider(target.id).await? {
        drop(tx);
        refresh_snapshot(&state).await?;
        return Err(ApiError::not_found());
    }
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "provider.delete",
        target_type: "provider",
        target_id: Some(target.id),
        summary: &if models == 0 {
            format!("Deleted provider {}", target.name)
        } else {
            format!("Deleted provider {} and its {models} models", target.name)
        },
    })
    .await?;
    tx.commit().await?;
    refresh_snapshot(&state).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(body: &str) -> Option<UpdateProviderRequest> {
        serde_json::from_str(body).ok()
    }

    #[test]
    fn a_null_key_differs_from_a_missing_one() {
        assert!(parse("{}").unwrap().api_key.is_none());
        assert!(parse(r#"{"api_key":null}"#).unwrap().api_key == Some(None));
        let sent = parse(r#"{"api_key":"sk-1"}"#).unwrap().api_key;
        assert!(sent == Some(Some("sk-1".to_string())));
        assert!(parse(r#"{"api_key":5}"#).is_none());
        assert!(parse(r#"{"name":"x"}"#).is_none());
    }

    #[test]
    fn empty_keys_are_refused() {
        for (key, refused) in [
            (None, false),
            (Some("sk-1"), false),
            (Some(" sk-1 "), false),
            (Some(""), true),
            (Some("  "), true),
            (Some("\n\t"), true),
        ] {
            let mut fields = BTreeMap::new();
            check_api_key(key, &mut fields);
            assert_eq!(fields.contains_key("api_key"), refused, "{key:?}");
        }
    }

    #[test]
    fn a_view_has_no_credential() {
        let row = ProviderRow {
            id: 1,
            name: "openai".into(),
            kind: "openai".into(),
            base_url: "https://api.openai.com/v1".into(),
            credential: Some(b"ciphertext".to_vec()),
            api_version: None,
        };
        let shown = serde_json::to_string(&ProviderView::of(&row)).unwrap();
        assert!(shown.contains(r#""has_credential":true"#));
        assert!(!shown.contains("ciphertext"));
        assert_eq!(shown.matches("credential").count(), 1);
    }
}
