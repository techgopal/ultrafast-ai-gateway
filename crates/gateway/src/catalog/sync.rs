//! Reading the model list of a provider.

use std::time::Duration;

use futures::StreamExt;
use serde_json::Value;
use ultrafast_translate::provider::ProviderKind;

use crate::snapshot::SnapProvider;

/// How long one request to a provider may take.
pub const SYNC_TIMEOUT: Duration = Duration::from_secs(10);
/// The most a provider's answer may hold.
pub const MAX_LIST_BYTES: usize = 4 * 1024 * 1024;
/// The most pages of a paged list that are followed.
pub const MAX_PAGES: usize = 10;
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Why a list could not be read. None of them holds anything the provider
/// sent, so any of them may be logged.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("the provider answered with status {status}")]
    Provider { status: u16 },
    #[error("the provider could not be reached")]
    Unreachable,
    #[error("the provider's answer is not a list of models")]
    Malformed,
    #[error("the provider's answer is too large")]
    TooLarge,
}

/// The ids of the models the provider offers, in the order it gives them.
/// The credential goes only into the request.
pub async fn fetch_model_names(
    http: &reqwest::Client,
    provider: &SnapProvider,
) -> Result<Vec<String>, SyncError> {
    let base = provider.base_url.trim_end_matches('/');
    match provider.kind {
        ProviderKind::OpenAi => {
            let url = format!("{base}/models");
            let page = get_json(http, &url, |r| match &provider.api_key {
                Some(key) => r.bearer_auth(key),
                None => r,
            })
            .await?;
            ids_of(&page)
        }
        ProviderKind::Anthropic => {
            let mut names = Vec::new();
            let mut after: Option<String> = None;
            for _ in 0..MAX_PAGES {
                let mut url = format!("{base}/v1/models?limit=1000");
                if let Some(id) = &after {
                    url.push_str("&after_id=");
                    url.push_str(&percent_encode(id));
                }
                let page = get_json(http, &url, |r| {
                    let r = r.header("anthropic-version", ANTHROPIC_VERSION);
                    match &provider.api_key {
                        Some(key) => r.header("x-api-key", key),
                        None => r,
                    }
                })
                .await?;
                names.extend(ids_of(&page)?);
                let more = page["has_more"].as_bool().unwrap_or(false);
                match page["last_id"].as_str() {
                    Some(last) if more => after = Some(last.to_string()),
                    _ => break,
                }
            }
            Ok(names)
        }
    }
}

async fn get_json(
    http: &reqwest::Client,
    url: &str,
    authorize: impl FnOnce(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
) -> Result<Value, SyncError> {
    let request = authorize(http.get(url).timeout(SYNC_TIMEOUT));
    let response = request.send().await.map_err(|_| SyncError::Unreachable)?;
    let status = response.status();
    if !status.is_success() {
        return Err(SyncError::Provider {
            status: status.as_u16(),
        });
    }
    let mut body = Vec::new();
    let mut chunks = response.bytes_stream();
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk.map_err(|_| SyncError::Unreachable)?;
        if chunk.len() > MAX_LIST_BYTES - body.len() {
            return Err(SyncError::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(|_| SyncError::Malformed)
}

/// `data[].id` of a page. An entry without a text id is skipped.
fn ids_of(page: &Value) -> Result<Vec<String>, SyncError> {
    let data = page["data"].as_array().ok_or(SyncError::Malformed)?;
    Ok(data
        .iter()
        .filter_map(|m| m["id"].as_str().map(str::to_string))
        .collect())
}

/// Escapes everything but the characters that are safe in a query value.
fn percent_encode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for b in raw.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(char::from(b));
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_values_are_escaped() {
        assert_eq!(percent_encode("claude-3.5_x~"), "claude-3.5_x~");
        assert_eq!(percent_encode("a b&c=d/é"), "a%20b%26c%3Dd%2F%C3%A9");
    }

    #[test]
    fn ids_come_from_data() {
        let page = serde_json::json!({ "data": [{ "id": "a" }, { "id": 5 }, {}, { "id": "b" }] });
        assert_eq!(ids_of(&page).unwrap(), ["a", "b"]);
        assert!(ids_of(&serde_json::json!({ "models": [] })).is_err());
    }
}
