//! Virtual key check. Runs before anything else on `/v1`.

use axum::http::{header::AUTHORIZATION, HeaderMap, StatusCode};
use axum::response::Response;

use crate::errors::error_response;
use crate::secrets::{hash_key, KEY_PREFIX};
use crate::snapshot::{SnapKey, Snapshot};
use crate::store::now;

fn unauthorized() -> Response {
    error_response(
        StatusCode::UNAUTHORIZED,
        "authentication_error",
        "Missing or invalid API key.",
    )
}

// The error is the finished answer; boxing it would change the interface.
#[allow(clippy::result_large_err)]
pub fn authenticate(snapshot: &Snapshot, headers: &HeaderMap) -> Result<SnapKey, Response> {
    let key = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split_once(' '))
        // HTTP auth schemes are case-insensitive.
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
        .map(|(_, key)| key.trim())
        .filter(|k| k.starts_with(KEY_PREFIX))
        .ok_or_else(unauthorized)?;
    snapshot
        .key(&hash_key(key), &now())
        .cloned()
        .ok_or_else(unauthorized)
}
