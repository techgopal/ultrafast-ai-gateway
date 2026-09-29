//! The web console, compiled into the binary and served at `/`.
//!
//! `build.rs` puts the console's build output, or a page that says it was
//! not built, into `$OUT_DIR/console`. Everything served here comes from
//! that copy inside the binary: a requested path is only ever a name looked
//! up in it, never a path read from disk.

use std::collections::HashMap;
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
use bytes::Bytes;
use rust_embed::Embed;
use sha2::{Digest, Sha256};

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

/// First segments of paths the console never answers. They belong to the
/// gateway's other handlers, so no spelling of them is a page of the app.
const RESERVED: [&str; 3] = ["api", "v1", "health"];
const ASSETS: &str = "/assets";
/// How many times a path is percent-decoded when it is checked.
const MAX_DECODE_ROUNDS: usize = 4;

/// The console: `/`, the files of its build, and as the fallback every
/// other path, so a link into the app works when opened directly.
pub fn router() -> Router<Arc<AppState>> {
    router_for(Console::embedded())
}

fn router_for<S>(console: Console) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let console = Arc::new(console);
    let root = console.clone();
    let assets = console.clone();
    Router::new()
        .route("/", get(move || async move { root.page.response() }))
        .route(
            "/assets/{*name}",
            get(move |uri: Uri, headers: HeaderMap| async move {
                assets.asset(uri.path(), &headers)
            }),
        )
        .method_not_allowed_fallback(|| async { method_not_allowed() })
        .fallback(
            move |method: Method, uri: Uri, headers: HeaderMap| async move {
                let head = method == Method::HEAD;
                let mut response = console.other(&method, uri.path(), &headers);
                if head {
                    *response.body_mut() = Body::empty();
                }
                response
            },
        )
}

fn not_found() -> Response {
    (StatusCode::NOT_FOUND, [(X_CONTENT_TYPE_OPTIONS, NOSNIFF)]).into_response()
}

fn method_not_allowed() -> Response {
    let allow = HeaderValue::from_static("GET, HEAD");
    let headers = [(ALLOW, allow), (X_CONTENT_TYPE_OPTIONS, NOSNIFF)];
    (StatusCode::METHOD_NOT_ALLOWED, headers).into_response()
}

/// A file of the console's build.
struct File {
    data: Bytes,
    sha256: [u8; 32],
}

/// What is served: the files of a build by their names, such as
/// `assets/index-1a2b3c4d.js`, and the page made from its `index.html`.
/// The answers depend on nothing else, so they are the same for the files
/// in the binary and for any other set.
struct Console {
    files: HashMap<String, File>,
    page: Page,
}

impl Console {
    /// The files compiled into the binary.
    fn embedded() -> Self {
        let files = Files::iter().filter_map(|name| {
            let file = Files::get(&name)?;
            let data = match file.data {
                std::borrow::Cow::Borrowed(bytes) => Bytes::from_static(bytes),
                std::borrow::Cow::Owned(bytes) => Bytes::from(bytes),
            };
            Some((name.into_owned(), data))
        });
        Self::of(files, CONSOLE_BUILT)
    }

    /// `built` says whether the files are a build of the console. Without
    /// one, or with one whose `index.html` cannot be used, the page says
    /// that the console was not built.
    fn of(files: impl IntoIterator<Item = (String, Bytes)>, built: bool) -> Self {
        let files: HashMap<String, File> = files
            .into_iter()
            .map(|(name, data)| {
                let sha256 = Sha256::digest(&data).into();
                (name, File { data, sha256 })
            })
            .collect();
        let page = if built {
            let index = files.get(INDEX).map(|file| &file.data[..]);
            Page::of_console(index.unwrap_or_default()).unwrap_or_else(|reason| {
                tracing::error!(reason, "the console cannot be served");
                Page::NotBuilt
            })
        } else {
            Page::NotBuilt
        };
        Self { files, page }
    }

    /// `GET /assets/<name>`: the file, or 404. Never the page.
    fn asset(&self, path: &str, headers: &HeaderMap) -> Response {
        if !is_plain(path) {
            return not_found();
        }
        let name = path.trim_start_matches('/');
        match self.files.get(name) {
            Some(file) => file_response(name, file, IMMUTABLE, headers),
            None => not_found(),
        }
    }

