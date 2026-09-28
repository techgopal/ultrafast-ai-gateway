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
use ultrafast_translate::provider::ProviderKind;

use super::{path_id, require, ApiError, ApiJson, Authed};
use crate::app::AppState;
use crate::config::{validate_base_url, validate_provider_name};
use crate::identity::policy::Action;
use crate::secrets::Cipher;
use crate::store::{AuditEntry, ProviderRow, Store, StoreError};

// The request types hold an API key, so they have neither `Debug` nor
// `Serialize`: there is no way to print one.

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateProviderRequest {
    name: String,
    kind: String,
    base_url: String,
    api_key: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateProviderRequest {
    base_url: Option<String>,
    /// Absent leaves the key, `null` removes it, a string replaces it.
    #[serde(default, deserialize_with = "present")]
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
#[derive(Debug, Serialize)]
pub struct ProviderView {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub base_url: String,
    pub has_credential: bool,
}

impl ProviderView {
    /// Built field by field: the credential is looked at, not carried.
    fn of(p: &ProviderRow) -> Self {
        Self {
            id: p.id,
            name: p.name.clone(),
            kind: p.kind.clone(),
            base_url: p.base_url.clone(),
            has_credential: p.credential.is_some(),
        }
    }
}

/// The messages of the checks in `config` do not repeat what was sent.
fn checked(field: &str, result: anyhow::Result<()>, fields: &mut BTreeMap<String, String>) {
    if let Err(e) = result {
        fields.insert(field.to_string(), e.to_string());
    }
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

pub async fn list(
    State(state): State<Arc<AppState>>,
    authed: Authed,
) -> Result<Response, ApiError> {
    require(&authed.principal, &Action::ListProviders)?;
    let providers = state.store.list_providers().await?;
    let providers: Vec<ProviderView> = providers.iter().map(ProviderView::of).collect();
    Ok(Json(json!({ "providers": providers })).into_response())
}

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
        let message = "kind must be openai or anthropic";
        fields.insert("kind".to_string(), message.to_string());
    }
    checked("base_url", validate_base_url(&req.base_url), &mut fields);
    check_api_key(req.api_key.as_deref(), &mut fields);
    if !fields.is_empty() {
        return Err(ApiError::validation(fields));
    }
    let CreateProviderRequest {
        name,
        kind,
        base_url,
        api_key,
    } = req;
    // From here on only the encrypted form exists.
    let credential = api_key.map(|key| encrypted(&state.cipher, &key));

    let store = &state.store;
    let mut tx = store.begin().await?;
    let inserted = tx
        .insert_provider(&name, &kind, &base_url, credential.as_deref())
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

    let row = store
        .provider_by_id(id)
        .await?
        .ok_or_else(|| anyhow!("the provider is missing after it was created"))?;
    Ok((StatusCode::CREATED, Json(ProviderView::of(&row))).into_response())
}

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
    if req.base_url.is_none() && req.api_key.is_none() {
        return Err(ApiError::bad_request(
            "Send at least one of base_url and api_key.",
        ));
    }

    let mut fields = BTreeMap::new();
    if let Some(base_url) = &req.base_url {
        checked("base_url", validate_base_url(base_url), &mut fields);
    }
    check_api_key(req.api_key.as_ref().and_then(|k| k.as_deref()), &mut fields);
    if !fields.is_empty() {
        return Err(ApiError::validation(fields));
    }
    let UpdateProviderRequest { base_url, api_key } = req;
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
    changes.extend(credential_change);
    if changes.is_empty() {
        // Nothing to change, so nothing to record.
        drop(tx);
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

    let row = store
        .provider_by_id(was.id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(ProviderView::of(&row)).into_response())
}

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
    let target = provider_of(store, &raw_id).await?;

    let mut tx = store.begin().await?;
    if !tx.delete_provider(target.id).await? {
        return Err(ApiError::not_found());
    }
    tx.audit(AuditEntry {
        actor_user_id: Some(me.user_id),
        actor_email: &me.email,
        action: "provider.delete",
        target_type: "provider",
        target_id: Some(target.id),
        summary: &format!("Deleted provider {}", target.name),
    })
    .await?;
    tx.commit().await?;
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
        };
        let shown = serde_json::to_string(&ProviderView::of(&row)).unwrap();
        assert!(shown.contains(r#""has_credential":true"#));
        assert!(!shown.contains("ciphertext"));
        assert_eq!(shown.matches("credential").count(), 1);
    }
}
