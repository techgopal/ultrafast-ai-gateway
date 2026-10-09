//! The console served at `/`, and what it must leave alone.
//!
//! These tests hold in every build. How files are answered is tested in
//! `src/web.rs` against a small set of files. The tests marked `ignore`
//! check the console that is really in the binary, so they need
//! `pnpm --dir ui build` before `cargo test -- --include-ignored`.

mod common;

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::Router;
use tower::ServiceExt;
use ultrafast_gateway::web::CONSOLE_BUILT;

const POLICY_BEFORE_NONCE: &str = "default-src 'none'; script-src 'self'; style-src 'self' 'nonce-";
const POLICY_AFTER_NONCE: &str = "'; style-src-attr 'unsafe-inline'; img-src 'self' data:; \
    media-src blob:; font-src 'self'; connect-src 'self'; form-action 'self'; base-uri 'none'; \
    frame-ancestors 'none'; manifest-src 'self'";
const NOT_BUILT: &str = "console was not built";

struct Answer {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}

impl Answer {
    fn header(&self, name: &str) -> &str {
        self.headers
            .get(name)
            .unwrap_or_else(|| panic!("no {name} header"))
            .to_str()
            .unwrap()
    }

    fn text(&self) -> String {
        String::from_utf8(self.body.clone()).expect("the body is UTF-8")
    }

    fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).expect("the body is JSON")
    }

    /// The nonce named by the Content Security Policy. Panics unless the
    /// rest of the policy is exactly the expected one.
    fn nonce(&self) -> String {
        let policy = self.header("content-security-policy");
        let rest = policy
            .strip_prefix(POLICY_BEFORE_NONCE)
            .unwrap_or_else(|| panic!("unexpected policy: {policy}"));
        let nonce = rest
            .strip_suffix(POLICY_AFTER_NONCE)
            .unwrap_or_else(|| panic!("unexpected policy: {policy}"));
        nonce.to_string()
    }
}

async fn request(app: &Router, method: &str, path: &str, headers: &[(&str, &str)]) -> Answer {
    let mut req = Request::builder().method(method).uri(path);
    for (name, value) in headers {
        req = req.header(*name, *value);
    }
    let resp = app
        .clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap();
    Answer {
        status: resp.status(),
        headers: resp.headers().clone(),
        body: axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    }
}

async fn get(app: &Router, path: &str) -> Answer {
    request(app, "GET", path, &[]).await
}

async fn app() -> Router {
    common::api().await.app
}

/// The page without what differs with every response.
fn without_nonce(answer: &Answer) -> String {
    let nonce = answer.nonce();
    answer.text().replace(&nonce, "")
}

/// Fails the test, rather than letting it pass on nothing, when the binary
/// holds no console.
fn needs_build() {
    let built = CONSOLE_BUILT;
    assert!(
        built,
        "this test needs the console: run `pnpm --dir ui build` first"
    );
}

/// The paths of the files the built page loads from `/assets/`.
fn asset_paths(page: &str) -> Vec<String> {
    page.split('"')
        .filter(|part| part.starts_with("/assets/"))
        .map(str::to_string)
        .collect()
}

#[tokio::test]
async fn root_serves_html() {
    let app = app().await;
    let page = get(&app, "/").await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(page.header("content-type").starts_with("text/html"));
    assert!(!page.body.is_empty());

    let head = request(&app, "HEAD", "/", &[]).await;
    assert_eq!(head.status, StatusCode::OK);
    assert!(head.header("content-type").starts_with("text/html"));
    assert!(head.body.is_empty());
}

#[tokio::test]
async fn deep_links_serve_the_app() {
    let app = app().await;
    let root = get(&app, "/").await;
    for path in [
        "/keys",
        "/users/12",
        "/accept-invite?token=x",
        "/index.html",
        "/apiary",
        "/v1x",
        "/healthy",
    ] {
        let page = get(&app, path).await;
        assert_eq!(page.status, StatusCode::OK, "{path}");
        assert!(page.header("content-type").starts_with("text/html"));
        assert_eq!(without_nonce(&page), without_nonce(&root), "{path}");
    }
    let head = request(&app, "HEAD", "/keys", &[]).await;
    assert_eq!(head.status, StatusCode::OK);
    assert!(head.body.is_empty());
}