    /// Every path no route claimed.
    fn other(&self, method: &Method, path: &str, headers: &HeaderMap) -> Response {
        if is_reserved(path) {
            return not_found();
        }
        if method != Method::GET && method != Method::HEAD {
            return method_not_allowed();
        }
        if !is_plain(path) || path == ASSETS || path.starts_with("/assets/") {
            return not_found();
        }
        // A file at the root of the build, such as `theme.js`.
        let name = path.trim_start_matches('/');
        if name != INDEX && !name.contains('/') {
            if let Some(file) = self.files.get(name) {
                return file_response(name, file, REVALIDATED, headers);
            }
        }
        self.page.response()
    }
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

/// Whether the first segment of the path that is not empty is a reserved
/// name, however it is percent-encoded and in whatever case.
fn is_reserved(path: &str) -> bool {
    let mut bytes = path.as_bytes().to_vec();
    for _ in 0..MAX_DECODE_ROUNDS {
        let decoded = percent_decoded(&bytes);
        if decoded == bytes {
            break;
        }
        bytes = decoded;
    }
    bytes
        .split(|b| *b == b'/')
        .find(|segment| !segment.is_empty())
        .is_some_and(|first| {
            RESERVED
                .iter()
                .any(|name| first.eq_ignore_ascii_case(name.as_bytes()))
        })
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

fn file_response(name: &str, file: &File, cache: &'static str, request: &HeaderMap) -> Response {
    let etag = format!("\"{}\"", hex::encode(&file.sha256[..16]));
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
        let mut response = file.data.clone().into_response();
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

    const SCRIPT: &str = "/assets/app-abc123.js";
    const STYLES: &str = "/assets/app-abc123.css";
    const NOT_BUILT: &str = "console was not built";

    /// A small build of the console.
    fn fixture() -> Console {
        console_with(BUILT.as_bytes())
    }

    fn console_with(index: &[u8]) -> Console {
        let files: [(&str, &[u8]); 5] = [
            ("index.html", index),
            ("assets/app-abc123.js", b"console.log(\"app\");"),
            ("assets/app-abc123.css", b"body { margin: 0 }"),
            ("theme.js", b"/* uf-theme */"),
            (
                "favicon.svg",
                b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>",
            ),
        ];
        let files = files
            .into_iter()
            .map(|(name, data)| (name.to_string(), Bytes::copy_from_slice(data)));
        Console::of(files, true)
    }

    /// What a binary without a console build holds.
    fn not_built() -> Console {
        let page = Bytes::from_static(NOT_BUILT_PAGE.as_bytes());
        Console::of([(INDEX.to_string(), page)], false)
    }

    async fn send(
        console: Console,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
    ) -> (StatusCode, HeaderMap, String) {
        let app: Router = router_for(console);
        let mut request = Request::builder().method(method).uri(path);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let response = app
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, headers, String::from_utf8(body.to_vec()).unwrap())
    }

    async fn served(console: Console, path: &str) -> (StatusCode, HeaderMap, String) {
        send(console, "GET", path, &[]).await
    }

    fn page_of(index: &[u8]) -> Console {
        console_with(index)
    }

    #[tokio::test]
    async fn assets_are_cached_for_good() {
        for (path, content_type, body) in [
            (
                SCRIPT,
                "text/javascript; charset=utf-8",
                "console.log(\"app\");",
            ),
            (STYLES, "text/css; charset=utf-8", "body { margin: 0 }"),
        ] {
            let (status, headers, text) = served(fixture(), path).await;
            assert_eq!(status, StatusCode::OK, "{path}");
            assert_eq!(text, body);
            assert_eq!(headers[CONTENT_TYPE], content_type);
            assert_eq!(
                headers[CACHE_CONTROL],
                "public, max-age=31536000, immutable"
            );
            assert_eq!(headers[X_CONTENT_TYPE_OPTIONS], "nosniff");
            assert_eq!(headers["content-length"], body.len().to_string());
            let etag = headers[ETAG].to_str().unwrap();
            assert!(etag.len() == 34 && etag.starts_with('"') && etag.ends_with('"'));
            assert!(!headers.contains_key(CONTENT_SECURITY_POLICY));

            let (status, headers, text) = send(fixture(), "HEAD", path, &[]).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(headers[CONTENT_TYPE], content_type);
            assert!(text.is_empty());
        }
    }

    #[tokio::test]
    async fn root_files_are_revalidated() {
        for (path, content_type, body) in [
            (
                "/theme.js",
                "text/javascript; charset=utf-8",
                "/* uf-theme */",
            ),
            (
                "/favicon.svg",
                "image/svg+xml",
                "<svg xmlns=\"http://www.w3.org/2000/svg\"/>",
            ),
        ] {
            let (status, headers, text) = served(fixture(), path).await;
            assert_eq!(status, StatusCode::OK, "{path}");
            assert_eq!(text, body);
            assert_eq!(headers[CONTENT_TYPE], content_type);
            assert_eq!(headers[CACHE_CONTROL], "no-cache");
            assert_eq!(headers[X_CONTENT_TYPE_OPTIONS], "nosniff");
            assert!(headers.contains_key(ETAG));
        }
        // Without a build there is no such file: the path is a page.
        let (status, headers, text) = served(not_built(), "/theme.js").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[CONTENT_TYPE], HTML);
        assert!(text.contains(NOT_BUILT));
    }

