//! The `prompt` object of a call: which stored template to render, at which
//! version, with which values. Shared by `/v1/chat/completions` (an extension
//! field) and `/v1/responses` (OpenAI's own field); the gateway applies it.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::error::TranslateError;

/// A reference to a stored prompt template (`prompt` in the request).
#[derive(Debug, Clone, PartialEq)]
pub struct PromptRef {
    /// The name of the template.
    pub id: String,
    /// Digits only. OpenAI's field is a string; an integer is accepted and
    /// written out. `None`: the latest version.
    pub version: Option<String>,
    pub variables: BTreeMap<String, String>,
}

fn invalid(m: &str) -> TranslateError {
    TranslateError::InvalidRequest(m.to_string())
}

/// Reads the `prompt` object. Nothing in it is repeated in an error.
pub fn parse_prompt(v: &Value) -> Result<PromptRef, TranslateError> {
    let o = v
        .as_object()
        .ok_or_else(|| invalid("prompt must be an object"))?;
    super::openai::reject_unknown(o, &["id", "version", "variables"])?;
    let id = v["id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| invalid("prompt 'id' must be a string"))?
        .to_string();
    let version =
        match &v["version"] {
            Value::Null => None,
            Value::String(s) if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) => {
                Some(s.clone())
            }
            Value::Number(n) if n.is_u64() => Some(n.to_string()),
            _ => return Err(invalid(
                "prompt 'version' must be a positive integer, as a number or a string of digits",
            )),
        };
    let mut variables = BTreeMap::new();
    match &v["variables"] {
        Value::Null => {}
        Value::Object(m) => {
            for (k, val) in m {
                let s = val
                    .as_str()
                    .ok_or_else(|| invalid("prompt 'variables' must map names to strings"))?;
                variables.insert(k.clone(), s.to_string());
            }
        }
        _ => return Err(invalid("prompt 'variables' must be an object")),
    }
    Ok(PromptRef {
        id,
        version,
        variables,
    })
}
