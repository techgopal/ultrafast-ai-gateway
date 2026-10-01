//! Virtual key check. Runs before anything else on `/v1`.

use axum::http::{header::AUTHORIZATION, HeaderMap, StatusCode};
use axum::response::Response;

use crate::errors::Shape;
use crate::secrets::{hash_key, KEY_PREFIX};
use std::sync::Arc;

use crate::snapshot::{SnapKey, Snapshot};
use crate::store::now;

fn unauthorized(shape: Shape) -> Response {
    shape.error(
        StatusCode::UNAUTHORIZED,
        "authentication_error",
        "Missing or invalid API key.",
    )
}

// The error is the finished answer; boxing it would change the interface.
#[allow(clippy::result_large_err)]
pub fn authenticate(
    snapshot: &Snapshot,
    headers: &HeaderMap,
    shape: Shape,
) -> Result<Arc<SnapKey>, Response> {
    // `Authorization: Bearer <key>` (OpenAI clients) or `x-api-key: <key>`
    // (Anthropic clients).
    let bearer = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split_once(' '))
        // HTTP auth schemes are case-insensitive.
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
        .map(|(_, key)| key.trim());
    let x_api_key = headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(str::trim);
    let key = bearer
        .into_iter()
        .chain(x_api_key)
        .find(|k| k.starts_with(KEY_PREFIX))
        .ok_or_else(|| unauthorized(shape))?;
    snapshot
        .key(&hash_key(key), &now())
        .map(Arc::clone)
        .ok_or_else(|| unauthorized(shape))
}