    #[tokio::test]
    async fn a_matching_etag_gives_304() {
        for path in [SCRIPT, STYLES, "/theme.js", "/favicon.svg"] {
            let (_, first, body) = served(fixture(), path).await;
            let etag = first[ETAG].to_str().unwrap().to_string();
            let weak = format!("W/{etag}");
            let listed = format!("\"other\", {etag}");
            for sent in [etag.as_str(), weak.as_str(), listed.as_str(), "*"] {
                let (status, headers, text) =
                    send(fixture(), "GET", path, &[("if-none-match", sent)]).await;
                assert_eq!(status, StatusCode::NOT_MODIFIED, "{path} {sent}");
                assert!(text.is_empty());
                assert_eq!(headers[ETAG], first[ETAG]);
                assert_eq!(headers[CACHE_CONTROL], first[CACHE_CONTROL]);
                assert_eq!(headers[X_CONTENT_TYPE_OPTIONS], "nosniff");
            }
            let (status, _, text) =
                send(fixture(), "GET", path, &[("if-none-match", "\"other\"")]).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(text, body);
        }
        // Files with other bytes have other tags.
        let (_, script, _) = served(fixture(), SCRIPT).await;
        let (_, styles, _) = served(fixture(), STYLES).await;
        assert_ne!(script[ETAG], styles[ETAG]);
        // A page has no tag, so nothing makes it a 304.
        let (status, headers, _) = send(fixture(), "GET", "/", &[("if-none-match", "*")]).await;
        assert_eq!(status, StatusCode::OK);
        assert!(!headers.contains_key(ETAG));
    }

    #[tokio::test]
    async fn a_missing_asset_is_404_and_never_the_page() {
        for console in [fixture, not_built] {
            for path in [
                "/assets/nope.js",
                "/assets/",
                "/assets",
                "/assets/index.html",
                "/assets/theme.js",
                "/assets/../theme.js",
                "/assets/%2e%2e/theme.js",
            ] {
                let (status, headers, text) = served(console(), path).await;
                assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
                assert!(text.is_empty(), "{path}");
                assert!(!headers.contains_key(CONTENT_SECURITY_POLICY));
            }
        }
        // A file of the build is served under its own name only.
        let (_, headers, _) = served(fixture(), "/app-abc123.js").await;
        assert_eq!(headers[CONTENT_TYPE], HTML);
    }

    #[tokio::test]
    async fn reserved_names_are_never_the_app() {
        for console in [fixture, not_built] {
            for path in [
                "/API/x",
                "//api/x",
                "/%61pi/x",
                "/%2561pi/x",
                "/Api",
                "/V1/x",
                "/health/",
                "/HEALTH",
                "/api",
                "/api/x",
                "/v1",
                "/v1/x",
                "/health",
                "/health/more",
                "///hEaLtH//x",
                "/%2fapi/x",
                "/API/x?next=/keys",
            ] {
                for method in ["GET", "HEAD", "POST"] {
                    let (status, headers, text) = send(console(), method, path, &[]).await;
                    assert_eq!(status, StatusCode::NOT_FOUND, "{method} {path}");
                    assert!(text.is_empty(), "{method} {path}");
                    assert!(!headers.contains_key(CONTENT_SECURITY_POLICY));
                }
            }
            // Names that only begin like them belong to the app.
            for path in [
                "/apiary",
                "/v1x",
                "/healthy",
                "/keys/api",
                "/x/v1/health",
                "/ap%69ary",
            ] {
                let (status, headers, text) = served(console(), path).await;
                assert_eq!(status, StatusCode::OK, "{path}");
                assert_eq!(headers[CONTENT_TYPE], HTML, "{path}");
                assert!(text.contains("<html"), "{path}");
            }
        }
    }

    #[tokio::test]
    async fn the_page_says_whether_the_console_was_built() {
        for path in ["/", "/keys", "/index.html"] {
            let (status, headers, text) = served(fixture(), path).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(headers[CACHE_CONTROL], "no-store");
            assert!(!headers.contains_key(ETAG));
            assert!(text.contains("<div id=\"root\">"));
            assert!(!text.contains(NOT_BUILT));
            assert!(!text.contains(NONCE_PLACEHOLDER));

            let (status, headers, text) = served(not_built(), path).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(headers[CACHE_CONTROL], "no-store");
            assert!(headers.contains_key(CONTENT_SECURITY_POLICY));
            assert!(text.contains(NOT_BUILT));
            assert!(text.contains("pnpm --dir ui build"));
        }
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
            let meta = format!("<meta name=\"csp-nonce\" content=\"{nonce}\">");
            assert_eq!(body.matches(&meta).count(), 1);
            assert_eq!(body.matches(nonce).count(), 1);
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
