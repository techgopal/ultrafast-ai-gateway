//! Virtual key check. Runs before anything else on `/v1`.

use axum::http::{header::AUTHORIZATION, HeaderMap, StatusCode};
use axum::response::Response;

use crate::errors::error_response;
use crate::secrets::{hash_key, KEY_PREFIX};
use crate::store::{KeyRow, Store};

fn unauthorized() -> Response {
    error_response(
        StatusCode::UNAUTHORIZED,
        "authentication_error",
        "Missing or invalid API key.",
    )
}

pub async fn authenticate(store: &Store, headers: &HeaderMap) -> Result<KeyRow, Response> {
    let key = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split_once(' '))
        // HTTP auth schemes are case-insensitive.
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
        .map(|(_, key)| key.trim())
        .filter(|k| k.starts_with(KEY_PREFIX))
        .ok_or_else(unauthorized)?;
    match store.active_key_by_hash(&hash_key(key)).await {
        Ok(Some(row)) => Ok(row),
        Ok(None) => Err(unauthorized()),
        Err(e) => {
            tracing::error!(error = %e, "key lookup failed");
            Err(error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "server_error",
                "Could not verify the API key.",
            ))
        }
    }
}
