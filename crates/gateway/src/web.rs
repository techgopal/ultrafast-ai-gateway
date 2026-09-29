//! The web console, compiled into the binary and served at `/`.
//!
//! `build.rs` puts the console's build output, or a page that says it was
//! not built, into `$OUT_DIR/console`. Everything served here comes from
//! that copy inside the binary: a requested path is only ever a name looked
//! up in it, never a path read from disk.

use std::sync::Arc;

use axum::body::Body;
use axum::http::header::{
    HeaderName, ALLOW, CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, ETAG, IF_NONE_MATCH,
    REFERRER_POLICY, X_CONTENT_TYPE_OPTIONS, X_FRAME_OPTIONS,
};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use rust_embed::{Embed, EmbeddedFile};

use crate::app::AppState;
use crate::secrets::fill_random;

/// Whether a real console build was embedded. When false, `/` serves a
/// page that says how to build it.
pub const CONSOLE_BUILT: bool = cfg!(console_built);

#[derive(Embed)]
#[folder = "$OUT_DIR/console"]
struct Files;

const INDEX: &str = "index.html";
/// Served when there is no console to serve.
const NOT_BUILT_PAGE: &str = include_str!(concat!(env!("OUT_DIR"), "/console_placeholder.html"));
/// Stands in `index.html` where the nonce of a response goes.
const NONCE_PLACEHOLDER: &str = "__CSP_NONCE__";
const NONCE_BYTES: usize = 16;

const HTML: &str = "text/html; charset=utf-8";
const NEVER_STORED: &str = "no-store";
const REVALIDATED: &str = "no-cache";
const IMMUTABLE: &str = "public, max-age=31536000, immutable";
const NOSNIFF: HeaderValue = HeaderValue::from_static("nosniff");
const CROSS_ORIGIN_OPENER_POLICY: HeaderName =
    HeaderName::from_static("cross-origin-opener-policy");
const PERMISSIONS_POLICY: HeaderName = HeaderName::from_static("permissions-policy");

/// Paths the console never answers, whatever reaches the fallback.
const RESERVED: [&str; 3] = ["/api", "/v1", "/health"];
const RESERVED_PREFIXES: [&str; 2] = ["/api/", "/v1/"];
const ASSETS: &str = "/assets";
/// How many times a path is percent-decoded when it is checked.
const MAX_DECODE_ROUNDS: usize = 4;

/// The console: `/`, the files of its build, and as the fallback every
/// other path, so a link into the app works when opened directly.
pub fn router() -> Router<Arc<AppState>> {
    router_for(Page::embedded())
}

fn router_for<S>(page: Page) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let page = Arc::new(page);
    let root = page.clone();
    Router::new()
        .route("/", get(move || async move { root.response() }))
        .route("/assets/{*name}", get(asset))
        .fallback(
            move |method: Method, uri: Uri, headers: HeaderMap| async move {
                let head = method == Method::HEAD;
                let mut response = other(&page, &method, uri.path(), &headers);
                if head {
                    *response.body_mut() = Body::empty();
                }
                response
            },
        )
}

/// The page every path of the app is answered with.
enum Page {
    /// `index.html` of the console, split where the nonce goes.
    Console {
        before: String,
        after: String,
    },
    NotBuilt,
}

impl Page {
    fn embedded() -> Self {
        if !CONSOLE_BUILT {
            return Self::NotBuilt;
        }
        let index = Files::get(INDEX).map(|file| file.data.into_owned());
        match Self::of_console(index.as_deref().unwrap_or_default()) {
            Ok(page) => page,
            Err(reason) => {
                tracing::error!(reason, "the console cannot be served");
                Self::NotBuilt
            }
        }
    }

    /// The page of a built console. Its `index.html` must hold the nonce
    /// placeholder exactly once.
    fn of_console(index: &[u8]) -> Result<Self, &'static str> {
        let index = std::str::from_utf8(index).map_err(|_| "index.html is not UTF-8")?;
        let mut parts = index.split(NONCE_PLACEHOLDER);
        match (parts.next(), parts.next(), parts.next()) {
            (Some(before), Some(after), None) => Ok(Self::Console {
                before: before.to_string(),
                after: after.to_string(),
            }),
            (_, None, _) => Err("index.html does not hold the nonce placeholder"),
            _ => Err("index.html holds the nonce placeholder more than once"),
        }
    }

    /// A fresh nonce for every response, so the page is never stored.
    fn response(&self) -> Response {
        let nonce = new_nonce();
        let body = match self {
            Self::Console { before, after } => format!("{before}{nonce}{after}"),
            Self::NotBuilt => NOT_BUILT_PAGE.to_string(),
        };
        let mut response = body.into_response();
        let headers = response.headers_mut();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static(HTML));
        headers.insert(CACHE_CONTROL, HeaderValue::from_static(NEVER_STORED));
        security_headers(headers, &nonce);
        response
    }
}

