//! The model catalog: what each provider offers and who may call it.

pub mod sync;

use anyhow::{bail, Result};

use crate::store::{AuditEntry, Grants, Store};

/// Longest model name, in characters.
pub const MAX_MODEL_NAME_CHARS: usize = 200;

/// A model name is the provider's model id verbatim: 1 to 200 characters,
/// no whitespace and no control character. `/`, `:`, `.`, `-`, `_` and `@`
/// are ordinary characters.
pub fn validate_model_name(name: &str) -> Result<(), &'static str> {
    let chars = name.chars().count();
    if chars == 0 || chars > MAX_MODEL_NAME_CHARS {
        return Err("name must be 1 to 200 characters");
    }
    if name.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("name must not contain whitespace or control characters");
    }
    Ok(())
}

/// What `add_model` did.
#[derive(Debug, PartialEq, Eq)]
pub struct ModelAdded {
    /// The model was not in the catalog before.
    pub created: bool,
}

/// `ultrafast model add`: puts the provider's model in the catalog when it is
/// missing, enables it when `enable`, and grants it to everyone when
/// `everyone`. The same name validation as the API. Audited as the CLI.
pub async fn add_model(
    store: &Store,
    provider: &str,
    model: &str,
    enable: bool,
    everyone: bool,
) -> Result<ModelAdded> {
    if let Err(message) = validate_model_name(model) {
        bail!("{message}");
    }
    let Some(provider_row) = store.provider_by_name(provider).await? else {
        bail!("there is no provider '{provider}'");
    };
    let mut tx = store.begin().await?;
    let (id, created) = match tx.model_id_by_name(provider_row.id, model).await? {
        Some(id) => (id, false),
        None => (tx.insert_model(provider_row.id, model).await?, true),
    };
    let mut did = Vec::new();
    if created {
        did.push("created");
    }
    if enable {
        tx.set_model_enabled(id, true).await?;
        did.push("enabled");
    }
    if everyone {
        tx.replace_grants(
            id,
            &Grants {
                everyone: true,
                ..Grants::default()
            },
        )
        .await?;
        did.push("granted to everyone");
    }
    if !did.is_empty() {
        tx.audit(AuditEntry {
            actor_user_id: None,
            actor_email: "cli",
            action: "model.add",
            target_type: "model",
            target_id: Some(id),
            summary: &format!("Model {model} of {provider}: {}", did.join(", ")),
        })
        .await?;
    }
    tx.commit().await?;
    Ok(ModelAdded { created })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_ids_are_valid_names() {
        for good in [
            "gpt-4o-2024-08-06",
            "models/gemini-2.0-flash",
            "meta-llama/Llama-3.3-70B",
            "llama3.2:3b",
            "claude-3-5-sonnet@20241022",
            "x",
        ] {
            assert!(validate_model_name(good).is_ok(), "{good}");
        }
        assert!(validate_model_name(&"é".repeat(200)).is_ok());
    }

    #[test]
    fn whitespace_control_and_length_are_refused() {
        for bad in [
            "",
            " a",
            "a b",
            "a\tb",
            "a\nb",
            "a\u{0}",
            "a\u{a0}b",
            &"x".repeat(201),
        ] {
            assert!(validate_model_name(bad).is_err(), "{bad:?}");
        }
    }
}