#[tokio::test]
async fn api_and_v1_are_not_shadowed() {
    let h = common::harness("openai").await;
    let app = &h.app;

    for path in ["/api/nope", "/api/", "/api"] {
        let answer = get(app, path).await;
        assert_eq!(answer.status, StatusCode::NOT_FOUND, "{path}");
        assert!(answer
            .header("content-type")
            .starts_with("application/json"));
        assert_eq!(common::error_code(&answer.json()), "not_found", "{path}");
    }

    for (method, path) in [
        ("GET", "/v1/nope"),
        ("GET", "/v1"),
        ("GET", "/v1/"),
        ("POST", "/v1/nope"),
        ("GET", "/v1/chat/completions/more"),
    ] {
        let answer = request(app, method, path, &[]).await;
        assert_eq!(answer.status, StatusCode::NOT_FOUND, "{method} {path}");
        assert!(answer
            .header("content-type")
            .starts_with("application/json"));
        let body = answer.json();
        assert_eq!(body["error"]["type"], "invalid_request_error");
        assert!(body["error"]["message"].is_string());
        assert!(body["error"]["param"].is_null());
        assert!(body["error"]["code"].is_null());
    }

    let health = get(app, "/health").await;
    assert_eq!(health.status, StatusCode::OK);
    assert!(health
        .header("content-type")
        .starts_with("application/json"));
    assert_eq!(health.json(), serde_json::json!({ "status": "ok" }));

    let (status, body) = common::post_chat(app, None, "{}").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(body.contains("\"error\""));

    // The methods these paths do not take are refused as before.
    let wrong = get(app, "/v1/chat/completions").await;
    assert_eq!(wrong.status, StatusCode::METHOD_NOT_ALLOWED);
    let wrong = request(app, "POST", "/health", &[]).await;
    assert_eq!(wrong.status, StatusCode::METHOD_NOT_ALLOWED);
    let wrong = request(app, "DELETE", "/api/setup", &[]).await;
    assert_eq!(wrong.status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(common::error_code(&wrong.json()), "method_not_allowed");
}

/// Other spellings of the reserved names reach no handler of theirs, and
/// are not the app either.
#[tokio::test]
async fn reserved_names_in_any_form_are_404() {
    let app = app().await;
    for path in [
        "/API/x", "//api/x", "/%61pi/x", "/Api", "/V1/x", "/health/", "/HEALTH",
    ] {
        let answer = get(&app, path).await;
        assert_eq!(answer.status, StatusCode::NOT_FOUND, "{path}");
        assert!(answer.body.is_empty(), "{path}");
        assert!(answer.headers.get("content-security-policy").is_none());
        assert_eq!(answer.header("x-content-type-options"), "nosniff");
    }
}

#[tokio::test]
async fn missing_asset_is_404_not_html() {
    let app = app().await;
    for path in [
        "/assets/nope.js",
        "/assets/",
        "/assets",
        "/assets/index.html",
    ] {
        let answer = get(&app, path).await;
        assert_eq!(answer.status, StatusCode::NOT_FOUND, "{path}");
        assert!(answer.headers.get("content-security-policy").is_none());
        let text = answer.text();
        assert!(!text.contains("<html"), "{path}");
        assert!(!text.contains(NOT_BUILT), "{path}");
        assert_eq!(answer.header("x-content-type-options"), "nosniff");
    }
}

#[tokio::test]
async fn html_has_the_security_headers() {
    let app = app().await;
    for path in ["/", "/keys"] {
        let page = get(&app, path).await;
        // Checks the whole policy around the nonce.
        page.nonce();
        assert_eq!(page.header("x-content-type-options"), "nosniff");
        assert_eq!(page.header("referrer-policy"), "no-referrer");
        assert_eq!(page.header("x-frame-options"), "DENY");
        assert_eq!(page.header("cross-origin-opener-policy"), "same-origin");
        assert_eq!(
            page.header("permissions-policy"),
            "camera=(), microphone=(), geolocation=()"
        );
    }
}

#[tokio::test]
async fn html_is_not_cached() {
    let app = app().await;
    for path in ["/", "/keys"] {
        let page = get(&app, path).await;
        assert_eq!(page.header("cache-control"), "no-store");
        assert!(page.headers.get("etag").is_none());
    }
}

#[tokio::test]
#[ignore = "needs a console build"]
async fn built_assets_are_cached() {
    needs_build();
    let app = app().await;
    let page = get(&app, "/").await;
    let paths = asset_paths(&page.text());
    assert!(
        paths.iter().any(|p| p.ends_with(".js")) && paths.iter().any(|p| p.ends_with(".css")),
        "the page names a script and a stylesheet: {paths:?}"
    );
    for path in paths {
        let asset = get(&app, &path).await;
        assert_eq!(asset.status, StatusCode::OK, "{path}");
        assert_eq!(
            asset.header("cache-control"),
            "public, max-age=31536000, immutable"
        );
        assert_eq!(asset.header("x-content-type-options"), "nosniff");
        assert!(asset.header("etag").starts_with('"'));
        let content_type = asset.header("content-type");
        if path.ends_with(".js") {
            assert!(content_type.contains("javascript"), "{content_type}");
        } else if path.ends_with(".css") {
            assert!(content_type.starts_with("text/css"), "{content_type}");
        }
        assert!(!asset.body.is_empty());
        assert_eq!(asset.header("content-length"), asset.body.len().to_string());

        let head = request(&app, "HEAD", &path, &[]).await;
        assert_eq!(head.status, StatusCode::OK);
        assert!(head.body.is_empty());
    }
}

#[tokio::test]
async fn post_to_a_page_path_is_405() {
    let app = app().await;
    for (method, path) in [
        ("POST", "/keys"),
        ("POST", "/"),
        ("DELETE", "/users/12"),
        ("PUT", "/theme.js"),
        ("POST", "/assets/nope.js"),
    ] {
        let answer = request(&app, method, path, &[]).await;
        assert_eq!(
            answer.status,
            StatusCode::METHOD_NOT_ALLOWED,
            "{method} {path}"
        );
        assert!(!answer.text().contains("<html"));
        assert_eq!(answer.header("x-content-type-options"), "nosniff");
        assert_eq!(answer.header("allow"), "GET, HEAD");
    }
}

#[tokio::test]
async fn traversal_is_refused() {
    let app = app().await;
    for path in [
        "/assets/../Cargo.toml",
        "/assets/..%2f..%2fCargo.toml",
        "/assets/..%2F..%2FCargo.toml",
        "/assets/%2e%2e/x",
        "/assets/%2E%2e/x",
        "/assets/a%5cb",
        "/assets/a%00b",
        "/../Cargo.toml",
        "/..%2fCargo.toml",
        "/%2e%2e/theme.js",
        "/keys%5c..",
        "/keys/%00",
        "/a/%252e%252e/b",
    ] {
        let answer = get(&app, path).await;
        assert_eq!(answer.status, StatusCode::NOT_FOUND, "{path}");
        assert!(!answer.text().contains("<html"), "{path}");
        assert!(!answer.text().contains("[workspace]"), "{path}");
        assert_eq!(answer.header("x-content-type-options"), "nosniff");
    }
}

/// `/assets/a\b`, if the HTTP library lets such a request exist.
#[tokio::test]
async fn a_literal_backslash_is_refused() {
    let app = app().await;
    let uri = axum::http::Uri::try_from("/assets/a\\b")
        .expect("the HTTP library accepts a backslash in a path");
    let req = Request::builder().uri(uri).body(Body::empty()).unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn api_responses_are_not_cached_or_sniffed() {
    let app = app().await;
    for (method, path) in [
        ("GET", "/api/setup"),
        ("GET", "/api/auth/me"),
        ("GET", "/api/nope"),
        ("GET", "/api/"),
        ("DELETE", "/api/setup"),
    ] {
        let answer = request(&app, method, path, &[]).await;
        assert_eq!(answer.header("cache-control"), "no-store", "{path}");
        assert_eq!(answer.header("x-content-type-options"), "nosniff");
        assert!(answer.headers.get("content-security-policy").is_none());
    }
    assert_eq!(get(&app, "/api/setup").await.status, StatusCode::OK);
}

#[tokio::test]
#[ignore = "needs a console build"]
async fn built_assets_are_sent_gzipped_to_who_accepts_it() {
    use std::io::Read;
    needs_build();
    let app = app().await;
    let page = get(&app, "/").await;
    let paths = asset_paths(&page.text());
    assert!(!paths.is_empty());
    for path in paths {
        let plain = get(&app, &path).await;
        assert!(plain.headers.get("content-encoding").is_none(), "{path}");
        assert_eq!(plain.header("vary"), "accept-encoding");

        let packed = request(
            &app,
            "GET",
            &path,
            &[("accept-encoding", "gzip, deflate, br, zstd")],
        )
        .await;
        assert_eq!(packed.status, StatusCode::OK, "{path}");
        assert_eq!(packed.header("content-encoding"), "gzip", "{path}");
        assert_eq!(packed.header("vary"), "accept-encoding");
        assert_eq!(
            packed.header("content-length"),
            packed.body.len().to_string()
        );
        assert!(packed.body.len() < plain.body.len() / 2, "{path}");
        assert_ne!(packed.header("etag"), plain.header("etag"));
        assert_eq!(
            packed.header("cache-control"),
            plain.header("cache-control")
        );
        assert_eq!(packed.header("content-type"), plain.header("content-type"));
        let mut unpacked = Vec::new();
        flate2::read::GzDecoder::new(&packed.body[..])
            .read_to_end(&mut unpacked)
            .expect("the body is gzip");
        assert_eq!(unpacked, plain.body, "{path}");
    }
}

#[tokio::test]
#[ignore = "needs a console build"]
async fn built_root_files_are_served() {
    needs_build();
    let app = app().await;
    let file = get(&app, "/theme.js").await;
    assert_eq!(file.status, StatusCode::OK);
    assert!(file.header("content-type").contains("javascript"));
    assert_eq!(file.header("cache-control"), "no-cache");
    assert_eq!(file.header("x-content-type-options"), "nosniff");
    assert!(file.header("etag").starts_with('"'));
    assert!(file.text().contains("uf-theme"));
}

#[tokio::test]
#[ignore = "needs a console build"]
async fn built_files_give_304_for_their_etag() {
    needs_build();
    let app = app().await;
    let page = get(&app, "/").await;
    let mut paths = asset_paths(&page.text());
    paths.push("/theme.js".to_string());
    for path in paths {
        let first = get(&app, &path).await;
        let etag = first.header("etag").to_string();
        let weak = format!("W/{etag}");
        let listed = format!("\"other\", {etag}");
        for sent in [etag.as_str(), weak.as_str(), listed.as_str(), "*"] {
            let second = request(&app, "GET", &path, &[("if-none-match", sent)]).await;
            assert_eq!(second.status, StatusCode::NOT_MODIFIED, "{path} {sent}");
            assert!(second.body.is_empty());
            assert_eq!(second.header("etag"), etag);
            assert_eq!(
                second.header("cache-control"),
                first.header("cache-control")
            );
        }
        let other = request(&app, "GET", &path, &[("if-none-match", "\"other\"")]).await;
        assert_eq!(other.status, StatusCode::OK);
        assert_eq!(other.body, first.body);
    }
    // A page has no ETag, so nothing makes it a 304.
    let again = request(&app, "GET", "/", &[("if-none-match", "*")]).await;
    assert_eq!(again.status, StatusCode::OK);
}

#[tokio::test]
#[ignore = "needs a console build"]
async fn the_built_console_is_the_page() {
    needs_build();
    let app = app().await;
    for path in ["/", "/keys"] {
        let page = get(&app, path).await;
        let nonce = page.nonce();
        let text = page.text();
        assert!(text.contains("<div id=\"root\">"));
        assert!(!text.contains(NOT_BUILT));
        let meta = format!("<meta name=\"csp-nonce\" content=\"{nonce}\"");
        assert_eq!(text.matches(&meta).count(), 1, "{text}");
        assert_eq!(text.matches(nonce.as_str()).count(), 1);
    }
}

#[tokio::test]
async fn every_page_has_its_own_nonce() {
    let app = app().await;
    let first = get(&app, "/").await;
    let second = get(&app, "/").await;
    let deep = get(&app, "/keys").await;
    let nonces = [first.nonce(), second.nonce(), deep.nonce()];
    assert_ne!(nonces[0], nonces[1]);
    assert_ne!(nonces[0], nonces[2]);
    assert_ne!(nonces[1], nonces[2]);

    for (page, nonce) in [first, second, deep].iter().zip(&nonces) {
        assert!(nonce.len() >= 22, "{nonce}");
        assert!(
            nonce
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/'),
            "{nonce}"
        );
        let text = page.text();
        assert!(!text.contains("__CSP_NONCE__"));
    }
}