/// 16 bytes from the operating system, as base64 without padding.
fn new_nonce() -> String {
    let mut bytes = [0u8; NONCE_BYTES];
    fill_random(&mut bytes);
    base64(&bytes)
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let mut group = [0u8; 3];
        group[..chunk.len()].copy_from_slice(chunk);
        let bits = u32::from_be_bytes([0, group[0], group[1], group[2]]);
        // One character for every 6 bits that hold data.
        for i in 0..=chunk.len() {
            let index = (bits >> (18 - 6 * i)) & 0x3f;
            out.push(char::from(ALPHABET[index as usize]));
        }
    }
    out
}

/// The headers of every HTML response.
fn security_headers(headers: &mut HeaderMap, nonce: &str) {
    let policy = format!(
        "default-src 'none'; script-src 'self'; style-src 'self' 'nonce-{nonce}'; \
         style-src-attr 'unsafe-inline'; img-src 'self' data:; font-src 'self'; \
         connect-src 'self'; form-action 'self'; base-uri 'none'; \
         frame-ancestors 'none'; manifest-src 'self'"
    );
    headers.insert(
        CONTENT_SECURITY_POLICY,
        HeaderValue::from_str(&policy).expect("a base64 nonce is valid in a header"),
    );
    headers.insert(X_CONTENT_TYPE_OPTIONS, NOSNIFF);
    headers.insert(REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    headers.insert(X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(
        CROSS_ORIGIN_OPENER_POLICY,
        HeaderValue::from_static("same-origin"),
    );
    headers.insert(
        PERMISSIONS_POLICY,
        HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
    );
}

/// `GET /assets/<name>`: the file, or 404. Never the page.
async fn asset(uri: Uri, headers: HeaderMap) -> Response {
    let path = uri.path();
    if !is_plain(path) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let name = path.trim_start_matches('/');
    match Files::get(name) {
        Some(file) => file_response(name, file, IMMUTABLE, &headers),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Every path no route claimed.
fn other(page: &Page, method: &Method, path: &str, headers: &HeaderMap) -> Response {
    if RESERVED.contains(&path) || RESERVED_PREFIXES.iter().any(|p| path.starts_with(p)) {
        return StatusCode::NOT_FOUND.into_response();
    }
    if method != Method::GET && method != Method::HEAD {
        return (StatusCode::METHOD_NOT_ALLOWED, [(ALLOW, "GET, HEAD")]).into_response();
    }
    if !is_plain(path) || path == ASSETS || path.starts_with("/assets/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    // A file at the root of the build, such as `theme.js`.
    let name = path.trim_start_matches('/');
    if name != INDEX && !name.contains('/') {
        if let Some(file) = Files::get(name) {
            return file_response(name, file, REVALIDATED, headers);
        }
    }
    page.response()
}

/// Whether the path, however often it was percent-encoded, is free of
/// `..`, backslashes and NUL bytes.
fn is_plain(path: &str) -> bool {
    let mut bytes = path.as_bytes().to_vec();
    for _ in 0..MAX_DECODE_ROUNDS {
        let bad =
            bytes.contains(&b'\\') || bytes.contains(&0) || bytes.windows(2).any(|w| w == b"..");
        if bad {
            return false;
        }
        let decoded = percent_decoded(&bytes);
        if decoded == bytes {
            return true;
        }
        bytes = decoded;
    }
    // Still encoded after every round: nothing the console names.
    false
}

/// Decodes every `%XX`. Anything else is kept as it is.
fn percent_decoded(bytes: &[u8]) -> Vec<u8> {
    let hex = |b: u8| char::from(b).to_digit(16);
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let pair = match bytes.get(i + 1..i + 3) {
            Some([high, low]) if bytes[i] == b'%' => hex(*high).zip(hex(*low)),
            _ => None,
        };
        match pair {
            Some((high, low)) => {
                // Two hex digits are at most 255.
                out.push((high * 16 + low) as u8);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    out
}

fn file_response(
    name: &str,
    file: EmbeddedFile,
    cache: &'static str,
    request: &HeaderMap,
) -> Response {
    let etag = format!("\"{}\"", hex::encode(&file.metadata.sha256_hash()[..16]));
    let unchanged = request
        .get_all(IF_NONE_MATCH)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(|tag| tag.trim())
        .any(|tag| tag == "*" || tag.strip_prefix("W/").unwrap_or(tag) == etag);

    let mime = mime_guess::from_path(name).first_or_octet_stream();
    let mut response = if unchanged {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        let content_type = if mime.type_() == mime_guess::mime::TEXT {
            format!("{}; charset=utf-8", mime.essence_str())
        } else {
            mime.essence_str().to_string()
        };
        let mut response = file.data.into_owned().into_response();
        response.headers_mut().insert(
            CONTENT_TYPE,
            HeaderValue::from_str(&content_type).expect("a media type is valid in a header"),
        );
        response
    };
    let headers = response.headers_mut();
    if mime.subtype() == mime_guess::mime::HTML {
        security_headers(headers, &new_nonce());
    }
    headers.insert(X_CONTENT_TYPE_OPTIONS, NOSNIFF);
    headers.insert(CACHE_CONTROL, HeaderValue::from_static(cache));
    headers.insert(
        ETAG,
        HeaderValue::from_str(&etag).expect("a hex digest is valid in a header"),
    );
    response
}

#[cfg(test)]
mod tests {
    use axum::http::Request;
    use tower::ServiceExt;

    use super::*;

    const BUILT: &str = "<html><head><meta name=\"csp-nonce\" content=\"__CSP_NONCE__\"></head>\
        <body><div id=\"root\"></div></body></html>";

    async fn served(page: Page, path: &str) -> (StatusCode, HeaderMap, String) {
        let app: Router = router_for(page);
        let request = Request::builder().uri(path).body(Body::empty()).unwrap();
        let response = app.oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, headers, String::from_utf8(body.to_vec()).unwrap())
    }

    /// What `embedded` does with the `index.html` of a build.
    fn page_of(index: &[u8]) -> Page {
        Page::of_console(index).unwrap_or(Page::NotBuilt)
    }

    #[tokio::test]
    async fn a_built_page_gets_the_nonce_of_its_response() {
        for path in ["/", "/keys"] {
            let (status, headers, body) = served(page_of(BUILT.as_bytes()), path).await;
            assert_eq!(status, StatusCode::OK);
            let policy = headers[CONTENT_SECURITY_POLICY].to_str().unwrap();
            let nonce = policy
                .split("'nonce-")
                .nth(1)
                .and_then(|rest| rest.split('\'').next())
                .unwrap();
            assert_eq!(nonce.len(), 22);
            assert_eq!(body, BUILT.replace(NONCE_PLACEHOLDER, nonce));
        }
    }

    #[tokio::test]
    async fn a_build_without_one_placeholder_gives_the_fallback_page() {
        let none = BUILT.replace(NONCE_PLACEHOLDER, "");
        let two = format!("{BUILT}<!-- {NONCE_PLACEHOLDER} -->");
        let cases: [&[u8]; 4] = [none.as_bytes(), two.as_bytes(), b"", b"\xff__CSP_NONCE__"];
        for index in cases {
            assert!(Page::of_console(index).is_err());
            for path in ["/", "/keys"] {
                let (status, headers, body) = served(page_of(index), path).await;
                assert_eq!(status, StatusCode::OK);
                assert!(body.contains("console was not built"), "{body}");
                assert!(!body.contains("id=\"root\""));
                assert!(!body.contains(NONCE_PLACEHOLDER));
                assert_eq!(headers[CACHE_CONTROL], NEVER_STORED);
                assert!(headers.contains_key(CONTENT_SECURITY_POLICY));
            }
        }
    }

    #[test]
    fn the_fallback_page_names_the_build_command() {
        assert!(NOT_BUILT_PAGE.contains("console was not built"));
        assert!(NOT_BUILT_PAGE.contains("pnpm --dir ui build"));
        assert!(!NOT_BUILT_PAGE.contains("<script"));
        assert!(!NOT_BUILT_PAGE.contains("<style"));
        assert!(!NOT_BUILT_PAGE.contains("style="));
    }

    #[test]
    fn base64_matches_known_answers() {
        // RFC 4648, without the padding.
        let cases = [
            ("", ""),
            ("f", "Zg"),
            ("fo", "Zm8"),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg"),
            ("fooba", "Zm9vYmE"),
            ("foobar", "Zm9vYmFy"),
        ];
        for (plain, encoded) in cases {
            assert_eq!(base64(plain.as_bytes()), encoded);
        }
        assert_eq!(base64(&[0xfb, 0xff, 0xfe]), "+//+");
        assert_eq!(new_nonce().len(), 22);
        assert_ne!(new_nonce(), new_nonce());
    }

    #[test]
    fn paths_are_checked_decoded() {
        for path in [
            "/",
            "/keys",
            "/users/12",
            "/assets/index-BwB5JA1d.js",
            "/a%20b",
            "/100%",
        ] {
            assert!(is_plain(path), "{path}");
        }
        for path in [
            "/..",
            "/a/../b",
            "/a..b",
            "/a\\b",
            "/a%5Cb",
            "/a%00",
            "/%2e%2e/",
            "/%2E./",
            "/%252e%252e/",
            "/%25252e%25252e/",
            "/%2525252525252e",
        ] {
            assert!(!is_plain(path), "{path}");
        }
    }
}
